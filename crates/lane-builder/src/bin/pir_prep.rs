//! PIR database preparation binary
//!
//! Extracts Ethereum storage state from reth MDBX and converts to plinko-compatible format:
//! - database.bin: flat 32-byte storage values
//! - storage-mapping.bin: sorted (address:20 + slot:32 + index:4) entries
//!
//! Usage:
//!   cargo run --bin pir-prep --features state-dump -- \
//!     --db-path /mnt/sepolia/data/db \
//!     --output-dir ./pir-data

#![cfg(feature = "state-dump")]

use std::ffi::CString;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::ptr;

use clap::Parser;
use eyre::Result;
use indicatif::{ProgressBar, ProgressStyle};
use mdbx_rs::{MDBX_cursor_op::*, *};
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(name = "pir-prep")]
#[command(about = "Prepare PIR database from reth MDBX (plinko-compatible format)")]
struct Args {
    /// Path to reth MDBX database directory (containing mdbx.dat)
    #[arg(long)]
    db_path: PathBuf,

    /// Output directory for PIR database files
    #[arg(long, default_value = "./pir-data")]
    output_dir: PathBuf,

    /// Chain name (for metadata)
    #[arg(long, default_value = "sepolia")]
    chain: String,

    /// Log progress every N records
    #[arg(long, default_value = "1000000")]
    progress_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PirMetadata {
    chain: String,
    num_storage_slots: u64,
    entry_size: usize,
    mapping_entry_size: usize,
    format_version: String,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    std::fs::create_dir_all(&args.output_dir)?;

    tracing::info!(
        db_path = %args.db_path.display(),
        output_dir = %args.output_dir.display(),
        chain = %args.chain,
        "Starting PIR database preparation"
    );

    let num_storage_slots = unsafe { extract_storage_for_pir(&args)? };

    let metadata = PirMetadata {
        chain: args.chain.clone(),
        num_storage_slots,
        entry_size: 32,
        mapping_entry_size: 56,
        format_version: "1.0.0".to_string(),
    };

    let metadata_path = args.output_dir.join("metadata.json");
    let metadata_json = serde_json::to_string_pretty(&metadata)?;
    std::fs::write(&metadata_path, &metadata_json)?;

    tracing::info!(
        storage_slots = num_storage_slots,
        metadata = %metadata_path.display(),
        "PIR database preparation complete"
    );

    Ok(())
}

unsafe fn extract_storage_for_pir(args: &Args) -> Result<u64> {
    let mut env: *mut MDBX_env = ptr::null_mut();
    let rc = mdbx_env_create(&mut env);
    if rc != MDBX_SUCCESS {
        return Err(eyre::eyre!("Failed to create MDBX environment: {}", rc));
    }

    let rc = mdbx_env_set_maxdbs(env, 64);
    if rc != MDBX_SUCCESS {
        mdbx_env_close(env);
        return Err(eyre::eyre!("Failed to set maxdbs: {}", rc));
    }

    let db_path_str = args.db_path.to_string_lossy();
    let path_cstr = CString::new(db_path_str.as_ref())?;

    tracing::info!("Opening MDBX database at {}", db_path_str);

    let rc = mdbx_env_open(env, path_cstr.as_ptr(), MDBX_RDONLY as u32, 0o644);
    if rc != MDBX_SUCCESS {
        mdbx_env_close(env);
        return Err(eyre::eyre!("Failed to open MDBX environment: {}", rc));
    }

    tracing::info!("MDBX environment opened successfully");

    let mut txn: *mut MDBX_txn = ptr::null_mut();
    let rc = mdbx_txn_begin(env, ptr::null_mut(), MDBX_RDONLY as u32, &mut txn);
    if rc != MDBX_SUCCESS {
        mdbx_env_close(env);
        return Err(eyre::eyre!("Failed to begin transaction: {}", rc));
    }

    let table_cstr = CString::new("PlainStorageState")?;
    let mut dbi: MDBX_dbi = 0;
    let rc = mdbx_dbi_open(txn, table_cstr.as_ptr(), 0, &mut dbi);
    if rc != MDBX_SUCCESS {
        mdbx_txn_abort(txn);
        mdbx_env_close(env);
        return Err(eyre::eyre!("Failed to open PlainStorageState: {}", rc));
    }

    tracing::info!("Opened PlainStorageState table");

    let mut cursor: *mut MDBX_cursor = ptr::null_mut();
    let rc = mdbx_cursor_open(txn, dbi, &mut cursor);
    if rc != MDBX_SUCCESS {
        mdbx_txn_abort(txn);
        mdbx_env_close(env);
        return Err(eyre::eyre!("Failed to open cursor: {}", rc));
    }

    let database_path = args.output_dir.join("database.bin");
    let mapping_path = args.output_dir.join("storage-mapping.bin");

    let mut db_writer = BufWriter::with_capacity(64 * 1024 * 1024, File::create(&database_path)?);
    let mut map_writer = BufWriter::with_capacity(64 * 1024 * 1024, File::create(&mapping_path)?);

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("[{elapsed_precise}] {spinner} {msg}")
            .unwrap(),
    );

