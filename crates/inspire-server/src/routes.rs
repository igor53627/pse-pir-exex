//! HTTP routes for the PIR server

use axum::{
    extract::{ws::WebSocketUpgrade, Path, State},
    http::header,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use crate::broadcast::handle_index_subscription;

use inspire_core::Lane;
use inspire_pir::{params::ShardConfig, ClientQuery, SeededClientQuery, ServerResponse};

use crate::error::{Result, ServerError};
use crate::metrics;
use crate::state::{ReloadResult, SharedState};

/// Health/readiness check response
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub hot_loaded: bool,
    pub cold_loaded: bool,
    pub mmap_mode: bool,
}

/// Liveness check response (lightweight)
#[derive(Serialize)]
pub struct LiveResponse {
    pub status: String,
}

/// PIR query request (full ciphertext)
#[derive(Deserialize)]
pub struct QueryRequest {
    pub query: ClientQuery,
}

/// Seeded PIR query request (~50% smaller)
#[derive(Deserialize)]
pub struct SeededQueryRequest {
    pub query: SeededClientQuery,
}

/// PIR query response
#[derive(Serialize)]
pub struct QueryResponse {
    pub response: ServerResponse,
    pub lane: Lane,
}

/// Server info response (for version negotiation)
#[derive(Serialize)]
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

/// CRS response
#[derive(Serialize)]
pub struct CrsResponse {
    pub crs: String,
    pub lane: Lane,
    pub entry_count: u64,
    pub shard_config: ShardConfig,
}

/// Health/readiness check endpoint
///
/// Returns 200 OK only when both lanes are loaded and ready to serve.
/// Returns 503 Service Unavailable otherwise.
async fn health(State(state): State<SharedState>) -> Response {
    let snapshot = state.load_snapshot();
    let stats = snapshot.stats();
    let ready = snapshot.is_ready();

    let response = HealthResponse {
        status: if ready {
            "ok".to_string()
        } else {
            "unavailable".to_string()
        },
        hot_loaded: stats.hot_loaded,
        cold_loaded: stats.cold_loaded,
        mmap_mode: state.config.use_mmap,
    };

    if ready {
        Json(response).into_response()
    } else {
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(response)).into_response()
    }
}

/// Liveness check endpoint (lightweight)
///
/// Always returns 200 OK if the server is alive. Does not check lane status.
async fn live() -> Json<LiveResponse> {
    Json(LiveResponse {
        status: "ok".to_string(),
    })
}

/// Server info endpoint (for version negotiation)
async fn info(State(state): State<SharedState>) -> Json<ServerInfo> {
    let snapshot = state.load_snapshot();
    let stats = snapshot.stats();

    Json(ServerInfo {
        version: state.config.version.clone(),
        pir_params_version: stats.pir_params_version,
        config_hash: state
            .config
            .config_hash
            .clone()
            .unwrap_or_else(|| state.config.compute_hash()),
        manifest_block: snapshot.router.as_ref().map(|r| r.manifest().block_number),
        hot_entries: stats.hot_entries,
        cold_entries: stats.cold_entries,
        hot_contracts: stats.hot_contracts,
        block_number: stats.block_number,
    })
}

/// Get CRS for a specific lane
async fn get_crs(
    State(state): State<SharedState>,
    Path(lane): Path<String>,
) -> Result<Json<CrsResponse>> {
    let lane = parse_lane(&lane)?;
    let snapshot = state.load_snapshot();
    let lane_data = snapshot.get_lane(lane)?;

    Ok(Json(CrsResponse {
        crs: lane_data.crs_json()?,
        lane,
        entry_count: lane_data.entry_count,
        shard_config: lane_data.shard_config(),
    }))
}

