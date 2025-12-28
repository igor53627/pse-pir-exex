//! Mock PIR server for E2E testing
//!
//! Creates a real PIR database with test balances and serves queries.
//! The WASM client can actually decrypt responses from this server.
//!
//! Usage:
//!   cargo run -p burner-wallet --features pir-mock --bin pir-mock-server
//!
//! The server listens on port 3001 by default.

use axum::{
    extract::State,
    http::{header, Method},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use inspire_pir::{
    math::GaussianSampler,
    params::{InspireParams, SecurityLevel, ShardConfig},
    pir::{respond, setup},
    EncodedDatabase, SeededClientQuery, ServerCrs,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const ENTRY_SIZE: usize = 64;

#[derive(Clone)]
struct MockState {
    crs: Arc<ServerCrs>,
    encoded_db: Arc<EncodedDatabase>,
    addresses: Vec<String>,
    balances: Vec<BalanceEntry>,
    snapshot_block: u64,
    block_hash: String,
}

#[derive(Clone, Serialize)]
struct BalanceEntry {
    address: String,
    eth_wei: String,
    usdc_raw: String,
}

#[derive(Serialize)]
struct CrsResponse {
    crs: String,
    lane: String,
    entry_count: u64,
    shard_config: ShardConfig,
}

#[derive(Serialize)]
struct MetadataResponse {
    addresses: Vec<String>,
    #[serde(rename = "snapshotBlock")]
    snapshot_block: u64,
    #[serde(rename = "blockHash")]
    block_hash: String,
    #[serde(rename = "entryCount")]
    entry_count: usize,
    lane: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    lanes: LaneStats,
}

#[derive(Serialize)]
struct LaneStats {
    hot_entries: usize,
    cold_entries: usize,
    hot_contracts: usize,
    block_number: u64,
}

#[derive(Serialize)]
struct InfoResponse {
    version: String,
    config_hash: String,
    manifest_block: u64,
    hot_entries: usize,
    cold_entries: usize,
    hot_contracts: usize,
    block_number: u64,
}

#[derive(Deserialize)]
struct SeededQueryRequest {
    query: SeededClientQuery,
}

fn test_params() -> InspireParams {
    InspireParams {
        ring_dim: 256,
        q: 1152921504606830593,
        p: 65537, // Fermat prime F4, ensures gcd(d, p) = 1 for mod_inverse
        sigma: 6.4,
        gadget_base: 1 << 20,
        gadget_len: 3,
        security_level: SecurityLevel::Bits128,
    }
}

fn create_test_database() -> (Vec<u8>, Vec<BalanceEntry>) {
    let test_balances = vec![
        BalanceEntry {
            address: "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045".to_lowercase(),
            eth_wei: "1000000000000000000000".to_string(),
            usdc_raw: "50000000000".to_string(),
        },
        BalanceEntry {
            address: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".to_lowercase(),
            eth_wei: "10000000000000000000000".to_string(),
            usdc_raw: "100000000000".to_string(),
        },
        BalanceEntry {
            address: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_lowercase(),
            eth_wei: "5000000000000000000000".to_string(),
            usdc_raw: "25000000000".to_string(),
        },
    ];

    let ring_dim = 256;
    let num_entries = ring_dim;
    let mut database = vec![0u8; num_entries * ENTRY_SIZE];

    for (i, balance) in test_balances.iter().enumerate() {
        let eth_wei: u128 = balance.eth_wei.parse().unwrap_or(0);
        let usdc_raw: u64 = balance.usdc_raw.parse().unwrap_or(0);

        let offset = i * ENTRY_SIZE;

        // ETH balance: bytes 0-32 (256-bit big-endian, use u128 in high bytes)
        let eth_bytes = eth_wei.to_be_bytes(); // 16 bytes
        database[offset + 16..offset + 32].copy_from_slice(&eth_bytes);

        // USDC balance: bytes 32-64 (256-bit big-endian, use u64 in high bytes)
        let usdc_bytes = usdc_raw.to_be_bytes(); // 8 bytes
        database[offset + 32 + 24..offset + 64].copy_from_slice(&usdc_bytes);
    }

    (database, test_balances)
}

async fn health(State(state): State<Arc<MockState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ready".to_string(),
        lanes: LaneStats {
            hot_entries: state.balances.len(),
            cold_entries: 0,
            hot_contracts: state.balances.len(),
            block_number: state.snapshot_block,
        },
    })
}

