//! Hot lane manifest: tracks which contracts are in the hot lane

use crate::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A contract included in the hot lane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotContract {
    /// Contract address (20 bytes)
    #[serde(with = "hex_address")]
    pub address: Address,
    /// Human-readable name (e.g., "USDC", "Uniswap V3 Router")
    pub name: String,
    /// Number of storage slots for this contract
    pub slot_count: u64,
    /// Starting index in the hot lane database
    pub start_index: u64,
    /// Category (e.g., "defi", "token", "privacy", "nft")
    pub category: String,
}

/// Hot lane manifest containing all contracts in the hot lane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotLaneManifest {
    /// Block number when this manifest was generated
    pub block_number: u64,
    /// Timestamp of generation (Unix seconds)
    pub timestamp: u64,
    /// List of contracts in the hot lane, ordered by index
    pub contracts: Vec<HotContract>,
    /// Total entries in the hot lane (sum of all slot_counts)
    pub total_entries: u64,
    /// Version of the manifest format
    pub version: u32,
}

impl HotLaneManifest {
    /// Create a new empty manifest
    pub fn new(block_number: u64) -> Self {
        Self {
            block_number,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            contracts: Vec::new(),
            total_entries: 0,
            version: 1,
        }
    }

    /// Add a contract to the manifest
    pub fn add_contract(&mut self, address: Address, name: String, slot_count: u64, category: String) {
        let start_index = self.total_entries;
        self.contracts.push(HotContract {
            address,
            name,
            slot_count,
            start_index,
            category,
        });
        self.total_entries += slot_count;
    }

    /// Check if an address is in the hot lane
    pub fn contains(&self, address: &Address) -> bool {
        self.contracts.iter().any(|c| &c.address == address)
    }

    /// Get contract by address
    pub fn get_contract(&self, address: &Address) -> Option<&HotContract> {
        self.contracts.iter().find(|c| &c.address == address)
    }

    /// Build a fast lookup set of addresses
    pub fn address_set(&self) -> HashSet<Address> {
        self.contracts.iter().map(|c| c.address).collect()
    }

    /// Load manifest from JSON file
    pub fn load(path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let manifest = serde_json::from_str(&content)?;
        Ok(manifest)
    }

    /// Save manifest to JSON file
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> crate::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path.as_ref(), content)?;
        Ok(())
    }

    /// Number of contracts in the hot lane
    pub fn contract_count(&self) -> usize {
        self.contracts.len()
    }
}

mod hex_address {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(address: &[u8; 20], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_str = format!("0x{}", hex::encode(address));
        serializer.serialize_str(&hex_str)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 20], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.strip_prefix("0x").unwrap_or(&s);
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        bytes.try_into().map_err(|_| serde::de::Error::custom("invalid address length"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_address() -> Address {
        let mut addr = [0u8; 20];
        addr[19] = 1;
        addr
    }

    #[test]
    fn test_manifest_add_contract() {
        let mut manifest = HotLaneManifest::new(1000);
        manifest.add_contract(test_address(), "Test".into(), 100, "defi".into());
        
        assert_eq!(manifest.contract_count(), 1);
        assert_eq!(manifest.total_entries, 100);
        assert!(manifest.contains(&test_address()));
    }

    #[test]
    fn test_manifest_indexing() {
        let mut manifest = HotLaneManifest::new(1000);
        
        let addr1 = [1u8; 20];
        let addr2 = [2u8; 20];
        
        manifest.add_contract(addr1, "Contract1".into(), 100, "defi".into());
        manifest.add_contract(addr2, "Contract2".into(), 200, "token".into());
        
        assert_eq!(manifest.contracts[0].start_index, 0);
        assert_eq!(manifest.contracts[1].start_index, 100);
        assert_eq!(manifest.total_entries, 300);
    }

    #[test]
    fn test_address_serialization() {
        let mut manifest = HotLaneManifest::new(1000);
        manifest.add_contract([0xdeu8; 20], "Test".into(), 50, "token".into());
        
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("0xdededede"));
        
        let parsed: HotLaneManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.contracts[0].address, [0xdeu8; 20]);
    }
}
