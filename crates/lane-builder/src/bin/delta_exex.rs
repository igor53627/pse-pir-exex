//! delta-exex: Run the delta exporter as a Reth Execution Extension
//!
//! Usage:
//!   cargo run --bin delta-exex --features exex -- node \
//!     --delta-output-dir ./pir-data/delta \
//!     --delta-keep-blocks 256

#![cfg(feature = "exex")]

use std::path::PathBuf;

use clap::Parser;
use eyre::Result;
use reth_ethereum::cli::Cli;
use reth_ethereum::node::EthereumNode;
use reth_tracing::RethTracer;
use reth_tracing::Tracer;

use lane_builder::{delta_export_exex, DeltaExporterConfig};

#[derive(Debug, Clone, Parser)]
#[command(name = "delta-exex", about = "Delta exporter ExEx for Reth")]
struct DeltaExExArgs {
    /// Directory to write delta state.bin files
    #[arg(long, env = "DELTA_OUTPUT_DIR", default_value = "./pir-data/delta")]
    output_dir: PathBuf,

    /// Number of recent blocks to keep (0 = keep all)
    #[arg(long, env = "DELTA_KEEP_BLOCKS", default_value = "256")]
    keep_blocks: u64,
}

fn main() -> Result<()> {
    let _guard = RethTracer::new().init()?;

    Cli::parse_args().run(|builder, _args| {
        let exex_args = DeltaExExArgs::parse_from(std::env::args().filter(|arg| {
            arg.starts_with("--delta") || !arg.starts_with("--")
        }));

        let config = DeltaExporterConfig {
            output_dir: exex_args.output_dir,
            keep_blocks: exex_args.keep_blocks,
        };

        tracing::info!(
            output_dir = %config.output_dir.display(),
            keep_blocks = config.keep_blocks,
            "Starting delta exporter ExEx"
        );

        Box::pin(async move {
            let handle = builder
                .node(EthereumNode::default())
                .install_exex("delta-exporter", move |ctx| {
                    let config = config.clone();
                    async move { delta_export_exex(ctx, config).await }
                })
                .launch()
                .await?;

            handle.wait_for_node_exit().await
        })
    })
}