/// Process a PIR query (full ciphertext)
async fn query(
    State(state): State<SharedState>,
    Path(lane): Path<String>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResponse>> {
    let lane = parse_lane(&lane)?;
    let lane_str = lane_to_string(lane);
    let start = std::time::Instant::now();
    metrics::record_pir_request_start(&lane_str);

    let snapshot = state.load_snapshot_full();
    let result = snapshot.process_query(lane, &req.query);

    metrics::record_pir_request_end(&lane_str);
    let duration = start.elapsed();

    match result {
        Ok(response) => {
            metrics::record_pir_request(&lane_str, metrics::OUTCOME_OK, duration);
            Ok(Json(QueryResponse { response, lane }))
        }
        Err(e) => {
            let outcome = if matches!(e, ServerError::InvalidQuery(_)) {
                metrics::OUTCOME_CLIENT_ERROR
            } else {
                metrics::OUTCOME_SERVER_ERROR
            };
            metrics::record_pir_request(&lane_str, outcome, duration);
            Err(e)
        }
    }
}

/// Process a seeded PIR query (~50% smaller, server expands)
async fn query_seeded(
    State(state): State<SharedState>,
    Path(lane): Path<String>,
    Json(req): Json<SeededQueryRequest>,
) -> Result<Json<QueryResponse>> {
    let lane = parse_lane(&lane)?;

    // Expand seeded query to full query (regenerate `a` polynomials from seeds)
    let expanded_query = req.query.expand();

    let snapshot = state.load_snapshot_full();
    let response = snapshot.process_query(lane, &expanded_query)?;

    Ok(Json(QueryResponse { response, lane }))
}

/// Process a seeded PIR query with binary response (~75% smaller total)
///
/// Request: seeded JSON query (~230 KB)
/// Response: binary bincode (~544 KB vs ~1,296 KB JSON)
async fn query_seeded_binary(
    State(state): State<SharedState>,
    Path(lane): Path<String>,
    Json(req): Json<SeededQueryRequest>,
) -> Result<Response> {
    let lane = parse_lane(&lane)?;

    let expanded_query = req.query.expand();

    let snapshot = state.load_snapshot_full();
    let response = snapshot.process_query(lane, &expanded_query)?;

    let binary = response
        .to_binary()
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], binary).into_response())
}

/// Process a full PIR query with binary response
async fn query_binary(
    State(state): State<SharedState>,
    Path(lane): Path<String>,
    Json(req): Json<QueryRequest>,
) -> Result<Response> {
    let lane = parse_lane(&lane)?;

    let snapshot = state.load_snapshot_full();
    let response = snapshot.process_query(lane, &req.query)?;

    let binary = response
        .to_binary()
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], binary).into_response())
}

/// Reload lanes from disk (admin endpoint)
///
/// Atomically swaps in a new snapshot without blocking ongoing queries.
async fn admin_reload(State(state): State<SharedState>) -> Result<Json<ReloadResult>> {
    let result = state.reload()?;
    Ok(Json(result))
}

/// Get bucket index (compressed)
///
/// Returns the bucket index for sparse client-side lookups.
/// ~150 KB compressed, enables O(1) index computation.
async fn get_bucket_index(State(state): State<SharedState>) -> Result<Response> {
    let snapshot = state.load_snapshot();

    let cached = snapshot
        .bucket_index
        .as_ref()
        .ok_or_else(|| ServerError::BucketIndexNotLoaded)?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CONTENT_ENCODING, "zstd"),
            (header::CACHE_CONTROL, "public, max-age=60"),
        ],
        cached.compressed.clone(),
    )
        .into_response())
}

/// Get bucket index (uncompressed, 512 KB)
///
/// For WASM clients that can't use zstd decompression.
async fn get_bucket_index_raw(State(state): State<SharedState>) -> Result<Response> {
    let snapshot = state.load_snapshot();

    let cached = snapshot
        .bucket_index
        .as_ref()
        .ok_or_else(|| ServerError::BucketIndexNotLoaded)?;

    let data = cached.index.to_bytes();

    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "public, max-age=60"),
        ],
        data,
    )
        .into_response())
}

/// Get bucket index info (metadata only)
async fn get_bucket_index_info(State(state): State<SharedState>) -> Result<Json<BucketIndexInfo>> {
    let snapshot = state.load_snapshot();

    let cached = snapshot
        .bucket_index
        .as_ref()
        .ok_or_else(|| ServerError::BucketIndexNotLoaded)?;

    Ok(Json(BucketIndexInfo {
        total_entries: cached.total_entries(),
        num_buckets: inspire_client::bucket_index::NUM_BUCKETS,
        block_number: snapshot.block_number,
        compressed_size: cached.compressed.len(),
    }))
}

