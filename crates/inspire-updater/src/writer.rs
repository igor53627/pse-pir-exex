//! Writes storage entries to PIR state.bin format

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use inspire_core::state_format::{StateHeader, StorageEntry, STATE_ENTRY_SIZE, STATE_HEADER_SIZE};
use tiny_keccak::{Hasher, Keccak};

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

    /// Compute keccak256(address || slot) for sorting
    fn entry_sort_key(address: &[u8; 20], slot: &[u8; 32]) -> [u8; 32] {
        let mut hasher = Keccak::v256();
        hasher.update(address);
        hasher.update(slot);
        let mut output = [0u8; 32];
        hasher.finalize(&mut output);
        output
    }

    /// Convert RPC entry to core StorageEntry
    fn to_core_entry(entry: &RpcStorageEntry) -> StorageEntry {
        let address: [u8; 20] = entry.address.into_array();
        let slot: [u8; 32] = entry.slot.0;
        let value: [u8; 32] = entry.value.to_be_bytes();
        StorageEntry::new(address, slot, value)
    }

    /// Write entries to state.bin file
    /// Entries are sorted by keccak256(address || slot)
    pub fn write_state_file(
        &self,
        entries: &[RpcStorageEntry],
        block_number: u64,
        block_hash: [u8; 32],
    ) -> anyhow::Result<std::path::PathBuf> {
        std::fs::create_dir_all(&self.data_dir)?;

        let output_path = self.data_dir.join("state.bin");

        // Convert and sort entries
        let mut sorted_entries: Vec<_> = entries
            .iter()
            .map(|e| {
                let core = Self::to_core_entry(e);
                let key = Self::entry_sort_key(&core.address, &core.slot);
                (key, core)
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

    /// Write full state dump
    pub async fn write_full_state(
        &self,
        entries: &[RpcStorageEntry],
        block_number: u64,
    ) -> anyhow::Result<std::path::PathBuf> {
        self.write_state_file(entries, block_number, [0u8; 32])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, B256, U256};

    #[test]
    fn test_entry_sort_key() {
        let addr = [0x42u8; 20];
        let slot = [0x01u8; 32];
        let key = ShardWriter::entry_sort_key(&addr, &slot);
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

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
