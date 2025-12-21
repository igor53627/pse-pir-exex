//! HTTP routes for the PIR server

use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

use inspire_core::Lane;
use inspire_pir::{params::ShardConfig, ClientQuery, SeededClientQuery, ServerResponse};

use crate::error::{Result, ServerError};
use crate::state::{ReloadResult, SharedState};
use crate::metrics;

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
        status: if ready { "ok".to_string() } else { "unavailable".to_string() },
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
async fn get_crs(State(state): State<SharedState>, Path(lane): Path<String>) -> Result<Json<CrsResponse>> {
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

    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        binary,
    )
        .into_response())
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

    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        binary,
    )
        .into_response())
}

/// Reload lanes from disk (admin endpoint)
///
/// Atomically swaps in a new snapshot without blocking ongoing queries.
async fn admin_reload(State(state): State<SharedState>) -> Result<Json<ReloadResult>> {
    let result = state.reload()?;
    Ok(Json(result))
}

/// Parse lane from URL path
fn parse_lane(s: &str) -> Result<Lane> {
    match s.to_lowercase().as_str() {
        "hot" => Ok(Lane::Hot),
        "cold" => Ok(Lane::Cold),
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
        .with_state(state);

    if let Some(handle) = prometheus_handle {
        router = router.route("/metrics", get(move || {
            let metrics = handle.render();
            async move { metrics }
        }));
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
        .route("/admin/reload", post(admin_reload))
        .with_state(state);

    if let Some(handle) = prometheus_handle {
        router = router.route("/metrics", get(move || {
            let metrics = handle.render();
            async move { metrics }
        }));
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
