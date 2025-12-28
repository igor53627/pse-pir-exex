use alloy_primitives::{Address, B256, U256};
use std::collections::HashMap;

use crate::rpc::StorageEntry;

/// Tracks current PIR database state
pub struct StateTracker {
    /// Last synced block
    last_block: Option<u64>,
    /// In-memory state (address -> slot -> value)
    /// Only used for hot lane tracking
    state: HashMap<Address, HashMap<B256, U256>>,
}

impl StateTracker {
    pub fn new() -> Self {
        Self {
            last_block: None,
            state: HashMap::new(),
        }
    }

    pub fn last_block(&self) -> Option<u64> {
        self.last_block
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

    /// Load full state from dump (for initial sync)
    pub fn load_from_dump(&mut self, block: u64, entries: Vec<StorageEntry>) {
        self.state.clear();
        for entry in entries {
            self.state
                .entry(entry.address)
                .or_default()
                .insert(entry.slot, entry.value);
        }
        self.last_block = Some(block);
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