/// Bucket index metadata
#[derive(Serialize)]
pub struct BucketIndexInfo {
    pub total_entries: u64,
    pub num_buckets: usize,
    pub block_number: Option<u64>,
    pub compressed_size: usize,
}

/// Stem index metadata
#[derive(Serialize)]
pub struct StemIndexInfo {
    pub stem_count: u64,
    pub total_entries: u64,
    pub block_number: Option<u64>,
}

/// Range delta info (for client sync)
#[derive(Serialize)]
pub struct RangeDeltaInfo {
    pub current_block: u64,
    pub ranges: Vec<RangeInfo>,
}

/// Info about a single delta range
#[derive(Serialize)]
pub struct RangeInfo {
    pub blocks_covered: u32,
    pub offset: u32,
    pub size: u32,
}

/// Get stem index (binary)
///
/// Returns the stem index for stem-ordered databases.
/// Format: count:8 + (stem:31 + offset:8)*
async fn get_stem_index(State(state): State<SharedState>) -> Result<Response> {
    let snapshot = state.load_snapshot();

    let cached = snapshot
        .stem_index
        .as_ref()
        .ok_or_else(|| ServerError::StemIndexNotLoaded)?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "public, max-age=60"),
        ],
        cached.data.clone(),
    )
        .into_response())
}

/// Get stem index info (metadata only)
async fn get_stem_index_info(State(state): State<SharedState>) -> Result<Json<StemIndexInfo>> {
    let snapshot = state.load_snapshot();

    let cached = snapshot
        .stem_index
        .as_ref()
        .ok_or_else(|| ServerError::StemIndexNotLoaded)?;

    Ok(Json(StemIndexInfo {
        stem_count: cached.stem_count,
        total_entries: cached.total_entries,
        block_number: snapshot.block_number,
    }))
}

/// Get range delta file info
///
/// Returns metadata about available delta ranges for efficient client sync.
/// Client reads this, picks appropriate range, then fetches via HTTP Range request.
async fn get_range_delta_info(State(state): State<SharedState>) -> Result<Json<RangeDeltaInfo>> {
    let snapshot = state.load_snapshot();

    let cached = snapshot
        .range_delta
        .as_ref()
        .ok_or_else(|| ServerError::Internal("Range delta not loaded".to_string()))?;

    Ok(Json(RangeDeltaInfo {
        current_block: cached.current_block,
        ranges: cached
            .ranges
            .iter()
            .map(|r| RangeInfo {
                blocks_covered: r.blocks_covered,
                offset: r.offset,
                size: r.size,
            })
            .collect(),
    }))
}

/// Get range delta file (supports HTTP Range requests)
///
/// Full file or partial range for efficient sync:
/// - `GET /index/deltas` - full file (~3 MB)
/// - `GET /index/deltas` with `Range: bytes=1024-2048` - specific range
async fn get_range_delta(State(state): State<SharedState>) -> Result<Response> {
    let snapshot = state.load_snapshot();

    let cached = snapshot
        .range_delta
        .as_ref()
        .ok_or_else(|| ServerError::Internal("Range delta not loaded".to_string()))?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "public, max-age=12"),
            (header::ACCEPT_RANGES, "bytes"),
        ],
        cached.data.clone(),
    )
        .into_response())
}

/// Subscribe to bucket index delta updates via WebSocket
///
/// Protocol:
/// 1. Server sends Hello message (JSON): `{"version":1,"block_number":12345}`
/// 2. Server sends binary BucketDelta messages after each block (~12 sec)
/// 3. Server responds to Ping with Pong
/// 4. If client lags, connection is closed with code 4000 and reason "lagged:<block>"
async fn subscribe_index(ws: WebSocketUpgrade, State(state): State<SharedState>) -> Response {
    let snapshot = state.load_snapshot();
    let current_block = snapshot.block_number;
    ws.on_upgrade(move |socket| {
        handle_index_subscription(socket, state.bucket_broadcast.clone(), current_block)
    })
}

