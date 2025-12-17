//! Reth ExEx integration for real-time lane updates
//!
//! This module provides an Execution Extension (ExEx) that hooks into Reth's
//! block processing pipeline to update PIR lane databases in real-time.
//!
//! Enable with the `exex` feature:
//! ```toml
//! lane-builder = { path = "...", features = ["exex"] }
//! ```

#![cfg(feature = "exex")]

use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use eyre::Result;
use futures::StreamExt;
use reth_exex::{ExExContext, ExExEvent, ExExNotification};
use reth_node_api::FullNodeComponents;
use tracing::{info, warn, error};

use crate::reload::ReloadClient;

/// Configuration for the lane updater ExEx
#[derive(Debug, Clone)]
pub struct LaneUpdaterConfig {
    /// URL of the PIR server to notify on updates
    pub server_url: String,
    /// Directory containing lane databases
    pub data_dir: PathBuf,
    /// Minimum interval between reloads (debounce)
    pub reload_debounce: Duration,
}

impl Default for LaneUpdaterConfig {
    fn default() -> Self {
        Self {
            server_url: "http://localhost:3000".to_string(),
            data_dir: PathBuf::from("./pir-data"),
            reload_debounce: Duration::from_secs(1),
        }
    }
}

/// Initialize the lane updater ExEx
///
/// This is the entry point for the ExEx. It sets up the reload client
/// and returns the main processing loop.
pub async fn lane_updater_exex<Node: FullNodeComponents>(
    ctx: ExExContext<Node>,
    config: LaneUpdaterConfig,
) -> Result<impl Future<Output = Result<()>>> {
    info!(
        server_url = %config.server_url,
        data_dir = %config.data_dir.display(),
        "Initializing lane updater ExEx"
    );

    let reload_client = ReloadClient::new(&config.server_url);
    
    if let Ok(healthy) = reload_client.health().await {
        if healthy {
            info!("PIR server is healthy");
        } else {
            warn!("PIR server health check failed");
        }
    }

    Ok(lane_updater_loop(ctx, config, reload_client))
}

/// Main processing loop for the lane updater
async fn lane_updater_loop<Node: FullNodeComponents>(
    mut ctx: ExExContext<Node>,
    config: LaneUpdaterConfig,
    reload_client: ReloadClient,
) -> Result<()> {
    let mut last_reload = std::time::Instant::now();

    while let Some(notification) = ctx.notifications.next().await {
        match &notification {
            ExExNotification::ChainCommitted { new } => {
                let tip = new.tip();
                let block_number = tip.number();
                
                info!(
                    block_number,
                    "Chain committed, checking for lane updates"
                );

                if last_reload.elapsed() >= config.reload_debounce {
                    match trigger_lane_update(&reload_client, block_number).await {
                        Ok(result) => {
                            info!(
                                old_block = ?result.old_block_number,
                                new_block = ?result.new_block_number,
                                duration_ms = result.reload_duration_ms,
                                "Lane databases reloaded"
                            );
                            last_reload = std::time::Instant::now();
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to reload lane databases");
                        }
                    }
                }

                if let Err(e) = ctx.events.send(ExExEvent::FinishedHeight(tip.num_hash())) {
                    error!(error = %e, "Failed to send finished height event");
                }
            }
            ExExNotification::ChainReverted { old } => {
                let block_range = old.range();
                warn!(
                    from = block_range.start,
                    to = block_range.end,
                    "Chain reverted - lane databases may need rebuild"
                );
            }
            ExExNotification::ChainReorged { old, new } => {
                let old_range = old.range();
                let new_range = new.range();
                warn!(
                    old_from = old_range.start,
                    old_to = old_range.end,
                    new_from = new_range.start,
                    new_to = new_range.end,
                    "Chain reorged - triggering lane rebuild"
                );

                match trigger_lane_update(&reload_client, new.tip().number()).await {
                    Ok(result) => {
                        info!(
                            new_block = ?result.new_block_number,
                            "Lane databases reloaded after reorg"
                        );
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to reload after reorg");
                    }
                }
            }
        }
    }

    Ok(())
}

/// Trigger a lane database update
async fn trigger_lane_update(
    client: &ReloadClient,
    _block_number: u64,
) -> anyhow::Result<crate::reload::ReloadResult> {
    client.reload().await
}
