//! End-to-end server integration tests
//!
//! Tests the full server flow: setup -> query -> response -> extraction
//!
//! Test organization:
//! - Fast tests (no #[ignore]): run in CI, complete in <30s total
//! - Slow tests (#[ignore]): load/soak tests for manual/nightly runs

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

use inspire_core::{Lane, TwoLaneConfig};
use inspire_pir::math::GaussianSampler;
use inspire_pir::rlwe::RlweSecretKey;
use inspire_pir::{extract, query as pir_query, ServerCrs};
use inspire_server::{create_router, create_shared_state, DbSnapshot, SharedState};
use lane_builder::{test_params, TwoLaneSetup};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

static PORT_COUNTER: AtomicU16 = AtomicU16::new(19100);

fn next_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Test harness for running E2E server tests
pub struct TestHarness {
    pub server_url: String,
    pub config: TwoLaneConfig,
    pub state: SharedState,
    pub temp_dir: PathBuf,
    pub http: Client,
    pub hot_crs: Option<ServerCrs>,
    pub cold_crs: Option<ServerCrs>,
    _shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl TestHarness {
    /// Create a new test harness with PIR databases
    pub async fn new() -> Self {
        Self::with_entries(256, 256).await
    }

    /// Create a test harness with specified entry counts
    pub async fn with_entries(hot_entries: usize, cold_entries: usize) -> Self {
        let temp_dir = std::env::temp_dir().join(format!("pir-e2e-test-{}", next_port()));
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

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let port = next_port();
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        let server_url = format!("http://127.0.0.1:{}", port);

        let router = create_router(state.clone());
        let listener = TcpListener::bind(addr).await.expect("Bind should succeed");

        tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        for _ in 0..10 {
            if Client::new()
                .get(format!("http://127.0.0.1:{}/live", port))
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
            config,
            state,
            temp_dir,
            http: Client::new(),
            hot_crs: Some(result.hot_crs),
            cold_crs: Some(result.cold_crs),
            _shutdown: Some(shutdown_tx),
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
            .json(&QueryRequest { query: query.clone() })
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
            .json(&SeededQueryRequest { query: query.clone() })
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

        let (client_state, client_query) =
            pir_query(crs, index, &shard_config, &sk, &mut sampler)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

        let resp = self.query_raw(lane, &client_query).await?;
        let query_resp: QueryResponse = resp.json().await?;

        let entry = extract(crs, &client_state, &query_resp.response, 32)
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
    let entry = harness.query_and_extract(Lane::Hot, index).await.expect("query");

    let expected_start = (index as usize) * 32;
    let expected: Vec<u8> = (expected_start..expected_start + 32)
        .map(|i| (i % 256) as u8)
        .collect();

    assert_eq!(entry, expected, "Retrieved entry should match expected data");
}

#[tokio::test]
async fn test_cold_lane_query() {
    let harness = TestHarness::new().await;
    let index = 100u64;
    let entry = harness.query_and_extract(Lane::Cold, index).await.expect("query");

    let expected_start = (index as usize) * 32;
    let expected: Vec<u8> = (expected_start..expected_start + 32)
        .map(|i| ((i + 128) % 256) as u8)
        .collect();

    assert_eq!(entry, expected, "Cold lane data should differ from hot lane");
}

#[tokio::test]
async fn test_hot_and_cold_queries_different_data() {
    let harness = TestHarness::new().await;

    let hot_entry = harness.query_and_extract(Lane::Hot, 0).await.expect("hot query");
    let cold_entry = harness.query_and_extract(Lane::Cold, 0).await.expect("cold query");

    assert_ne!(hot_entry, cold_entry, "Hot and cold lanes should have different data");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_invalid_lane_returns_400() {
    let harness = TestHarness::new().await;

    let resp = harness.http
        .get(format!("{}/crs/invalid", harness.server_url))
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status().as_u16(), 400);
}

#[tokio::test]
async fn test_invalid_json_query_returns_4xx() {
    let harness = TestHarness::new().await;

    let resp = harness.http
        .post(format!("{}/query/hot", harness.server_url))
        .header("content-type", "application/json")
        .body(r#"{"invalid": "json"}"#)
        .send()
        .await
        .expect("request");

    let status = resp.status().as_u16();
    assert!(status >= 400 && status < 500, "Expected 4xx, got {}", status);
}

#[tokio::test]
async fn test_server_continues_after_error() {
    let harness = TestHarness::new().await;

    let _ = harness.http
        .post(format!("{}/query/hot", harness.server_url))
        .header("content-type", "application/json")
        .body("not json")
        .send()
        .await;

    let health = harness.health().await.expect("health after error");
    assert_eq!(health.status, "ok");

    let entry = harness.query_and_extract(Lane::Hot, 10).await.expect("query after error");
    assert!(!entry.is_empty());
}

// ============================================================================
// Snapshot Consistency Tests
// ============================================================================

#[tokio::test]
async fn test_snapshot_consistent_across_queries() {
    let harness = TestHarness::new().await;

    let snap1 = harness.snapshot();
    let entry1 = harness.query_and_extract(Lane::Hot, 5).await.expect("query 1");

    let snap2 = harness.snapshot();
    let entry2 = harness.query_and_extract(Lane::Hot, 5).await.expect("query 2");

    assert_eq!(entry1, entry2, "Same query should return same result");
    assert!(Arc::ptr_eq(&snap1, &snap2), "Snapshot should be same without reload");
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
    assert!(!Arc::ptr_eq(&snap_before, &snap_after), "Snapshot should change after reload");
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
            http: Client::new(),
            hot_crs: harness.hot_crs.clone(),
            cold_crs: harness.cold_crs.clone(),
            _shutdown: None,
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
            let client = Client::new();

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
                .json(&QueryRequest { query: client_query })
                .send()
                .await?
                .json()
                .await?;

            let _entry = extract(&crs, &client_state, &resp.response, 32)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            Ok::<_, anyhow::Error>(())
        }));
    }

    harness.reload().await.expect("reload during queries");

    let mut successes = 0;
    for h in handles {
        if h.await.unwrap().is_ok() {
            successes += 1;
        }
    }

    assert_eq!(successes, 10, "All concurrent queries should succeed");
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
            let _ = client
                .post(format!("{}/admin/reload", url))
                .send()
                .await;
        }));
    }

    for h in reload_handles {
        let _ = h.await;
    }

    let health = harness.health().await.expect("health after reload storm");
    assert_eq!(health.status, "ok");

    let entry = harness.query_and_extract(Lane::Hot, 10).await.expect("query after storm");
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

                let (client_state, client_query) =
                    pir_query(&crs, index as u64, &crs_resp.shard_config, &sk, &mut sampler)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;

                let resp: QueryResponse = client
                    .post(format!("{}/query/hot", url))
                    .json(&QueryRequest { query: client_query })
                    .send()
                    .await?
                    .json()
                    .await?;

                let _entry = extract(&crs, &client_state, &resp.response, 32)
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
