//! HTTP routes for the PIR server

use axum::{
    extract::{Path, State},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

use inspire_core::Lane;

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
        crs: lane_data.crs_json.clone(),
        lane,
        entry_count: lane_data.entry_count,
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
    let _lane_data = state.get_lane(lane)?;

    let response = process_pir_query(&req.query)?;

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

/// Process PIR query using inspire-rs
///
/// TODO: Integrate with actual inspire-rs library
fn process_pir_query(_query_json: &str) -> Result<String> {
    Err(ServerError::Internal(
        "PIR query processing not yet implemented".to_string(),
    ))
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
