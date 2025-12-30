# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- State export now uses EIP-7864 tree_index instead of raw storage slots (#71)
  - Renamed `StorageEntry.slot` to `StorageEntry.tree_index` in state_format.rs
  - Added `StorageEntry::from_storage_slot()` helper that computes proper tree_index
  - Updated `ShardWriter` to compute tree_key (stem || subindex) for sorting
  - Updated `gen_test_db` to generate EIP-7864 compliant test databases
  - Expected stem reduction: 7.5M -> ~60K stems (120x improvement)
  - Stem index size reduction: 285 MB -> ~2.3 MB

- Fixed cold lane PIR extraction failure caused by GaussianSampler state contamination (#65)
  - `TwoLaneSetup` now uses separate samplers for hot and cold lane setup
  - Previously, sharing a sampler between setups caused cryptographic failures in the second lane
  
- Fixed plaintext modulus from p=65536 to p=65537 (Fermat prime F4) in all params
  - Ensures `gcd(d, p) = 1` so `mod_inverse(d, p)` exists for tree packing unscaling
  - Required for OnePacking extraction to work correctly
  - Updated `default_params()`, `test_params()`, `PIR_PARAMS`, and all examples/docs

- Updated server E2E tests to use `extract_with_variant(..., InspireVariant::OnePacking)`
  - Server uses `respond_one_packing()`, so extraction must use matching variant

### Added

- Range-based delta sync for fresh PIR queries (#69)
  - New `range_delta` module in `inspire-core::bucket_index` with:
    - `RangeDeltaHeader`, `RangeEntry` - file format structures
    - `select_range(behind_blocks, ranges)` - pick optimal range to download
    - `merge_deltas(deltas)` - cumulative delta merging
    - `DEFAULT_RANGES: [1, 10, 100, 1000, 10000]` blocks
  - New `RangeDeltaWriter` in `inspire-updater` for maintaining delta files
    - Automatically trims deltas per range tier
    - Writes single `bucket-deltas.bin` file with all ranges
  - New server endpoints in `inspire-server`:
    - `GET /index/deltas` - range delta file (supports HTTP Range requests)
    - `GET /index/deltas/info` - metadata about available ranges
  - New `range_sync` module in `inspire-client::bucket_index`:
    - `parse_info(data)` - parse range delta header
    - `RangeDeltaInfo.get_fetch_range(client_block)` - compute byte range to fetch
  - New `range_delta_path` in `TwoLaneConfig`
  - Benefits: Fresh data per query with minimal bandwidth (4-400 KB vs 256 KB full index)

- EIP-7864 state restructuring for efficient stem indexing (#67)
  - Refactored `inspire-core::ubt` to use tree_index semantics per EIP-7864
  - New tree_index computation functions:
    - `compute_storage_tree_index(slot)` - slots 0-63 map to subindex 64-127, slots >= 64 use overflow stems
    - `compute_basic_data_tree_index()` - subindex 0 for account header
    - `compute_code_hash_tree_index()` - subindex 1 for code hash
    - `compute_code_chunk_tree_index(chunk_id)` - chunks 0-127 map to subindex 128-255
  - Helper functions: `pack_basic_data()`, `pack_code_chunk()`, `code_chunk_count()`
  - Updated `inspire-client-wasm::ubt_index` with new WASM bindings:
    - `computeStorageTreeIndex(slot)`, `computeBasicDataTreeIndex()`, etc.
    - `StemIndex.lookupStorage()`, `lookupBasicData()`, `lookupCodeHash()`, `lookupCodeChunk()`
  - Added `--ordering eip7864` mode to ethrex-pir-export for full tree embedding
  - Updated STATE_FORMAT.md with EIP-7864 tree_index specification
  - Expected stem reduction: ~30K-60K unique stems vs 5.6M (100x improvement)

- UBT stem-based indexing for WASM clients (#66)
  - Added `inspire-core::ubt` module with EIP-7864 stem computation using BLAKE3
  - Added `inspire-client-wasm::ubt_index` with WASM-bindgen exports:
    - `computeStem(address, treeIndex)` - compute 31-byte stem
    - `computeTreeKey(address, treeIndex)` - compute full 32-byte tree key  
    - `getSubindex(treeIndex)` - extract subindex from tree_index
    - `StemIndex` - lookup table for O(log N) index computation
  - Eliminates 512 KB bucket index download for stem-ordered databases
  - Updated README with correct BLAKE3 formula (was incorrectly showing Pedersen hash)

- New `inspire-updater` crate for syncing PIR database from ethrex node (#62)
  - `EthrexClient` with `pir_dumpStorage`, `pir_getStateDelta`, and `ubt_getRoot` support
  - `StateTracker` for tracking storage state changes
  - `ShardWriter` writes `state.bin` in PIR2 format (sorted by keccak256(address||slot))
  - `UpdaterService` with initial sync and efficient incremental updates
  - CLI binary: `cargo run -p inspire-updater --bin updater`
  - Tested with ethrex Sepolia node on hsiao

- WebSocket protocol improvements (#56)
  - Server now responds to Ping with Pong (fixes keepalive for clients)
  - Hello message on connect: `{"version":1,"block_number":12345}`
  - Lagged close now includes block number: `"lagged:12345"`

- Adversarial tests for bucket index security (#60)
  - `test_delta_huge_update_count_does_not_oom`: Validates OOM protection
  - `test_delta_truncated_updates`: Validates truncated payload rejection
  - `test_from_compressed_rejects_oversized`: Validates decompression bomb protection
  - `test_bucket_lookup_correctness`: Proper correctness verification for lookups

### Changed

- Improved API error handling and response consistency (#61)
  - `BucketDeltaError` now includes context: `HeaderTooShort { actual }`, `Truncated { expected, actual }`
  - Added `Decompression(String)` error variant for zstd errors (was misleading `Io`)
  - `DecompressionBomb` now includes payload size
  - Added `BucketIndexNotLoaded` server error (was confusing `LaneNotLoaded`)
  - Server errors now return structured JSON: `{"error": "...", "code": "BUCKET_INDEX_NOT_LOADED"}`

- Deduplicated bucket index logic between native and WASM clients (#59)
  - Shared `inspire_core::bucket_index` module with `compute_bucket_id`, `compute_cumulative`, `BucketDelta`
  - `inspire-client` re-exports shared types
  - `inspire-client-wasm` wraps shared logic with `#[wasm_bindgen]` annotations
  - Eliminates code drift risk between implementations

- Cache precompressed bucket index to avoid zstd level 19 on every request (#57)
  - `CachedBucketIndex` struct stores both parsed index and compressed bytes
  - `/index` endpoint now serves cached bytes instead of recompressing
  - Added `Cache-Control: public, max-age=60` headers to `/index` and `/index/raw`
  - `/index/info` now includes `compressed_size` field

### Security

- Fixed DoS vulnerabilities in bucket index parsing (#55)
  - `BucketDelta::from_bytes`: Validate `update_count` before allocating to prevent OOM via malicious delta frames
  - `BucketIndex::from_compressed`: Limit decompression size to prevent decompression bombs
  - WASM `apply_delta`: Same validation as native client

### Added

- Sparse Bucket Index for minimal client state (#53)
  - `BucketIndex` client library with load/decompress/lookup/delta support
  - 256K buckets (2^18), using first 18 bits of keccak256(address || slot)
  - O(1) bucket range lookup via cumulative sums (returns start_index + count, not exact index)
  - `BucketDelta` struct for streaming updates via websocket
  - **Note**: Requires DB to be ordered by bucket ID; exact within-bucket index requires additional structure
  - `bucket-index` binary in lane-builder crate for building index from state.bin
  - Server endpoints: `GET /index` (compressed ~150 KB), `GET /index/raw` (uncompressed 512 KB for WASM), `GET /index/info` (metadata)
  - WebSocket endpoint: `GET /index/subscribe` for delta broadcasts
  - `BucketBroadcast` channel for per-block delta streaming to clients
  - Client integration: `fetch_bucket_index()`, `lookup_bucket()`, `apply_bucket_delta()`
  - WASM client: `BucketIndex` class with `from_bytes()`, `lookup()`, `apply_delta()`
  - wallet-core: `BucketIndexWrapper` TypeScript class with `lookup()`, `applyDelta()`
  - Compressed with zstd level 19 (~150 KB for Sepolia scale)

- Hot Contracts List Documentation (#19)
  - `docs/HOT_CONTRACTS.md`: Comprehensive documentation of hot lane contract selection
  - `data/hot-contracts.json`: Machine-readable contract list with 43 initial contracts
  - Categories: stablecoins, DEX, lending, privacy, bridges, liquid staking, restaking, NFT
  - Category weights: Privacy 3x, Bridge 2x, DeFi 1.5x, Standard 1x
  - Scoring algorithm combining gas usage, tx count, TVL, and curated boosts
  - Data sources: Etherscan Gas Tracker, Dune Analytics, DeFiLlama
  - Weekly update process documentation
  - Versioning scheme for contract list updates

- Snapshot Freshness & Helios Verification (#42, #49)
  - `WalletCoreConfig` policy knobs for snapshot verification:
    - `minConfirmationsForSafety` (default: 64 blocks)
    - `maxSnapshotStalenessBlocks` (default: 900 blocks / ~3h)
    - `requireVerifiedSnapshot` (default: true for production)
  - Enhanced `VerificationResult` with rich status codes:
    - `hash_mismatch`, `snapshot_in_future`, `too_recent_reorg_risk`
    - `too_stale`, `not_finalized`, `chain_id_mismatch`, `helios_error`
  - Effect-based typed errors for all verification failure modes
  - Chain ID validation against expected network
  - Finality and staleness checks in `verifySnapshot()`
  - `getBalanceWithFallback()` API with `source: 'pir' | 'rpc'` tracking
  - `getBalanceEffect()` and `getBalanceWithFallbackEffect()` Effect APIs
  - Unit tests for verification error types and constants
  - Uses [Effect](https://effect.website/) for typed error handling

### Changed

- **inspire-server**: Use InspiRING packing (~35x faster) when packing keys available in query, otherwise tree packing
- **BREAKING**: `getBalance()` now throws `AddressNotFoundError` when address is not in PIR database (#49)
  - Previously returned zero balances, which was semantically incorrect
  - Use `getBalanceWithFallback()` for the "always returns a result" behavior

- Production readiness features (#38)
  - **PIR Parameter Versioning** (#39): Version checks prevent client/server crypto mismatches
  - **Admin Network Isolation** (#40): Admin endpoints on separate 127.0.0.1 listener
  - **WASM Client Security** (#41): SecureSecretKey with zeroize, WebCrypto CSPRNG check
  - **Prometheus Observability** (#43): `/metrics`, `/health`, `/live` endpoints
  - **Testing Infrastructure** (#44): E2E tests, load testing, reload safety tests

- Testing infrastructure for E2E and load testing (#44)
  - `TestHarness` helper for spinning up test servers with PIR databases
  - Happy path tests: hot/cold lane queries, CRS fetching, server info
  - Error handling tests: invalid lane (400), invalid JSON (4xx)
  - Snapshot consistency tests: verify atomic reads during reloads
  - Concurrent reload safety tests: queries continue during database swaps
  - Load test binary (`loadtest`): configurable clients, queries, with-reloads mode
  - Heavy tests marked `#[ignore]` for nightly/manual runs

- Admin endpoint network isolation and rate limiting (#40)
  - `ServerBuilder::admin_port(port)` to run admin endpoints on separate localhost listener
  - Admin endpoints (`/admin/reload`, `/admin/health`) bound to 127.0.0.1 only
  - Rate limiting: 1 request/second on admin endpoints (returns 429 on excess)
  - Public router excludes admin routes when admin_port is configured
  - CLI: `inspire-server config.json 3000 3001` runs public on :3000, admin on 127.0.0.1:3001
  - Backwards compatible: omit admin_port for combined single-listener mode

- WASM client security hardening (#41)
  - `SecureSecretKey` wrapper with `Drop` impl that zeroizes RlweSecretKey memory
  - Fail-fast check for WebCrypto CSPRNG availability on init
  - `CryptoUnavailable` error variant for clear error messages
  - Uses `zeroize` crate for secure memory clearing

- Prometheus metrics and observability (#43)
  - PIR request metrics: `pir_requests_total`, `pir_request_duration_seconds` (by lane/outcome)
  - Lane status metrics: `pir_lane_loaded`, `pir_lane_block_number`, `pir_lane_mmap_mode`
  - Reload metrics: `pir_reload_total`, `pir_reload_duration_seconds`
  - `/metrics` endpoint for Prometheus scraping
  - `/health` returns 503 when lanes not ready (proper readiness semantics)
  - `/live` liveness endpoint (always 200 if server alive)
  - Privacy-safe: no query content in metrics labels

- PIR parameter versioning for client/server compatibility (#39)
  - `PIR_PARAMS_VERSION` constant (v2) in `inspire-core`
  - `PirParams` struct with all RLWE parameters
  - `CrsMetadata` sidecar files (`crs.meta.json`) generated by `lane-builder`
  - Server startup validation against CRS metadata version
  - `/info` endpoint now includes `pir_params_version` field
  - WASM client checks version on init, fails fast on mismatch

- `burner-wallet` crate: Axum+Askama server-rendered wallet UI (#35)
  - Full Rust/WASM stack (no React/Next.js)
  - Generate/import burner wallet with localStorage persistence
  - EIP-7702 authorization signing for EOA delegation
  - EIP-7702 transaction sending via `sign_eip7702_tx()` WASM function
  - Send Transaction UI with gas estimation, authorization inclusion
  - PIR balance queries via inspire-client-wasm integration
    - "Connect PIR" button to initialize PIR client
    - Query balances privately without revealing address
    - Falls back to RPC if address not in hot lane
  - Server View panel demonstrating PIR privacy (encrypted vs cleartext)
  - Helios light client integration for snapshot verification
  - Tenderly Virtual TestNet integration (Sepolia fork)
  - Fund test accounts with 10 ETH + 1000 USDC via admin RPC
  - Real-time ETH/USDC balance display (RPC or PIR modes)
  - Playwright E2E tests (48 tests covering full flow)
  - Minimal CSS, no Tailwind
  - Target: Sepolia testnet (EIP-7702 live after Pectra)

- Moved `alloy-wasm` from experiments/ to crates/ (#35)

- `alloy-wasm` experimental package for browser-based EIP-7702 signing (#35)
  - Compiles alloy-rs to WASM (wasm32-unknown-unknown target)
  - ~207 KB WASM bundle (89 KB gzipped)
  - `generate_wallet()` - create random burner wallet
  - `get_address(privateKey)` - derive address from private key
  - `sign_authorization(privateKey, authRequest)` - sign EIP-7702 authorization
  - `sign_eip7702_tx(privateKey, txRequest)` - build and sign Type 4 transaction
  - `sign_message(privateKey, message)` - EIP-191 personal sign
  - `sign_typed_data_hash(privateKey, hash)` - raw hash signing
  - `encode_balance_of(address)` / `encode_transfer(to, amount)` - ERC20 ABI encoding
  - `format_units(value, decimals)` / `parse_units(value, decimals)` - unit conversion
  - Browser test page at `experiments/alloy-wasm/test.html`

- `inspire-client-wasm` crate for browser-based PIR queries (#33)
  - WASM-compatible PIR client using browser fetch API
  - ~247 KB WASM bundle size (optimized with wasm-opt)
  - Exports `PirClient` with `init(lane)`, `query()`, `query_binary()` methods
  - Dynamic lane routing (uses lane specified at init time)
  - Uses gloo-net for HTTP requests
  - TypeScript definitions included
  - Browser smoke test HTML at `examples/index.html`

- ETH/USDC balance hot lane support (#33 Phase 2)
  - `BalanceRecord` struct (64 bytes: 32 ETH + 32 USDC)
  - `BalanceDbMetadata` for tracking snapshot block, chain, addresses
  - `balance-builder` binary for extracting balances at snapshot block
  - `BalanceExtractor` with concurrent RPC fetching
  - Network presets: mainnet, holesky, sepolia
  - Builds binary DB + metadata.json from address list

- `@inspire/wallet-core` TypeScript package (#33 Phase 3+4)
  - Integrates PIR client (inspire-client-wasm) with Helios light client
  - `WalletCore` class: unified API for private verified balance queries
  - `HeliosVerifier`: verify snapshot block hash against canonical chain
  - `PirBalanceClient`: WASM PIR client wrapper for balance queries
  - `getBalance(address)` / `getBalances(addresses)` APIs
  - `verifySnapshot()` for Helios-based block hash verification
  - Uses `@a16z/helios` npm package (no vendoring needed)
  - Browser demo at `web/wallet-core/examples/index.html`

- Seed expansion support for ~50% smaller queries

### Fixed

- Fixed axum 0.7 route path parameter syntax (`/:lane` instead of `/{lane}`)

### Changed

- Renamed repository and workspace from `pse-inspire-exex` to `inspire-exex`

- Updated sigma parameter from 3.2 to 6.4 to match InsPIRe paper security parameters
  - Affects `default_params()` and `test_params()` in lane-builder
  - **Breaking**: Existing CRS/DB files must be regenerated with new sigma value

- Refactored inspire-pir to support WASM builds
  - Added `server` and `cli` feature flags
  - Server-only deps (axum, tokio, memmap2, reqwest) are now optional
  - Library can be built with `--no-default-features` for WASM
  - `/query/{lane}/seeded` endpoint for seeded queries
  - `SeededQueryRequest` type on server
  - `ClientBuilder::seed_expansion(bool)` to enable/disable
  - Seed expansion enabled by default

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
