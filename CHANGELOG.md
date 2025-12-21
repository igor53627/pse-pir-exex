# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Admin endpoint network isolation and rate limiting (#40)
  - `ServerBuilder::admin_port(port)` to run admin endpoints on separate localhost listener
  - Admin endpoints (`/admin/reload`, `/admin/health`) bound to 127.0.0.1 only
  - Rate limiting: 1 request/second on admin endpoints (returns 429 on excess)
  - Public router excludes admin routes when admin_port is configured
  - CLI: `inspire-server config.json 3000 3001` runs public on :3000, admin on 127.0.0.1:3001
  - Backwards compatible: omit admin_port for combined single-listener mode

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
