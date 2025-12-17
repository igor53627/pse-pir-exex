//! Reth ExEx integration for real-time lane updates
//!
//! This module provides an Execution Extension (ExEx) that hooks into Reth's
//! block processing pipeline to update PIR lane databases in real-time.
//!
//! Enable with the `exex` feature:
//! ```toml
//! lane-builder = { path = "...", features = ["exex"] }
//! ```
//!
//! ## Metrics
//!
//! The following metrics are exposed:
//! - `lane_updater_reload_total`: Total number of reload requests
//! - `lane_updater_reload_duration_ms`: Reload latency histogram
//! - `lane_updater_reload_errors_total`: Total reload errors
//! - `lane_updater_blocks_processed`: Total blocks processed
//! - `lane_updater_reorgs_total`: Total chain reorgs detected
//! - `lane_updater_reverts_total`: Total chain reverts detected
//! - `lane_updater_debounce_skips_total`: Reloads skipped due to debouncing

#![cfg(feature = "exex")]

use std::future::Future;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eyre::Result;
use futures::TryStreamExt;
use metrics::{counter, histogram};
use reth_ethereum::exex::{ExExContext, ExExEvent, ExExNotification};
use reth_ethereum::node::api::FullNodeComponents;
use tracing::{info, warn, error};

use crate::reload::ReloadClient;

const METRIC_RELOAD_TOTAL: &str = "lane_updater_reload_total";
const METRIC_RELOAD_DURATION_MS: &str = "lane_updater_reload_duration_ms";
const METRIC_RELOAD_ERRORS: &str = "lane_updater_reload_errors_total";
const METRIC_BLOCKS_PROCESSED: &str = "lane_updater_blocks_processed";
const METRIC_REORGS: &str = "lane_updater_reorgs_total";
const METRIC_REVERTS: &str = "lane_updater_reverts_total";
const METRIC_DEBOUNCE_SKIPS: &str = "lane_updater_debounce_skips_total";

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
    
    match reload_client.health().await {
        Ok(true) => info!("PIR server is healthy"),
        Ok(false) => warn!("PIR server health check failed"),
        Err(e) => warn!(error = %e, "PIR server health check error - server may be unavailable"),
    }

    Ok(lane_updater_loop(ctx, config, reload_client))
}

/// Main processing loop for the lane updater
async fn lane_updater_loop<Node: FullNodeComponents>(
    mut ctx: ExExContext<Node>,
    config: LaneUpdaterConfig,
    reload_client: ReloadClient,
) -> Result<()> {
    let mut last_reload = Instant::now();

    while let Some(notification) = ctx.notifications.try_next().await? {
        match &notification {
            ExExNotification::ChainCommitted { new } => {
                let committed_range = new.range();
                
                counter!(METRIC_BLOCKS_PROCESSED).increment(1);
                
                info!(
                    chain = ?committed_range,
                    "Chain committed, checking for lane updates"
                );

                if last_reload.elapsed() >= config.reload_debounce {
                    let start = Instant::now();
                    match trigger_lane_update(&reload_client).await {
                        Ok(result) => {
                            let latency_ms = start.elapsed().as_millis() as f64;
                            counter!(METRIC_RELOAD_TOTAL).increment(1);
                            histogram!(METRIC_RELOAD_DURATION_MS).record(latency_ms);
                            
                            info!(
                                old_block = ?result.old_block_number,
                                new_block = ?result.new_block_number,
                                duration_ms = result.reload_duration_ms,
                                client_latency_ms = %latency_ms,
                                "Lane databases reloaded"
                            );
                            last_reload = Instant::now();
                        }
                        Err(e) => {
                            counter!(METRIC_RELOAD_ERRORS).increment(1);
                            warn!(error = %e, "Failed to reload lane databases");
                        }
                    }
                } else {
                    counter!(METRIC_DEBOUNCE_SKIPS).increment(1);
                }

                ctx.events.send(ExExEvent::FinishedHeight(new.tip().num_hash()))?;
            }
            ExExNotification::ChainReverted { old } => {
                counter!(METRIC_REVERTS).increment(1);
                
                warn!(
                    reverted_chain = ?old.range(),
                    "Chain reverted - triggering lane rebuild"
                );

                let start = Instant::now();
                match trigger_lane_update(&reload_client).await {
                    Ok(result) => {
                        let latency_ms = start.elapsed().as_millis() as f64;
                        counter!(METRIC_RELOAD_TOTAL).increment(1);
                        histogram!(METRIC_RELOAD_DURATION_MS).record(latency_ms);
                        
                        info!(
                            new_block = ?result.new_block_number,
                            duration_ms = %latency_ms,
                            "Lane databases reloaded after revert"
                        );
                        last_reload = Instant::now();
                    }
                    Err(e) => {
                        counter!(METRIC_RELOAD_ERRORS).increment(1);
                        error!(error = %e, "Failed to reload after revert");
                    }
                }
            }
            ExExNotification::ChainReorged { old, new } => {
                counter!(METRIC_REORGS).increment(1);
                
                warn!(
                    from_chain = ?old.range(),
                    to_chain = ?new.range(),
                    "Chain reorged - triggering lane rebuild"
                );

                let start = Instant::now();
                match trigger_lane_update(&reload_client).await {
                    Ok(result) => {
                        let latency_ms = start.elapsed().as_millis() as f64;
                        counter!(METRIC_RELOAD_TOTAL).increment(1);
                        histogram!(METRIC_RELOAD_DURATION_MS).record(latency_ms);
                        
                        info!(
                            new_block = ?result.new_block_number,
                            duration_ms = %latency_ms,
                            "Lane databases reloaded after reorg"
                        );
                        last_reload = Instant::now();
                    }
                    Err(e) => {
                        counter!(METRIC_RELOAD_ERRORS).increment(1);
                        error!(error = %e, "Failed to reload after reorg");
                    }
                }

                ctx.events.send(ExExEvent::FinishedHeight(new.tip().num_hash()))?;
            }
        }
    }

    Ok(())
}

/// Trigger a lane database update
async fn trigger_lane_update(
    client: &ReloadClient,
) -> anyhow::Result<crate::reload::ReloadResult> {
    client.reload().await
}
