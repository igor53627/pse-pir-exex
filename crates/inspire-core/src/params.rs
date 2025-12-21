//! PIR parameter versioning for client/server compatibility
//!
//! When cryptographic parameters change (sigma, ring dimension, moduli, etc.),
//! the version must be bumped to prevent silent incompatibility.

use serde::{Deserialize, Serialize};

/// PIR parameter version
///
/// Bump this when changing:
/// - sigma (Gaussian noise parameter)
/// - ring dimension (d)
/// - modulus chain (q, p)
/// - decomposition/gadget parameters
/// - CRS serialization format
///
/// History:
/// - v1: Initial version (sigma = 3.2)
/// - v2: Updated sigma to 6.4 per InsPIRe paper
pub const PIR_PARAMS_VERSION: u16 = 2;

/// PIR parameters for RLWE-based PIR
///
/// These must match between client and server for queries to succeed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PirParams {
    /// Parameter version (must match PIR_PARAMS_VERSION)
    pub version: u16,
    /// Ring dimension (RLWE polynomial degree)
    pub ring_dim: u32,
    /// Gaussian noise parameter (sigma)
    pub sigma: f64,
    /// Ciphertext modulus
    pub q: u64,
    /// Plaintext modulus
    pub p: u64,
    /// Gadget base for decomposition
    pub gadget_base: u64,
    /// Gadget decomposition length
    pub gadget_len: usize,
}

impl PirParams {
    /// Check if parameters are compatible with current version
    pub fn is_compatible(&self) -> bool {
        self.version == PIR_PARAMS_VERSION
    }

    /// Validate parameters match current version
    pub fn validate(&self) -> Result<(), ParamsVersionError> {
        if self.version != PIR_PARAMS_VERSION {
            return Err(ParamsVersionError::VersionMismatch {
                expected: PIR_PARAMS_VERSION,
                actual: self.version,
            });
        }
        Ok(())
    }
}

/// Default production parameters (must match lane-builder defaults)
pub const PIR_PARAMS: PirParams = PirParams {
    version: PIR_PARAMS_VERSION,
    ring_dim: 2048,
    sigma: 6.4,
    q: 1152921504606830593, // 2^60 - 2^14 + 1
    p: 65536,               // 2^16
    gadget_base: 1 << 20,
    gadget_len: 3,
};

/// Error for parameter version mismatches
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ParamsVersionError {
    #[error("PIR params version mismatch: expected v{expected}, got v{actual}. Regenerate CRS/DB.")]
    VersionMismatch { expected: u16, actual: u16 },
}

/// CRS metadata sidecar (generated alongside CRS files)
///
/// Contains version info and generation metadata for validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrsMetadata {
    /// PIR parameter version
    pub pir_params_version: u16,
    /// Full PIR parameters used to generate CRS
    pub pir_params: PirParams,
    /// lane-builder version that generated this CRS
    pub generated_by: String,
    /// ISO 8601 timestamp of generation
    pub generated_at: String,
    /// Entry size in bytes
    pub entry_size: usize,
    /// Number of entries in this lane
    pub entry_count: u64,
    /// Lane name (hot/cold/balances)
    pub lane: String,
}

impl CrsMetadata {
    /// Create new metadata for a CRS file
    ///
    /// Note: `generated_at` should be an ISO 8601 timestamp string.
    /// Callers should use their preferred time library to generate this.
    pub fn new(
        params: &PirParams,
        entry_size: usize,
        entry_count: u64,
        lane: &str,
        generated_by: &str,
        generated_at: &str,
    ) -> Self {
        Self {
            pir_params_version: params.version,
            pir_params: params.clone(),
            generated_by: generated_by.to_string(),
            generated_at: generated_at.to_string(),
            entry_size,
            entry_count,
            lane: lane.to_string(),
        }
    }

    /// Validate metadata against current version
    ///
    /// Checks both that the version matches `PIR_PARAMS_VERSION` and that the
    /// metadata fields are internally consistent.
    pub fn validate(&self) -> Result<(), ParamsVersionError> {
        if self.pir_params_version != self.pir_params.version {
            return Err(ParamsVersionError::VersionMismatch {
                expected: self.pir_params_version,
                actual: self.pir_params.version,
            });
        }
        self.pir_params.validate()
    }

    /// Save metadata to a JSON file
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Load metadata from a JSON file
    pub fn load(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pir_params_version() {
        assert_eq!(PIR_PARAMS.version, PIR_PARAMS_VERSION);
        assert!(PIR_PARAMS.is_compatible());
    }

    #[test]
    fn test_version_mismatch() {
        let old_params = PirParams {
            version: 1,
            ..PIR_PARAMS
        };
        assert!(!old_params.is_compatible());
        assert!(matches!(
            old_params.validate(),
            Err(ParamsVersionError::VersionMismatch { expected: 2, actual: 1 })
        ));
    }

    #[test]
    fn test_crs_metadata_serialization() {
        let meta = CrsMetadata::new(
            &PIR_PARAMS,
            32,
            1000,
            "hot",
            "lane-builder 0.1.0",
            "2025-01-01T00:00:00Z",
        );
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: CrsMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.pir_params_version, PIR_PARAMS_VERSION);
        assert_eq!(parsed.lane, "hot");
    }
}