    let mut key = MDBX_val::default();
    let mut val = MDBX_val::default();
    let mut count = 0u64;
    let mut skipped = 0u64;

    let mut rc = mdbx_cursor_get(cursor, &mut key, &mut val, MDBX_FIRST as MDBX_cursor_op);

    while rc == MDBX_SUCCESS {
        let key_bytes = std::slice::from_raw_parts(key.iov_base as *const u8, key.iov_len);
        let val_bytes = std::slice::from_raw_parts(val.iov_base as *const u8, val.iov_len);

        // PlainStorageState format:
        // - key: 20-byte address
        // - value: variable-length encoded (slot + storage_value)
        //
        // In reth with DUPSORT, the value contains the slot as the dupsort key
        // and the storage value as the data. The exact format depends on reth version.
        //
        // For modern reth (post-1.0), the value is:
        // - First 32 bytes: storage slot (B256)
        // - Remaining bytes: RLP-encoded U256 storage value

        if key_bytes.len() != 20 {
            skipped += 1;
            rc = mdbx_cursor_get(cursor, &mut key, &mut val, MDBX_NEXT as MDBX_cursor_op);
            continue;
        }

        // Value should be at least 32 bytes (slot) + 1 byte (minimal RLP)
        if val_bytes.len() < 33 {
            skipped += 1;
            rc = mdbx_cursor_get(cursor, &mut key, &mut val, MDBX_NEXT as MDBX_cursor_op);
            continue;
        }

        let address = key_bytes;
        let slot = &val_bytes[0..32];

        // Decode the storage value from RLP
        // The value is an RLP-encoded U256. For simplicity, we'll handle common cases:
        // - Single byte 0x00-0x7f: value is the byte itself
        // - 0x80: empty value (0)
        // - 0x81-0xb7: short string (1-55 bytes)
        let storage_value = decode_rlp_u256(&val_bytes[32..])?;

        // Write 32-byte storage value to database.bin
        db_writer.write_all(&storage_value)?;

        // Write mapping entry: address(20) + slot(32) + index(4 LE)
        map_writer.write_all(address)?;
        map_writer.write_all(slot)?;
        map_writer.write_all(&(count as u32).to_le_bytes())?;

        count += 1;

        if count % args.progress_interval == 0 {
            pb.set_message(format!(
                "PlainStorageState: {} entries (skipped: {})",
                count, skipped
            ));
            db_writer.flush()?;
            map_writer.flush()?;
        }

        rc = mdbx_cursor_get(cursor, &mut key, &mut val, MDBX_NEXT as MDBX_cursor_op);
    }

    if rc != MDBX_NOTFOUND {
        mdbx_cursor_close(cursor);
        mdbx_txn_abort(txn);
        mdbx_env_close(env);
        return Err(eyre::eyre!("Cursor error: {}", rc));
    }

    db_writer.flush()?;
    map_writer.flush()?;
    mdbx_cursor_close(cursor);
    mdbx_txn_abort(txn);
    mdbx_env_close(env);

    pb.finish_with_message(format!(
        "PlainStorageState: {} entries complete (skipped: {})",
        count, skipped
    ));

    tracing::info!(
        count,
        skipped,
        database = %database_path.display(),
        mapping = %mapping_path.display(),
        "Storage extraction complete"
    );

    Ok(count)
}

/// Decode RLP-encoded U256 to 32-byte big-endian array
fn decode_rlp_u256(data: &[u8]) -> Result<[u8; 32]> {
    if data.is_empty() {
        return Ok([0u8; 32]);
    }

    let first = data[0];
    let mut result = [0u8; 32];

    if first == 0x80 {
        // Empty string = 0
        return Ok(result);
    }

    if first < 0x80 {
        // Single byte value
        result[31] = first;
        return Ok(result);
    }

    if first <= 0xb7 {
        // Short string: length is (first - 0x80)
        let len = (first - 0x80) as usize;
        if data.len() < 1 + len {
            return Err(eyre::eyre!("RLP truncated: expected {} bytes", len));
        }
        if len > 32 {
            return Err(eyre::eyre!("RLP value too large: {} bytes", len));
        }
        // Copy to right-aligned position in result
        let start = 32 - len;
        result[start..].copy_from_slice(&data[1..1 + len]);
        return Ok(result);
    }

    if first <= 0xbf {
        // Long string: next (first - 0xb7) bytes are the length
        let len_of_len = (first - 0xb7) as usize;
        if data.len() < 1 + len_of_len {
            return Err(eyre::eyre!("RLP length truncated"));
        }
        let mut len = 0usize;
        for i in 0..len_of_len {
            len = (len << 8) | (data[1 + i] as usize);
        }
        if data.len() < 1 + len_of_len + len {
            return Err(eyre::eyre!("RLP data truncated"));
        }
        if len > 32 {
            return Err(eyre::eyre!("RLP value too large: {} bytes", len));
        }
        let start = 32 - len;
        result[start..].copy_from_slice(&data[1 + len_of_len..1 + len_of_len + len]);
        return Ok(result);
    }

    // List types (0xc0-0xff) shouldn't appear for storage values
    Err(eyre::eyre!("Unexpected RLP list type: 0x{:02x}", first))
}
