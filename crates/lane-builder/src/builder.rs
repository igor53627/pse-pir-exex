//! Hot lane database builder

use std::path::Path;

use inspire_core::HotLaneManifest;

use crate::extractor::{ContractExtractor, ContractStats};
use crate::hybrid_scorer::ScoredContract;

/// Builds the hot lane database from contract data
pub struct HotLaneBuilder {
    extractor: ContractExtractor,
    output_dir: std::path::PathBuf,
    block_number: u64,
}

impl HotLaneBuilder {
    /// Create a new builder with output directory
    pub fn new(output_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            extractor: ContractExtractor::new(),
            output_dir: output_dir.into(),
            block_number: 0,
        }
    }

    /// Set the block number for the manifest
    pub fn at_block(mut self, block: u64) -> Self {
        self.block_number = block;
        self
    }

    /// Load known contracts from the curated list
    pub fn load_known_contracts(mut self) -> Self {
        self.extractor.load_known_contracts();
        self
    }

    /// Load contracts from a JSON file
    pub fn load_from_file(mut self, path: &Path) -> anyhow::Result<Self> {
        self.extractor = ContractExtractor::load_from_file(path)?;
        Ok(self)
    }

    /// Load contracts from scored contracts (hybrid ranking output)
    pub fn load_scored_contracts(mut self, scored: &[ScoredContract]) -> Self {
        for sc in scored {
            self.extractor.add_contract(ContractStats {
                address: sc.address,
                name: sc.name.clone().unwrap_or_else(|| format!("0x{}", hex::encode(&sc.address[..6]))),
                category: sc.category.clone().unwrap_or_else(|| "unknown".to_string()),
                tx_count: sc.final_score,
                storage_slots: 0,
            });
        }
        self
    }

    /// Load scored contracts from a JSON file (output of lane-backfill)
    pub fn load_scored_from_file(self, path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let scored: Vec<ScoredContract> = serde_json::from_str(&content)?;
        Ok(self.load_scored_contracts(&scored))
    }

    /// Set maximum contracts
    pub fn max_contracts(mut self, max: usize) -> Self {
        self.extractor = self.extractor.with_max_contracts(max);
        self
    }

    /// Set maximum entries
    pub fn max_entries(mut self, max: u64) -> Self {
        self.extractor = self.extractor.with_max_entries(max);
        self
    }

    /// Build the manifest and save to output directory
    pub fn build(self) -> anyhow::Result<HotLaneManifest> {
        std::fs::create_dir_all(&self.output_dir)?;
        
        let manifest = self.extractor.build_manifest(self.block_number);
        
        let manifest_path = self.output_dir.join("manifest.json");
        manifest.save(&manifest_path)?;
        
        tracing::info!(
            contracts = manifest.contract_count(),
            entries = manifest.total_entries,
            path = %manifest_path.display(),
            "Hot lane manifest built"
        );
        
        Ok(manifest)
    }

    /// Get a reference to the extractor
    pub fn extractor(&self) -> &ContractExtractor {
        &self.extractor
    }

    /// Get a mutable reference to the extractor
    pub fn extractor_mut(&mut self) -> &mut ContractExtractor {
        &mut self.extractor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_builder_with_known_contracts() {
        let dir = tempdir().unwrap();
        
        let manifest = HotLaneBuilder::new(dir.path())
            .at_block(12345)
            .load_known_contracts()
            .build()
            .unwrap();
        
        assert!(manifest.contract_count() >= 4);
        assert_eq!(manifest.block_number, 12345);
        
        let manifest_path = dir.path().join("manifest.json");
        assert!(manifest_path.exists());
    }
}
