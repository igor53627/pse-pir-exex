//! Gas backfill CLI for data-driven hot lane selection
//!
//! Run with:
//! ```bash
//! cargo run --bin lane-backfill --features backfill -- \
//!     --rpc-url http://localhost:8545 \
//!     --blocks 100000 \
//!     --output gas-rankings.json
//! ```

use std::path::PathBuf;

use clap::Parser;
use lane_builder::gas_tracker::{BackfillConfig, GasTracker};
use lane_builder::hybrid_scorer::{HybridScorer, HybridScorerConfig};

#[derive(Parser, Debug)]
#[command(name = "lane-backfill")]
#[command(about = "Backfill gas usage data for hot lane selection")]
struct Args {
    /// RPC URL for Ethereum node (archive node recommended)
    #[arg(long, default_value = "http://localhost:8545")]
    rpc_url: String,

    /// Number of blocks to backfill from head
    #[arg(long, default_value = "100000")]
    blocks: u64,

    /// Output file for gas rankings JSON
    #[arg(long, short, default_value = "gas-rankings.json")]
    output: PathBuf,

    /// Output file for scored contracts (hybrid ranking)
    #[arg(long, default_value = "hot-contracts.json")]
    scored_output: PathBuf,

    /// Batch size for parallel block fetching
    #[arg(long, default_value = "100")]
    batch_size: usize,

    /// Concurrency level for parallel requests
    #[arg(long, default_value = "10")]
    concurrency: usize,

    /// Number of top contracts to include in hot lane
    #[arg(long, default_value = "1000")]
    top_n: usize,

    /// Priority boost for known contracts (in gas units)
    #[arg(long, default_value = "100000000000")]
    known_boost: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = Args::parse();

    println!("Lane Backfill - Gas Guzzler Analysis");
    println!("=====================================");
    println!("RPC URL:     {}", args.rpc_url);
    println!("Blocks:      {}", args.blocks);
    println!("Batch size:  {}", args.batch_size);
    println!("Concurrency: {}", args.concurrency);
    println!();

    let config = BackfillConfig {
        rpc_url: args.rpc_url,
        block_count: args.blocks,
        batch_size: args.batch_size,
        concurrency: args.concurrency,
    };

    let tracker = GasTracker::new(config).await?;
    let result = tracker.backfill().await?;

    println!();
    println!("Backfill Results");
    println!("-----------------");
    println!("Blocks processed:    {}", result.blocks_processed);
    println!("Total transactions:  {}", result.total_transactions);
    println!("Unique contracts:    {}", result.unique_contracts);
    println!();

    result.save(&args.output)?;
    println!("Raw gas data saved to: {}", args.output.display());

    let scorer_config = HybridScorerConfig {
        known_contract_boost: args.known_boost,
        max_contracts: args.top_n,
        ..Default::default()
    };

    let scorer = HybridScorer::new(scorer_config);
    let scored = scorer.score_from_backfill(&result);

    let scored_json = serde_json::to_string_pretty(&scored)?;
    std::fs::write(&args.scored_output, scored_json)?;
    println!("Scored contracts saved to: {}", args.scored_output.display());

    println!();
    println!("Top 20 Gas Guzzlers (Hybrid Ranked)");
    println!("------------------------------------");
    for (i, contract) in scored.iter().take(20).enumerate() {
        let name = contract.name.as_deref().unwrap_or("Unknown");
        let category = contract.category.as_deref().unwrap_or("-");
        let source = match contract.source {
            lane_builder::hybrid_scorer::ContractSource::GasBackfill => "gas",
            lane_builder::hybrid_scorer::ContractSource::KnownList => "known",
            lane_builder::hybrid_scorer::ContractSource::Both => "both",
        };
        println!(
            "{:>3}. {} ({}) - score: {}, gas: {}, txs: {} [{}]",
            i + 1,
            name,
            category,
            contract.final_score,
            contract.gas_score,
            contract.tx_count,
            source,
        );
    }

    println!();
    println!("Use hot-contracts.json with lane-builder to generate the hot lane manifest.");

    Ok(())
}
