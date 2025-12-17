//! Server state: holds loaded lane data

use std::sync::Arc;

use inspire_core::{HotLaneManifest, Lane, LaneRouter, TwoLaneConfig};
use inspire_pir::{ServerCrs, EncodedDatabase, ClientQuery, ServerResponse, respond};

use crate::error::{Result, ServerError};

/// Lane-specific PIR data (CRS + encoded database)
pub struct LaneData {
    /// Server CRS for this lane
    pub crs: ServerCrs,
    /// Encoded database for this lane
    pub encoded_db: EncodedDatabase,
    /// Number of entries in this lane
    pub entry_count: u64,
}

impl LaneData {
    /// Load lane data from disk
    pub fn load(crs_path: &std::path::Path, db_path: &std::path::Path) -> Result<Self> {
        let crs_json = std::fs::read_to_string(crs_path)?;
        let crs: ServerCrs = serde_json::from_str(&crs_json)
            .map_err(|e| ServerError::Internal(format!("Failed to parse CRS: {}", e)))?;
        
        let db_json = std::fs::read_to_string(db_path)?;
        let encoded_db: EncodedDatabase = serde_json::from_str(&db_json)
            .map_err(|e| ServerError::Internal(format!("Failed to parse database: {}", e)))?;

        let entry_count = encoded_db.config.total_entries;
        
        Ok(Self {
            crs,
            encoded_db,
            entry_count,
        })
    }

    /// Process a PIR query and return the response
    pub fn process_query(&self, query: &ClientQuery) -> Result<ServerResponse> {
        respond(&self.crs, &self.encoded_db, query)
            .map_err(|e| ServerError::PirError(e.to_string()))
    }

    /// Get CRS as JSON string
    pub fn crs_json(&self) -> Result<String> {
        serde_json::to_string(&self.crs)
            .map_err(|e| ServerError::Json(e))
    }
}

/// Server state containing both lane databases
pub struct ServerState {
    /// Hot lane data (smaller, faster queries)
    pub hot_lane: Option<LaneData>,
    /// Cold lane data (larger, slower queries)
    pub cold_lane: Option<LaneData>,
    /// Lane router for determining query routing
    pub router: Option<LaneRouter>,
    /// Configuration
    pub config: TwoLaneConfig,
}

impl ServerState {
    /// Create empty server state
    pub fn new(config: TwoLaneConfig) -> Self {
        Self {
            hot_lane: None,
            cold_lane: None,
            router: None,
            config,
        }
    }

    /// Load hot lane from disk
    pub fn load_hot_lane(&mut self) -> Result<()> {
        let crs_path = &self.config.hot_lane_crs;
        let db_path = &self.config.hot_lane_db;
        let manifest_path = &self.config.hot_lane_manifest;

        if !crs_path.exists() {
            return Err(ServerError::LaneNotLoaded(format!(
                "Hot lane CRS not found: {}",
                crs_path.display()
            )));
        }

        let lane_data = LaneData::load(crs_path, db_path)?;
        
        tracing::info!(
            entries = lane_data.entry_count,
            "Hot lane loaded"
        );
        
        self.hot_lane = Some(lane_data);

        if manifest_path.exists() {
            let manifest = HotLaneManifest::load(manifest_path)
                .map_err(|e| ServerError::Internal(e.to_string()))?;
            self.router = Some(LaneRouter::new(manifest));
        }

        Ok(())
    }

    /// Load cold lane from disk
    pub fn load_cold_lane(&mut self) -> Result<()> {
        let crs_path = &self.config.cold_lane_crs;
        let db_path = &self.config.cold_lane_db;

        if !crs_path.exists() {
            return Err(ServerError::LaneNotLoaded(format!(
                "Cold lane CRS not found: {}",
                crs_path.display()
            )));
        }

        let lane_data = LaneData::load(crs_path, db_path)?;
        
        tracing::info!(
            entries = lane_data.entry_count,
            "Cold lane loaded"
        );
        
        self.cold_lane = Some(lane_data);

        Ok(())
    }

    /// Get lane data for a specific lane
    pub fn get_lane(&self, lane: Lane) -> Result<&LaneData> {
        match lane {
            Lane::Hot => self.hot_lane.as_ref().ok_or_else(|| {
                ServerError::LaneNotLoaded("Hot lane not loaded".to_string())
            }),
            Lane::Cold => self.cold_lane.as_ref().ok_or_else(|| {
                ServerError::LaneNotLoaded("Cold lane not loaded".to_string())
            }),
        }
    }

    /// Process a PIR query for a specific lane
    pub fn process_query(&self, lane: Lane, query: &ClientQuery) -> Result<ServerResponse> {
        let lane_data = self.get_lane(lane)?;
        lane_data.process_query(query)
    }

    /// Check if both lanes are loaded
    pub fn is_ready(&self) -> bool {
        self.hot_lane.is_some() && self.cold_lane.is_some()
    }

    /// Get lane statistics
    pub fn stats(&self) -> LaneStats {
        LaneStats {
            hot_loaded: self.hot_lane.is_some(),
            cold_loaded: self.cold_lane.is_some(),
            hot_entries: self.hot_lane.as_ref().map(|l| l.entry_count).unwrap_or(0),
            cold_entries: self.cold_lane.as_ref().map(|l| l.entry_count).unwrap_or(0),
            hot_contracts: self.router.as_ref().map(|r| r.hot_contract_count()).unwrap_or(0),
        }
    }
}

/// Lane statistics for monitoring
#[derive(Debug, Clone, serde::Serialize)]
pub struct LaneStats {
    pub hot_loaded: bool,
    pub cold_loaded: bool,
    pub hot_entries: u64,
    pub cold_entries: u64,
    pub hot_contracts: usize,
}

/// Shared server state type
pub type SharedState = Arc<tokio::sync::RwLock<ServerState>>;

/// Create shared state from config
pub fn create_shared_state(config: TwoLaneConfig) -> SharedState {
    Arc::new(tokio::sync::RwLock::new(ServerState::new(config)))
}
