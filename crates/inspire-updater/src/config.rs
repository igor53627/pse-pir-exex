use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct UpdaterConfig {
    /// ethrex public RPC (eth_* methods)
    pub rpc_url: String,
    /// ethrex admin RPC (pir_* methods) - optional, fallback to rpc_url
    pub admin_rpc_url: Option<String>,
    /// PIR server URL for reload
    pub pir_server_url: String,
    /// Directory to write PIR data
    pub data_dir: PathBuf,
    /// Poll interval
    pub poll_interval: Duration,
    /// Max blocks per delta fetch
    pub max_blocks_per_fetch: u64,
    /// Ethereum chain ID (1=mainnet, 11155111=sepolia)
    pub chain_id: u64,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8545".into(),
            admin_rpc_url: None,
            pir_server_url: "http://localhost:3000".into(),
            data_dir: PathBuf::from("./pir-data"),
            poll_interval: Duration::from_secs(1),
            max_blocks_per_fetch: 100,
            chain_id: 11155111, // Sepolia
        }
    }
}
