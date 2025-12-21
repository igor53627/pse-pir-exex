//! Balance database builder binary
//!
//! Builds ETH/USDC balance hot lane database from a list of addresses.

use std::path::PathBuf;

use alloy_primitives::Address;
use alloy_provider::ProviderBuilder;
use clap::{Parser, ValueEnum};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use lane_builder::balance_extractor::{BalanceExtractor, BalanceExtractorConfig, load_addresses_from_file};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Network {
    Mainnet,
    Holesky,
    Sepolia,
}

#[derive(Parser, Debug)]
#[command(name = "balance-builder")]
#[command(about = "Build ETH/USDC balance hot lane database")]
struct Args {
    #[arg(long)]
    rpc_url: String,

    #[arg(long)]
    block: u64,

    #[arg(long)]
    block_hash: String,

    #[arg(long, default_value = "mainnet")]
    network: Network,

    #[arg(long)]
    addresses_file: Option<PathBuf>,

    #[arg(long, default_value = "./balance_db")]
    output_dir: PathBuf,

    #[arg(long, default_value = "100")]
    batch_size: usize,

    #[arg(long, default_value = "10")]
    max_concurrent: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("lane_builder=info".parse()?))
        .init();

    let args = Args::parse();

    let mut config = match args.network {
        Network::Mainnet => BalanceExtractorConfig::default(),
        Network::Holesky => BalanceExtractorConfig::holesky(),
        Network::Sepolia => BalanceExtractorConfig::sepolia(),
    };
    config.batch_size = args.batch_size;
    config.max_concurrent = args.max_concurrent;

    tracing::info!(
        network = ?args.network,
        block = args.block,
        usdc = %config.usdc_contract,
        "Starting balance extraction"
    );

    let addresses: Vec<Address> = if let Some(path) = &args.addresses_file {
        tracing::info!(path = %path.display(), "Loading addresses from file");
        load_addresses_from_file(path)?
    } else {
        tracing::info!("Using default hot addresses (demo mode)");
        lane_builder::balance_extractor::default_hot_addresses()
    };

    tracing::info!(count = addresses.len(), "Loaded addresses");

    let provider = ProviderBuilder::new().connect_http(args.rpc_url.parse()?);

    let extractor = BalanceExtractor::new(provider, config);

    let metadata = extractor
        .build_database(&addresses, args.block, &args.block_hash, &args.output_dir)
        .await?;

    tracing::info!(
        records = metadata.num_records,
        chain_id = metadata.chain_id,
        block = metadata.snapshot_block,
        "Balance database complete"
    );

    Ok(())
}
