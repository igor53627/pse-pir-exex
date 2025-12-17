//! Two-lane PIR client implementation

use reqwest::Client;
use serde::{Deserialize, Serialize};

use inspire_core::{Address, Lane, LaneRouter, StorageKey, StorageValue};
use inspire_pir::{
    ServerCrs, ClientQuery, ClientState, ServerResponse,
    query as pir_query, extract,
    InspireParams,
};
use inspire_pir::math::GaussianSampler;
use inspire_pir::rlwe::RlweSecretKey;

use crate::error::{ClientError, Result};

/// Response from CRS endpoint
#[derive(Deserialize)]
pub struct CrsResponse {
    pub crs: String,
    pub lane: Lane,
    pub entry_count: u64,
}

/// Request to query endpoint
#[derive(Serialize)]
struct QueryRequest {
    query: String,
}

/// Response from query endpoint
#[derive(Deserialize)]
pub struct QueryResponse {
    pub response: String,
    pub lane: Lane,
}

/// Lane-specific client state
struct LaneState {
    crs: ServerCrs,
    secret_key: RlweSecretKey,
    entry_count: u64,
}

/// Two-lane PIR client that routes queries to the appropriate lane
pub struct TwoLaneClient {
    router: LaneRouter,
    http: Client,
    server_url: String,
    hot_state: Option<LaneState>,
    cold_state: Option<LaneState>,
}

impl TwoLaneClient {
    /// Create a new client with the given router and server URL
    pub fn new(router: LaneRouter, server_url: String) -> Self {
        Self {
            router,
            http: Client::new(),
            server_url: server_url.trim_end_matches('/').to_string(),
            hot_state: None,
            cold_state: None,
        }
    }

    /// Initialize the client by fetching CRS from server and generating keys
    pub async fn init(&mut self) -> Result<()> {
        let hot_crs_resp = self.fetch_crs(Lane::Hot).await?;
        let hot_crs: ServerCrs = serde_json::from_str(&hot_crs_resp.crs)?;
        let hot_sk = generate_secret_key(&hot_crs.params);
        self.hot_state = Some(LaneState {
            crs: hot_crs,
            secret_key: hot_sk,
            entry_count: hot_crs_resp.entry_count,
        });
        
        let cold_crs_resp = self.fetch_crs(Lane::Cold).await?;
        let cold_crs: ServerCrs = serde_json::from_str(&cold_crs_resp.crs)?;
        let cold_sk = generate_secret_key(&cold_crs.params);
        self.cold_state = Some(LaneState {
            crs: cold_crs,
            secret_key: cold_sk,
            entry_count: cold_crs_resp.entry_count,
        });
        
        tracing::info!(
            hot_entries = self.hot_state.as_ref().map(|s| s.entry_count).unwrap_or(0),
            cold_entries = self.cold_state.as_ref().map(|s| s.entry_count).unwrap_or(0),
            "Client initialized with both lanes"
        );
        
        Ok(())
    }