async fn info(State(state): State<Arc<MockState>>) -> Json<InfoResponse> {
    Json(InfoResponse {
        version: "0.1.0-mock".to_string(),
        config_hash: "mock-real-pir".to_string(),
        manifest_block: state.snapshot_block,
        hot_entries: state.balances.len(),
        cold_entries: 0,
        hot_contracts: state.balances.len(),
        block_number: state.snapshot_block,
    })
}

async fn get_crs(State(state): State<Arc<MockState>>) -> Result<Json<CrsResponse>, String> {
    let crs_json =
        serde_json::to_string(state.crs.as_ref()).map_err(|e| format!("CRS serialize: {}", e))?;

    Ok(Json(CrsResponse {
        crs: crs_json,
        lane: "balances".to_string(),
        entry_count: state.encoded_db.config.total_entries,
        shard_config: state.encoded_db.config.clone(),
    }))
}

async fn get_metadata(State(state): State<Arc<MockState>>) -> Json<MetadataResponse> {
    Json(MetadataResponse {
        addresses: state.addresses.clone(),
        snapshot_block: state.snapshot_block,
        block_hash: state.block_hash.clone(),
        entry_count: state.balances.len(),
        lane: "balances".to_string(),
    })
}

async fn query_seeded_binary(
    State(state): State<Arc<MockState>>,
    Json(req): Json<SeededQueryRequest>,
) -> Result<Response, String> {
    tracing::info!(shard_id = req.query.shard_id, "Processing PIR query");

    let expanded_query = req.query.expand();

    let response = respond(
        state.crs.as_ref(),
        state.encoded_db.as_ref(),
        &expanded_query,
    )
    .map_err(|e| format!("PIR respond failed: {}", e))?;

    let binary = response
        .to_binary()
        .map_err(|e| format!("Binary encode failed: {}", e))?;

    tracing::info!(response_bytes = binary.len(), "Sending PIR response");

    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], binary).into_response())
}

/// Fetch block hash from Sepolia RPC
async fn fetch_block_hash(block_number: u64) -> Result<String, String> {
    let rpc_url =
        std::env::var("SEPOLIA_RPC_URL").unwrap_or_else(|_| "https://rpc.sepolia.org".to_string());

    let client = reqwest::Client::new();
    let block_hex = format!("0x{:x}", block_number);

    let response = client
        .post(&rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [block_hex, false],
            "id": 1
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

    json["result"]["hash"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No hash in response".to_string())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pir_mock_server=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Setting up PIR mock server with real crypto...");

    let params = test_params();
    let (database, balances) = create_test_database();
    let addresses: Vec<String> = balances.iter().map(|b| b.address.clone()).collect();

    let mut sampler = GaussianSampler::new(params.sigma);
    let (crs, encoded_db, _sk) =
        setup(&params, &database, ENTRY_SIZE, &mut sampler).expect("PIR setup failed");

    tracing::info!(
        entries = encoded_db.config.total_entries,
        shards = encoded_db.config.num_shards(),
        "PIR database created"
    );

    // Fetch real block hash from Sepolia for verification
    let snapshot_block = 7500000u64;
    let block_hash = fetch_block_hash(snapshot_block).await.unwrap_or_else(|e| {
        tracing::warn!("Failed to fetch block hash: {}, using placeholder", e);
        "0x0000000000000000000000000000000000000000000000000000000000000000".to_string()
    });
    tracing::info!(block = snapshot_block, hash = %block_hash, "Snapshot block hash");

    let state = Arc::new(MockState {
        crs: Arc::new(crs),
        encoded_db: Arc::new(encoded_db),
        addresses,
        balances,
        snapshot_block,
        block_hash,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health))
        .route("/info", get(info))
        .route("/crs/{lane}", get(get_crs))
        .route("/crs/balances", get(get_crs))
        .route("/metadata/balances", get(get_metadata))
        .route("/query/{lane}/seeded/binary", post(query_seeded_binary))
        .route("/query/balances/seeded/binary", post(query_seeded_binary))
        .layer(cors)
        .with_state(state);

    let addr = std::env::var("PIR_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3001".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    tracing::info!("PIR mock server listening on {}", addr);
    tracing::info!("Test addresses:");
    tracing::info!("  [0] 0xd8da6bf26964af9d7eed9e03e53415d37aa96045 - 1000 ETH, 50000 USDC");
    tracing::info!("  [1] 0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266 - 10000 ETH, 100000 USDC");
    tracing::info!("  [2] 0x70997970c51812dc3a010c7d01b50e0d17dc79c8 - 5000 ETH, 25000 USDC");

    axum::serve(listener, app).await.unwrap();
}
