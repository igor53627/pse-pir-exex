//! Server state: holds loaded lane data with lock-free reads via ArcSwap
//!
//! Uses ArcSwap for zero-contention reads during PIR queries.
//! Updates atomically swap in a new snapshot without blocking ongoing queries.
//!
//! Supports two loading modes:
//! - In-memory (JSON): Loads entire database into RAM
//! - Mmap (binary): Memory-maps shard files for O(1) swap time

use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use inspire_client::BucketIndex;
use inspire_core::{
    CrsMetadata, HotLaneManifest, Lane, LaneRouter, TwoLaneConfig, PIR_PARAMS_VERSION,
};
use inspire_pir::{
    params::ShardConfig, respond_inspiring, respond_mmap_one_packing, respond_one_packing,
    ClientQuery, EncodedDatabase, MmapDatabase, ServerCrs, ServerResponse,
};

use crate::broadcast::BucketBroadcast;
use crate::error::{Result, ServerError};

/// Database storage mode
pub enum LaneDatabase {
    /// In-memory encoded database (loaded from JSON)
    InMemory(EncodedDatabase),
    /// Memory-mapped database (binary shards, O(1) swap)
    Mmap(MmapDatabase),
}

impl LaneDatabase {
    /// Get shard configuration
    pub fn shard_config(&self) -> ShardConfig {
        match self {
            LaneDatabase::InMemory(db) => db.config.clone(),
            LaneDatabase::Mmap(db) => db.config.clone(),
        }
    }

    /// Get total entry count
    pub fn entry_count(&self) -> u64 {
        match self {
            LaneDatabase::InMemory(db) => db.config.total_entries,
            LaneDatabase::Mmap(db) => db.config.total_entries,
        }
    }
}

/// Lane-specific PIR data (CRS + database)
pub struct LaneData {
    /// Server CRS for this lane
    pub crs: ServerCrs,
    /// Database (in-memory or mmap)
    pub database: LaneDatabase,
    /// Number of entries in this lane
    pub entry_count: u64,
}

impl LaneData {
    /// Load lane data from disk (in-memory mode)
    pub fn load_inmemory(crs_path: &Path, db_path: &Path) -> Result<Self> {
        let crs_json = std::fs::read_to_string(crs_path)?;
        let crs: ServerCrs = serde_json::from_str(&crs_json)
            .map_err(|e| ServerError::Internal(format!("Failed to parse CRS: {}", e)))?;

        let db_json = std::fs::read_to_string(db_path)?;
        let encoded_db: EncodedDatabase = serde_json::from_str(&db_json)
            .map_err(|e| ServerError::Internal(format!("Failed to parse database: {}", e)))?;

        let entry_count = encoded_db.config.total_entries;

        Ok(Self {
            crs,
            database: LaneDatabase::InMemory(encoded_db),
            entry_count,
        })
    }

    /// Load lane data with mmap (O(1) swap time)
    pub fn load_mmap(crs_path: &Path, shards_dir: &Path, config: ShardConfig) -> Result<Self> {
        let crs_json = std::fs::read_to_string(crs_path)?;
        let crs: ServerCrs = serde_json::from_str(&crs_json)
            .map_err(|e| ServerError::Internal(format!("Failed to parse CRS: {}", e)))?;

        let mmap_db = MmapDatabase::open(shards_dir, config.clone())
            .map_err(|e| ServerError::Internal(format!("Failed to open mmap database: {}", e)))?;

        let entry_count = config.total_entries;

        Ok(Self {
            crs,
            database: LaneDatabase::Mmap(mmap_db),
            entry_count,
        })
    }

