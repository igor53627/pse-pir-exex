use alloy_primitives::{Address, B256, U256};
use std::collections::HashMap;

use crate::rpc::StorageEntry;
use inspire_core::bucket_index::{compute_bucket_id, BucketDelta, NUM_BUCKETS};

/// Tracks current PIR database state
pub struct StateTracker {
    /// Last synced block
    last_block: Option<u64>,
    /// In-memory state (address -> slot -> value)
    /// Only used for hot lane tracking
    state: HashMap<Address, HashMap<B256, U256>>,
    /// Bucket counts for delta tracking
    bucket_counts: Vec<u16>,
}

impl StateTracker {
    pub fn new() -> Self {
        Self {
            last_block: None,
            state: HashMap::new(),
            bucket_counts: vec![0u16; NUM_BUCKETS],
        }
    }

    pub fn last_block(&self) -> Option<u64> {
        self.last_block
    }

    pub fn set_last_block(&mut self, block: u64) {
        self.last_block = Some(block);
    }

    /// Apply entries and return ones that changed
    pub fn apply_entries(&mut self, block: u64, entries: Vec<StorageEntry>) -> Vec<StorageEntry> {
        let mut changed = Vec::new();

        for entry in entries {
            let slots = self.state.entry(entry.address).or_default();
            let old_value = slots.insert(entry.slot, entry.value);

            // Only track if value actually changed
            if old_value != Some(entry.value) {
                changed.push(entry);
            }
        }

        self.last_block = Some(block);
        changed
    }

    /// Apply entries and return bucket delta for changed buckets
    pub fn apply_entries_with_delta(
        &mut self,
        block: u64,
        entries: Vec<StorageEntry>,
    ) -> (Vec<StorageEntry>, BucketDelta) {
        let mut changed = Vec::new();
        let mut affected_buckets: HashMap<usize, i32> = HashMap::new();

        for entry in entries {
            let address_bytes: [u8; 20] = entry.address.into();
            let slot_bytes: [u8; 32] = entry.slot.0;
            let bucket_id = compute_bucket_id(&address_bytes, &slot_bytes);

            let slots = self.state.entry(entry.address).or_default();
            let old_value = slots.insert(entry.slot, entry.value);

            if old_value.is_none() {
                // New entry: increment bucket count
                *affected_buckets.entry(bucket_id).or_insert(0) += 1;
                changed.push(entry);
            } else if old_value != Some(entry.value) {
                // Value changed but entry existed - no count change
                changed.push(entry);
            }
            // If value is zero and entry existed, technically a delete, but
            // PIR DB keeps entries so no count change
        }

        // Apply count changes and build delta
        let mut updates = Vec::new();
        for (bucket_id, delta) in affected_buckets {
            let new_count = (self.bucket_counts[bucket_id] as i32 + delta).max(0) as u16;
            self.bucket_counts[bucket_id] = new_count;
            updates.push((bucket_id, new_count));
        }
        updates.sort_by_key(|(id, _)| *id);

        self.last_block = Some(block);

        let bucket_delta = BucketDelta {
            block_number: block,
            updates,
        };

        (changed, bucket_delta)
    }

    /// Load full state from dump (for initial sync)
    pub fn load_from_dump(&mut self, block: u64, entries: Vec<StorageEntry>) {
        self.state.clear();
        self.bucket_counts = vec![0u16; NUM_BUCKETS];

        for entry in &entries {
            let address_bytes: [u8; 20] = entry.address.into();
            let slot_bytes: [u8; 32] = entry.slot.0;
            let bucket_id = compute_bucket_id(&address_bytes, &slot_bytes);

            self.state
                .entry(entry.address)
                .or_default()
                .insert(entry.slot, entry.value);

            self.bucket_counts[bucket_id] = self.bucket_counts[bucket_id].saturating_add(1);
        }
        self.last_block = Some(block);
    }

    /// Get current bucket counts (for writing initial bucket index)
    pub fn bucket_counts(&self) -> &[u16] {
        &self.bucket_counts
    }

    #[allow(dead_code)]
    pub fn entry_count(&self) -> usize {
        self.state.values().map(|s| s.len()).sum()
    }
}

impl Default for StateTracker {
    fn default() -> Self {
        Self::new()
    }
}
