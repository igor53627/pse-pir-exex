//! reth-state-export: Export storage state from a vanilla reth DB into state.bin
//!
//! Reads PlainStorageState from a reth MDBX database via reth-db APIs and
//! writes a state.bin file in EIP-7864 (UBT) ordering.
//!
//! Usage:
//!   cargo run --bin reth-state-export --features reth-export -- \
//!     --db-path /path/to/reth/db \
//!     --output ./state.bin \
//!     --chain-id 11155111

#![cfg(feature = "reth-export")]

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use inspire_core::state_format::{StateHeader, STATE_ENTRY_SIZE, STATE_HEADER_SIZE};
use inspire_core::ubt::{compute_storage_tree_index, compute_tree_key};
use reth_db::mdbx::DatabaseArguments;
use reth_db::table::Table;
use reth_db::transaction::DbTx;
use reth_db::{open_db_read_only, tables, ClientVersion};
use tracing::{info, warn};

const RECORD_SIZE: usize = 32 + STATE_ENTRY_SIZE;

type EntryBytes = [u8; STATE_ENTRY_SIZE];
type PlainStorageKey = <tables::PlainStorageState as Table>::Key;
type PlainStorageValue = <tables::PlainStorageState as Table>::Value;

#[derive(Parser, Debug)]
#[command(name = "reth-state-export")]
#[command(about = "Export UBT-ordered state.bin from a vanilla reth DB")]
struct Args {
    /// Path to reth MDBX database directory (contains mdbx.dat)
    #[arg(long)]
    db_path: PathBuf,

    /// Output state.bin file
    #[arg(long, default_value = "./state.bin")]
    output: PathBuf,

    /// Chain ID to store in the header (e.g. 1 for mainnet, 11155111 for Sepolia)
    #[arg(long, default_value = "1")]
    chain_id: u64,

    /// Number of entries per sort chunk (when sorting)
    #[arg(long, default_value = "250000")]
    chunk_entries: usize,

    /// Temporary directory for sorted chunks (defaults to output dir)
    #[arg(long)]
    tmp_dir: Option<PathBuf>,

    /// Log progress every N entries
    #[arg(long, default_value = "1000000")]
    progress_interval: u64,

    /// Skip sorting (writes in DB order; not suitable for UBT lookups)
    #[arg(long)]
    no_sort: bool,

    /// Keep temporary chunk files after merge
    #[arg(long)]
    keep_temp: bool,
}

#[derive(Clone)]
struct EntryWithKey {
    tree_key: [u8; 32],
    entry: EntryBytes,
}

struct ChunkRecord {
    tree_key: [u8; 32],
    entry: EntryBytes,
}

#[derive(Eq)]
struct HeapItem {
    tree_key: [u8; 32],
    entry: EntryBytes,
    chunk_index: usize,
}

impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.tree_key
            .cmp(&other.tree_key)
            .then_with(|| self.chunk_index.cmp(&other.chunk_index))
    }
}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for HeapItem {
    fn eq(&self, other: &Self) -> bool {
        self.tree_key == other.tree_key && self.chunk_index == other.chunk_index
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let args = Args::parse();

    info!(db_path = %args.db_path.display(), "Opening reth DB");
    let db = open_db_read_only(&args.db_path, DatabaseArguments::new(ClientVersion::default()))
        .with_context(|| format!("Failed to open reth DB at {}", args.db_path.display()))?;

    let mut tx = db.tx()?;
    tx.disable_long_read_transaction_safety();

    let (block_number, block_hash) = latest_canonical_header(&tx)?;
    info!(block_number, block_hash = %hex::encode(block_hash), "Using canonical head");

    if args.no_sort {
        export_unsorted(&mut tx, &args, block_number, block_hash)?;
    } else {
        let (total_entries, chunk_paths) = build_sorted_chunks(&mut tx, &args)?;
        tx.commit()?;
        write_sorted_output(&args, block_number, block_hash, total_entries, &chunk_paths)?;
        if !args.keep_temp {
            cleanup_chunks(&chunk_paths)?;
        }
        return Ok(());
    }

    tx.commit()?;
    Ok(())
}

fn latest_canonical_header(tx: &impl DbTx) -> Result<(u64, [u8; 32])> {
    let mut cursor = tx.cursor_read::<tables::CanonicalHeaders>()?;
    let (block_number, block_hash) = cursor
        .last()?
        .ok_or_else(|| anyhow!("CanonicalHeaders table is empty"))?;
    Ok((block_number, block_hash.0))
}

fn export_unsorted(
    tx: &mut impl DbTx,
    args: &Args,
    block_number: u64,
    block_hash: [u8; 32],
) -> Result<()> {
    info!(output = %args.output.display(), "Writing state.bin (unsorted)");

    let file = File::create(&args.output)?;
    let mut writer = BufWriter::new(file);
    writer.write_all(&[0u8; STATE_HEADER_SIZE])?; // placeholder

    let mut cursor = tx.cursor_dup_read::<tables::PlainStorageState>()?;
    let mut walker = cursor.walk_dup(None, None)?;

    let pb = spinner("Exporting entries (unsorted)");
    let mut count = 0u64;

    while let Some(row) = walker.next() {
        let (address, storage_entry) = row?;
        let entry = encode_entry(address, storage_entry)?;
        writer.write_all(&entry)?;
        count += 1;

        if count % args.progress_interval == 0 {
            pb.set_message(format!("{} entries", count));
            writer.flush()?;
        }
    }

    writer.flush()?;
    let mut file = writer.into_inner().map_err(|e| e.into_error())?;
    file.seek(SeekFrom::Start(0))?;

    let header = StateHeader::new(count, block_number, args.chain_id, block_hash);
    file.write_all(&header.to_bytes())?;
    file.flush()?;

    pb.finish_with_message(format!("Wrote {} entries (unsorted)", count));
    info!(entries = count, "Export complete (unsorted)");

    Ok(())
}

fn build_sorted_chunks(tx: &mut impl DbTx, args: &Args) -> Result<(u64, Vec<PathBuf>)> {
    let tmp_dir = temp_dir(&args.output, args.tmp_dir.as_ref())?;
    fs::create_dir_all(&tmp_dir)?;

    info!(tmp_dir = %tmp_dir.display(), "Writing sorted chunks");

    let mut cursor = tx.cursor_dup_read::<tables::PlainStorageState>()?;
    let mut walker = cursor.walk_dup(None, None)?;

    let pb = spinner("Sorting chunks");
    let mut count = 0u64;
    let mut chunk_index = 0usize;
    let mut chunk_paths = Vec::new();

    let mut buffer: Vec<EntryWithKey> = Vec::with_capacity(args.chunk_entries);

    while let Some(row) = walker.next() {
        let (address, storage_entry) = row?;
        let (tree_key, entry) = encode_entry_with_key(address, storage_entry)?;
        buffer.push(EntryWithKey { tree_key, entry });
        count += 1;

        if buffer.len() >= args.chunk_entries {
            let path = flush_chunk(&mut buffer, &tmp_dir, chunk_index)?;
            chunk_paths.push(path);
            chunk_index += 1;
        }

        if count % args.progress_interval == 0 {
            pb.set_message(format!("{} entries", count));
        }
    }

    if !buffer.is_empty() {
        let path = flush_chunk(&mut buffer, &tmp_dir, chunk_index)?;
        chunk_paths.push(path);
    }

    pb.finish_with_message(format!("Prepared {} entries", count));

    Ok((count, chunk_paths))
}

fn write_sorted_output(
    args: &Args,
    block_number: u64,
    block_hash: [u8; 32],
    total_entries: u64,
    chunk_paths: &[PathBuf],
) -> Result<()> {
    info!(output = %args.output.display(), "Merging sorted chunks");

    let file = File::create(&args.output)?;
    let mut writer = BufWriter::new(file);
    let header = StateHeader::new(total_entries, block_number, args.chain_id, block_hash);
    writer.write_all(&header.to_bytes())?;

    let mut readers: Vec<BufReader<File>> = Vec::with_capacity(chunk_paths.len());
    for path in chunk_paths {
        readers.push(BufReader::new(File::open(path)?));
    }

    let mut heap: BinaryHeap<std::cmp::Reverse<HeapItem>> = BinaryHeap::new();
    for (idx, reader) in readers.iter_mut().enumerate() {
        if let Some(record) = read_record(reader)? {
            heap.push(std::cmp::Reverse(HeapItem {
                tree_key: record.tree_key,
                entry: record.entry,
                chunk_index: idx,
            }));
        }
    }

    let pb = spinner("Merging chunks");
    let mut written = 0u64;

    while let Some(std::cmp::Reverse(item)) = heap.pop() {
        writer.write_all(&item.entry)?;
        written += 1;

        if written % args.progress_interval == 0 {
            pb.set_message(format!("{} entries", written));
            writer.flush()?;
        }

        if let Some(record) = read_record(&mut readers[item.chunk_index])? {
            heap.push(std::cmp::Reverse(HeapItem {
                tree_key: record.tree_key,
                entry: record.entry,
                chunk_index: item.chunk_index,
            }));
        }
    }

    writer.flush()?;
    pb.finish_with_message(format!("Merged {} entries", written));

    if written != total_entries {
        warn!(written, total = total_entries, "Entry count mismatch after merge");
    }

    Ok(())
}

fn encode_entry(address: PlainStorageKey, storage_entry: PlainStorageValue) -> Result<EntryBytes> {
    let address_bytes = address.0 .0;
    let slot_bytes: [u8; 32] = storage_entry.key.0;
    let value_bytes: [u8; 32] = storage_entry.value.to_be_bytes();

    let tree_index = compute_storage_tree_index(&slot_bytes);
    let entry = inspire_core::state_format::StorageEntry::new(address_bytes, tree_index, value_bytes);
    Ok(entry.to_bytes())
}

fn encode_entry_with_key(
    address: PlainStorageKey,
    storage_entry: PlainStorageValue,
) -> Result<([u8; 32], EntryBytes)> {
    let address_bytes = address.0 .0;
    let slot_bytes: [u8; 32] = storage_entry.key.0;
    let value_bytes: [u8; 32] = storage_entry.value.to_be_bytes();

    let tree_index = compute_storage_tree_index(&slot_bytes);
    let tree_key = compute_tree_key(&address_bytes, &tree_index);
    let entry = inspire_core::state_format::StorageEntry::new(address_bytes, tree_index, value_bytes);

    Ok((tree_key, entry.to_bytes()))
}

fn flush_chunk(buffer: &mut Vec<EntryWithKey>, dir: &Path, index: usize) -> Result<PathBuf> {
    buffer.sort_by_key(|entry| entry.tree_key);

    let path = dir.join(format!("chunk_{:05}.bin", index));
    let file = File::create(&path)?;
    let mut writer = BufWriter::new(file);

    for entry in buffer.iter() {
        writer.write_all(&entry.tree_key)?;
        writer.write_all(&entry.entry)?;
    }

    writer.flush()?;
    buffer.clear();

    Ok(path)
}

fn read_record(reader: &mut BufReader<File>) -> Result<Option<ChunkRecord>> {
    let mut buf = [0u8; RECORD_SIZE];
    match reader.read_exact(&mut buf) {
        Ok(()) => {
            let mut tree_key = [0u8; 32];
            let mut entry = [0u8; STATE_ENTRY_SIZE];
            tree_key.copy_from_slice(&buf[..32]);
            entry.copy_from_slice(&buf[32..]);
            Ok(Some(ChunkRecord { tree_key, entry }))
        }
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn temp_dir(output: &Path, override_dir: Option<&PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = override_dir {
        return Ok(dir.clone());
    }

    let base = output
        .parent()
        .ok_or_else(|| anyhow!("Output path has no parent directory"))?;
    Ok(base.join("reth-export-chunks"))
}

fn cleanup_chunks(paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        if let Err(err) = fs::remove_file(path) {
            warn!(path = %path.display(), error = %err, "Failed to remove chunk file");
        }
    }
    Ok(())
}

fn spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("[{elapsed_precise}] {spinner} {msg}")
            .unwrap(),
    );
    pb.set_message(message.to_string());
    pb
}
