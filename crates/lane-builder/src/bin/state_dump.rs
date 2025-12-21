//! State dump binary: Extract full Ethereum state from reth MDBX database
//!
//! Iterates through PlainAccountState and PlainStorageState tables to create
//! PIR-ready database files.
//!
//! Usage:
//!   cargo run --bin state-dump --features state-dump -- \
//!     --db-path /mnt/sepolia/data/db \
//!     --output-dir ./pir-data \
//!     --chain sepolia

#![cfg(feature = "state-dump")]

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use alloy_primitives::U256;
use clap::Parser;
use eyre::Result;
use indicatif::{ProgressBar, ProgressStyle};
use reth_db::{
    cursor::DbCursorRO,
    open_db_read_only,
    tables,
    transaction::DbTx,
    Database,
};
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
        "Starting state dump"
    );

    let db = open_database_read_only(&args.db_path)?;
    let tx = db.tx()?;

    let mut num_accounts = 0u64;
    let mut num_storage_slots = 0u64;

    if !args.storage_only {
        num_accounts = dump_accounts(&tx, &args.output_dir, args.progress_interval)?;
    }

    if !args.accounts_only {
        num_storage_slots = dump_storage(&tx, &args.output_dir, args.progress_interval)?;
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

fn open_database_read_only(path: &PathBuf) -> Result<impl Database> {
    tracing::info!(path = %path.display(), "Opening MDBX database read-only");
    let db = open_db_read_only(path, Default::default())?;
    Ok(db)
}

fn dump_accounts<T: DbTx>(tx: &T, output_dir: &PathBuf, progress_interval: u64) -> Result<u64> {
    let accounts_path = output_dir.join("accounts.bin");
    let mut writer = BufWriter::new(File::create(&accounts_path)?);

    tracing::info!(path = %accounts_path.display(), "Dumping accounts");

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("[{elapsed_precise}] {spinner} {msg}")
            .unwrap(),
    );

    let mut cursor = tx.cursor_read::<tables::PlainAccountState>()?;
    let mut count = 0u64;

    while let Some((address, account)) = cursor.next()? {
        let mut record = [0u8; 92];

        record[0..20].copy_from_slice(address.as_slice());
        record[20..28].copy_from_slice(&account.nonce.to_be_bytes());
        record[28..60].copy_from_slice(&u256_to_be_bytes(account.balance));
        record[60..92].copy_from_slice(
            account
                .bytecode_hash
                .unwrap_or_default()
                .as_slice(),
        );

        writer.write_all(&record)?;
        count += 1;

        if count % progress_interval == 0 {
            pb.set_message(format!("Accounts: {}", count));
            writer.flush()?;
        }
    }

    writer.flush()?;
    pb.finish_with_message(format!("Accounts complete: {}", count));

    tracing::info!(count, path = %accounts_path.display(), "Account dump complete");
    Ok(count)
}

fn dump_storage<T: DbTx>(tx: &T, output_dir: &PathBuf, progress_interval: u64) -> Result<u64> {
    let values_path = output_dir.join("storage_values.bin");
    let manifest_path = output_dir.join("storage_manifest.bin");

    let mut values_writer = BufWriter::with_capacity(64 * 1024 * 1024, File::create(&values_path)?);
    let mut manifest_writer =
        BufWriter::with_capacity(64 * 1024 * 1024, File::create(&manifest_path)?);

    tracing::info!(
        values = %values_path.display(),
        manifest = %manifest_path.display(),
        "Dumping storage slots"
    );

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("[{elapsed_precise}] {spinner} {msg}")
            .unwrap(),
    );

    let mut cursor = tx.cursor_read::<tables::PlainStorageState>()?;
    let mut count = 0u64;

    while let Some((address, storage_entry)) = cursor.next()? {
        let value_bytes = u256_to_be_bytes(storage_entry.value);
        values_writer.write_all(&value_bytes)?;

        let mut manifest_record = [0u8; 52];
        manifest_record[0..20].copy_from_slice(address.as_slice());
        manifest_record[20..52].copy_from_slice(storage_entry.key.as_slice());
        manifest_writer.write_all(&manifest_record)?;

        count += 1;

        if count % progress_interval == 0 {
            pb.set_message(format!("Storage slots: {}", count));
            values_writer.flush()?;
            manifest_writer.flush()?;
        }
    }

    values_writer.flush()?;
    manifest_writer.flush()?;
    pb.finish_with_message(format!("Storage complete: {}", count));

    tracing::info!(
        count,
        values = %values_path.display(),
        manifest = %manifest_path.display(),
        "Storage dump complete"
    );
    Ok(count)
}

fn u256_to_be_bytes(value: U256) -> [u8; 32] {
    value.to_be_bytes::<32>()
}
