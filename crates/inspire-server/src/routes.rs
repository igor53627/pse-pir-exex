//! HTTP routes for the PIR server

use axum::{
    extract::{Path, State},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

use inspire_core::{Lane, PROTOCOL_VERSION};
use inspire_pir::{ClientQuery, ServerResponse, params::ShardConfig};

use crate::error::{Result, ServerError};
use crate::state::{LaneStats, SharedState};

/// Health check response
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub lanes: LaneStats,
}

/// PIR query request
#[derive(Deserialize)]
pub struct QueryRequest {
    /// Client PIR query
    pub query: ClientQuery,
}

/// PIR query response
#[derive(Serialize)]
pub struct QueryResponse {
    /// Server PIR response
    pub response: ServerResponse,
    /// Lane that processed the query
    pub lane: Lane,
}

/// Server info response (for version negotiation)
#[derive(Serialize)]
pub struct ServerInfo {
    /// Protocol version
    pub version: String,
    /// Configuration hash for change detection
    pub config_hash: String,
    /// Manifest block number
    pub manifest_block: Option<u64>,
    /// Number of entries in hot lane
    pub hot_entries: u64,
    /// Number of entries in cold lane
    pub cold_entries: u64,
    /// Number of contracts in hot lane
    pub hot_contracts: usize,
}

/// CRS response
#[derive(Serialize)]
pub struct CrsResponse {
    /// Serialized ServerCrs (JSON)
    pub crs: String,
    /// Lane this CRS belongs to
    pub lane: Lane,
    /// Number of entries in this lane
    pub entry_count: u64,
    /// Shard configuration for query building
    pub shard_config: ShardConfig,
}

/// Health check endpoint
async fn health(State(state): State<SharedState>) -> Json<HealthResponse> {
    let state = state.read().await;
    let stats = state.stats();
    let status = if state.is_ready() { "ready" } else { "loading" };
    
    Json(HealthResponse {
        status: status.to_string(),
        lanes: stats,
    })
}

/// Server info endpoint (for version negotiation)
async fn info(State(state): State<SharedState>) -> Json<ServerInfo> {
    let state = state.read().await;
    let stats = state.stats();
    
    Json(ServerInfo {
        version: state.config.version.clone(),
        config_hash: state.config.config_hash.clone().unwrap_or_else(|| state.config.compute_hash()),
        manifest_block: state.router.as_ref().map(|r| r.manifest().block_number),
        hot_entries: stats.hot_entries,
        cold_entries: stats.cold_entries,
        hot_contracts: stats.hot_contracts,
    })
}

/// Get CRS for a specific lane
async fn get_crs(
    State(state): State<SharedState>,
    Path(lane): Path<String>,
) -> Result<Json<CrsResponse>> {
    let lane = parse_lane(&lane)?;
    let state = state.read().await;
    let lane_data = state.get_lane(lane)?;

    Ok(Json(CrsResponse {
        crs: lane_data.crs_json()?,
        lane,
        entry_count: lane_data.entry_count,
        shard_config: lane_data.shard_config(),
    }))
}

/// Process a PIR query
async fn query(
    State(state): State<SharedState>,
    Path(lane): Path<String>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResponse>> {
    let lane = parse_lane(&lane)?;
    
    let state = state.read().await;
    let response = state.process_query(lane, &req.query)?;

    Ok(Json(QueryResponse { response, lane }))
}

/// Parse lane from URL path
fn parse_lane(s: &str) -> Result<Lane> {
    match s.to_lowercase().as_str() {
        "hot" => Ok(Lane::Hot),
        "cold" => Ok(Lane::Cold),
        _ => Err(ServerError::InvalidQuery(format!("Invalid lane: {}", s))),
    }
}

/// Create the router with all routes
pub fn create_router(state: SharedState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/info", get(info))
        .route("/crs/{lane}", get(get_crs))
        .route("/query/{lane}", post(query))
        .with_state(state)
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
