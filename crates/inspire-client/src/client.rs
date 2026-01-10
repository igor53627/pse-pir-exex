//! Two-lane PIR client implementation

use reqwest::Client;
use serde::{Deserialize, Serialize};

use inspire_core::{Address, Lane, LaneRouter, StorageKey, StorageValue};
use inspire_pir::math::GaussianSampler;
use inspire_pir::params::InspireVariant;
use inspire_pir::rlwe::RlweSecretKey;
use inspire_pir::{
    extract_with_variant, query as pir_query, query_seeded as pir_query_seeded,
    query_switched as pir_query_switched, ClientQuery, ClientState, InspireParams,
    SeededClientQuery, ServerCrs, ServerResponse, SwitchedClientQuery,
};

use crate::bucket_index::BucketIndex;
use crate::error::{ClientError, Result};

/// Response from CRS endpoint
#[derive(Deserialize)]
pub struct CrsResponse {
    pub crs: String,
    pub lane: Lane,
    pub entry_count: u64,
    pub shard_config: inspire_pir::params::ShardConfig,
}

/// Request to query endpoint (full ciphertext)
#[derive(Serialize)]
struct QueryRequest {
    query: ClientQuery,
}

/// Request to seeded query endpoint (~50% smaller)
#[derive(Serialize)]
struct SeededQueryRequest {
    query: SeededClientQuery,
}

/// Request to switched query endpoint (~75% smaller)
#[derive(Serialize)]
struct SwitchedQueryRequest {
    query: SwitchedClientQuery,
}

/// Response from query endpoint
#[derive(Deserialize)]
pub struct QueryResponse {
    pub response: ServerResponse,
    pub lane: Lane,
}

/// Lane-specific client state
struct LaneState {
    crs: ServerCrs,
    secret_key: RlweSecretKey,
    entry_count: u64,
    shard_config: inspire_pir::params::ShardConfig,
}

/// Two-lane PIR client that routes queries to the appropriate lane
pub struct TwoLaneClient {
    router: LaneRouter,
    http: Client,
    server_url: String,
    hot_state: Option<LaneState>,
    cold_state: Option<LaneState>,
    /// Use seed expansion for ~50% smaller queries
    use_seed_expansion: bool,
    /// Use switched+seeded queries for maximum compression
    use_switched_query: bool,
    /// Use binary responses for ~58% smaller downloads
    use_binary_response: bool,
    /// Bucket index for sparse lookups (optional, used for cold lane)
    bucket_index: Option<BucketIndex>,
}

impl std::fmt::Debug for TwoLaneClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwoLaneClient")
            .field("server_url", &self.server_url)
            .field("hot_contract_count", &self.router.hot_contract_count())
            .finish_non_exhaustive()
    }
}

impl TwoLaneClient {
    /// Create a new client with the given router and server URL
    pub fn new(router: LaneRouter, server_url: String) -> Self {
        Self::with_options(router, server_url, true, false, true) // seed expansion + binary response on by default
    }

    /// Create a new client with explicit settings
    pub fn with_options(
        router: LaneRouter,
        server_url: String,
        use_seed_expansion: bool,
        use_switched_query: bool,
        use_binary_response: bool,
    ) -> Self {
        Self {
            router,
            http: Client::new(),
            server_url: server_url.trim_end_matches('/').to_string(),
            hot_state: None,
            cold_state: None,
            use_seed_expansion,
            use_switched_query,
            use_binary_response,
            bucket_index: None,
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
            shard_config: hot_crs_resp.shard_config,
        });

        let cold_crs_resp = self.fetch_crs(Lane::Cold).await?;
        let cold_crs: ServerCrs = serde_json::from_str(&cold_crs_resp.crs)?;
        let cold_sk = generate_secret_key(&cold_crs.params);
        let cold_entries = cold_crs_resp.entry_count;
        self.cold_state = Some(LaneState {
            crs: cold_crs,
            secret_key: cold_sk,
            entry_count: cold_entries,
            shard_config: cold_crs_resp.shard_config,
        });

