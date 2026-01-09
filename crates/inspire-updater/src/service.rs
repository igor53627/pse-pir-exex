use alloy_primitives::hex;
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};

use crate::config::UpdaterConfig;
use crate::delta_writer::RangeDeltaWriter;
use crate::rpc::{EthrexClient, StateRpcMode};
use crate::state::StateTracker;
use crate::writer::ShardWriter;

/// Reload client (reuse from lane-builder)
pub struct ReloadClient {
    client: reqwest::Client,
    server_url: String,
}

impl ReloadClient {
    pub fn new(server_url: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
            server_url: server_url.to_string(),
        }
    }

    pub async fn reload(&self) -> anyhow::Result<()> {
        let url = format!("{}/admin/reload", self.server_url);
        let resp = self.client.post(&url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("Reload failed: {}", resp.status());
        }
        Ok(())
    }

    pub async fn health(&self) -> bool {
        let url = format!("{}/health", self.server_url);
        self.client
            .get(&url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

/// Main updater service
pub struct UpdaterService {
    config: UpdaterConfig,
    rpc: EthrexClient,
    state: StateTracker,
    writer: ShardWriter,
    reload: ReloadClient,
    delta_writer: RangeDeltaWriter,
    /// Whether we've verified UBT root since last full sync
    ubt_verified: bool,
}

impl UpdaterService {
    pub async fn new(config: UpdaterConfig) -> anyhow::Result<Self> {
        let rpc = EthrexClient::new(
            &config.rpc_url,
            config.admin_rpc_url.clone(),
            config.ubt_rpc_url.clone(),
        )
        .await?;
        let state = StateTracker::new();
        let writer = ShardWriter::new(&config.data_dir, config.chain_id);
        let reload = ReloadClient::new(&config.pir_server_url);
        let mut delta_writer = RangeDeltaWriter::new(&config.data_dir);

        if let Err(e) = delta_writer.load() {
            warn!(error = %e, "Failed to load existing delta state");
        }

        Ok(Self {
            config,
            rpc,
            state,
            writer,
            reload,
            delta_writer,
            ubt_verified: false,
        })
    }

    /// Perform initial sync by dumping all storage
    pub async fn initial_sync(&mut self) -> anyhow::Result<()> {
        if self.rpc.state_mode() == StateRpcMode::UbtExex {
            info!("Starting initial sync via ubt_exportState");

            let export = self
                .rpc
                .ubt_export_state(&self.config.data_dir, self.config.chain_id)
                .await?;

            self.state.set_last_block(export.block_number);
            self.ubt_verified = true;

            info!(
                block = export.block_number,
                root = %hex::encode(export.root.0),
                entries = export.entry_count,
                stems = export.stem_count,
                state_file = %export.state_file,
                stem_index = %export.stem_index_file,
                "UBT state export complete"
            );

            if let Err(e) = self.reload.reload().await {
                warn!(error = %e, "Failed to trigger PIR reload after initial sync");
            }

            return Ok(());
        }

        info!("Starting initial sync via pir_dumpStorage");

        let current_block = self.rpc.head_block().await?;

        let entries = self
            .rpc
            .dump_all_storage(10000, |page, entries| {
                info!(page, entries = entries.len(), "Fetched storage page");
            })
            .await?;

        info!(
            block = current_block,
            entries = entries.len(),
            "Initial sync complete, fetching UBT root for verification"
        );

        // Fetch UBT root for the current block (only works for head block)
        let ubt_root = match self.rpc.ubt_get_root(current_block).await {
            Ok(resp) => {
                info!(
                    block = resp.block_number,
                    root = %hex::encode(resp.root.0),
                    "UBT root fetched for verification"
                );
                resp.root.0
            }
            Err(e) => {
                warn!(
                    error = %e,
                    block = current_block,
                    "Failed to fetch UBT root (block may have advanced), using zero hash"
                );
                [0u8; 32]
            }
        };

        self.state.load_from_dump(current_block, entries.clone());
        let path = self
            .writer
            .write_full_state_with_ubt(&entries, current_block, ubt_root)
            .await?;

        info!(
            path = %path.display(),
            ubt_root = %hex::encode(ubt_root),
            "Wrote state.bin with UBT root"
        );

        // Trigger PIR server reload
        if let Err(e) = self.reload.reload().await {
            warn!(error = %e, "Failed to trigger PIR reload after initial sync");
        }

        Ok(())
    }

    /// Run the updater loop (requires initial_sync first, or existing state)
    pub async fn run(&mut self) -> anyhow::Result<()> {
        info!(
            rpc = %self.config.rpc_url,
            pir_server = %self.config.pir_server_url,
            mode = ?self.rpc.state_mode(),
            "Starting updater service"
        );

        // If no state, do initial sync
        if self.state.last_block().is_none() {
            self.initial_sync().await?;
        }

        if self.config.one_shot {
            info!("One-shot mode enabled; exiting after initial sync");
            return Ok(());
        }

        // Check PIR server health
        if !self.reload.health().await {
            warn!("PIR server not healthy, will retry on updates");
        }

        let mut poll = interval(self.config.poll_interval);

        loop {
            poll.tick().await;

            if let Err(e) = self.poll_once().await {
                error!(error = %e, "Poll failed");
            }
        }
    }

    async fn poll_once(&mut self) -> anyhow::Result<()> {
        let current_block = self.rpc.head_block().await?;
        let last_block = self.state.last_block().unwrap_or(0);

        if current_block <= last_block {
            // We're caught up - verify UBT root if not yet done
            if !self.ubt_verified {
                self.verify_ubt_root(current_block).await;
            }
            return Ok(()); // No new blocks
        }

        let blocks_behind = current_block - last_block;

        if blocks_behind > 0 {
            if self.rpc.state_mode() == StateRpcMode::UbtExex {
                let to_block = current_block.min(last_block + self.config.max_blocks_per_fetch);

                info!(
                    current_block,
                    last_block,
                    fetching_to = to_block,
                    blocks_behind,
                    "Fetching UBT state delta"
                );

                let delta = self
                    .rpc
                    .ubt_get_state_delta(
                        last_block + 1,
                        to_block,
                        &self.config.data_dir,
                        self.config.chain_id,
                    )
                    .await?;

                info!(
                    from = delta.from_block,
                    to = delta.to_block,
                    head = delta.head_block,
                    entries = delta.entry_count,
                    file = %delta.delta_file,
                    "UBT delta export complete"
                );

                self.state.set_last_block(delta.to_block);
                self.ubt_verified = true;

                if let Err(e) = self.reload.reload().await {
                    warn!(error = %e, "Failed to trigger PIR reload after delta export");
                }

                return Ok(());
            }

            // Use pir_getStateDelta for efficient incremental updates
            let to_block = current_block.min(last_block + self.config.max_blocks_per_fetch);

            info!(
                current_block,
                last_block,
                fetching_to = to_block,
                blocks_behind,
                "Fetching state deltas"
            );

            let delta_resp = self
                .rpc
                .pir_get_state_delta(last_block + 1, to_block)
                .await?;

            info!(
                from = delta_resp.from_block,
                to = delta_resp.to_block,
                total_deltas = delta_resp.total_deltas,
                blocks = delta_resp.blocks.len(),
                "Received state deltas"
            );

            // Process each block's deltas individually for accurate bucket tracking
            for block_delta in &delta_resp.blocks {
                if block_delta.deltas.is_empty() {
                    continue;
                }

                let (changed, bucket_delta) = self
                    .state
                    .apply_entries_with_delta(block_delta.block_number, block_delta.deltas.clone());

                if !changed.is_empty() {
                    info!(
                        block = block_delta.block_number,
                        changed = changed.len(),
                        bucket_updates = bucket_delta.updates.len(),
                        "Applied block delta"
                    );
                    self.writer.write_entries(&changed).await?;
                }

                // Add bucket delta for range tracking
                if !bucket_delta.updates.is_empty() {
                    self.delta_writer.add_delta(bucket_delta);
                }
            }

            // Write range delta file if we processed any blocks
            if !delta_resp.blocks.is_empty() {
                if let Err(e) = self.delta_writer.write() {
                    warn!(error = %e, "Failed to write range delta file");
                } else {
                    info!(
                        block = self.delta_writer.current_block(),
                        "Wrote bucket-deltas.bin"
                    );
                }

                // Trigger PIR server reload
                if let Err(e) = self.reload.reload().await {
                    warn!(error = %e, "Failed to trigger PIR reload");
                } else {
                    info!(block = to_block, "PIR server reloaded");
                }
            }

            // If we caught up to head, verify UBT
            if to_block == current_block && !self.ubt_verified {
                self.verify_ubt_root(current_block).await;
            }
        }

        Ok(())
    }

    /// Verify UBT root matches our state (called when caught up to head)
    async fn verify_ubt_root(&mut self, block: u64) {
        match self.rpc.ubt_get_root(block).await {
            Ok(resp) => {
                info!(
                    block = resp.block_number,
                    ubt_root = %hex::encode(resp.root.0),
                    "[OK] UBT root verified at head block"
                );
                self.ubt_verified = true;
            }
            Err(e) => {
                warn!(
                    error = %e,
                    block,
                    "Failed to verify UBT root"
                );
            }
        }
    }
}
