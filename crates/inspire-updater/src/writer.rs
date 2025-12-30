//! Writes storage entries to PIR state.bin format

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use inspire_core::state_format::{StateHeader, StorageEntry, STATE_ENTRY_SIZE, STATE_HEADER_SIZE};
use inspire_core::ubt::compute_tree_key;

use crate::rpc::StorageEntry as RpcStorageEntry;

/// Writes storage entries to PIR shard files
pub struct ShardWriter {
    data_dir: std::path::PathBuf,
    chain_id: u64,
}

impl ShardWriter {
    pub fn new(data_dir: impl AsRef<Path>, chain_id: u64) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
            chain_id,
        }
    }

    /// Compute EIP-7864 tree_key (stem || subindex) for UBT-ordered sorting
    fn compute_entry_tree_key(entry: &StorageEntry) -> [u8; 32] {
        compute_tree_key(&entry.address, &entry.tree_index)
    }

    /// Convert RPC entry to core StorageEntry with EIP-7864 tree_index
    fn to_core_entry(entry: &RpcStorageEntry) -> StorageEntry {
        let address: [u8; 20] = entry.address.into_array();
        let slot: [u8; 32] = entry.slot.0;
        let value: [u8; 32] = entry.value.to_be_bytes();
        StorageEntry::from_storage_slot(address, slot, value)
    }

    /// Write entries to state.bin file
    /// Entries are sorted by tree_key (stem || subindex) per EIP-7864
    pub fn write_state_file(
        &self,
        entries: &[RpcStorageEntry],
        block_number: u64,
        block_hash: [u8; 32],
    ) -> anyhow::Result<std::path::PathBuf> {
        std::fs::create_dir_all(&self.data_dir)?;

        let output_path = self.data_dir.join("state.bin");

        // Convert and sort entries by tree_key (EIP-7864 ordering)
        let mut sorted_entries: Vec<_> = entries
            .iter()
            .map(|e| {
                let core = Self::to_core_entry(e);
                let tree_key = Self::compute_entry_tree_key(&core);
                (tree_key, core)
            })
            .collect();

        sorted_entries.sort_by(|a, b| a.0.cmp(&b.0));

        // Write file
        let file = File::create(&output_path)?;
        let mut writer = BufWriter::new(file);

        // Write header
        let header = StateHeader::new(
            sorted_entries.len() as u64,
            block_number,
            self.chain_id,
            block_hash,
        );
        writer.write_all(&header.to_bytes())?;

        // Write entries
        for (_, entry) in &sorted_entries {
            writer.write_all(&entry.to_bytes())?;
        }

        writer.flush()?;

        let file_size = STATE_HEADER_SIZE + sorted_entries.len() * STATE_ENTRY_SIZE;
        tracing::info!(
            path = %output_path.display(),
            entries = sorted_entries.len(),
            size_mb = file_size / (1024 * 1024),
            block = block_number,
            "Wrote state.bin"
        );

        Ok(output_path)
    }

    /// Write entries (convenience wrapper for incremental updates)
    pub async fn write_entries(&self, entries: &[RpcStorageEntry]) -> anyhow::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        tracing::info!(
            count = entries.len(),
            data_dir = %self.data_dir.display(),
            "Incremental update (full rewrite needed for state.bin)"
        );

        Ok(())
    }

    /// Write full state dump (without UBT root)
    pub async fn write_full_state(
        &self,
        entries: &[RpcStorageEntry],
        block_number: u64,
    ) -> anyhow::Result<std::path::PathBuf> {
        self.write_state_file(entries, block_number, [0u8; 32])
    }

    /// Write full state dump with UBT root for verification
    pub async fn write_full_state_with_ubt(
        &self,
        entries: &[RpcStorageEntry],
        block_number: u64,
        ubt_root: [u8; 32],
    ) -> anyhow::Result<std::path::PathBuf> {
        self.write_state_file(entries, block_number, ubt_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, B256, U256};

    #[test]
    fn test_compute_entry_tree_key() {
        let entry = StorageEntry::from_storage_slot([0x42u8; 20], [0x01u8; 32], [0xff; 32]);
        let key = ShardWriter::compute_entry_tree_key(&entry);
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_write_state_file() {
        let temp_dir = std::env::temp_dir().join("inspire-updater-test");
        let _ = std::fs::remove_dir_all(&temp_dir);

        let writer = ShardWriter::new(&temp_dir, 11155111); // Sepolia

        let entries = vec![
            RpcStorageEntry {
                address: Address::repeat_byte(0x42),
                slot: B256::repeat_byte(0x01),
                value: U256::from(100),
            },
            RpcStorageEntry {
                address: Address::repeat_byte(0x43),
                slot: B256::repeat_byte(0x02),
                value: U256::from(200),
            },
        ];

        let path = writer.write_state_file(&entries, 1000, [0u8; 32]).unwrap();
        assert!(path.exists());

        let data = std::fs::read(&path).unwrap();
        assert_eq!(data.len(), STATE_HEADER_SIZE + 2 * STATE_ENTRY_SIZE);

        // Check header
        let header = StateHeader::from_bytes(&data).unwrap();
        assert_eq!(header.entry_count, 2);
        assert_eq!(header.block_number, 1000);
        assert_eq!(header.chain_id, 11155111);

        // Verify entries have EIP-7864 tree_index (not raw slots)
        // Both slots (0x01..01 and 0x02..02) are large, so both go to overflow stems
        // After sorting by tree_key, we just verify the tree_index is properly computed
        let entry1 = StorageEntry::from_bytes(&data[STATE_HEADER_SIZE..]).unwrap();
        let entry2 =
            StorageEntry::from_bytes(&data[STATE_HEADER_SIZE + STATE_ENTRY_SIZE..]).unwrap();

        // Both large slots should have MAIN_STORAGE_OFFSET in high bytes
        // tree_index = MAIN_STORAGE_OFFSET + slot, so high bytes depend on slot
        // For 0x01..01 slot: MAIN_STORAGE_OFFSET[0]=1, slot[0]=1, so tree_index[0] could be 2
        // Just verify they're different from raw slots (which would be 0x01..01 and 0x02..02)
        assert_ne!(
            entry1.tree_index, [0x01; 32],
            "Entry should not have raw slot as tree_index"
        );
        assert_ne!(
            entry2.tree_index, [0x02; 32],
            "Entry should not have raw slot as tree_index"
        );

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
