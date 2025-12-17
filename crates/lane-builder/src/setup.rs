//! Two-lane database setup using inspire-pir
//!
//! Creates PIR databases for both hot and cold lanes.

use std::path::Path;

use inspire_core::{HotLaneManifest, TwoLaneConfig};
use inspire_pir::{
    setup as pir_setup,
    InspireParams, SecurityLevel,
    ServerCrs, EncodedDatabase,
};
use inspire_pir::math::GaussianSampler;
use inspire_pir::rlwe::RlweSecretKey;

/// Result of two-lane setup
pub struct TwoLaneSetupResult {
    /// Hot lane CRS
    pub hot_crs: ServerCrs,
    /// Hot lane encoded database
    pub hot_db: EncodedDatabase,
    /// Cold lane CRS
    pub cold_crs: ServerCrs,
    /// Cold lane encoded database
    pub cold_db: EncodedDatabase,
    /// Secret key (shared between lanes for simplicity)
    pub secret_key: RlweSecretKey,
    /// Configuration
    pub config: TwoLaneConfig,
}

/// Builder for two-lane PIR setup
pub struct TwoLaneSetup {
    output_dir: std::path::PathBuf,
    hot_data: Vec<u8>,
    cold_data: Vec<u8>,
    entry_size: usize,
    manifest: Option<HotLaneManifest>,
    params: InspireParams,
}

impl TwoLaneSetup {
    /// Create a new setup builder
    pub fn new(output_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            hot_data: Vec::new(),
            cold_data: Vec::new(),
            entry_size: 32,
            manifest: None,
            params: default_params(),
        }
    }

    /// Set the hot lane data
    pub fn hot_data(mut self, data: Vec<u8>) -> Self {
        self.hot_data = data;
        self
    }

    /// Set the cold lane data
    pub fn cold_data(mut self, data: Vec<u8>) -> Self {
        self.cold_data = data;
        self
    }

    /// Set entry size (default: 32 bytes for Ethereum storage slots)
    pub fn entry_size(mut self, size: usize) -> Self {
        self.entry_size = size;
        self
    }

    /// Set the hot lane manifest
    pub fn manifest(mut self, manifest: HotLaneManifest) -> Self {
        self.manifest = Some(manifest);
        self
    }

    /// Set custom PIR parameters
    pub fn params(mut self, params: InspireParams) -> Self {
        self.params = params;
        self
    }

    /// Run the setup and save to disk
    pub fn build(self) -> anyhow::Result<TwoLaneSetupResult> {
        let hot_dir = self.output_dir.join("hot");
        let cold_dir = self.output_dir.join("cold");
        
        std::fs::create_dir_all(&hot_dir)?;
        std::fs::create_dir_all(&cold_dir)?;

        let mut sampler = GaussianSampler::new(self.params.sigma);

        tracing::info!(
            hot_entries = self.hot_data.len() / self.entry_size,
            cold_entries = self.cold_data.len() / self.entry_size,
            "Setting up two-lane PIR databases"
        );

        let (hot_crs, hot_db, secret_key) = pir_setup(
            &self.params,
            &self.hot_data,
            self.entry_size,
            &mut sampler,
        ).map_err(|e| anyhow::anyhow!("{}", e))?;

        let (cold_crs, cold_db, _cold_sk) = pir_setup(
            &self.params,
            &self.cold_data,
            self.entry_size,
            &mut sampler,
        ).map_err(|e| anyhow::anyhow!("{}", e))?;

        save_crs(&hot_crs, &hot_dir.join("crs.json"))?;
        save_db(&hot_db, &hot_dir.join("encoded.json"))?;
        
        save_crs(&cold_crs, &cold_dir.join("crs.json"))?;
        save_db(&cold_db, &cold_dir.join("encoded.json"))?;

        if let Some(manifest) = &self.manifest {
            manifest.save(&hot_dir.join("manifest.json"))?;
        }

        save_secret_key(&secret_key, &self.output_dir.join("secret_key.json"))?;

        let config = TwoLaneConfig {
            hot_lane_db: hot_dir.join("encoded.json"),
            hot_lane_crs: hot_dir.join("crs.json"),
            hot_lane_manifest: hot_dir.join("manifest.json"),
            cold_lane_db: cold_dir.join("encoded.json"),
            cold_lane_crs: cold_dir.join("crs.json"),
            hot_entries: (self.hot_data.len() / self.entry_size) as u64,
            cold_entries: (self.cold_data.len() / self.entry_size) as u64,
            entry_size: self.entry_size,
        };

        config.save(&self.output_dir.join("config.json"))?;

        tracing::info!(
            hot_crs = %hot_dir.join("crs.json").display(),
            cold_crs = %cold_dir.join("crs.json").display(),
            "Two-lane setup complete"
        );

        Ok(TwoLaneSetupResult {
            hot_crs,
            hot_db,
            cold_crs,
            cold_db,
            secret_key,
            config,
        })
    }
}

/// Default PIR parameters for Ethereum state queries
pub fn default_params() -> InspireParams {
    InspireParams {
        ring_dim: 2048,
        q: 1152921504606830593, // 2^60 - 2^14 + 1
        p: 65536,              // 2^16
        sigma: 3.2,
        gadget_base: 1 << 20,
        gadget_len: 3,
        security_level: SecurityLevel::Bits128,
    }
}

/// Small parameters for testing
pub fn test_params() -> InspireParams {
    InspireParams {
        ring_dim: 256,
        q: 1152921504606830593,
        p: 65536,
        sigma: 3.2,
        gadget_base: 1 << 20,
        gadget_len: 3,
        security_level: SecurityLevel::Bits128,
    }
}

fn save_crs(crs: &ServerCrs, path: &Path) -> anyhow::Result<()> {
    let json = serde_json::to_string(crs)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn save_db(db: &EncodedDatabase, path: &Path) -> anyhow::Result<()> {
    let json = serde_json::to_string(db)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn save_secret_key(sk: &RlweSecretKey, path: &Path) -> anyhow::Result<()> {
    let json = serde_json::to_string(sk)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Load a secret key from disk
pub fn load_secret_key(path: &Path) -> anyhow::Result<RlweSecretKey> {
    let json = std::fs::read_to_string(path)?;
    let sk: RlweSecretKey = serde_json::from_str(&json)?;
    Ok(sk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_two_lane_setup() {
        let dir = tempdir().unwrap();
        
        let hot_data: Vec<u8> = (0..256 * 32).map(|i| (i % 256) as u8).collect();
        let cold_data: Vec<u8> = (0..256 * 32).map(|i| ((i + 1) % 256) as u8).collect();
        
        let result = TwoLaneSetup::new(dir.path())
            .hot_data(hot_data)
            .cold_data(cold_data)
            .entry_size(32)
            .params(test_params())
            .build()
            .unwrap();
        
        assert!(dir.path().join("hot/crs.json").exists());
        assert!(dir.path().join("hot/encoded.json").exists());
        assert!(dir.path().join("cold/crs.json").exists());
        assert!(dir.path().join("cold/encoded.json").exists());
        assert!(dir.path().join("config.json").exists());
        assert!(dir.path().join("secret_key.json").exists());
        
        assert_eq!(result.config.hot_entries, 256);
        assert_eq!(result.config.cold_entries, 256);
    }
}
