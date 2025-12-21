//! Two-lane database setup using inspire-pir
//!
//! Creates PIR databases for both hot and cold lanes.

use std::path::Path;

use inspire_core::{HotLaneManifest, TwoLaneConfig, CrsMetadata, PirParams, PIR_PARAMS_VERSION};
use inspire_pir::{
    setup as pir_setup,
    InspireParams, SecurityLevel,
    ServerCrs, EncodedDatabase,
};
use inspire_pir::math::GaussianSampler;
use inspire_pir::rlwe::RlweSecretKey;

/// Convert InspireParams to PirParams for metadata
fn to_pir_params(p: &InspireParams) -> PirParams {
    PirParams {
        version: PIR_PARAMS_VERSION,
        ring_dim: p.ring_dim as u32,
        sigma: p.sigma,
        q: p.q,
        p: p.p,
        gadget_base: p.gadget_base,
        gadget_len: p.gadget_len,
    }
}

/// Get current timestamp in ISO 8601 format
fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    format!("{}", secs)
}

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
    /// Configuration
    pub config: TwoLaneConfig,
}

/// Builder for two-lane PIR setup
///
/// # Security Note
///
/// This builder creates server-side PIR data only (CRS and encoded database).
/// Secret keys are NOT saved to disk - clients should generate their own keys.
/// For testing/development, use `emit_secret_key()` to optionally save a key
/// to a separate (non-server) location.
pub struct TwoLaneSetup {
    output_dir: std::path::PathBuf,
    hot_data: Vec<u8>,
    cold_data: Vec<u8>,
    entry_size: usize,
    manifest: Option<HotLaneManifest>,
    params: InspireParams,
    #[cfg(any(test, feature = "dev-keys"))]
    secret_key_path: Option<std::path::PathBuf>,
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
            #[cfg(any(test, feature = "dev-keys"))]
            secret_key_path: None,
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