        // Update router with cold lane entry count for proper indexing
        self.router.set_cold_entries(cold_entries);

        tracing::info!(
            hot_entries = self.hot_state.as_ref().map(|s| s.entry_count).unwrap_or(0),
            cold_entries = cold_entries,
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

    /// Fetch and store the bucket index from server
    ///
    /// The bucket index enables O(1) index lookups without downloading the full manifest.
    /// ~150 KB compressed download.
    pub async fn fetch_bucket_index(&mut self) -> Result<()> {
        let url = format!("{}/index", self.server_url);
        let resp = self.http.get(&url).send().await?;

        if !resp.status().is_success() {
            return Err(ClientError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let bytes = resp.bytes().await?;
        let index = BucketIndex::from_compressed(&bytes).map_err(|e| {
            ClientError::InvalidResponse(format!("Failed to parse bucket index: {}", e))
        })?;

        tracing::info!(
            total_entries = index.total_entries(),
            size_kb = bytes.len() / 1024,
            "Bucket index loaded"
        );

        self.bucket_index = Some(index);
        Ok(())
    }

    /// Set the bucket index directly (e.g., from local cache)
    pub fn set_bucket_index(&mut self, index: BucketIndex) {
        self.bucket_index = Some(index);
    }

    /// Get the current bucket index
    pub fn bucket_index(&self) -> Option<&BucketIndex> {
        self.bucket_index.as_ref()
    }

    /// Check if bucket index is loaded
    pub fn has_bucket_index(&self) -> bool {
        self.bucket_index.is_some()
    }

    /// Query a storage slot using PIR
    pub async fn query(&self, contract: Address, slot: StorageKey) -> Result<StorageValue> {
        let lane = self.router.route(&contract);

        tracing::debug!(
            contract = hex::encode(contract),
            lane = %lane,
            seed_expansion = self.use_seed_expansion,
            binary_response = self.use_binary_response,
            "Routing query"
        );

        let lane_state = self.get_lane_state(lane)?;
        let index = self.compute_index(&contract, &slot, lane)?;

        let (client_state, server_response) = if self.use_switched_query {
            if self.use_binary_response {
                let (state, query) = self.build_pir_query_switched(lane_state, index)?;
                let resp = self.send_query_switched_binary(lane, &query).await?;
                (state, resp)
            } else {
                let (state, query) = self.build_pir_query_switched(lane_state, index)?;
                let resp = self.send_query_switched(lane, &query).await?;
                (state, resp.response)
            }
        } else {
            match (self.use_seed_expansion, self.use_binary_response) {
                (true, true) => {
                    let (state, query) = self.build_pir_query_seeded(lane_state, index)?;
                    let resp = self.send_query_seeded_binary(lane, &query).await?;
                    (state, resp)
                }
                (true, false) => {
                    let (state, query) = self.build_pir_query_seeded(lane_state, index)?;
                    let resp = self.send_query_seeded(lane, &query).await?;
                    (state, resp.response)
                }
                (false, true) => {
                    let (state, query) = self.build_pir_query(lane_state, index)?;
                    let resp = self.send_query_binary(lane, &query).await?;
                    (state, resp)
                }
                (false, false) => {
                    let (state, query) = self.build_pir_query(lane_state, index)?;
                    let resp = self.send_query(lane, &query).await?;
                    (state, resp.response)
                }
            }
        };

        let entry = extract_with_variant(
            &lane_state.crs,
            &client_state,
            &server_response,
            32,
            InspireVariant::OnePacking,
        )
        .map_err(|e| ClientError::InvalidResponse(e.to_string()))?;

        let mut result = [0u8; 32];
        result.copy_from_slice(&entry[..32]);
        Ok(result)
    }

    /// Build a PIR query for the given index (full ciphertext)
    fn build_pir_query(
        &self,
        lane_state: &LaneState,
        index: u64,
    ) -> Result<(ClientState, ClientQuery)> {
        let mut sampler = GaussianSampler::new(lane_state.crs.params.sigma);

        let (state, query) = pir_query(
            &lane_state.crs,
            index,
            &lane_state.shard_config,
            &lane_state.secret_key,
            &mut sampler,
        )
        .map_err(|e| ClientError::InvalidResponse(format!("Failed to build query: {}", e)))?;

        Ok((state, query))
    }

    /// Build a seeded PIR query for the given index (~50% smaller)
    fn build_pir_query_seeded(
        &self,
        lane_state: &LaneState,
        index: u64,
    ) -> Result<(ClientState, SeededClientQuery)> {
        let mut sampler = GaussianSampler::new(lane_state.crs.params.sigma);

        let (state, query) = pir_query_seeded(
            &lane_state.crs,
            index,
            &lane_state.shard_config,
            &lane_state.secret_key,
            &mut sampler,
        )
        .map_err(|e| {
            ClientError::InvalidResponse(format!("Failed to build seeded query: {}", e))
        })?;

        Ok((state, query))
    }

    /// Build a switched PIR query for the given index (~75% smaller)
    fn build_pir_query_switched(
        &self,
        lane_state: &LaneState,
        index: u64,
    ) -> Result<(ClientState, SwitchedClientQuery)> {
        let mut sampler = GaussianSampler::new(lane_state.crs.params.sigma);

        let (state, query) = pir_query_switched(
            &lane_state.crs,
            index,
            &lane_state.shard_config,
            &lane_state.secret_key,
            &mut sampler,
        )
        .map_err(|e| {
            ClientError::InvalidResponse(format!("Failed to build switched query: {}", e))
        })?;

        Ok((state, query))
    }

    /// Send a query to the server (full ciphertext)
    async fn send_query(&self, lane: Lane, query: &ClientQuery) -> Result<QueryResponse> {
        let url = format!("{}/query/{}", self.server_url, lane);

        let resp = self
            .http
            .post(&url)
            .json(&QueryRequest {
                query: query.clone(),
            })
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

    /// Send a seeded query to the server (~50% smaller)
    async fn send_query_seeded(
        &self,
        lane: Lane,
        query: &SeededClientQuery,
    ) -> Result<QueryResponse> {
        let url = format!("{}/query/{}/seeded", self.server_url, lane);

        let resp = self
            .http
            .post(&url)
            .json(&SeededQueryRequest {
                query: query.clone(),
            })
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

    /// Send a seeded query with binary response (~75% smaller total)
    async fn send_query_seeded_binary(
        &self,
        lane: Lane,
        query: &SeededClientQuery,
    ) -> Result<ServerResponse> {
        let url = format!("{}/query/{}/seeded/binary", self.server_url, lane);

        let resp = self
            .http
            .post(&url)
            .json(&SeededQueryRequest {
                query: query.clone(),
            })
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let bytes = resp.bytes().await?;
        let response = ServerResponse::from_binary(&bytes).map_err(|e| {
            ClientError::InvalidResponse(format!("Failed to decode binary response: {}", e))
        })?;
        Ok(response)
    }

    /// Send a switched query to the server (~75% smaller)
    async fn send_query_switched(
        &self,
        lane: Lane,
        query: &SwitchedClientQuery,
    ) -> Result<QueryResponse> {
        let url = format!("{}/query/{}/switched", self.server_url, lane);

        let resp = self
            .http
            .post(&url)
            .json(&SwitchedQueryRequest {
                query: query.clone(),
            })
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

    /// Send a switched query with binary response
    async fn send_query_switched_binary(
        &self,
        lane: Lane,
        query: &SwitchedClientQuery,
    ) -> Result<ServerResponse> {
        let url = format!("{}/query/{}/switched/binary", self.server_url, lane);

        let resp = self
            .http
            .post(&url)
            .json(&SwitchedQueryRequest {
                query: query.clone(),
            })
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let bytes = resp.bytes().await?;
        let response = ServerResponse::from_binary(&bytes).map_err(|e| {
            ClientError::InvalidResponse(format!("Failed to decode binary response: {}", e))
        })?;
        Ok(response)
    }

    /// Send a query with binary response (~58% smaller)
    async fn send_query_binary(&self, lane: Lane, query: &ClientQuery) -> Result<ServerResponse> {
        let url = format!("{}/query/{}/binary", self.server_url, lane);

        let resp = self
            .http
            .post(&url)
            .json(&QueryRequest {
                query: query.clone(),
            })
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let bytes = resp.bytes().await?;
        let response = ServerResponse::from_binary(&bytes).map_err(|e| {
            ClientError::InvalidResponse(format!("Failed to decode binary response: {}", e))
        })?;
        Ok(response)
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
    fn compute_index(&self, contract: &Address, slot: &StorageKey, lane: Lane) -> Result<u64> {
        match lane {
            Lane::Hot => self.router.get_hot_index(contract, slot).ok_or_else(|| {
                ClientError::InvalidResponse(format!(
                    "Contract {} not found in hot lane manifest or has invalid slot_count",
                    hex::encode(contract)
                ))
            }),
            Lane::Cold => self.router.get_cold_index(contract, slot).ok_or_else(|| {
                ClientError::LaneNotAvailable(
                    "Cold lane not initialized (cold_total_entries = 0)".to_string(),
                )
            }),
        }
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

    /// Look up bucket range for a contract/slot using the bucket index
    ///
    /// Returns (start_index, count) for the bucket containing this entry.
    /// The client must query within this range to find the exact entry.
    ///
    /// This enables sparse lookups without downloading the full manifest.
    pub fn lookup_bucket(
        &self,
        contract: &Address,
        slot: &StorageKey,
    ) -> Result<crate::BucketRange> {
        let index = self
            .bucket_index
            .as_ref()
            .ok_or_else(|| ClientError::LaneNotAvailable("Bucket index not loaded".to_string()))?;

        Ok(index.lookup_bucket(contract, slot))
    }

    /// Apply a delta update to the bucket index
    ///
    /// Called when receiving updates via websocket subscription.
    pub fn apply_bucket_delta(&mut self, delta: &crate::BucketDelta) {
        if let Some(ref mut index) = self.bucket_index {
            index.apply_delta(delta);
            tracing::debug!(
                block = delta.block_number,
                updates = delta.updates.len(),
                "Applied bucket index delta"
            );
        }
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
    use_seed_expansion: bool,
    use_switched_query: bool,
    use_binary_response: bool,
}

impl ClientBuilder {
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            manifest_path: None,
            use_seed_expansion: true,  // on by default
            use_switched_query: false,
            use_binary_response: true, // on by default
        }
    }

    pub fn manifest(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.manifest_path = Some(path.into());
        self
    }

    /// Enable/disable seed expansion (~50% smaller queries)
    pub fn seed_expansion(mut self, enabled: bool) -> Self {
        self.use_seed_expansion = enabled;
        self
    }

    /// Enable/disable switched+seeded queries (~75% smaller queries)
    pub fn switched_query(mut self, enabled: bool) -> Self {
        self.use_switched_query = enabled;
        self
    }

    /// Enable/disable binary responses (~58% smaller downloads)
    pub fn binary_response(mut self, enabled: bool) -> Self {
        self.use_binary_response = enabled;
        self
    }

    pub fn build(self) -> Result<TwoLaneClient> {
        let manifest = if let Some(path) = self.manifest_path {
            inspire_core::HotLaneManifest::load(&path)?
        } else {
            inspire_core::HotLaneManifest::new(0)
        };

        let router = LaneRouter::new(manifest);
        Ok(TwoLaneClient::with_options(
            router,
            self.server_url,
            self.use_seed_expansion,
            self.use_switched_query,
            self.use_binary_response,
        ))
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
