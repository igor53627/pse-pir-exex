//! Updater CLI binary
//!
//! Run with:
//! ```bash
//! cargo run -p inspire-updater --bin updater -- --rpc-url http://localhost:8545
//! ```

use clap::Parser;
use inspire_updater::{UpdaterConfig, UpdaterService};
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "inspire-updater")]
#[command(about = "Sync PIR database from ethrex node")]
struct Args {
    /// ethrex RPC URL
    #[arg(long, default_value = "http://localhost:8545")]
    rpc_url: String,

    /// ethrex admin RPC URL (for pir_* methods)
    #[arg(long)]
    admin_rpc_url: Option<String>,

    /// PIR server URL
    #[arg(long, default_value = "http://localhost:3000")]
    pir_server: String,

    /// Data directory for PIR shards
    #[arg(long, default_value = "./pir-data")]
    data_dir: PathBuf,

    /// Poll interval in milliseconds
    #[arg(long, default_value = "1000")]
    poll_interval_ms: u64,

    /// Just check connection and exit
    #[arg(long)]
    check: bool,

    /// Test pir_dumpStorage and print first N entries
    #[arg(long)]
    dump_test: Option<u64>,

    /// Test pir_getStateDelta for last N blocks
    #[arg(long)]
    delta_test: Option<u64>,

    /// Ethereum chain ID (default: 11155111 for Sepolia)
    #[arg(long, default_value = "11155111")]
    chain_id: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("inspire_updater=info".parse()?))
        .init();

    let args = Args::parse();

    let config = UpdaterConfig {
        rpc_url: args.rpc_url,
        admin_rpc_url: args.admin_rpc_url,
        pir_server_url: args.pir_server,
        data_dir: args.data_dir,
        poll_interval: Duration::from_millis(args.poll_interval_ms),
        max_blocks_per_fetch: 100,
        chain_id: args.chain_id,
    };

    if args.check {
        // Just check connection
        let rpc = inspire_updater::EthrexClient::new(&config.rpc_url, config.admin_rpc_url).await?;
        let block = rpc.block_number().await?;
        println!("[OK] Connected to ethrex at block {}", block);
        return Ok(());
    }

    if let Some(limit) = args.dump_test {
        // Test pir_dumpStorage
        let rpc = inspire_updater::EthrexClient::new(&config.rpc_url, config.admin_rpc_url.clone()).await?;
        let resp = rpc.pir_dump_storage(None, limit).await?;
        println!("[OK] pir_dumpStorage returned {} entries (has_more: {})", resp.entries.len(), resp.has_more);
        for entry in resp.entries.iter().take(5) {
            println!("  {} slot {} = {}", entry.address, entry.slot, entry.value);
        }
        if resp.entries.len() > 5 {
            println!("  ... and {} more", resp.entries.len() - 5);
        }
        return Ok(());
    }

    if let Some(blocks) = args.delta_test {
        // Test pir_getStateDelta
        let rpc = inspire_updater::EthrexClient::new(&config.rpc_url, config.admin_rpc_url.clone()).await?;
        let current = rpc.block_number().await?;
        let from = current.saturating_sub(blocks);
        println!("Fetching deltas from block {} to {}", from, current);
        let resp = rpc.pir_get_state_delta(from, current).await?;
        println!("[OK] pir_getStateDelta: {} total deltas across {} blocks", resp.total_deltas, resp.blocks.len());
        for block in resp.blocks.iter().take(3) {
            println!("  Block {}: {} deltas", block.block_number, block.deltas.len());
            for delta in block.deltas.iter().take(2) {
                println!("    {} slot {} = {}", delta.address, delta.slot, delta.value);
            }
        }
        if resp.blocks.len() > 3 {
            println!("  ... and {} more blocks", resp.blocks.len() - 3);
        }
        return Ok(());
    }

    let mut service = UpdaterService::new(config).await?;
    service.run().await
}
