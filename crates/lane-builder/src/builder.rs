//! Hot lane database builder

use std::path::Path;

use inspire_core::HotLaneManifest;

use crate::extractor::ContractExtractor;

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
