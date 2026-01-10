//! stem-index: Generate stem index from state.bin
//!
//! Reads a state.bin file (84-byte records with EIP-7864 tree_index) and
//! generates a stem index for O(log N) PIR lookups.
//!
//! Usage:
//!   stem-index --input state.bin --output stem-index.bin
//!
//! Output format:
//!   count:8 (LE u64) + (stem:31 + offset:8 (LE u64))*
//!
//! The stem index maps each unique stem to its starting offset in the PIR database.
//! Entries must be sorted by tree_key (stem || subindex) in the input state.bin.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;

use clap::Parser;
use inspire_core::state_format::{StateHeader, STATE_ENTRY_SIZE, STATE_HEADER_SIZE};
use inspire_core::ubt::{compute_stem, Stem};

#[derive(Parser)]
#[command(about = "Generate stem index from state.bin")]
struct Args {
    /// Input state.bin file
    #[arg(long)]
    input: PathBuf,

    /// Output stem-index.bin file
    #[arg(long)]
    output: PathBuf,

    /// Verify the stem index after generation
    #[arg(long)]
    verify: bool,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = Args::parse();

    tracing::info!(
        input = %args.input.display(),
        output = %args.output.display(),
        "Generating stem index"
    );

    // Read header
    let file = File::open(&args.input)?;
    let mut reader = BufReader::new(file);
    let mut header_buf = [0u8; STATE_HEADER_SIZE];
    reader.read_exact(&mut header_buf)?;

    let header = StateHeader::from_bytes(&header_buf)
        .map_err(|e| anyhow::anyhow!("Invalid header: {}", e))?;

    tracing::info!(
        entries = header.entry_count,
        block = header.block_number,
        "State file: {} entries",
        header.entry_count
    );

    // Build stem -> first_offset map
    // Since entries are sorted by tree_key, we just need to track first occurrence
    let mut stem_offsets: BTreeMap<Stem, u64> = BTreeMap::new();
    let mut entry_buf = [0u8; STATE_ENTRY_SIZE];
    let mut current_offset: u64 = 0;

    for i in 0..header.entry_count {
        reader.read_exact(&mut entry_buf)?;

        // Entry format: address:20 + tree_index:32 + value:32
        let address: [u8; 20] = entry_buf[0..20].try_into().unwrap();
        let tree_index: [u8; 32] = entry_buf[20..52].try_into().unwrap();

        let stem = compute_stem(&address, &tree_index);

        // Only record first occurrence of each stem
        stem_offsets.entry(stem).or_insert(current_offset);
        current_offset += 1;

        if (i + 1) % 1_000_000 == 0 {
            tracing::info!(
                progress = format!("{:.1}%", (i + 1) as f64 / header.entry_count as f64 * 100.0),
                stems = stem_offsets.len(),
                "Processing entries"
            );
        }
    }

    tracing::info!(
        stems = stem_offsets.len(),
        entries = header.entry_count,
        "Found {} unique stems",
        stem_offsets.len()
    );

    // Write stem index
    let out_file = File::create(&args.output)?;
    let mut writer = BufWriter::new(out_file);

    // Header: count as u64 LE
    let count = stem_offsets.len() as u64;
    writer.write_all(&count.to_le_bytes())?;

    // Entries: stem:31 + offset:8, sorted by stem (BTreeMap maintains order)
    for (stem, offset) in &stem_offsets {
        writer.write_all(stem)?;
        writer.write_all(&offset.to_le_bytes())?;
    }

    writer.flush()?;

    let file_size = std::fs::metadata(&args.output)?.len();
    tracing::info!(
        output = %args.output.display(),
        size_kb = file_size / 1024,
        stems = count,
        "Stem index written: {} KB",
        file_size / 1024
    );

    // Verify if requested
    if args.verify {
        tracing::info!("Verifying stem index...");
        let verify_data = std::fs::read(&args.output)?;

        let verify_count = u64::from_le_bytes(verify_data[0..8].try_into().unwrap()) as usize;
        assert_eq!(verify_count, stem_offsets.len(), "Count mismatch");

        let mut offset = 8;
        let mut prev_stem: Option<Stem> = None;
        for (expected_stem, expected_offset) in &stem_offsets {
            let stem: Stem = verify_data[offset..offset + 31].try_into().unwrap();
            let file_offset =
                u64::from_le_bytes(verify_data[offset + 31..offset + 39].try_into().unwrap());

            assert_eq!(&stem, expected_stem, "Stem mismatch");
            assert_eq!(file_offset, *expected_offset, "Offset mismatch");

            // Verify sorted order
            if let Some(prev) = prev_stem {
                assert!(stem > prev, "Stems not sorted");
            }
            prev_stem = Some(stem);

            offset += 39;
        }

        tracing::info!("Verification passed!");
    }

    println!("\nStem index generated:");
    println!("  Stems: {}", count);
    println!("  Size: {} KB", file_size / 1024);
    println!("  Output: {}", args.output.display());

    Ok(())
}
