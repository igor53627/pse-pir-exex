//! State dump binary: Extract full Ethereum state from reth MDBX database
//!
//! Uses mdbx-rs directly to iterate through PlainAccountState and PlainStorageState
//! tables to create PIR-ready database files.
//!
//! Usage:
//!   cargo run --bin state-dump --features state-dump -- \
//!     --db-path /mnt/sepolia/data/db \
//!     --output-dir ./pir-data \
//!     --chain sepolia

#![cfg(feature = "state-dump")]

use std::ffi::CString;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::ptr;

use clap::Parser;
use eyre::Result;
use indicatif::{ProgressBar, ProgressStyle};
use mdbx_rs::{*, MDBX_cursor_op::*};
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(name = "state-dump")]
#[command(about = "Extract full Ethereum state from reth MDBX database for PIR")]
struct Args {
    /// Path to reth MDBX database directory (containing mdbx.dat)
    #[arg(long)]
    db_path: PathBuf,

    /// Output directory for PIR database files
    #[arg(long, default_value = "./state-dump")]
    output_dir: PathBuf,

    /// Chain name (for metadata)
    #[arg(long, default_value = "sepolia")]
    chain: String,

    /// Only dump storage (skip accounts)
    #[arg(long)]
    storage_only: bool,

    /// Only dump accounts (skip storage)
    #[arg(long)]
    accounts_only: bool,

    /// Log progress every N records
    #[arg(long, default_value = "1000000")]
    progress_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DumpMetadata {
    chain: String,
    num_accounts: u64,
    num_storage_slots: u64,
    entry_size: usize,
    manifest_entry_size: usize,
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
        "Starting state dump with mdbx-rs"
    );

    let mut num_accounts = 0u64;
    let mut num_storage_slots = 0u64;

    unsafe {
        let mut env: *mut MDBX_env = ptr::null_mut();
        let rc = mdbx_env_create(&mut env);
        if rc != MDBX_SUCCESS {
            return Err(eyre::eyre!("Failed to create MDBX environment: {}", rc));
        }

        // Reth has 31+ named tables, set maxdbs before opening
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

        tracing::info!("Read-only transaction started");

        if !args.storage_only {
            num_accounts = dump_table(
                txn,
                "PlainAccountState",
                &args.output_dir.join("accounts.bin"),
                args.progress_interval,
                false,
            )?;
        }

        if !args.accounts_only {
            num_storage_slots = dump_table(
                txn,
                "PlainStorageState",
                &args.output_dir.join("storage.bin"),
                args.progress_interval,
                true,
            )?;
        }

        mdbx_txn_abort(txn);
        mdbx_env_close(env);
    }

    let metadata = DumpMetadata {
        chain: args.chain.clone(),
        num_accounts,
        num_storage_slots,
        entry_size: 32,
        manifest_entry_size: 52,
    };

    let metadata_path = args.output_dir.join("metadata.json");
    let metadata_json = serde_json::to_string_pretty(&metadata)?;
    std::fs::write(&metadata_path, &metadata_json)?;

    tracing::info!(
        accounts = num_accounts,
        storage_slots = num_storage_slots,
        metadata = %metadata_path.display(),
        "State dump complete"
    );

    Ok(())
}

unsafe fn dump_table(
    txn: *mut MDBX_txn,
    table_name: &str,
    output_path: &PathBuf,
    progress_interval: u64,
    is_storage: bool,
) -> Result<u64> {
    let table_cstr = CString::new(table_name)?;

    let mut dbi: MDBX_dbi = 0;
    let rc = mdbx_dbi_open(txn, table_cstr.as_ptr(), 0, &mut dbi);
    if rc != MDBX_SUCCESS {
        return Err(eyre::eyre!("Failed to open table {}: {}", table_name, rc));
    }

    tracing::info!(table = table_name, path = %output_path.display(), "Dumping table");

    let mut cursor: *mut MDBX_cursor = ptr::null_mut();
    let rc = mdbx_cursor_open(txn, dbi, &mut cursor);
    if rc != MDBX_SUCCESS {
        return Err(eyre::eyre!("Failed to open cursor: {}", rc));
    }

    let mut writer = BufWriter::with_capacity(64 * 1024 * 1024, File::create(output_path)?);

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("[{elapsed_precise}] {spinner} {msg}")
            .unwrap(),
    );

    let mut key = MDBX_val::default();
    let mut val = MDBX_val::default();
    let mut count = 0u64;

    let mut rc = mdbx_cursor_get(cursor, &mut key, &mut val, MDBX_FIRST as MDBX_cursor_op);

    while rc == MDBX_SUCCESS {
        let key_bytes = std::slice::from_raw_parts(key.iov_base as *const u8, key.iov_len);
        let val_bytes = std::slice::from_raw_parts(val.iov_base as *const u8, val.iov_len);

        if is_storage {
            writer.write_all(key_bytes)?;
            writer.write_all(val_bytes)?;
        } else {
            writer.write_all(key_bytes)?;
            writer.write_all(val_bytes)?;
        }

        count += 1;

        if count % progress_interval == 0 {
            pb.set_message(format!("{}: {} entries", table_name, count));
            writer.flush()?;
        }

        rc = mdbx_cursor_get(cursor, &mut key, &mut val, MDBX_NEXT as MDBX_cursor_op);
    }

    if rc != MDBX_NOTFOUND {
        mdbx_cursor_close(cursor);
        return Err(eyre::eyre!("Cursor error: {}", rc));
    }

    writer.flush()?;
    mdbx_cursor_close(cursor);

    pb.finish_with_message(format!("{}: {} entries complete", table_name, count));

    tracing::info!(
        table = table_name,
        count,
        path = %output_path.display(),
        "Table dump complete"
    );

    Ok(count)
}
