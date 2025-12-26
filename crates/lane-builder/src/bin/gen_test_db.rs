//! Generate test STATE_FORMAT database for e2e testing
//!
//! Creates a small state.bin file with deterministic test data
//! that can be used for testing inspire-setup and full PIR pipeline.
//!
//! Usage:
//!   cargo run --bin gen-test-db -- --entries 1000 --output test-state.bin

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use clap::Parser;
use eyre::Result;
use inspire_core::bucket_index::compute_bucket_id;
use inspire_core::state_format::{StateHeader, StorageEntry, STATE_ENTRY_SIZE, STATE_HEADER_SIZE};
use tiny_keccak::{Hasher, Keccak};

#[derive(Parser)]
#[command(name = "gen-test-db")]
#[command(about = "Generate test STATE_FORMAT database")]
struct Args {
    /// Number of entries to generate
    #[arg(long, default_value = "1000")]
    entries: u64,

    /// Output file path
    #[arg(long, default_value = "test-state.bin")]
    output: PathBuf,

    /// Block number for header
    #[arg(long, default_value = "20000000")]
    block: u64,

    /// Chain ID for header
    #[arg(long, default_value = "1")]
    chain_id: u64,

    /// Number of unique contracts
    #[arg(long, default_value = "100")]
    contracts: u64,

    /// Sort by bucket ID (required for bucket index)
    #[arg(long, default_value = "true")]
    sort: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Generating test database:");
    println!("  Entries: {}", args.entries);
    println!("  Contracts: {}", args.contracts);
    println!("  Block: {}", args.block);
    println!("  Chain ID: {}", args.chain_id);
    println!("  Output: {}", args.output.display());

    // Generate deterministic entries
    let mut entries: Vec<StorageEntry> = Vec::with_capacity(args.entries as usize);

    for i in 0..args.entries {
        // Deterministic address based on contract index
        let contract_idx = i % args.contracts;
        let mut address = [0u8; 20];
        address[0..8].copy_from_slice(&contract_idx.to_le_bytes());
        // Add some variety to addresses
        let mut hasher = Keccak::v256();
        hasher.update(&address);
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        address.copy_from_slice(&hash[0..20]);

        // Deterministic slot based on entry index
        let slot_idx = i / args.contracts;
        let mut slot = [0u8; 32];
        slot[0..8].copy_from_slice(&slot_idx.to_le_bytes());
        slot[24..32].copy_from_slice(&i.to_le_bytes());

        // Deterministic value
        let mut value = [0u8; 32];
        value[0..8].copy_from_slice(&(i * 1000).to_le_bytes());
        // Add pattern to make values recognizable
        value[31] = 0xff;

        entries.push(StorageEntry::new(address, slot, value));
    }

    // Sort by bucket ID for bucket index compatibility
    if args.sort {
        entries.sort_by_key(|e| compute_bucket_id(&e.address, &e.slot));
        println!("  Sorted by bucket ID");
    }

    // Create block hash from block number
    let mut block_hash = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(b"test-block-");
    hasher.update(&args.block.to_le_bytes());
    hasher.finalize(&mut block_hash);

    // Write file
    let file = File::create(&args.output)?;
    let mut writer = BufWriter::new(file);

    // Write header
    let header = StateHeader::new(args.entries, args.block, args.chain_id, block_hash);
    writer.write_all(&header.to_bytes())?;

    // Write entries
    for entry in &entries {
        writer.write_all(&entry.to_bytes())?;
    }

    writer.flush()?;

    let file_size = STATE_HEADER_SIZE + (args.entries as usize * STATE_ENTRY_SIZE);
    println!("\nGenerated {} bytes ({} header + {} entries)", 
             file_size, STATE_HEADER_SIZE, args.entries);

    // Verify by reading back
    let data = std::fs::read(&args.output)?;
    let recovered_header = StateHeader::from_bytes(&data)?;
    println!("\nVerification:");
    println!("  Magic: {:?}", std::str::from_utf8(&recovered_header.magic).unwrap_or("?"));
    println!("  Version: {}", recovered_header.version);
    println!("  Entry count: {}", recovered_header.entry_count);
    println!("  Block: {}", recovered_header.block_number);
    println!("  Chain ID: {}", recovered_header.chain_id);

    Ok(())
}
