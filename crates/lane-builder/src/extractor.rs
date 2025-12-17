//! Contract extractor: identifies hot lane contracts

use std::collections::HashMap;
use std::path::Path;

use inspire_core::{Address, HotLaneManifest};

use crate::contracts::{ContractInfo, HOT_CONTRACTS};

/// Contract popularity data
#[derive(Debug, Clone)]
pub struct ContractStats {
    pub address: Address,
    pub name: String,
    pub category: String,
    pub tx_count: u64,
    pub storage_slots: u64,
}

/// Extracts and ranks contracts for hot lane inclusion
pub struct ContractExtractor {
    contracts: HashMap<Address, ContractStats>,
    max_contracts: usize,
    max_entries: u64,
}

impl ContractExtractor {
    /// Create a new extractor with default limits
    pub fn new() -> Self {
        Self {
            contracts: HashMap::new(),
            max_contracts: 1000,
            max_entries: 1_000_000,
        }
    }

    /// Set maximum number of contracts
    pub fn with_max_contracts(mut self, max: usize) -> Self {
        self.max_contracts = max;
        self
    }

    /// Set maximum total entries
    pub fn with_max_entries(mut self, max: u64) -> Self {
        self.max_entries = max;
        self
    }

    /// Load known contracts from the curated list
    pub fn load_known_contracts(&mut self) {
        for contract in HOT_CONTRACTS {
            self.add_contract(ContractStats {
                address: contract.address,
                name: contract.name.to_string(),
                category: contract.category.to_string(),
                tx_count: 0,
                storage_slots: 0,
            });
        }
    }

    /// Add a contract to the extractor
    pub fn add_contract(&mut self, stats: ContractStats) {
        self.contracts.insert(stats.address, stats);
    }

    /// Update storage slot count for a contract
    pub fn update_slots(&mut self, address: &Address, slots: u64) {
        if let Some(stats) = self.contracts.get_mut(address) {
            stats.storage_slots = slots;
        }
    }

    /// Get contracts sorted by popularity (tx_count)
    pub fn ranked_contracts(&self) -> Vec<&ContractStats> {
        let mut contracts: Vec<_> = self.contracts.values().collect();
        contracts.sort_by(|a, b| b.tx_count.cmp(&a.tx_count));
        contracts
    }

    /// Build a manifest from the current contracts
    pub fn build_manifest(&self, block_number: u64) -> HotLaneManifest {
        let mut manifest = HotLaneManifest::new(block_number);
        let mut total_entries = 0u64;
        let mut count = 0usize;

        for stats in self.ranked_contracts() {
            if count >= self.max_contracts {
                break;
            }
            
            let slots = if stats.storage_slots > 0 {
                stats.storage_slots
            } else {
                1000
            };

            if total_entries + slots > self.max_entries {
                continue;
            }

            manifest.add_contract(
                stats.address,
                stats.name.clone(),
                slots,
                stats.category.clone(),
            );
            
            total_entries += slots;
            count += 1;
        }

        manifest
    }

    /// Save manifest to a file
    pub fn save_manifest(&self, block_number: u64, path: &Path) -> anyhow::Result<()> {
        let manifest = self.build_manifest(block_number);
        manifest.save(path)?;
        Ok(())
    }

    /// Load contract info from a JSON file
    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let infos: Vec<ContractInfo> = serde_json::from_str(&content)?;
        
        let mut extractor = Self::new();
        for info in infos {
            extractor.add_contract(ContractStats {
                address: info.address,
                name: info.name,
                category: info.category,
                tx_count: info.tx_count.unwrap_or(0),
                storage_slots: info.storage_slots.unwrap_or(0),
            });
        }
        
        Ok(extractor)
    }

    /// Number of contracts currently tracked
    pub fn contract_count(&self) -> usize {
        self.contracts.len()
    }
}

impl Default for ContractExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extractor_new() {
        let extractor = ContractExtractor::new();
        assert_eq!(extractor.contract_count(), 0);
    }

    #[test]
    fn test_load_known_contracts() {
        let mut extractor = ContractExtractor::new();
        extractor.load_known_contracts();
        assert!(extractor.contract_count() >= 4);
    }

    #[test]
    fn test_build_manifest() {
        let mut extractor = ContractExtractor::new();
        extractor.add_contract(ContractStats {
            address: [0x11u8; 20],
            name: "Test1".into(),
            category: "token".into(),
            tx_count: 1000,
            storage_slots: 500,
        });
        extractor.add_contract(ContractStats {
            address: [0x22u8; 20],
            name: "Test2".into(),
            category: "defi".into(),
            tx_count: 2000,
            storage_slots: 300,
        });

        let manifest = extractor.build_manifest(12345);
        
        assert_eq!(manifest.contract_count(), 2);
        assert_eq!(manifest.total_entries, 800);
        assert_eq!(manifest.contracts[0].name, "Test2");
    }

    #[test]
    fn test_max_entries_limit() {
        let mut extractor = ContractExtractor::new().with_max_entries(500);
        extractor.add_contract(ContractStats {
            address: [0x11u8; 20],
            name: "Test1".into(),
            category: "token".into(),
            tx_count: 1000,
            storage_slots: 400,
        });
        extractor.add_contract(ContractStats {
            address: [0x22u8; 20],
            name: "Test2".into(),
            category: "defi".into(),
            tx_count: 500,
            storage_slots: 200,
        });

        let manifest = extractor.build_manifest(12345);
        
        assert_eq!(manifest.contract_count(), 1);
        assert_eq!(manifest.total_entries, 400);
    }
}