    /// Fetch CRS for a specific lane
    pub async fn fetch_crs(&self, lane: Lane) -> Result<CrsResponse> {
        let url = format!("{}/crs/{}", self.server_url, lane);
        let resp = self.http.get(&url).send().await?;
        
        if !resp.status().is_success() {
            return Err(ClientError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        
        let crs_resp: CrsResponse = resp.json().await?;
        Ok(crs_resp)
    }

    /// Query a storage slot using PIR
    pub async fn query(&self, contract: Address, slot: StorageKey) -> Result<StorageValue> {
        let lane = self.router.route(&contract);
        
        tracing::debug!(
            contract = hex::encode(contract),
            lane = %lane,
            "Routing query"
        );

        let lane_state = self.get_lane_state(lane)?;
        
        let index = self.compute_index(&contract, &slot, lane)?;
        
        let (client_state, client_query) = self.build_pir_query(lane_state, index)?;
        
        let response = self.send_query(lane, &client_query).await?;
        
        let server_response: ServerResponse = serde_json::from_str(&response.response)?;
        
        let entry = extract(
            &lane_state.crs,
            &client_state,
            &server_response,
            32,
        ).map_err(|e| ClientError::InvalidResponse(e.to_string()))?;
        
        let mut result = [0u8; 32];
        result.copy_from_slice(&entry[..32]);
        Ok(result)
    }

    /// Build a PIR query for the given index
    fn build_pir_query(&self, lane_state: &LaneState, index: u64) -> Result<(ClientState, ClientQuery)> {
        let mut sampler = GaussianSampler::new(lane_state.crs.params.sigma);
        
        let shard_config = inspire_pir::params::ShardConfig {
            shard_size_bytes: (lane_state.crs.params.ring_dim as u64) * 32,
            entry_size_bytes: 32,
            total_entries: lane_state.entry_count,
        };
        
        let (state, query) = pir_query(
            &lane_state.crs,
            index,
            &shard_config,
            &lane_state.secret_key,
            &mut sampler,
        ).map_err(|e| ClientError::InvalidResponse(format!("Failed to build query: {}", e)))?;
        
        Ok((state, query))
    }

    /// Send a query to the server
    async fn send_query(&self, lane: Lane, query: &ClientQuery) -> Result<QueryResponse> {
        let url = format!("{}/query/{}", self.server_url, lane);
        
        let query_json = serde_json::to_string(query)?;
        
        let resp = self.http
            .post(&url)
            .json(&QueryRequest { query: query_json })
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let query_resp: QueryResponse = resp.json().await?;
        Ok(query_resp)
    }

    /// Get lane state
    fn get_lane_state(&self, lane: Lane) -> Result<&LaneState> {
        match lane {
            Lane::Hot => self.hot_state.as_ref().ok_or_else(|| {
                ClientError::LaneNotAvailable("Hot lane not initialized".to_string())
            }),
            Lane::Cold => self.cold_state.as_ref().ok_or_else(|| {
                ClientError::LaneNotAvailable("Cold lane not initialized".to_string())
            }),
        }
    }

    /// Compute the database index for a contract/slot pair
    fn compute_index(&self, contract: &Address, _slot: &StorageKey, lane: Lane) -> Result<u64> {
        if lane == Lane::Hot {
            if let Some(idx) = self.router.get_hot_index(contract, _slot) {
                return Ok(idx);
            }
        }
        Ok(0)
    }

    /// Get which lane a contract would be routed to
    pub fn get_lane(&self, contract: &Address) -> Lane {
        self.router.route(contract)
    }

    /// Check if a contract is in the hot lane
    pub fn is_hot(&self, contract: &Address) -> bool {
        self.router.is_hot(contract)
    }

    /// Get the number of contracts in the hot lane
    pub fn hot_contract_count(&self) -> usize {
        self.router.hot_contract_count()
    }
}

/// Generate a secret key for PIR
fn generate_secret_key(params: &InspireParams) -> RlweSecretKey {
    let mut sampler = GaussianSampler::new(params.sigma);
    RlweSecretKey::generate(params, &mut sampler)
}

/// Builder for TwoLaneClient
pub struct ClientBuilder {
    server_url: String,
    manifest_path: Option<std::path::PathBuf>,
}

impl ClientBuilder {
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            manifest_path: None,
        }
    }

    pub fn manifest(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.manifest_path = Some(path.into());
        self
    }

    pub fn build(self) -> Result<TwoLaneClient> {
        let manifest = if let Some(path) = self.manifest_path {
            inspire_core::HotLaneManifest::load(&path)?
        } else {
            inspire_core::HotLaneManifest::new(0)
        };
        
        let router = LaneRouter::new(manifest);
        Ok(TwoLaneClient::new(router, self.server_url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspire_core::HotLaneManifest;

    fn create_test_manifest() -> HotLaneManifest {
        let mut manifest = HotLaneManifest::new(1000);
        manifest.add_contract([0x11u8; 20], "Test1".into(), 100, "token".into());
        manifest.add_contract([0x22u8; 20], "Test2".into(), 200, "defi".into());
        manifest
    }

    #[test]
    fn test_client_routing() {
        let router = LaneRouter::new(create_test_manifest());
        let client = TwoLaneClient::new(router, "http://localhost:3000".into());
        
        assert!(client.is_hot(&[0x11u8; 20]));
        assert!(client.is_hot(&[0x22u8; 20]));
        assert!(!client.is_hot(&[0x33u8; 20]));
        
        assert_eq!(client.get_lane(&[0x11u8; 20]), Lane::Hot);
        assert_eq!(client.get_lane(&[0x33u8; 20]), Lane::Cold);
    }

    #[test]
    fn test_hot_contract_count() {
        let router = LaneRouter::new(create_test_manifest());
        let client = TwoLaneClient::new(router, "http://localhost:3000".into());
        
        assert_eq!(client.hot_contract_count(), 2);
    }
}
