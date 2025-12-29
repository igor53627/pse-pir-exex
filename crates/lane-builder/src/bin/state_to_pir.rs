//! state-to-pir: Convert state.bin to PIR database format
//!
//! Reads a state.bin file (84-byte records) and creates a two-lane PIR database
//! using TwoLaneSetup. Since we don't have hot/cold lane separation yet, we put
//! all entries in the "hot" lane and leave cold empty.
//!
//! Usage:
//!   state-to-pir --input state.bin --output ./pir-data
//!
//! Output structure:
//!   pir-data/
//!     hot/
//!       crs.json          # Common Reference String
//!       encoded.json      # Encoded PIR database
//!       crs.meta.json     # CRS metadata
//!     cold/
//!       crs.json
//!       encoded.json
//!       crs.meta.json
//!     config.json         # TwoLaneConfig

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;

use clap::Parser;
use inspire_core::state_format::{StateHeader, STATE_ENTRY_SIZE, STATE_HEADER_SIZE};
use lane_builder::{default_params, TwoLaneSetup};

#[derive(Parser)]
#[command(about = "Convert state.bin to PIR database format")]
struct Args {
    /// Input state.bin file
    #[arg(long)]
    input: PathBuf,

    /// Output directory for PIR data
    #[arg(long)]
    output: PathBuf,

    /// Entry size for PIR (32 = values only, 84 = full records)
    #[arg(long, default_value = "32")]
    entry_size: usize,

    /// Maximum entries to encode (0 = all)
    #[arg(long, default_value = "0")]
    max_entries: usize,

    /// Use test parameters (smaller, faster)
    #[arg(long)]
    test_params: bool,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = Args::parse();

    tracing::info!(
        input = %args.input.display(),
        output = %args.output.display(),
        "Reading state.bin"
    );

    // Read and validate header
    let file = File::open(&args.input)?;
    let mut reader = BufReader::new(file);
    let mut header_buf = [0u8; STATE_HEADER_SIZE];
    reader.read_exact(&mut header_buf)?;

    let header = StateHeader::from_bytes(&header_buf)
        .map_err(|e| anyhow::anyhow!("Invalid header: {}", e))?;

    tracing::info!(
        entries = header.entry_count,
        block = header.block_number,
        chain_id = header.chain_id,
        "State file parsed"
    );

    // Determine how many entries to process
    let entry_count = if args.max_entries > 0 {
        std::cmp::min(args.max_entries, header.entry_count as usize)
    } else {
        header.entry_count as usize
    };

    tracing::info!(entries = entry_count, "Processing entries");

    // Read entries and extract values
    // For PIR, we use the 32-byte value from each 84-byte entry
    let pir_entry_size = args.entry_size;
    let mut hot_data = Vec::with_capacity(entry_count * pir_entry_size);

    let mut entry_buf = [0u8; STATE_ENTRY_SIZE];
    for i in 0..entry_count {
        reader.read_exact(&mut entry_buf)?;

        if pir_entry_size == 32 {
            // Just the value (last 32 bytes of 84-byte entry)
            hot_data.extend_from_slice(&entry_buf[52..84]);
        } else {
            // Full entry
            hot_data.extend_from_slice(&entry_buf[..pir_entry_size]);
        }

        if (i + 1) % 1_000_000 == 0 {
            tracing::info!(
                progress = format!("{:.1}%", (i + 1) as f64 / entry_count as f64 * 100.0),
                entries = i + 1,
                "Reading entries"
            );
        }
    }

    tracing::info!(
        hot_size_mb = hot_data.len() / (1024 * 1024),
        entries = entry_count,
        "Data loaded, starting PIR setup"
    );

    // Cold lane is empty (single-lane mode for now)
    let cold_data: Vec<u8> = vec![0u8; pir_entry_size]; // At least 1 entry

    // Run PIR setup
    let params = if args.test_params {
        tracing::warn!("Using test parameters (not secure for production)");
        lane_builder::test_params()
    } else {
        default_params()
    };

    tracing::info!(
        ring_dim = params.ring_dim,
        sigma = params.sigma,
        "Running TwoLaneSetup with PIR parameters"
    );

    let result = TwoLaneSetup::new(&args.output)
        .hot_data(hot_data)
        .cold_data(cold_data)
        .entry_size(pir_entry_size)
        .params(params)
        .build()?;

    tracing::info!(
        hot_crs = %args.output.join("hot/crs.json").display(),
        hot_db = %args.output.join("hot/encoded.json").display(),
        cold_crs = %args.output.join("cold/crs.json").display(),
        config = %args.output.join("config.json").display(),
        "PIR database created"
    );

    println!("\nPIR database ready:");
    println!("  Hot entries: {}", result.config.hot_entries);
    println!("  Cold entries: {}", result.config.cold_entries);
    println!("  Entry size: {} bytes", pir_entry_size);
    println!("  Output: {}", args.output.display());
    println!("\nTo start server:");
    println!("  inspire-server {}/config.json", args.output.display());

    Ok(())
}
