//! Reth ExEx integration for per-block delta extraction.
//!
//! This module watches canonical chain updates and writes per-block delta
//! `state.bin` files (UBT-ordered) derived from StorageChangeSets + PlainStorageState.

#![cfg(feature = "exex")]

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use futures::TryStreamExt;
use reth_ethereum::exex::{ExExContext, ExExNotification};
use reth_execution_types::Chain;
use reth_node_api::FullNodeComponents;
use reth_storage_api::{DatabaseProviderFactory, StorageReader};
use tracing::{info, warn};

use inspire_core::state_format::{StateHeader, StorageEntry, STATE_ENTRY_SIZE};
use inspire_core::ubt::{compute_storage_tree_index, compute_tree_key};

/// Configuration for delta exporter ExEx.
#[derive(Debug, Clone)]
pub struct DeltaExporterConfig {
    /// Directory to write delta state.bin files.
    pub output_dir: PathBuf,
    /// Number of recent blocks to keep (rolling window). 0 = keep all.
    pub keep_blocks: u64,
}

impl Default for DeltaExporterConfig {
    fn default() -> Self {
        Self { output_dir: PathBuf::from("./pir-data/delta"), keep_blocks: 256 }
    }
}

/// Initialize the delta exporter ExEx.
pub async fn delta_export_exex<Node: FullNodeComponents>(
    ctx: ExExContext<Node>,
    config: DeltaExporterConfig,
) -> Result<impl std::future::Future<Output = Result<()>>> {
    info!(
        output_dir = %config.output_dir.display(),
        keep_blocks = config.keep_blocks,
        "Initializing delta exporter ExEx"
    );

    Ok(delta_export_loop(ctx, config))
}

async fn delta_export_loop<Node: FullNodeComponents>(
    mut ctx: ExExContext<Node>,
    config: DeltaExporterConfig,
) -> Result<()> {
    while let Some(notification) = ctx.notifications.try_next().await? {
        let chain_id = ctx.config.chain.chain().id();
        match &notification {
            ExExNotification::ChainCommitted { new } => {
                export_chain_delta(ctx.provider(), &config, new, chain_id)?;
                ctx.send_finished_height(new.tip().num_hash())?;
            }
            ExExNotification::ChainReorged { old, new } => {
                delete_chain_deltas(&config.output_dir, old)?;
                export_chain_delta(ctx.provider(), &config, new, chain_id)?;
                ctx.send_finished_height(new.tip().num_hash())?;
            }
            ExExNotification::ChainReverted { old } => {
                delete_chain_deltas(&config.output_dir, old)?;
            }
        }
    }

    Ok(())
}

fn export_chain_delta<P, N>(
    provider: &P,
    config: &DeltaExporterConfig,
    chain: &Chain<N>,
    chain_id: u64,
) -> Result<()>
where
    P: DatabaseProviderFactory,
    N: reth_primitives_traits::NodePrimitives,
{
    fs::create_dir_all(&config.output_dir)?;

    let db = provider.database_provider_ro()?;

    for (block_number, block) in chain.blocks() {
        let block_hash = block.hash();
        let entries = collect_block_entries(&db, *block_number)?;
        let output_path = write_block_delta(
            &config.output_dir,
            *block_number,
            chain_id,
            block_hash.0,
            &entries,
        )?;

        info!(
            block = *block_number,
            entries = entries.len(),
            path = %output_path.display(),
            "Delta state written"
        );

        prune_old_blocks(&config.output_dir, config.keep_blocks, *block_number)?;
    }

    Ok(())
}

fn collect_block_entries<P>(
    provider: &P,
    block_number: u64,
) -> Result<Vec<([u8; 32], [u8; STATE_ENTRY_SIZE])>>
where
    P: StorageReader,
{
    let changed = provider.changed_storages_with_range(block_number..=block_number)?;

    if changed.is_empty() {
        return Ok(Vec::new());
    }

    let address_keys =
        changed
            .iter()
            .map(|(address, keys)| (*address, keys.iter().cloned().collect::<Vec<_>>()));

    let updated = provider.plain_state_storages(address_keys)?;

    let mut entries = Vec::new();
    for (address, storage_entries) in updated {
        let address_bytes = address.0 .0;
        for storage_entry in storage_entries {
            let slot_bytes: [u8; 32] = storage_entry.key.0;
            let value_bytes: [u8; 32] = storage_entry.value.to_be_bytes();
            let tree_index = compute_storage_tree_index(&slot_bytes);
            let tree_key = compute_tree_key(&address_bytes, &tree_index);
            let entry = StorageEntry::new(address_bytes, tree_index, value_bytes).to_bytes();
            entries.push((tree_key, entry));
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

fn write_block_delta(
    output_dir: &Path,
    block_number: u64,
    chain_id: u64,
    block_hash: [u8; 32],
    entries: &[([u8; 32], [u8; STATE_ENTRY_SIZE])],
) -> Result<PathBuf> {
    let path = output_dir.join(format!("delta_{:010}.bin", block_number));
    let file = File::create(&path)?;
    let mut writer = BufWriter::new(file);

    let header = StateHeader::new(entries.len() as u64, block_number, chain_id, block_hash);
    writer.write_all(&header.to_bytes())?;

    for (_, entry) in entries {
        writer.write_all(entry)?;
    }

    writer.flush()?;
    Ok(path)
}

fn prune_old_blocks(output_dir: &Path, keep_blocks: u64, current_block: u64) -> Result<()> {
    if keep_blocks == 0 || current_block < keep_blocks {
        return Ok(());
    }

    let prune_block = current_block - keep_blocks;
    let prune_path = output_dir.join(format!("delta_{:010}.bin", prune_block));
    if prune_path.exists() {
        if let Err(err) = fs::remove_file(&prune_path) {
            warn!(path = %prune_path.display(), error = %err, "Failed to prune old delta file");
        }
    }

    Ok(())
}

fn delete_chain_deltas<N>(output_dir: &Path, chain: &Chain<N>) -> Result<()>
where
    N: reth_primitives_traits::NodePrimitives,
{
    for (block_number, _block) in chain.blocks() {
        let path = output_dir.join(format!("delta_{:010}.bin", block_number));
        if path.exists() {
            if let Err(err) = fs::remove_file(&path) {
                warn!(path = %path.display(), error = %err, "Failed to remove reverted delta file");
            }
        }
    }

    Ok(())
}
