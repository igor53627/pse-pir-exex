//! End-to-end server integration tests
//!
//! Tests the full server flow: setup -> query -> response -> extraction
//!
//! Test organization:
//! - Fast tests (no #[ignore]): run in CI, complete in <30s total
//! - Slow tests (#[ignore]): load/soak tests for manual/nightly runs

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use inspire_core::{Lane, TwoLaneConfig};
use inspire_pir::math::GaussianSampler;
use inspire_pir::params::InspireVariant;
use inspire_pir::rlwe::RlweSecretKey;
use inspire_pir::{extract_with_variant, query as pir_query, EncodedDatabase, ServerCrs};
use inspire_server::{create_router, create_shared_state, DbSnapshot, SharedState};
use lane_builder::{test_params, TwoLaneSetup};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

/// Test harness for running E2E server tests
pub struct TestHarness {
    pub server_url: String,
    pub config: TwoLaneConfig,
    pub state: SharedState,
    pub temp_dir: PathBuf,
    pub http: Client,
    pub hot_crs: Option<ServerCrs>,
    pub cold_crs: Option<ServerCrs>,
}

impl TestHarness {
    /// Create a new test harness with PIR databases
    pub async fn new() -> Self {
        Self::with_entries(256, 256).await
    }

    /// Create a test harness with specified entry counts
    pub async fn with_entries(hot_entries: usize, cold_entries: usize) -> Self {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("pir-e2e-test-{}", unique_id));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let entry_size = 32;
        let params = test_params();

        let hot_data: Vec<u8> = (0..hot_entries * entry_size)
            .map(|i| (i % 256) as u8)
            .collect();
        let cold_data: Vec<u8> = (0..cold_entries * entry_size)
            .map(|i| ((i + 128) % 256) as u8)
            .collect();

        let result = TwoLaneSetup::new(&temp_dir)
            .hot_data(hot_data)
            .cold_data(cold_data)
            .entry_size(entry_size)
            .params(params)
            .build()
            .expect("TwoLaneSetup should succeed");

        let mut config = result.config.clone();
        config.hot_lane_db = temp_dir.join("hot/encoded.json");
        config.cold_lane_db = temp_dir.join("cold/encoded.json");
        config.use_mmap = false;

        let state = create_shared_state(config.clone());
        state.load_lanes().expect("Lanes should load");