/// Parse lane from URL path
fn parse_lane(s: &str) -> Result<Lane> {
    match s.to_lowercase().as_str() {
        "hot" | "balances" => Ok(Lane::Hot),
        "cold" | "storage" => Ok(Lane::Cold),
        _ => Err(ServerError::InvalidQuery(format!("Invalid lane: {}", s))),
    }
}

/// Convert lane to string for metrics
fn lane_to_string(lane: Lane) -> String {
    match lane {
        Lane::Hot => metrics::LANE_HOT.to_string(),
        Lane::Cold => metrics::LANE_COLD.to_string(),
    }
}

/// Create the public router (exposed to the internet)
pub fn create_public_router(state: SharedState) -> Router {
    create_public_router_with_metrics(state, None)
}

/// Create the public router with optional metrics
pub fn create_public_router_with_metrics(
    state: SharedState,
    prometheus_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
) -> Router {
    let mut router = Router::new()
        .route("/health", get(health))
        .route("/live", get(live))
        .route("/info", get(info))
        .route("/crs/:lane", get(get_crs))
        .route("/query/:lane", post(query))
        .route("/query/:lane/binary", post(query_binary))
        .route("/query/:lane/seeded", post(query_seeded))
        .route("/query/:lane/seeded/binary", post(query_seeded_binary))
        .route("/index", get(get_bucket_index))
        .route("/index/raw", get(get_bucket_index_raw))
        .route("/index/info", get(get_bucket_index_info))
        .route("/index/subscribe", get(subscribe_index))
        .route("/index/stems", get(get_stem_index))
        .route("/index/stems/info", get(get_stem_index_info))
        .route("/index/deltas", get(get_range_delta))
        .route("/index/deltas/info", get(get_range_delta_info))
        .with_state(state)
        .layer(CorsLayer::permissive());

    if let Some(handle) = prometheus_handle {
        router = router.route(
            "/metrics",
            get(move || {
                let metrics = handle.render();
                async move { metrics }
            }),
        );
    }

    router
}

/// Create the admin router (bound to localhost only)
pub fn create_admin_router(state: SharedState) -> Router {
    Router::new()
        .route("/admin/reload", post(admin_reload))
        .route("/admin/health", get(health))
        .with_state(state)
}

/// Create combined router (for backwards compatibility / testing)
pub fn create_router(state: SharedState) -> Router {
    create_router_with_metrics(state, None)
}

/// Create the router with metrics endpoint
pub fn create_router_with_metrics(
    state: SharedState,
    prometheus_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
) -> Router {
    let mut router = Router::new()
        .route("/health", get(health))
        .route("/live", get(live))
        .route("/info", get(info))
        .route("/crs/:lane", get(get_crs))
        .route("/query/:lane", post(query))
        .route("/query/:lane/binary", post(query_binary))
        .route("/query/:lane/seeded", post(query_seeded))
        .route("/query/:lane/seeded/binary", post(query_seeded_binary))
        .route("/index", get(get_bucket_index))
        .route("/index/raw", get(get_bucket_index_raw))
        .route("/index/info", get(get_bucket_index_info))
        .route("/index/subscribe", get(subscribe_index))
        .route("/index/stems", get(get_stem_index))
        .route("/index/stems/info", get(get_stem_index_info))
        .route("/index/deltas", get(get_range_delta))
        .route("/index/deltas/info", get(get_range_delta_info))
        .route("/admin/reload", post(admin_reload))
        .with_state(state)
        .layer(CorsLayer::permissive());

    if let Some(handle) = prometheus_handle {
        router = router.route(
            "/metrics",
            get(move || {
                let metrics = handle.render();
                async move { metrics }
            }),
        );
    }

    router
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_lane() {
        assert_eq!(parse_lane("hot").unwrap(), Lane::Hot);
        assert_eq!(parse_lane("HOT").unwrap(), Lane::Hot);
        assert_eq!(parse_lane("cold").unwrap(), Lane::Cold);
        assert!(parse_lane("invalid").is_err());
    }
}
