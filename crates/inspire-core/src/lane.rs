//! Lane identifier for two-lane PIR architecture

use serde::{Deserialize, Serialize};
use std::fmt;

/// Lane identifier
///
/// The two-lane architecture splits the Ethereum state database:
/// - Hot: Top ~1000 most queried contracts (~1M entries, ~10 KB queries)
/// - Cold: Everything else (~2.7B entries, ~500 KB queries)
///
/// This reduces average query size from 500 KB to ~60 KB since ~90% of
/// wallet queries target popular contracts (USDC, WETH, Uniswap, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lane {
    /// Hot lane: top ~1000 contracts, ~1M entries, ~10 KB queries
    Hot,
    /// Cold lane: everything else, ~2.7B entries, ~500 KB queries
    Cold,
}

impl Lane {
    /// Returns the expected query size for this lane in bytes
    pub fn expected_query_size(&self) -> usize {
        match self {
            Lane::Hot => crate::constants::HOT_LANE_QUERY_SIZE,
            Lane::Cold => crate::constants::COLD_LANE_QUERY_SIZE,
        }
    }

    /// Returns true if this is the hot lane
    pub fn is_hot(&self) -> bool {
        matches!(self, Lane::Hot)
    }

    /// Returns true if this is the cold lane
    pub fn is_cold(&self) -> bool {
        matches!(self, Lane::Cold)
    }
}

impl fmt::Display for Lane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lane::Hot => write!(f, "hot"),
            Lane::Cold => write!(f, "cold"),
        }
    }
}

impl Default for Lane {
    fn default() -> Self {
        Lane::Cold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lane_serialization() {
        assert_eq!(serde_json::to_string(&Lane::Hot).unwrap(), "\"hot\"");
        assert_eq!(serde_json::to_string(&Lane::Cold).unwrap(), "\"cold\"");
    }

    #[test]
    fn test_lane_deserialization() {
        assert_eq!(serde_json::from_str::<Lane>("\"hot\"").unwrap(), Lane::Hot);
        assert_eq!(serde_json::from_str::<Lane>("\"cold\"").unwrap(), Lane::Cold);
    }

    #[test]
    fn test_lane_display() {
        assert_eq!(Lane::Hot.to_string(), "hot");
        assert_eq!(Lane::Cold.to_string(), "cold");
    }

    #[test]
    fn test_lane_query_sizes() {
        assert_eq!(Lane::Hot.expected_query_size(), 10_000);
        assert_eq!(Lane::Cold.expected_query_size(), 500_000);
    }
}
