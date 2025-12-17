//! Server state: holds loaded lane data with lock-free reads via ArcSwap
//!
//! Uses ArcSwap for zero-contention reads during PIR queries.
//! Updates atomically swap in a new snapshot without blocking ongoing queries.

use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use inspire_core::{HotLaneManifest, Lane, LaneRouter, TwoLaneConfig};
use inspire_pir::{params::ShardConfig, ClientQuery, EncodedDatabase, ServerCrs, ServerResponse, respond};

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
    pub fn load(crs_path: &Path, db_path: &Path) -> Result<Self> {
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
        respond(&self.crs, &self.encoded_db, query).map_err(|e| ServerError::PirError(e.to_string()))
    }

    /// Get CRS as JSON string
    pub fn crs_json(&self) -> Result<String> {
        serde_json::to_string(&self.crs).map_err(ServerError::Json)
    }

    /// Get shard configuration for query building
    pub fn shard_config(&self) -> ShardConfig {
        self.encoded_db.config.clone()
    }
}

/// Immutable snapshot of server state
///
/// All queries operate on a cloned Arc of this snapshot, ensuring consistency
/// even if an update swaps in a new snapshot mid-query.
pub struct DbSnapshot {
    /// Hot lane data (smaller, faster queries)
    pub hot_lane: Option<LaneData>,
    /// Cold lane data (larger, slower queries)
    pub cold_lane: Option<LaneData>,
    /// Lane router for determining query routing
    pub router: Option<LaneRouter>,
    /// Block number this snapshot reflects
    pub block_number: Option<u64>,
}