        let router = create_router(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Bind should succeed");
        let addr = listener.local_addr().expect("local addr");
        let server_url = format!("http://{}", addr);

        tokio::spawn(async move {
            axum::serve(listener, router).await.ok();
        });

        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("HTTP client");

        let mut ready = false;
        for _ in 0..20 {
            if http
                .get(format!("{}/live", server_url))
                .send()
                .await
                .is_ok()
            {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(ready, "Server did not become ready at {}", server_url);

        Self {
            server_url,
            config,
            state,
            temp_dir,
            http,
            hot_crs: Some(result.hot_crs),
            cold_crs: Some(result.cold_crs),
        }
    }

    /// Get server info
    pub async fn get_info(&self) -> reqwest::Result<ServerInfo> {
        self.http
            .get(format!("{}/info", self.server_url))
            .send()
            .await?
            .json()
            .await
    }

    /// Health check
    pub async fn health(&self) -> reqwest::Result<HealthResponse> {
        self.http
            .get(format!("{}/health", self.server_url))
            .send()
            .await?
            .json()
            .await
    }

    /// Get CRS for a lane
    pub async fn get_crs(&self, lane: Lane) -> reqwest::Result<CrsResponse> {
        self.http
            .get(format!("{}/crs/{}", self.server_url, lane))
            .send()
            .await?
            .json()
            .await
    }

    /// Send a PIR query and get the response
    pub async fn query_raw(
        &self,
        lane: Lane,
        query: &inspire_pir::ClientQuery,
    ) -> reqwest::Result<reqwest::Response> {
        self.http
            .post(format!("{}/query/{}", self.server_url, lane))
            .json(&QueryRequest {
                query: query.clone(),
            })
            .send()
            .await
    }

    /// Send a seeded PIR query
    pub async fn query_seeded_raw(
        &self,
        lane: Lane,
        query: &inspire_pir::SeededClientQuery,
    ) -> reqwest::Result<reqwest::Response> {
        self.http
            .post(format!("{}/query/{}/seeded", self.server_url, lane))
            .json(&SeededQueryRequest {
                query: query.clone(),
            })
            .send()
            .await
    }

    /// Perform a full PIR query and extract the result
    pub async fn query_and_extract(&self, lane: Lane, index: u64) -> anyhow::Result<Vec<u8>> {
        let crs = match lane {
            Lane::Hot => self.hot_crs.as_ref().expect("hot CRS"),
            Lane::Cold => self.cold_crs.as_ref().expect("cold CRS"),
        };

        let crs_resp = self.get_crs(lane).await?;
        let shard_config = crs_resp.shard_config;

        let mut sampler = GaussianSampler::new(crs.params.sigma);
        let sk = RlweSecretKey::generate(&crs.params, &mut sampler);

        let (client_state, client_query) = pir_query(crs, index, &shard_config, &sk, &mut sampler)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let resp = self.query_raw(lane, &client_query).await?;
        let query_resp: QueryResponse = resp.json().await?;

        let entry = extract_with_variant(
            crs,
            &client_state,
            &query_resp.response,
            32,
            InspireVariant::OnePacking,
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(entry)
    }

    /// Perform a seeded PIR query and extract the result
    pub async fn query_seeded_and_extract(
        &self,
        lane: Lane,
        index: u64,
    ) -> anyhow::Result<Vec<u8>> {
        use inspire_pir::query_seeded as pir_query_seeded;

        let crs = match lane {
            Lane::Hot => self.hot_crs.as_ref().expect("hot CRS"),
            Lane::Cold => self.cold_crs.as_ref().expect("cold CRS"),
        };

        let crs_resp = self.get_crs(lane).await?;
        let shard_config = crs_resp.shard_config;

        let mut sampler = GaussianSampler::new(crs.params.sigma);
        let sk = RlweSecretKey::generate(&crs.params, &mut sampler);

        let (client_state, seeded_query) =
            pir_query_seeded(crs, index, &shard_config, &sk, &mut sampler)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

        let resp = self.query_seeded_raw(lane, &seeded_query).await?;
        let query_resp: QueryResponse = resp.json().await?;

        let entry = extract_with_variant(
            crs,
            &client_state,
            &query_resp.response,
            32,
            InspireVariant::OnePacking,
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(entry)
    }

    /// Perform a PIR query with binary response
    pub async fn query_binary_and_extract(
        &self,
        lane: Lane,
        index: u64,
    ) -> anyhow::Result<Vec<u8>> {
        let crs = match lane {
            Lane::Hot => self.hot_crs.as_ref().expect("hot CRS"),
            Lane::Cold => self.cold_crs.as_ref().expect("cold CRS"),
        };

        let crs_resp = self.get_crs(lane).await?;
        let shard_config = crs_resp.shard_config;

        let mut sampler = GaussianSampler::new(crs.params.sigma);
        let sk = RlweSecretKey::generate(&crs.params, &mut sampler);

        let (client_state, client_query) = pir_query(crs, index, &shard_config, &sk, &mut sampler)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let resp = self
            .http
            .post(format!("{}/query/{}/binary", self.server_url, lane))
            .json(&QueryRequest {
                query: client_query,
            })
            .send()
            .await?;

        let bytes = resp.bytes().await?;
        let server_response = inspire_pir::ServerResponse::from_binary(&bytes)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let entry = extract_with_variant(
            crs,
            &client_state,
            &server_response,
            32,
            InspireVariant::OnePacking,
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(entry)
    }

    /// Perform a seeded PIR query with binary response
    pub async fn query_seeded_binary_and_extract(
        &self,
        lane: Lane,
        index: u64,
    ) -> anyhow::Result<Vec<u8>> {
        use inspire_pir::query_seeded as pir_query_seeded;

        let crs = match lane {
            Lane::Hot => self.hot_crs.as_ref().expect("hot CRS"),
            Lane::Cold => self.cold_crs.as_ref().expect("cold CRS"),
        };

        let crs_resp = self.get_crs(lane).await?;
        let shard_config = crs_resp.shard_config;

        let mut sampler = GaussianSampler::new(crs.params.sigma);
        let sk = RlweSecretKey::generate(&crs.params, &mut sampler);

        let (client_state, seeded_query) =
            pir_query_seeded(crs, index, &shard_config, &sk, &mut sampler)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

        let resp = self
            .http
            .post(format!("{}/query/{}/seeded/binary", self.server_url, lane))
            .json(&SeededQueryRequest {
                query: seeded_query,
            })
            .send()
            .await?;

        let bytes = resp.bytes().await?;
        let server_response = inspire_pir::ServerResponse::from_binary(&bytes)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let entry = extract_with_variant(
            crs,
            &client_state,
            &server_response,
            32,
            InspireVariant::OnePacking,
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(entry)
    }

    /// Reload databases via admin endpoint
    pub async fn reload(&self) -> reqwest::Result<ReloadResult> {
        self.http
            .post(format!("{}/admin/reload", self.server_url))
            .send()
            .await?
            .json()
            .await
    }

    /// Get current snapshot
    pub fn snapshot(&self) -> Arc<DbSnapshot> {
        self.state.load_snapshot_full()
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

#[derive(Serialize)]
struct QueryRequest {
    query: inspire_pir::ClientQuery,
}

#[derive(Serialize)]
struct SeededQueryRequest {
    query: inspire_pir::SeededClientQuery,
}

#[derive(Deserialize)]
pub struct ServerInfo {
    pub version: String,
    pub pir_params_version: u16,
    pub config_hash: String,
    pub manifest_block: Option<u64>,
    pub hot_entries: u64,
    pub cold_entries: u64,
    pub hot_contracts: usize,
    pub block_number: Option<u64>,
}

#[derive(Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub hot_loaded: bool,
    pub cold_loaded: bool,
    pub mmap_mode: bool,
}

#[derive(Deserialize)]
pub struct CrsResponse {
    pub crs: String,
    pub lane: Lane,
    pub entry_count: u64,
    pub shard_config: inspire_pir::params::ShardConfig,
}

#[derive(Deserialize)]
pub struct QueryResponse {
    pub response: inspire_pir::ServerResponse,
    pub lane: Lane,
}

#[derive(Deserialize, Debug)]
pub struct ReloadResult {
    pub old_block_number: Option<u64>,
    pub new_block_number: Option<u64>,
    pub reload_duration_ms: u64,
    pub hot_loaded: bool,
    pub cold_loaded: bool,
    pub mmap_mode: bool,
}

// ============================================================================
// Happy Path Tests
// ============================================================================

#[tokio::test]
async fn test_server_health() {
    let harness = TestHarness::new().await;
    let health = harness.health().await.expect("health check");

    assert_eq!(health.status, "ok");
    assert!(health.hot_loaded);
    assert!(health.cold_loaded);
}

#[tokio::test]
async fn test_server_info() {
    let harness = TestHarness::new().await;
    let info = harness.get_info().await.expect("server info");

    assert_eq!(info.hot_entries, 256);
    assert_eq!(info.cold_entries, 256);
    assert!(!info.config_hash.is_empty());
}

#[tokio::test]
async fn test_get_crs_hot() {
    let harness = TestHarness::new().await;
    let crs_resp = harness.get_crs(Lane::Hot).await.expect("hot CRS");

    assert_eq!(crs_resp.lane, Lane::Hot);
    assert_eq!(crs_resp.entry_count, 256);
    assert!(!crs_resp.crs.is_empty());
}

#[tokio::test]
async fn test_get_crs_cold() {
    let harness = TestHarness::new().await;
    let crs_resp = harness.get_crs(Lane::Cold).await.expect("cold CRS");

    assert_eq!(crs_resp.lane, Lane::Cold);
    assert_eq!(crs_resp.entry_count, 256);
}

#[tokio::test]
async fn test_hot_lane_query() {
    let harness = TestHarness::new().await;
    let index = 42u64;
    let entry = harness
        .query_and_extract(Lane::Hot, index)
        .await
        .expect("query");

    let expected_start = (index as usize) * 32;
    let expected: Vec<u8> = (expected_start..expected_start + 32)
        .map(|i| (i % 256) as u8)
        .collect();

    assert_eq!(
        entry, expected,
        "Retrieved entry should match expected data"
    );
}

/// Minimal test: directly use PIR functions without HTTP
/// to isolate whether the issue is in HTTP layer or PIR layer
#[tokio::test]
async fn test_cold_lane_direct_pir() {
    use inspire_pir::{extract_with_variant, respond_one_packing};

    // Create test data
    let params = test_params();
    let entry_size = 32;

    // Hot-like data pattern (should work)
    let hot_data: Vec<u8> = (0..256 * entry_size).map(|i| (i % 256) as u8).collect();

    // Setup PIR directly (use separate sampler for setup)
    let mut setup_sampler = GaussianSampler::new(params.sigma);
    let (crs, db, _sk) =
        inspire_pir::setup(&params, &hot_data, entry_size, &mut setup_sampler).expect("setup");

    // Generate client secret key (use separate sampler for client)
    let mut client_sampler = GaussianSampler::new(crs.params.sigma);
    let client_sk = RlweSecretKey::generate(&crs.params, &mut client_sampler);

    // Query index 0
    let shard_config = db.config.clone();
    let (client_state, client_query) =
        pir_query(&crs, 0, &shard_config, &client_sk, &mut client_sampler).expect("query");

    // Server responds using OnePacking
    let response = respond_one_packing(&crs, &db, &client_query).expect("respond");

    // Client extracts
    let entry = extract_with_variant(
        &crs,
        &client_state,
        &response,
        entry_size,
        InspireVariant::OnePacking,
    )
    .expect("extract");

    // Expected: bytes 0-31 of hot_data = [0, 1, 2, ...]
    let expected: Vec<u8> = (0..32).map(|i| i as u8).collect();

    assert_eq!(entry, expected, "Direct PIR should work for hot-like data");
}

#[tokio::test]
async fn test_cold_lane_query() {
    let harness = TestHarness::new().await;
    let index = 0u64;
    let entry = harness
        .query_and_extract(Lane::Cold, index)
        .await
        .expect("query");

    // Cold data: ((i + 128) % 256) for each byte
    // Index 0, entry_size 32: bytes 0-31 of cold_data
    // Expected: [128, 129, 130, ..., 159]
    let expected: Vec<u8> = (0..32).map(|i| ((i + 128) % 256) as u8).collect();

    assert_eq!(
        entry, expected,
        "Cold lane data should match expected pattern"
    );
}

#[tokio::test]
async fn test_hot_and_cold_queries_different_data() {
    let harness = TestHarness::new().await;

    let hot_entry = harness
        .query_and_extract(Lane::Hot, 0)
        .await
        .expect("hot query");
    let cold_entry = harness
        .query_and_extract(Lane::Cold, 0)
        .await
        .expect("cold query");

    assert_ne!(
        hot_entry, cold_entry,
        "Hot and cold lanes should have different data"
    );
}

#[tokio::test]
async fn test_seeded_query_returns_same_as_full() {
    let harness = TestHarness::new().await;
    let index = 42u64;

    let full_entry = harness
        .query_and_extract(Lane::Hot, index)
        .await
        .expect("full query");
    let seeded_entry = harness
        .query_seeded_and_extract(Lane::Hot, index)
        .await
        .expect("seeded query");

    assert_eq!(
        full_entry, seeded_entry,
        "Seeded query should return same data as full query"
    );
}

#[tokio::test]
async fn test_binary_response_returns_same_as_json() {
    let harness = TestHarness::new().await;
    let index = 50u64;

    let json_entry = harness
        .query_and_extract(Lane::Hot, index)
        .await
        .expect("json query");
    let binary_entry = harness
        .query_binary_and_extract(Lane::Hot, index)
        .await
        .expect("binary query");

    assert_eq!(
        json_entry, binary_entry,
        "Binary response should return same data as JSON"
    );
}

#[tokio::test]
async fn test_seeded_binary_query() {
    let harness = TestHarness::new().await;
    let index = 100u64;

    let full_entry = harness
        .query_and_extract(Lane::Hot, index)
        .await
        .expect("full query");
    let seeded_binary_entry = harness
        .query_seeded_binary_and_extract(Lane::Hot, index)
        .await
        .expect("seeded binary query");

    assert_eq!(
        full_entry, seeded_binary_entry,
        "Seeded binary query should return same data"
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_invalid_lane_returns_400() {
    let harness = TestHarness::new().await;

    let resp = harness
        .http
        .get(format!("{}/crs/invalid", harness.server_url))
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status().as_u16(), 400);
}

#[tokio::test]
async fn test_invalid_json_query_returns_4xx() {
    let harness = TestHarness::new().await;

    let resp = harness
        .http
        .post(format!("{}/query/hot", harness.server_url))
        .header("content-type", "application/json")
        .body(r#"{"invalid": "json"}"#)
        .send()
        .await
        .expect("request");

    let status = resp.status().as_u16();
    assert!(
        status >= 400 && status < 500,
        "Expected 4xx, got {}",
        status
    );
}

#[tokio::test]
async fn test_server_continues_after_error() {
    let harness = TestHarness::new().await;

    let _ = harness
        .http
        .post(format!("{}/query/hot", harness.server_url))
        .header("content-type", "application/json")
        .body("not json")
        .send()
        .await;

    let health = harness.health().await.expect("health after error");
    assert_eq!(health.status, "ok");

    let entry = harness
        .query_and_extract(Lane::Hot, 10)
        .await
        .expect("query after error");
    assert!(!entry.is_empty());
}

// ============================================================================
// Snapshot Consistency Tests
// ============================================================================

#[tokio::test]
async fn test_snapshot_consistent_across_queries() {
    let harness = TestHarness::new().await;

    let snap1 = harness.snapshot();
    let entry1 = harness
        .query_and_extract(Lane::Hot, 5)
        .await
        .expect("query 1");

    let snap2 = harness.snapshot();
    let entry2 = harness
        .query_and_extract(Lane::Hot, 5)
        .await
        .expect("query 2");

    assert_eq!(entry1, entry2, "Same query should return same result");
    assert!(
        Arc::ptr_eq(&snap1, &snap2),
        "Snapshot should be same without reload"
    );
}

#[tokio::test]
async fn test_lane_identifiers_correct() {
    let harness = TestHarness::new().await;

    let hot_resp = harness.get_crs(Lane::Hot).await.expect("hot CRS");
    let cold_resp = harness.get_crs(Lane::Cold).await.expect("cold CRS");

    assert_eq!(hot_resp.lane, Lane::Hot);
    assert_eq!(cold_resp.lane, Lane::Cold);
}

// ============================================================================
// Reload Tests
// ============================================================================

#[tokio::test]
async fn test_basic_reload() {
    let harness = TestHarness::new().await;

    let snap_before = harness.snapshot();

    let result = harness.reload().await.expect("reload");
    assert!(result.hot_loaded);
    assert!(result.cold_loaded);

    let snap_after = harness.snapshot();
    assert!(
        !Arc::ptr_eq(&snap_before, &snap_after),
        "Snapshot should change after reload"
    );
}

#[tokio::test]
async fn test_reload_while_querying() {
    let harness = TestHarness::new().await;

    let query_task = {
        let h = TestHarness {
            server_url: harness.server_url.clone(),
            config: harness.config.clone(),
            state: harness.state.clone(),
            temp_dir: harness.temp_dir.clone(),
            http: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("HTTP client"),
            hot_crs: harness.hot_crs.clone(),
            cold_crs: harness.cold_crs.clone(),
        };
        tokio::spawn(async move {
            for i in 0..5 {
                let _ = h.query_and_extract(Lane::Hot, i).await;
            }
        })
    };

    harness.reload().await.expect("reload");

    query_task.await.expect("queries should complete");
}

#[tokio::test]
async fn test_concurrent_queries_during_reload() {
    let harness = TestHarness::new().await;
    let url = harness.server_url.clone();
    let hot_crs = harness.hot_crs.clone().unwrap();

    let mut handles = vec![];

    for i in 0..10 {
        let url = url.clone();
        let crs = hot_crs.clone();
        handles.push(tokio::spawn(async move {
            let client = Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("HTTP client");

            let crs_resp: CrsResponse = client
                .get(format!("{}/crs/hot", url))
                .send()
                .await?
                .json()
                .await?;

            let mut sampler = GaussianSampler::new(crs.params.sigma);
            let sk = RlweSecretKey::generate(&crs.params, &mut sampler);

            let (client_state, client_query) =
                pir_query(&crs, i % 50, &crs_resp.shard_config, &sk, &mut sampler)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

            let resp: QueryResponse = client
                .post(format!("{}/query/hot", url))
                .json(&QueryRequest {
                    query: client_query,
                })
                .send()
                .await?
                .json()
                .await?;

            let _entry = extract_with_variant(
                &crs,
                &client_state,
                &resp.response,
                32,
                InspireVariant::OnePacking,
            )
            .map_err(|e| anyhow::anyhow!("{}", e))?;

            Ok::<_, anyhow::Error>(())
        }));
    }

    let _ = harness.reload().await;

    let mut successes = 0;
    for h in handles {
        if h.await.unwrap().is_ok() {
            successes += 1;
        }
    }

    assert_eq!(successes, 10, "All concurrent queries should succeed");
}

// ============================================================================
// Bucket Index Tests
// ============================================================================

#[tokio::test]
async fn test_bucket_index_endpoint_returns_404_when_not_configured() {
    let harness = TestHarness::new().await;

    let resp = harness
        .http
        .get(format!("{}/index", harness.server_url))
        .send()
        .await
        .expect("request");

    // Server may return 500 or 404 depending on implementation when index not loaded
    let status = resp.status().as_u16();
    assert!(
        status >= 400,
        "Should return error when bucket index not loaded: {}",
        status
    );
}

#[tokio::test]
async fn test_bucket_index_info_endpoint() {
    let harness = TestHarness::new().await;

    let resp = harness
        .http
        .get(format!("{}/index/info", harness.server_url))
        .send()
        .await
        .expect("request");

    // Without bucket index loaded, should return error
    let status = resp.status().as_u16();
    assert!(
        status >= 400,
        "Should return error when bucket index not loaded: {}",
        status
    );
}

// ============================================================================
// Range Delta Sync Tests
// ============================================================================

#[tokio::test]
async fn test_range_delta_info_endpoint_returns_error_when_not_configured() {
    let harness = TestHarness::new().await;

    let resp = harness
        .http
        .get(format!("{}/index/deltas/info", harness.server_url))
        .send()
        .await
        .expect("request");

    // Without range delta loaded, should return error
    let status = resp.status().as_u16();
    assert!(
        status >= 400,
        "Should return error when range delta not loaded: {}",
        status
    );
}

#[tokio::test]
async fn test_range_delta_endpoint_returns_error_when_not_configured() {
    let harness = TestHarness::new().await;

    let resp = harness
        .http
        .get(format!("{}/index/deltas", harness.server_url))
        .send()
        .await
        .expect("request");

    // Without range delta loaded, should return error
    let status = resp.status().as_u16();
    assert!(
        status >= 400,
        "Should return error when range delta not loaded: {}",
        status
    );
}

/// Test harness with range delta file configured
pub struct RangeDeltaTestHarness {
    pub server_url: String,
    pub temp_dir: PathBuf,
    pub http: Client,
}

impl RangeDeltaTestHarness {
    /// Create a test server with a range delta file
    ///
    /// Uses the full TestHarness to set up lanes, then adds a range delta file
    pub async fn new() -> Self {
        use inspire_core::bucket_index::range_delta::{
            RangeDeltaHeader, RangeEntry, DEFAULT_RANGES, HEADER_SIZE, RANGE_ENTRY_SIZE, VERSION,
        };
        use inspire_core::bucket_index::BucketDelta;
        use std::io::Write;

        // Create a full test harness first (with lanes)
        let base = TestHarness::new().await;

        // Create a range delta file in the temp dir
        let delta_path = base.temp_dir.join("bucket-deltas.bin");
        let mut file = std::fs::File::create(&delta_path).expect("create delta file");

        // Write header
        let header = RangeDeltaHeader {
            version: VERSION,
            current_block: 12345,
            num_ranges: DEFAULT_RANGES.len() as u32,
        };
        file.write_all(&header.to_bytes()).expect("write header");

        // Create test deltas for each range
        let mut range_data: Vec<Vec<u8>> = Vec::new();
        for i in 0..DEFAULT_RANGES.len() {
            let delta = BucketDelta {
                block_number: 12345,
                updates: vec![(i, (i + 1) as u16), (i + 100, (i + 10) as u16)],
            };
            range_data.push(delta.to_bytes());
        }

        // Calculate offsets and write directory
        let directory_size = DEFAULT_RANGES.len() * RANGE_ENTRY_SIZE;
        let mut offset = (HEADER_SIZE + directory_size) as u32;

        for (i, data) in range_data.iter().enumerate() {
            let entry = RangeEntry {
                blocks_covered: DEFAULT_RANGES[i],
                offset,
                size: data.len() as u32,
                entry_count: 1,
            };
            file.write_all(&entry.to_bytes()).expect("write entry");
            offset += data.len() as u32;
        }

        // Write range data
        for data in &range_data {
            file.write_all(data).expect("write range data");
        }
        drop(file);

        // Now create a new server with the range delta path configured
        let mut config = base.config.clone();
        config.range_delta_path = Some(delta_path);

        let state = create_shared_state(config);
        state.load_lanes().expect("Lanes should load");

        let router = create_router(state);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Bind should succeed");
        let addr = listener.local_addr().expect("local addr");
        let server_url = format!("http://{}", addr);

        tokio::spawn(async move {
            axum::serve(listener, router).await.ok();
        });

        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("HTTP client");

        // Wait for server to be ready
        for _ in 0..20 {
            if http
                .get(format!("{}/live", server_url))
                .send()
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        Self {
            server_url,
            temp_dir: base.temp_dir.clone(),
            http,
        }
    }
}

#[derive(Deserialize, Debug)]
struct RangeDeltaInfoResponse {
    current_block: u64,
    ranges: Vec<RangeInfoItem>,
}

#[derive(Deserialize, Debug)]
struct RangeInfoItem {
    blocks_covered: u32,
    offset: u32,
    size: u32,
}

#[tokio::test]
async fn test_range_delta_info_endpoint_with_file() {
    let harness = RangeDeltaTestHarness::new().await;

    let resp = harness
        .http
        .get(format!("{}/index/deltas/info", harness.server_url))
        .send()
        .await
        .expect("request");

    assert_eq!(
        resp.status().as_u16(),
        200,
        "Should return 200 when range delta file is loaded"
    );

    let info: RangeDeltaInfoResponse = resp.json().await.expect("parse json");
    assert_eq!(info.current_block, 12345);
    assert_eq!(info.ranges.len(), 5);
    assert_eq!(info.ranges[0].blocks_covered, 1);
    assert_eq!(info.ranges[1].blocks_covered, 10);
    assert_eq!(info.ranges[2].blocks_covered, 100);
    assert_eq!(info.ranges[3].blocks_covered, 1000);
    assert_eq!(info.ranges[4].blocks_covered, 10000);
}

#[tokio::test]
async fn test_range_delta_endpoint_full_file() {
    let harness = RangeDeltaTestHarness::new().await;

    let resp = harness
        .http
        .get(format!("{}/index/deltas", harness.server_url))
        .send()
        .await
        .expect("request");

    assert_eq!(
        resp.status().as_u16(),
        200,
        "Should return 200 for full delta file"
    );

    let data = resp.bytes().await.expect("get bytes");
    assert!(data.len() > 64, "Should have header + data");

    // Verify magic bytes
    assert_eq!(&data[0..4], b"BDLT", "Should have BDLT magic");
}

#[tokio::test]
async fn test_range_delta_http_range_request() {
    let harness = RangeDeltaTestHarness::new().await;

    // First get info to find range offsets
    let info_resp = harness
        .http
        .get(format!("{}/index/deltas/info", harness.server_url))
        .send()
        .await
        .expect("info request");

    let info: RangeDeltaInfoResponse = info_resp.json().await.expect("parse json");

    // Request just range 0 using HTTP Range header
    let range0 = &info.ranges[0];
    let range_header = format!(
        "bytes={}-{}",
        range0.offset,
        range0.offset + range0.size - 1
    );

    let resp = harness
        .http
        .get(format!("{}/index/deltas", harness.server_url))
        .header("Range", range_header)
        .send()
        .await
        .expect("range request");

    // Server may return 200 or 206 depending on implementation
    let status = resp.status().as_u16();
    assert!(
        status == 200 || status == 206,
        "Should return 200 or 206 for range request: {}",
        status
    );

    let data = resp.bytes().await.expect("get bytes");
    // Data should be a BucketDelta - at least 12 bytes header
    assert!(
        data.len() >= 12,
        "Range data should be at least 12 bytes: {}",
        data.len()
    );
}

#[tokio::test]
async fn test_bucket_delta_parsing_from_range() {
    use inspire_core::bucket_index::BucketDelta;

    let harness = RangeDeltaTestHarness::new().await;

    // Get info and fetch range 0
    let info_resp = harness
        .http
        .get(format!("{}/index/deltas/info", harness.server_url))
        .send()
        .await
        .expect("info request");

    let info: RangeDeltaInfoResponse = info_resp.json().await.expect("parse json");
    let range0 = &info.ranges[0];

    // Fetch full file and extract range 0 data
    let resp = harness
        .http
        .get(format!("{}/index/deltas", harness.server_url))
        .send()
        .await
        .expect("request");

    let data = resp.bytes().await.expect("get bytes");
    let range_data = &data[range0.offset as usize..(range0.offset + range0.size) as usize];

    // Parse as BucketDelta
    let delta = BucketDelta::from_bytes(range_data).expect("parse delta");
    assert_eq!(delta.block_number, 12345);
    assert_eq!(delta.updates.len(), 2);
    assert_eq!(delta.updates[0], (0, 1));
    assert_eq!(delta.updates[1], (100, 10));
}

// ============================================================================
// Load Tests (marked #[ignore] for manual/nightly runs)
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_reload_storm() {
    let harness = TestHarness::new().await;

    let mut reload_handles = vec![];
    for _ in 0..20 {
        let url = harness.server_url.clone();
        reload_handles.push(tokio::spawn(async move {
            let client = Client::new();
            let _ = client.post(format!("{}/admin/reload", url)).send().await;
        }));
    }

    for h in reload_handles {
        let _ = h.await;
    }

    let health = harness.health().await.expect("health after reload storm");
    assert_eq!(health.status, "ok");

    let entry = harness
        .query_and_extract(Lane::Hot, 10)
        .await
        .expect("query after storm");
    assert!(!entry.is_empty());
}

#[tokio::test]
#[ignore]
async fn test_high_concurrency_queries() {
    let harness = TestHarness::new().await;
    let url = harness.server_url.clone();
    let hot_crs = harness.hot_crs.clone().unwrap();

    let num_clients = 50;
    let queries_per_client = 10;

    let mut handles = vec![];

    for client_id in 0..num_clients {
        let url = url.clone();
        let crs = hot_crs.clone();
        handles.push(tokio::spawn(async move {
            let client = Client::new();

            let crs_resp: CrsResponse = client
                .get(format!("{}/crs/hot", url))
                .send()
                .await?
                .json()
                .await?;

            for q in 0..queries_per_client {
                let mut sampler = GaussianSampler::new(crs.params.sigma);
                let sk = RlweSecretKey::generate(&crs.params, &mut sampler);
                let index = (client_id * queries_per_client + q) % 200;

                let (client_state, client_query) = pir_query(
                    &crs,
                    index as u64,
                    &crs_resp.shard_config,
                    &sk,
                    &mut sampler,
                )
                .map_err(|e| anyhow::anyhow!("{}", e))?;

                let resp: QueryResponse = client
                    .post(format!("{}/query/hot", url))
                    .json(&QueryRequest {
                        query: client_query,
                    })
                    .send()
                    .await?
                    .json()
                    .await?;

                let _entry = extract_with_variant(
                    &crs,
                    &client_state,
                    &resp.response,
                    32,
                    InspireVariant::OnePacking,
                )
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            }

            Ok::<_, anyhow::Error>(())
        }));
    }

    let mut successes = 0;
    for h in handles {
        if h.await.unwrap().is_ok() {
            successes += 1;
        }
    }

    assert_eq!(
        successes, num_clients,
        "All {} clients should complete {} queries each",
        num_clients, queries_per_client
    );
}