    /// Process a PIR query and return the response
    ///
    /// Uses InspiRING packing (~35x faster) when packing keys available,
    /// otherwise falls back to tree packing. Both reduce response from 544 KB to 32 KB.
    pub fn process_query(&self, query: &ClientQuery) -> Result<ServerResponse> {
        match &self.database {
            LaneDatabase::InMemory(db) => {
                // Use InspiRING if packing keys available (~35x faster), otherwise tree packing
                if query.inspiring_packing_keys.is_some() {
                    respond_inspiring(&self.crs, db, query)
                        .map_err(|e| ServerError::PirError(e.to_string()))
                } else {
                    respond_one_packing(&self.crs, db, query)
                        .map_err(|e| ServerError::PirError(e.to_string()))
                }
            }
            LaneDatabase::Mmap(db) => {
                // TODO: Add respond_mmap_inspiring when needed
                respond_mmap_one_packing(&self.crs, db, query)
                    .map_err(|e| ServerError::PirError(e.to_string()))
            }
        }
    }

    /// Get CRS as JSON string
    pub fn crs_json(&self) -> Result<String> {
        serde_json::to_string(&self.crs).map_err(ServerError::Json)
    }

    /// Get shard configuration for query building
    pub fn shard_config(&self) -> ShardConfig {
        self.database.shard_config()
    }
}

/// Cached bucket index with precompressed bytes
///
/// Stores both the parsed index (for raw endpoint) and precompressed bytes
/// (for compressed endpoint) to avoid expensive zstd level 19 compression per request.
pub struct CachedBucketIndex {
    /// Parsed bucket index for lookups and raw serving
    pub index: BucketIndex,
    /// Precompressed bytes (zstd level 19)
    pub compressed: Vec<u8>,
}

impl CachedBucketIndex {
    /// Create cached index from parsed BucketIndex
    pub fn new(index: BucketIndex) -> Result<Self> {
        let compressed = index.to_compressed().map_err(|e| {
            ServerError::Internal(format!("Failed to compress bucket index: {}", e))
        })?;
        Ok(Self { index, compressed })
    }

    /// Total entries in the index
    pub fn total_entries(&self) -> u64 {
        self.index.total_entries()
    }
}

/// Cached stem index with entry count
///
/// Format: count:8 + (stem:31 + offset:8)* - same as StemIndex in inspire-client-wasm
pub struct CachedStemIndex {
    /// Raw binary data (count:8 + (stem:31 + offset:8)*)
    pub data: Vec<u8>,
    /// Number of stems in the index
    pub stem_count: u64,
    /// Total entries in the database
    pub total_entries: u64,
}

impl CachedStemIndex {
    /// Parse stem index from raw bytes
    pub fn from_bytes(data: Vec<u8>) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        let stem_count = u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]);
        // Each entry is 31 bytes (stem) + 8 bytes (offset) = 39 bytes
        let expected_len = 8usize
            .checked_add((stem_count as usize).checked_mul(39)?)?;
        if data.len() != expected_len {
            return None;
        }
        // Total entries is in the last offset value (at the end of the last entry)
        let total_entries = if stem_count > 0 {
            let last_offset_start = data.len() - 8;
            u64::from_le_bytes(data[last_offset_start..].try_into().ok()?)
        } else {
            0
        };
        Some(Self {
            data,
            stem_count,
            total_entries,
        })
    }
}

/// Range delta info for a single range
#[derive(Clone)]
pub struct CachedRangeInfo {
    pub blocks_covered: u32,
    pub offset: u32,
    pub size: u32,
}

/// Cached range delta file for efficient client sync
pub struct CachedRangeDelta {
    /// Full file data (supports HTTP Range requests)
    pub data: Vec<u8>,
    /// Current block number
    pub current_block: u64,
    /// Range directory
    pub ranges: Vec<CachedRangeInfo>,
}

