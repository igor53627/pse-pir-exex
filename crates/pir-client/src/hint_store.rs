//! Local hint storage with rotation support

use pir_core::{subset::Subset, Hint, ENTRY_SIZE};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Stored hint with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredHint {
    pub id: usize,
    pub subset: Subset,
    pub hint: Hint,
}

/// Tracks which hints were recently used for rotation
#[derive(Debug, Default)]
pub struct RotationState {
    /// target_index -> list of recently used hint_ids (ring buffer)
    recently_used: HashMap<u64, Vec<usize>>,
    /// Max recent hints to track per target
    max_recent: usize,
}

impl RotationState {
    pub fn new(max_recent: usize) -> Self {
        Self {
            recently_used: HashMap::new(),
            max_recent,
        }
    }

    /// Record that a hint was used for a target
    pub fn record_use(&mut self, target: u64, hint_id: usize) {
        let recent = self.recently_used.entry(target).or_default();
        recent.push(hint_id);
        if recent.len() > self.max_recent {
            recent.remove(0);
        }
    }

    /// Check if a hint was recently used for a target
    pub fn was_recently_used(&self, target: u64, hint_id: usize) -> bool {
        self.recently_used
            .get(&target)
            .map(|r| r.contains(&hint_id))
            .unwrap_or(false)
    }

    /// Get hints to avoid for a target
    pub fn hints_to_avoid(&self, target: u64) -> &[usize] {
        self.recently_used.get(&target).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// Local hint store with rotation support
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HintStore {
    /// Block number of the snapshot
    pub block_number: u64,
    /// All stored hints
    pub hints: Vec<StoredHint>,
    /// Index: target_index -> hint_ids that contain it (for rotation)
    #[serde(skip)]
    pub index: HashMap<u64, Vec<usize>>,
    /// Rotation state (not persisted)
    #[serde(skip)]
    pub rotation: RotationState,
}

impl HintStore {
    /// Create a new empty store
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with custom rotation settings
    pub fn with_rotation(max_recent: usize) -> Self {
        Self {
            rotation: RotationState::new(max_recent),
            ..Default::default()
        }
    }

    /// Load from file
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let data = std::fs::read(path)?;
        let mut store: Self = bincode::deserialize(&data)?;
        store.rebuild_index();
        store.rotation = RotationState::new(10); // Default: avoid last 10 hints per target
        Ok(store)
    }

    /// Save to file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let data = bincode::serialize(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Add hints from a manifest
    pub fn add_hints(&mut self, hints: Vec<(Subset, Hint)>, block_number: u64) {
        self.block_number = block_number;
        self.hints = hints
            .into_iter()
            .enumerate()
            .map(|(id, (subset, hint))| StoredHint { id, subset, hint })
            .collect();
        self.rebuild_index();
    }

    /// Find ALL hints that contain the target index (for rotation)
    pub fn find_all_hints_for_target(&self, target: u64) -> Vec<&StoredHint> {
        if let Some(hint_ids) = self.index.get(&target) {
            hint_ids
                .iter()
                .filter_map(|&id| self.hints.get(id))
                .collect()
        } else {
            // Fallback to linear scan
            self.hints
                .iter()
                .filter(|h| h.subset.contains(target))
                .collect()
        }
    }

    /// Find a hint for target WITH ROTATION (avoids recently used hints)
    pub fn find_hint_with_rotation(&mut self, target: u64) -> Option<&StoredHint> {
        let hints_to_avoid = self.rotation.hints_to_avoid(target);
        let all_hints = self.find_all_hints_for_target(target);
        
        if all_hints.is_empty() {
            return None;
        }

        // Filter out recently used hints
        let available: Vec<_> = all_hints
            .iter()
            .filter(|h| !hints_to_avoid.contains(&h.id))
            .collect();

        // Pick randomly from available (or all if we've used them all)
        let chosen = if available.is_empty() {
            // All hints recently used, pick any random one
            all_hints.choose(&mut rand::thread_rng())
        } else {
            available.choose(&mut rand::thread_rng()).copied()
        };

        if let Some(hint) = chosen {
            self.rotation.record_use(target, hint.id);
            Some(*hint)
        } else {
            None
        }
    }

    /// Find a hint (simple, no rotation - for backward compatibility)
    pub fn find_hint_for_target(&self, target: u64) -> Option<&StoredHint> {
        if let Some(hint_ids) = self.index.get(&target) {
            if let Some(&id) = hint_ids.first() {
                return self.hints.get(id);
            }
        }
        
        // Fallback to linear scan
        for hint in &self.hints {
            if hint.subset.contains(target) {
                return Some(hint);
            }
        }
        
        None
    }

    /// Get count of available hints for a target (useful for privacy analysis)
    pub fn hint_count_for_target(&self, target: u64) -> usize {
        self.index.get(&target).map(|v| v.len()).unwrap_or(0)
    }

    /// Rebuild the index (called after loading or adding hints)
    fn rebuild_index(&mut self) {
        self.index.clear();
        
        for (hint_id, stored) in self.hints.iter().enumerate() {
            let indices = stored.subset.expand();
            for idx in indices {
                self.index
                    .entry(idx)
                    .or_insert_with(Vec::new)
                    .push(hint_id);
            }
        }
    }

    /// Update hints based on state changes
    pub fn apply_delta(&mut self, changes: &[(u64, [u8; ENTRY_SIZE], [u8; ENTRY_SIZE])]) {
        for &(idx, ref old_value, ref new_value) in changes {
            if let Some(hint_ids) = self.index.get(&idx) {
                for &hint_id in hint_ids {
                    if let Some(stored) = self.hints.get_mut(hint_id) {
                        pir_core::hint::update_hint(&mut stored.hint, old_value, new_value);
                    }
                }
            }
        }
    }

    /// Reset rotation state (e.g., after long idle period)
    pub fn reset_rotation(&mut self) {
        self.rotation = RotationState::new(self.rotation.max_recent);
    }

    /// Total storage size
    pub fn size_bytes(&self) -> usize {
        self.hints.len() * (std::mem::size_of::<StoredHint>())
    }

    /// Statistics for debugging
    pub fn stats(&self) -> HintStoreStats {
        let total_hints = self.hints.len();
        let indexed_targets = self.index.len();
        let avg_hints_per_target = if indexed_targets > 0 {
            self.index.values().map(|v| v.len()).sum::<usize>() as f64 / indexed_targets as f64
        } else {
            0.0
        };
        
        HintStoreStats {
            total_hints,
            indexed_targets,
            avg_hints_per_target,
            block_number: self.block_number,
        }
    }
}

/// Statistics about the hint store
#[derive(Debug)]
pub struct HintStoreStats {
    pub total_hints: usize,
    pub indexed_targets: usize,
    pub avg_hints_per_target: f64,
    pub block_number: u64,
}
