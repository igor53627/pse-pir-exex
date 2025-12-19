# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Seed expansion support for ~50% smaller queries
  - `/query/{lane}/seeded` endpoint for seeded queries
  - `SeededQueryRequest` type on server
  - `ClientBuilder::seed_expansion(bool)` to enable/disable
  - Seed expansion enabled by default

### Changed

- Updated README to reflect accurate InsPIRe communication costs
  - InsPIRe is O(d) not O(sqrt(N)) - query size is independent of database size
  - Hot lane benefit is faster server response, not smaller queries
  - Updated performance tables with seed expansion numbers (~230 KB/query)

### Added (prior)

- Gas backfill for data-driven hot lane selection (`backfill` feature)
  - `lane-backfill` binary for scanning historical blocks to find gas guzzlers
  - Concurrent block fetching via alloy-provider with configurable batch size
  - Progress bar showing backfill progress and ETA
  - `GasTracker` for aggregating gas usage per contract address
  - `HybridScorer` combining gas data with curated contracts and category weights
  - Category weight multipliers (privacy 3x, bridge 2x, DeFi 1.5x)
  - `BackfillResult` and `GasStats` types for serializable output
  - Integration with `HotLaneBuilder` via `load_scored_contracts()`
  - Documentation at docs/GAS_BACKFILL.md
- Reth ExEx integration for real-time lane updates (`exex` feature) (#17)
  - `lane-exex` binary for running ExEx as a standalone Reth execution extension
  - Subscribes to ChainCommitted/Reverted/Reorged notifications from Reth
  - Triggers `/admin/reload` on PIR server when chain state changes
  - Debouncing support to avoid rapid reloads (configurable via `--reload-debounce-secs`)
  - Upgraded to Reth v1.9.3 API with `reth-ethereum` consolidated crate
- ExEx metrics for monitoring reload performance:
  - `lane_updater_reload_total`: Total number of reload requests
  - `lane_updater_reload_duration_ms`: Reload latency histogram
  - `lane_updater_reload_errors_total`: Total reload errors
  - `lane_updater_blocks_processed`: Total blocks processed
  - `lane_updater_reorgs_total`: Total chain reorgs detected
  - `lane_updater_debounce_skips_total`: Skipped reloads due to debouncing
- Integration tests for `ReloadClient` with mock HTTP server
- `ReloadClient` for triggering server database reloads via HTTP
- `LaneUpdaterConfig` for configuring ExEx behavior
- Lock-free database reads via `ArcSwap` for zero-contention query handling (#29)
- `/admin/reload` endpoint for hot-reloading lane databases without restart
- `DbSnapshot` immutable snapshot type for consistent query execution
- Block number tracking in `ServerInfo` and `LaneStats`
- `ReloadResult` type for reload operation feedback
- Mmap mode for O(1) database swap times (`use_mmap` config option)
- `LaneDatabase` enum supporting both in-memory and mmap-backed databases
- `hot_lane_shards` and `cold_lane_shards` config paths for binary shard files

### Changed

- Refactored `ServerState` from `Arc<RwLock<..>>` to `Arc<ServerState>` with internal `ArcSwap`
- Queries now clone `Arc<DbSnapshot>` at start, ensuring consistency even during swaps
- `ServerBuilder::build()` is now synchronous (no longer async)
- `LaneData` now supports both `load_inmemory()` and `load_mmap()` loading modes

### Technical Notes

- Uses RCU (Read-Copy-Update) semantics: old snapshots stay alive until last in-flight query completes
- Memory consideration: during swap, temporarily need 2x memory for the swapped lane
- Mmap mode: swap time is O(1) (~1-5ms) regardless of database size