    /// Emit secret key to a specified path (for testing/development only)
    ///
    /// # Security Warning
    ///
    /// This should only be used for testing or air-gapped client setup.
    /// In production, clients should generate their own secret keys.
    /// Never store secret keys on the same host as the PIR server.
    ///
    /// In particular, do NOT store dev keys inside the same directory tree
    /// as the generated PIR CRS/DB. This function will panic if you attempt
    /// to write the key inside the output directory.
    ///
    /// # Panics
    ///
    /// Panics if `path` is inside the PIR output directory.
    #[cfg(any(test, feature = "dev-keys"))]
    pub fn emit_secret_key(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        
        // Guard: don't allow secret key in or under the PIR output directory
        if path.starts_with(&self.output_dir) {
            panic!(
                "emit_secret_key path must not be inside the PIR output directory ({}). \
                 Store dev keys separately from server data.",
                self.output_dir.display()
            );
        }
        
        self.secret_key_path = Some(path);
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

        // Note: pir_setup returns a secret key, but we discard it for security.
        // Clients should generate their own keys. The secret key from setup is
        // only used internally and not returned or saved (unless emit_secret_key is used).
        let (hot_crs, hot_db, _hot_sk) = pir_setup(
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

        let pir_params = to_pir_params(&self.params);
        let generated_by = format!("lane-builder {}", env!("CARGO_PKG_VERSION"));
        let generated_at = now_iso8601();

        let hot_entries = (self.hot_data.len() / self.entry_size) as u64;
        let cold_entries = (self.cold_data.len() / self.entry_size) as u64;

        let hot_meta = CrsMetadata::new(
            &pir_params,
            self.entry_size,
            hot_entries,
            "hot",
            &generated_by,
            &generated_at,
        );
        hot_meta.save(&hot_dir.join("crs.meta.json"))?;

        let cold_meta = CrsMetadata::new(
            &pir_params,
            self.entry_size,
            cold_entries,
            "cold",
            &generated_by,
            &generated_at,
        );
        cold_meta.save(&cold_dir.join("crs.meta.json"))?;

        tracing::info!(
            pir_params_version = PIR_PARAMS_VERSION,
            "Generated CRS metadata sidecars"
        );

        if let Some(manifest) = &self.manifest {
            manifest.save(&hot_dir.join("manifest.json"))?;
        }

        // Only save secret key if explicitly requested (dev/test only)
        #[cfg(any(test, feature = "dev-keys"))]
        if let Some(sk_path) = &self.secret_key_path {
            // For testing, we need a key - regenerate one since we discarded the setup keys
            let test_sk = RlweSecretKey::generate(&self.params, &mut sampler);
            save_secret_key(&test_sk, sk_path)?;
            tracing::warn!(
                path = %sk_path.display(),
                "Secret key saved for testing - DO NOT use in production"
            );
        }

        let config = TwoLaneConfig::from_base_dir(&self.output_dir)
            .with_entries(hot_entries, cold_entries)
            .with_hash();

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
        sigma: 6.4,            // Updated to match InsPIRe paper
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
        sigma: 6.4,            // Updated to match InsPIRe paper
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

#[cfg(any(test, feature = "dev-keys"))]
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
        assert!(dir.path().join("hot/crs.meta.json").exists());
        assert!(dir.path().join("cold/crs.json").exists());
        assert!(dir.path().join("cold/encoded.json").exists());
        assert!(dir.path().join("cold/crs.meta.json").exists());
        assert!(dir.path().join("config.json").exists());
        // Secret key should NOT be saved by default (security)
        assert!(!dir.path().join("secret_key.json").exists());
        
        assert_eq!(result.config.hot_entries, 256);
        assert_eq!(result.config.cold_entries, 256);

        // Verify CRS metadata content
        let hot_meta = CrsMetadata::load(dir.path().join("hot/crs.meta.json")).unwrap();
        assert_eq!(hot_meta.pir_params_version, PIR_PARAMS_VERSION);
        assert_eq!(hot_meta.entry_count, 256);
        assert_eq!(hot_meta.lane, "hot");
        assert!(hot_meta.validate().is_ok());
    }

    #[test]
    fn test_emit_secret_key() {
        // Use separate directories for PIR data and secret key
        let pir_dir = tempdir().unwrap();
        let key_dir = tempdir().unwrap();
        let sk_path = key_dir.path().join("test_secret_key.json");
        
        let hot_data: Vec<u8> = (0..256 * 32).map(|i| (i % 256) as u8).collect();
        let cold_data: Vec<u8> = (0..256 * 32).map(|i| ((i + 1) % 256) as u8).collect();
        
        TwoLaneSetup::new(pir_dir.path())
            .hot_data(hot_data)
            .cold_data(cold_data)
            .entry_size(32)
            .params(test_params())
            .emit_secret_key(&sk_path)
            .build()
            .unwrap();
        
        // Secret key should be saved when explicitly requested
        assert!(sk_path.exists());
        
        // Should be loadable
        let _sk = load_secret_key(&sk_path).unwrap();
    }

    #[test]
    #[should_panic(expected = "emit_secret_key path must not be inside the PIR output directory")]
    fn test_emit_secret_key_in_output_dir_panics() {
        let dir = tempdir().unwrap();
        // Try to save secret key inside the PIR output directory - should panic
        let sk_path = dir.path().join("secret_key.json");
        
        let hot_data: Vec<u8> = (0..256 * 32).map(|i| (i % 256) as u8).collect();
        let cold_data: Vec<u8> = (0..256 * 32).map(|i| ((i + 1) % 256) as u8).collect();
        
        TwoLaneSetup::new(dir.path())
            .hot_data(hot_data)
            .cold_data(cold_data)
            .entry_size(32)
            .params(test_params())
            .emit_secret_key(&sk_path) // This should panic
            .build()
            .unwrap();
    }
}
