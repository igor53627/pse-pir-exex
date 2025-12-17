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

### Changed

- Refactored `ServerState` from `Arc<RwLock<..>>` to `Arc<ServerState>` with internal `ArcSwap`
- Queries now clone `Arc<DbSnapshot>` at start, ensuring consistency even during swaps
- `ServerBuilder::build()` is now synchronous (no longer async)

### Technical Notes

- Uses RCU (Read-Copy-Update) semantics: old snapshots stay alive until last in-flight query completes
- Memory consideration: during swap, temporarily need 2x memory for the swapped lane