impl DbSnapshot {
    /// Get lane data for a specific lane
    pub fn get_lane(&self, lane: Lane) -> Result<&LaneData> {
        match lane {
            Lane::Hot => self
                .hot_lane
                .as_ref()
                .ok_or_else(|| ServerError::LaneNotLoaded("Hot lane not loaded".to_string())),
            Lane::Cold => self
                .cold_lane
                .as_ref()
                .ok_or_else(|| ServerError::LaneNotLoaded("Cold lane not loaded".to_string())),
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
            block_number: self.block_number,
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
    pub block_number: Option<u64>,
}

/// Server state with lock-free reads via ArcSwap
///
/// Pattern: ArcSwap<Arc<DbSnapshot>>
/// - Queries: `snapshot.load_full()` returns `Arc<DbSnapshot>` (lock-free, O(1))
/// - Updates: Build new snapshot, then `snapshot.store(new_arc)` (atomic swap)
/// - In-flight queries continue using their cloned Arc until they complete
pub struct ServerState {
    /// Current database snapshot (lock-free access via ArcSwap)
    pub snapshot: ArcSwap<DbSnapshot>,
    /// Configuration (immutable)
    pub config: TwoLaneConfig,
}

impl ServerState {
    /// Create empty server state
    pub fn new(config: TwoLaneConfig) -> Self {
        let empty_snapshot = Arc::new(DbSnapshot {
            hot_lane: None,
            cold_lane: None,
            router: None,
            block_number: None,
        });
        Self {
            snapshot: ArcSwap::from(empty_snapshot),
            config,
        }
    }

    /// Get current snapshot for querying (lock-free)
    ///
    /// Returns an `Arc<DbSnapshot>` that stays valid even if a swap occurs.
    pub fn load_snapshot(&self) -> arc_swap::Guard<Arc<DbSnapshot>> {
        self.snapshot.load()
    }

    /// Get current snapshot as owned Arc (for long-running operations)
    pub fn load_snapshot_full(&self) -> Arc<DbSnapshot> {
        self.snapshot.load_full()
    }

    /// Load both lanes from disk and swap in the new snapshot
    pub fn load_lanes(&self) -> Result<()> {
        let hot_lane = self.try_load_hot_lane();
        let cold_lane = self.try_load_cold_lane();
        let router = self.try_load_router();

        let block_number = router.as_ref().map(|r| r.manifest().block_number);

        let new_snapshot = Arc::new(DbSnapshot {
            hot_lane,
            cold_lane,
            router,
            block_number,
        });

        self.snapshot.store(new_snapshot);
        Ok(())
    }

    /// Reload lanes from disk (for /admin/reload endpoint)
    ///
    /// Builds new snapshot off to the side, then atomically swaps it in.
    /// In-flight queries continue using the old snapshot until they finish.
    pub fn reload(&self) -> Result<ReloadResult> {
        let old_snapshot = self.snapshot.load_full();
        let old_block = old_snapshot.block_number;

        let start = std::time::Instant::now();
        self.load_lanes()?;
        let duration = start.elapsed();

        let new_snapshot = self.snapshot.load_full();
        let new_block = new_snapshot.block_number;

        tracing::info!(
            old_block = ?old_block,
            new_block = ?new_block,
            duration_ms = duration.as_millis(),
            "Database snapshot reloaded"
        );

        Ok(ReloadResult {
            old_block_number: old_block,
            new_block_number: new_block,
            reload_duration_ms: duration.as_millis() as u64,
            hot_loaded: new_snapshot.hot_lane.is_some(),
            cold_loaded: new_snapshot.cold_lane.is_some(),
        })
    }

    fn try_load_hot_lane(&self) -> Option<LaneData> {
        let crs_path = &self.config.hot_lane_crs;
        let db_path = &self.config.hot_lane_db;

        if !crs_path.exists() {
            tracing::warn!("Hot lane CRS not found: {}", crs_path.display());
            return None;
        }

        match LaneData::load(crs_path, db_path) {
            Ok(lane_data) => {
                if let Err(e) = self.validate_lane_data(&lane_data, Lane::Hot) {
                    tracing::warn!("Hot lane validation failed: {}", e);
                    return None;
                }
                tracing::info!(entries = lane_data.entry_count, "Hot lane loaded");
                Some(lane_data)
            }
            Err(e) => {
                tracing::warn!("Failed to load hot lane: {}", e);
                None
            }
        }
    }

    fn try_load_cold_lane(&self) -> Option<LaneData> {
        let crs_path = &self.config.cold_lane_crs;
        let db_path = &self.config.cold_lane_db;

        if !crs_path.exists() {
            tracing::warn!("Cold lane CRS not found: {}", crs_path.display());
            return None;
        }

        match LaneData::load(crs_path, db_path) {
            Ok(lane_data) => {
                if let Err(e) = self.validate_lane_data(&lane_data, Lane::Cold) {
                    tracing::warn!("Cold lane validation failed: {}", e);
                    return None;
                }
                tracing::info!(entries = lane_data.entry_count, "Cold lane loaded");
                Some(lane_data)
            }
            Err(e) => {
                tracing::warn!("Failed to load cold lane: {}", e);
                None
            }
        }
    }

    fn try_load_router(&self) -> Option<LaneRouter> {
        let manifest_path = &self.config.hot_lane_manifest;

        if !manifest_path.exists() {
            return None;
        }

        match HotLaneManifest::load(manifest_path) {
            Ok(manifest) => {
                tracing::info!(
                    block = manifest.block_number,
                    contracts = manifest.contracts.len(),
                    "Lane router loaded"
                );
                Some(LaneRouter::new(manifest))
            }
            Err(e) => {
                tracing::warn!("Failed to load manifest: {}", e);
                None
            }
        }
    }

    fn validate_lane_data(&self, lane_data: &LaneData, lane: Lane) -> Result<()> {
        let (expected_entries, lane_name) = match lane {
            Lane::Hot => (self.config.hot_entries, "hot"),
            Lane::Cold => (self.config.cold_entries, "cold"),
        };

        if lane_data.entry_count != expected_entries {
            return Err(ServerError::ConfigMismatch {
                field: format!("{}_entries", lane_name),
                config_value: expected_entries.to_string(),
                actual_value: lane_data.entry_count.to_string(),
            });
        }

        let db_entry_size = lane_data.encoded_db.config.entry_size_bytes as usize;
        if db_entry_size != self.config.entry_size {
            return Err(ServerError::ConfigMismatch {
                field: "entry_size".to_string(),
                config_value: self.config.entry_size.to_string(),
                actual_value: db_entry_size.to_string(),
            });
        }

        Ok(())
    }
}

/// Result of a reload operation
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReloadResult {
    pub old_block_number: Option<u64>,
    pub new_block_number: Option<u64>,
    pub reload_duration_ms: u64,
    pub hot_loaded: bool,
    pub cold_loaded: bool,
}

/// Shared server state type (now just Arc, no RwLock needed)
pub type SharedState = Arc<ServerState>;

/// Create shared state from config
pub fn create_shared_state(config: TwoLaneConfig) -> SharedState {
    Arc::new(ServerState::new(config))
}
