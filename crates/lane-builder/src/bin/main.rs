//! lane-builder binary: Build hot lane manifest

use std::path::PathBuf;

use lane_builder::HotLaneBuilder;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args: Vec<String> = std::env::args().collect();
    
    let output_dir = args.get(1).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("./pir-data/hot")
    });
    
    let block_number: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

    tracing::info!(
        output = %output_dir.display(),
        block = block_number,
        "Building hot lane manifest"
    );

    let manifest = HotLaneBuilder::new(&output_dir)
        .at_block(block_number)
        .load_known_contracts()
        .max_contracts(1000)
        .max_entries(1_000_000)
        .build()?;

    println!("Hot lane manifest built:");
    println!("  Contracts: {}", manifest.contract_count());
    println!("  Total entries: {}", manifest.total_entries);
    println!("  Block: {}", manifest.block_number);

    Ok(())
}