impl CachedRangeDelta {
    /// Load from file
    pub fn from_file(path: &std::path::Path) -> Option<Self> {
        use inspire_core::bucket_index::range_delta::{
            RangeDeltaHeader, RangeEntry, HEADER_SIZE, RANGE_ENTRY_SIZE,
        };

        let data = std::fs::read(path).ok()?;
        if data.len() < HEADER_SIZE {
            return None;
        }

        let header = RangeDeltaHeader::from_bytes(&data)?;

        let mut ranges = Vec::new();
        let mut offset = HEADER_SIZE;
        for _ in 0..header.num_ranges {
            if offset + RANGE_ENTRY_SIZE > data.len() {
                break;
            }
            let entry = RangeEntry::from_bytes(&data[offset..])?;
            ranges.push(CachedRangeInfo {
                blocks_covered: entry.blocks_covered,
                offset: entry.offset,
                size: entry.size,
            });
            offset += RANGE_ENTRY_SIZE;
        }

        Some(Self {
            data,
            current_block: header.current_block,
            ranges,
        })
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
    /// Bucket index for sparse client lookups (with precompressed cache)
    pub bucket_index: Option<CachedBucketIndex>,
    /// Stem index for stem-ordered databases
    pub stem_index: Option<CachedStemIndex>,
    /// Range delta file for efficient client sync
    pub range_delta: Option<CachedRangeDelta>,
    /// Block number this snapshot reflects
    pub block_number: Option<u64>,
    /// PIR params version (from CRS metadata)
    pub pir_params_version: u16,
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
            hot_contracts: self
                .router
                .as_ref()
                .map(|r| r.hot_contract_count())
                .unwrap_or(0),
            block_number: self.block_number,
            pir_params_version: self.pir_params_version,
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
    pub pir_params_version: u16,
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
    /// Broadcast channel for bucket index deltas
    pub bucket_broadcast: BucketBroadcast,
}

impl ServerState {
    /// Create empty server state
    pub fn new(config: TwoLaneConfig) -> Self {
        let empty_snapshot = Arc::new(DbSnapshot {
            hot_lane: None,
            cold_lane: None,
            router: None,
            bucket_index: None,
            stem_index: None,
            range_delta: None,
            block_number: None,
            pir_params_version: PIR_PARAMS_VERSION,
        });
        Self {
            snapshot: ArcSwap::from(empty_snapshot),
            config,
            bucket_broadcast: BucketBroadcast::new(),
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
    ///
    /// Returns an error if no lanes could be loaded (server cannot serve queries).
    pub fn load_lanes(&self) -> Result<()> {
        let hot_lane = self.try_load_hot_lane();
        let cold_lane = self.try_load_cold_lane();
        let router = self.try_load_router();
        let bucket_index = self.try_load_bucket_index();
        let stem_index = self.try_load_stem_index();
        let range_delta = self.try_load_range_delta();

        if hot_lane.is_none() && cold_lane.is_none() {
            return Err(ServerError::Internal(
                "Failed to load any lanes - server cannot serve queries".to_string(),
            ));
        }

        let block_number = router.as_ref().map(|r| r.manifest().block_number);

        let new_snapshot = Arc::new(DbSnapshot {
            hot_lane,
            cold_lane,
            router,
            bucket_index,
            stem_index,
            range_delta,
            block_number,
            pir_params_version: PIR_PARAMS_VERSION,
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
            mmap_mode = self.config.use_mmap,
            "Database snapshot reloaded"
        );

        Ok(ReloadResult {
            old_block_number: old_block,
            new_block_number: new_block,
            reload_duration_ms: duration.as_millis() as u64,
            hot_loaded: new_snapshot.hot_lane.is_some(),
            cold_loaded: new_snapshot.cold_lane.is_some(),
            mmap_mode: self.config.use_mmap,
        })
    }

    fn try_load_hot_lane(&self) -> Option<LaneData> {
        let crs_path = &self.config.hot_lane_crs;

        if !crs_path.exists() {
            tracing::warn!("Hot lane CRS not found: {}", crs_path.display());
            return None;
        }

        let result = if self.config.use_mmap {
            self.load_lane_mmap(Lane::Hot)
        } else {
            self.load_lane_inmemory(Lane::Hot)
        };

        match result {
            Ok(lane_data) => {
                let mode = if self.config.use_mmap {
                    "mmap"
                } else {
                    "inmemory"
                };
                tracing::info!(entries = lane_data.entry_count, mode, "Hot lane loaded");
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

        if !crs_path.exists() {
            tracing::warn!("Cold lane CRS not found: {}", crs_path.display());
            return None;
        }

        let result = if self.config.use_mmap {
            self.load_lane_mmap(Lane::Cold)
        } else {
            self.load_lane_inmemory(Lane::Cold)
        };

        match result {
            Ok(lane_data) => {
                let mode = if self.config.use_mmap {
                    "mmap"
                } else {
                    "inmemory"
                };
                tracing::info!(entries = lane_data.entry_count, mode, "Cold lane loaded");
                Some(lane_data)
            }
            Err(e) => {
                tracing::warn!("Failed to load cold lane: {}", e);
                None
            }
        }
    }

    fn load_lane_inmemory(&self, lane: Lane) -> Result<LaneData> {
        let (crs_path, db_path) = match lane {
            Lane::Hot => (&self.config.hot_lane_crs, &self.config.hot_lane_db),
            Lane::Cold => (&self.config.cold_lane_crs, &self.config.cold_lane_db),
        };

        self.validate_crs_metadata(lane)?;

        let lane_data = LaneData::load_inmemory(crs_path, db_path)?;
        self.validate_lane_data(&lane_data, lane)?;
        Ok(lane_data)
    }

    fn load_lane_mmap(&self, lane: Lane) -> Result<LaneData> {
        let (crs_path, shards_dir, expected_entries) = match lane {
            Lane::Hot => (
                &self.config.hot_lane_crs,
                self.config.hot_lane_shards.as_ref(),
                self.config.hot_entries,
            ),
            Lane::Cold => (
                &self.config.cold_lane_crs,
                self.config.cold_lane_shards.as_ref(),
                self.config.cold_entries,
            ),
        };

        let shards_dir = shards_dir.ok_or_else(|| {
            ServerError::Internal(format!(
                "{:?} lane shards directory not configured for mmap mode",
                lane
            ))
        })?;

        if !shards_dir.exists() {
            return Err(ServerError::Internal(format!(
                "Shards directory not found: {}",
                shards_dir.display()
            )));
        }

        let config = ShardConfig {
            shard_size_bytes: self.config.shard_size_bytes,
            entry_size_bytes: self.config.entry_size,
            total_entries: expected_entries,
        };

        self.validate_crs_metadata(lane)?;

        let lane_data = LaneData::load_mmap(crs_path, shards_dir, config)?;
        Ok(lane_data)
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

    fn try_load_bucket_index(&self) -> Option<CachedBucketIndex> {
        let index_path = self.config.bucket_index_path.as_ref()?;

        if !index_path.exists() {
            tracing::debug!("Bucket index not found: {}", index_path.display());
            return None;
        }

        match std::fs::read(index_path) {
            Ok(data) => {
                // Try compressed first, fall back to uncompressed
                let result = if index_path.extension().map_or(false, |e| e == "zst") {
                    BucketIndex::from_compressed(&data)
                } else {
                    BucketIndex::from_bytes(&data)
                };

                match result {
                    Ok(index) => {
                        let total_entries = index.total_entries();
                        match CachedBucketIndex::new(index) {
                            Ok(cached) => {
                                tracing::info!(
                                    path = %index_path.display(),
                                    total_entries,
                                    compressed_size = cached.compressed.len(),
                                    "Bucket index loaded and precompressed"
                                );
                                Some(cached)
                            }
                            Err(e) => {
                                tracing::warn!("Failed to precompress bucket index: {}", e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse bucket index: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read bucket index: {}", e);
                None
            }
        }
    }

    fn try_load_stem_index(&self) -> Option<CachedStemIndex> {
        let index_path = self.config.stem_index_path.as_ref()?;

        if !index_path.exists() {
            tracing::debug!("Stem index not found: {}", index_path.display());
            return None;
        }

        match std::fs::read(index_path) {
            Ok(data) => match CachedStemIndex::from_bytes(data) {
                Some(cached) => {
                    tracing::info!(
                        path = %index_path.display(),
                        stem_count = cached.stem_count,
                        total_entries = cached.total_entries,
                        size_bytes = cached.data.len(),
                        "Stem index loaded"
                    );
                    Some(cached)
                }
                None => {
                    tracing::warn!("Failed to parse stem index: invalid format");
                    None
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read stem index: {}", e);
                None
            }
        }
    }

    fn try_load_range_delta(&self) -> Option<CachedRangeDelta> {
        let delta_path = self.config.range_delta_path.as_ref()?;

        if !delta_path.exists() {
            tracing::debug!("Range delta file not found: {}", delta_path.display());
            return None;
        }

        match CachedRangeDelta::from_file(&delta_path) {
            Some(cached) => {
                tracing::info!(
                    path = %delta_path.display(),
                    current_block = cached.current_block,
                    ranges = cached.ranges.len(),
                    size_bytes = cached.data.len(),
                    "Range delta file loaded"
                );
                Some(cached)
            }
            None => {
                tracing::warn!("Failed to parse range delta file: invalid format");
                None
            }
        }
    }

    fn validate_lane_data(&self, lane_data: &LaneData, lane: Lane) -> Result<()> {
        let (expected_entries, lane_name) = match lane {
            Lane::Hot => (self.config.hot_entries, "hot"),
            Lane::Cold => (self.config.cold_entries, "cold"),
        };

        if expected_entries > 0 && lane_data.entry_count != expected_entries {
            return Err(ServerError::ConfigMismatch {
                field: format!("{}_entries", lane_name),
                config_value: expected_entries.to_string(),
                actual_value: lane_data.entry_count.to_string(),
            });
        }

        let db_entry_size = lane_data.shard_config().entry_size_bytes as usize;
        if self.config.entry_size > 0 && db_entry_size != self.config.entry_size {
            return Err(ServerError::ConfigMismatch {
                field: "entry_size".to_string(),
                config_value: self.config.entry_size.to_string(),
                actual_value: db_entry_size.to_string(),
            });
        }

        Ok(())
    }

    fn validate_crs_metadata(&self, lane: Lane) -> Result<()> {
        let (crs_path, lane_name) = match lane {
            Lane::Hot => (&self.config.hot_lane_crs, "hot"),
            Lane::Cold => (&self.config.cold_lane_crs, "cold"),
        };

        let meta_path = crs_path.with_file_name("crs.meta.json");

        if !meta_path.exists() {
            tracing::warn!(
                lane = lane_name,
                path = %meta_path.display(),
                "CRS metadata not found - skipping version check (legacy CRS)"
            );
            return Ok(());
        }

        let metadata = CrsMetadata::load(&meta_path)
            .map_err(|e| ServerError::Internal(format!("Failed to load CRS metadata: {}", e)))?;

        if metadata.pir_params_version != PIR_PARAMS_VERSION {
            return Err(ServerError::ParamsVersionMismatch {
                crs_version: metadata.pir_params_version,
                expected_version: PIR_PARAMS_VERSION,
                lane: lane_name.to_string(),
            });
        }

        tracing::info!(
            lane = lane_name,
            pir_params_version = metadata.pir_params_version,
            entry_count = metadata.entry_count,
            "CRS metadata validated"
        );

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
    pub mmap_mode: bool,
}

/// Shared server state type (now just Arc, no RwLock needed)
pub type SharedState = Arc<ServerState>;

/// Create shared state from config
pub fn create_shared_state(config: TwoLaneConfig) -> SharedState {
    Arc::new(ServerState::new(config))
}
