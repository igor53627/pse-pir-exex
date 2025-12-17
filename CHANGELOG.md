# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
