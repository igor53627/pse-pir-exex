//! HTTP routes for the PIR server

use axum::{
    extract::{Path, State},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

use inspire_core::Lane;
use inspire_pir::{ClientQuery, params::ShardConfig};

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
    /// Serialized ClientQuery (JSON)
    pub query: String,
}

/// PIR query response
#[derive(Serialize)]
pub struct QueryResponse {
    /// Serialized ServerResponse (JSON)
    pub response: String,
    /// Lane that processed the query
    pub lane: Lane,
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
    
    let client_query: ClientQuery = serde_json::from_str(&req.query)
        .map_err(|e| ServerError::InvalidQuery(format!("Invalid query JSON: {}", e)))?;
    
    let state = state.read().await;
    let response = state.process_query(lane, &client_query)?;
    
    let response_json = serde_json::to_string(&response)
        .map_err(|e| ServerError::Internal(format!("Failed to serialize response: {}", e)))?;

    Ok(Json(QueryResponse { 
        response: response_json, 
        lane 
    }))
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
