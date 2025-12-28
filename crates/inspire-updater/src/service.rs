use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};

use crate::config::UpdaterConfig;
use crate::rpc::EthrexClient;
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
}

impl UpdaterService {
    pub async fn new(config: UpdaterConfig) -> anyhow::Result<Self> {
        let rpc = EthrexClient::new(&config.rpc_url, config.admin_rpc_url.clone()).await?;
        let state = StateTracker::new();
        let writer = ShardWriter::new(&config.data_dir, config.chain_id);
        let reload = ReloadClient::new(&config.pir_server_url);

        Ok(Self {
            config,
            rpc,
            state,
            writer,
            reload,
        })
    }

    /// Perform initial sync by dumping all storage
    pub async fn initial_sync(&mut self) -> anyhow::Result<()> {
        info!("Starting initial sync via pir_dumpStorage");

        let current_block = self.rpc.block_number().await?;

        let entries = self
            .rpc
            .dump_all_storage(10000, |page, entries| {
                info!(page, entries = entries.len(), "Fetched storage page");
            })
            .await?;

        info!(
            block = current_block,
            entries = entries.len(),
            "Initial sync complete"
        );

        self.state.load_from_dump(current_block, entries.clone());
        let path = self.writer.write_full_state(&entries, current_block).await?;

        info!(path = %path.display(), "Wrote state.bin");

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
            "Starting updater service"
        );

        // If no state, do initial sync
        if self.state.last_block().is_none() {
            self.initial_sync().await?;
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
        let current_block = self.rpc.block_number().await?;
        let last_block = self.state.last_block().unwrap_or(0);

        if current_block <= last_block {
            return Ok(()); // No new blocks
        }

        let blocks_behind = current_block - last_block;

        if blocks_behind > 0 {
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

            // Collect all deltas from all blocks
            let mut all_entries = Vec::new();
            for block_delta in &delta_resp.blocks {
                all_entries.extend(block_delta.deltas.clone());
            }

            if !all_entries.is_empty() {
                let changed = self.state.apply_entries(to_block, all_entries);

                info!(changed = changed.len(), "Storage entries changed");
                self.writer.write_entries(&changed).await?;

                // Trigger PIR server reload
                if let Err(e) = self.reload.reload().await {
                    warn!(error = %e, "Failed to trigger PIR reload");
                } else {
                    info!(block = to_block, "PIR server reloaded");
                }
            } else {
                // No deltas but still update block number
                self.state.apply_entries(to_block, vec![]);
            }
        }

        Ok(())
    }
}
