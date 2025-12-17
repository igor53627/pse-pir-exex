//! Two-lane configuration

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Protocol version constant
pub const PROTOCOL_VERSION: &str = "1.0.0";

/// Configuration for the two-lane PIR system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwoLaneConfig {
    /// Hot lane database path (encoded PIR database)
    pub hot_lane_db: PathBuf,
    /// Cold lane database path (encoded PIR database)
    pub cold_lane_db: PathBuf,
    /// Hot lane CRS (Common Reference String) path
    pub hot_lane_crs: PathBuf,
    /// Cold lane CRS path
    pub cold_lane_crs: PathBuf,
    /// Hot lane manifest (contract list)
    pub hot_lane_manifest: PathBuf,
    /// Number of entries in hot lane
    pub hot_entries: u64,
    /// Number of entries in cold lane
    pub cold_entries: u64,
    /// Entry size in bytes
    pub entry_size: usize,
    /// Protocol version
    #[serde(default = "default_version")]
    pub version: String,
    /// Configuration hash for change detection
    #[serde(default)]
    pub config_hash: Option<String>,
    /// Hot lane shards directory (for mmap mode)
    #[serde(default)]
    pub hot_lane_shards: Option<PathBuf>,
    /// Cold lane shards directory (for mmap mode)
    #[serde(default)]
    pub cold_lane_shards: Option<PathBuf>,
    /// Use mmap mode for database loading (faster swaps)
    #[serde(default)]
    pub use_mmap: bool,
    /// Shard size in bytes (for mmap mode, default 128KB)
    #[serde(default = "default_shard_size")]
    pub shard_size_bytes: u64,
}

fn default_shard_size() -> u64 {
    128 * 1024
}

fn default_version() -> String {
    PROTOCOL_VERSION.to_string()
}

impl TwoLaneConfig {
    /// Create a new configuration from a base directory
    ///
    /// Expects the following structure:
    /// ```text
    /// base_dir/
    ///   hot/
    ///     encoded.bin
    ///     crs.json
    ///     manifest.json
    ///   cold/
    ///     encoded.bin
    ///     crs.json
    /// ```
    pub fn from_base_dir(base_dir: impl Into<PathBuf>) -> Self {
        let base = base_dir.into();
        let hot = base.join("hot");
        let cold = base.join("cold");

        Self {
            hot_lane_db: hot.join("encoded.bin"),
            hot_lane_crs: hot.join("crs.json"),
            hot_lane_manifest: hot.join("manifest.json"),
            cold_lane_db: cold.join("encoded.bin"),
            cold_lane_crs: cold.join("crs.json"),
            hot_entries: 0,
            cold_entries: 0,
            entry_size: crate::constants::ENTRY_SIZE,
            version: PROTOCOL_VERSION.to_string(),
            config_hash: None,
            hot_lane_shards: Some(hot.join("shards")),
            cold_lane_shards: Some(cold.join("shards")),
            use_mmap: true,
            shard_size_bytes: default_shard_size(),
        }
    }

    /// Set the number of entries
    pub fn with_entries(mut self, hot: u64, cold: u64) -> Self {
        self.hot_entries = hot;
        self.cold_entries = cold;
        self
    }

    /// Enable mmap mode for faster database loading/swapping
    pub fn with_mmap(mut self, enabled: bool) -> Self {
        self.use_mmap = enabled;
        self
    }

    /// Load configuration from a JSON file
    pub fn load(path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to a JSON file
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> crate::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path.as_ref(), content)?;
        Ok(())
    }

    /// Total entries across both lanes
    pub fn total_entries(&self) -> u64 {
        self.hot_entries + self.cold_entries
    }

    /// Estimated average query size based on 90% hot lane hit rate
    pub fn estimated_avg_query_size(&self) -> usize {
        let hot_rate = 0.90;
        let cold_rate = 0.10;
        let hot_size = crate::constants::HOT_LANE_QUERY_SIZE as f64;
        let cold_size = crate::constants::COLD_LANE_QUERY_SIZE as f64;
        (hot_rate * hot_size + cold_rate * cold_size) as usize
    }

    /// Compute a hash of the configuration for change detection
    ///
    /// This hash includes entry counts and entry size, which are the key
    /// parameters that must match between client and server.
    pub fn compute_hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        self.hot_entries.hash(&mut hasher);
        self.cold_entries.hash(&mut hasher);
        self.entry_size.hash(&mut hasher);
        self.version.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Set the config hash (call after setting entries)
    pub fn with_hash(mut self) -> Self {
        self.config_hash = Some(self.compute_hash());
        self
    }
}

impl Default for TwoLaneConfig {
    fn default() -> Self {
        Self::from_base_dir("./pir-data")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_base_dir() {
        let config = TwoLaneConfig::from_base_dir("/data/pir");
        assert_eq!(config.hot_lane_db, PathBuf::from("/data/pir/hot/encoded.bin"));
        assert_eq!(config.cold_lane_crs, PathBuf::from("/data/pir/cold/crs.json"));
    }

    #[test]
    fn test_estimated_query_size() {
        let config = TwoLaneConfig::default();
        let avg = config.estimated_avg_query_size();
        assert!(avg > 50_000 && avg < 70_000);
    }

    #[test]
    fn test_config_hash_changes_on_shape_or_version_change() {
        let base = TwoLaneConfig::from_base_dir("/data/pir")
            .with_entries(100, 200)
            .with_hash();
        
        // Same config should produce same hash
        let same = TwoLaneConfig::from_base_dir("/data/pir")
            .with_entries(100, 200)
            .with_hash();
        assert_eq!(base.config_hash, same.config_hash);
        
        // Different hot entries should change hash
        let different_hot = TwoLaneConfig::from_base_dir("/data/pir")
            .with_entries(101, 200)
            .with_hash();
        assert_ne!(base.config_hash, different_hot.config_hash);
        
        // Different cold entries should change hash
        let different_cold = TwoLaneConfig::from_base_dir("/data/pir")
            .with_entries(100, 201)
            .with_hash();
        assert_ne!(base.config_hash, different_cold.config_hash);
    }

    #[test]
    fn test_config_has_version() {
        let config = TwoLaneConfig::default();
        assert_eq!(config.version, PROTOCOL_VERSION);
    }
}
