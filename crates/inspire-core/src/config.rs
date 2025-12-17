//! Two-lane configuration

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
        }
    }

    /// Set the number of entries
    pub fn with_entries(mut self, hot: u64, cold: u64) -> Self {
        self.hot_entries = hot;
        self.cold_entries = cold;
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
}
