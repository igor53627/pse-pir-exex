//! Lane routing: determines which lane handles a query

use crate::{Address, HotLaneManifest, Lane, StorageKey};
use std::collections::HashSet;

/// Routes queries to the appropriate lane based on contract address
pub struct LaneRouter {
    hot_addresses: HashSet<Address>,
    manifest: HotLaneManifest,
}

impl LaneRouter {
    /// Create a router from a hot lane manifest
    pub fn new(manifest: HotLaneManifest) -> Self {
        let hot_addresses = manifest.address_set();
        Self {
            hot_addresses,
            manifest,
        }
    }

    /// Route a query to the appropriate lane
    pub fn route(&self, contract: &Address) -> Lane {
        if self.hot_addresses.contains(contract) {
            Lane::Hot
        } else {
            Lane::Cold
        }
    }

    /// Get the local index within a lane for a storage query
    ///
    /// For hot lane: returns index within the hot lane database
    /// For cold lane: returns the global index (caller must handle cold lane indexing)
    pub fn get_hot_index(&self, contract: &Address, _slot: &StorageKey) -> Option<u64> {
        let contract_info = self.manifest.get_contract(contract)?;
        Some(contract_info.start_index)
    }

    /// Get the manifest
    pub fn manifest(&self) -> &HotLaneManifest {
        &self.manifest
    }

    /// Number of contracts in hot lane
    pub fn hot_contract_count(&self) -> usize {
        self.hot_addresses.len()
    }

    /// Check if address is in hot lane
    pub fn is_hot(&self, address: &Address) -> bool {
        self.hot_addresses.contains(address)
    }
}

/// Query target: identifies what the client wants to query
#[derive(Debug, Clone)]
pub struct QueryTarget {
    /// Contract address
    pub contract: Address,
    /// Storage slot key
    pub slot: StorageKey,
}

impl QueryTarget {
    pub fn new(contract: Address, slot: StorageKey) -> Self {
        Self { contract, slot }
    }
}

/// Routed query: a query with its determined lane and index
#[derive(Debug, Clone)]
pub struct RoutedQuery {
    /// Original query target
    pub target: QueryTarget,
    /// Determined lane
    pub lane: Lane,
    /// Index within the lane's database
    pub index: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_manifest() -> HotLaneManifest {
        let mut manifest = HotLaneManifest::new(1000);
        manifest.add_contract([0x11u8; 20], "USDC".into(), 1000, "token".into());
        manifest.add_contract([0x22u8; 20], "WETH".into(), 500, "token".into());
        manifest
    }

    #[test]
    fn test_routing() {
        let router = LaneRouter::new(create_test_manifest());
        
        assert_eq!(router.route(&[0x11u8; 20]), Lane::Hot);
        assert_eq!(router.route(&[0x22u8; 20]), Lane::Hot);
        assert_eq!(router.route(&[0x33u8; 20]), Lane::Cold);
    }

    #[test]
    fn test_hot_index() {
        let router = LaneRouter::new(create_test_manifest());
        let slot = [0u8; 32];
        
        assert_eq!(router.get_hot_index(&[0x11u8; 20], &slot), Some(0));
        assert_eq!(router.get_hot_index(&[0x22u8; 20], &slot), Some(1000));
        assert_eq!(router.get_hot_index(&[0x33u8; 20], &slot), None);
    }
}
