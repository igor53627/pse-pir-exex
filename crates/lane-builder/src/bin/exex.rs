//! ExEx binary: Run the lane updater as a Reth Execution Extension
//!
//! This binary hooks into Reth's block processing pipeline to update
//! PIR lane databases in real-time.
//!
//! Usage:
//!   cargo run --bin lane-exex --features exex -- node \
//!     --pir-server-url http://localhost:3000 \
//!     --pir-data-dir ./pir-data
//!
//! The ExEx will:
//! 1. Subscribe to Reth chain notifications
//! 2. On ChainCommitted: trigger /admin/reload on PIR server
//! 3. On ChainReorged: force immediate reload
//! 4. Debounce rapid updates (configurable)

#![cfg(feature = "exex")]

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use eyre::Result;
use reth_ethereum::cli::Cli;
use reth_ethereum::node::EthereumNode;
use reth_tracing::RethTracer;
use reth_tracing::Tracer;

use lane_builder::{lane_updater_exex, LaneUpdaterConfig};

#[derive(Debug, Clone, Parser)]
#[command(name = "lane-exex", about = "Lane updater ExEx for Reth")]
struct LaneExExArgs {
    /// URL of the PIR server to notify on updates
    #[arg(long, env = "PIR_SERVER_URL", default_value = "http://localhost:3000")]
    pir_server_url: String,

    /// Directory containing lane databases
    #[arg(long, env = "PIR_DATA_DIR", default_value = "./pir-data")]
    pir_data_dir: PathBuf,

    /// Minimum interval between reloads in seconds (debounce)
    #[arg(long, env = "RELOAD_DEBOUNCE_SECS", default_value = "1")]
    reload_debounce_secs: u64,
}

fn main() -> Result<()> {
    let _guard = RethTracer::new().init()?;

    Cli::parse_args().run(|builder, _args| {
        let exex_args = LaneExExArgs::parse_from(
            std::env::args().filter(|arg| {
                arg.starts_with("--pir") || arg.starts_with("--reload") || !arg.starts_with("--")
            })
        );

        let config = LaneUpdaterConfig {
            server_url: exex_args.pir_server_url,
            data_dir: exex_args.pir_data_dir,
            reload_debounce: Duration::from_secs(exex_args.reload_debounce_secs),
        };

        tracing::info!(
            server_url = %config.server_url,
            data_dir = %config.data_dir.display(),
            debounce_secs = config.reload_debounce.as_secs(),
            "Starting lane updater ExEx"
        );

        Box::pin(async move {
            let handle = builder
                .node(EthereumNode::default())
                .install_exex("lane-updater", move |ctx| {
                    let config = config.clone();
                    async move { lane_updater_exex(ctx, config).await }
                })
                .launch()
                .await?;

            handle.wait_for_node_exit().await
        })
    })
}
