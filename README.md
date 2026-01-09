# inspire-exex

Private Ethereum storage queries using InsPIRe PIR (Private Information Retrieval).

## Overview

Query Ethereum storage slots via single-server PIR so the server cannot determine which database index you requested. The server sees that a query occurred (timing, size, variant), but not which storage index it targets.

With UBT (Unified Binary Trie, EIP-7864), clients compute indices directly using deterministic `stem(31B) + subindex(1B)` addressing - no manifest download required.

**Current deployment**: Sepolia testnet via ethrex with UBT

## Snapshots

Pre-built PIR databases are available for download:

| Network | Block | Entries | Shards | PIR DB Size | Download |
|---------|-------|---------|--------|-------------|----------|
| Sepolia | 9,930,973 | 6.4M | 3,133 | 1.1 GB | [pir-db/](https://snapshots.53627.org/inspire/sepolia/pir-db/) |

Files in `pir-db/`:
- `metadata.json` - Database parameters (p=65537, d=2048)
- `crs.json` - Common reference string (80 MB)
- `bucket-index.bin` - Sparse lookup index (512 KB)
- `shards/` - Binary shards for mmap loading

Raw state dump: [state.bin](https://snapshots.53627.org/inspire/sepolia/state.bin) (514 MB)

## Performance

### Bandwidth

| Variant | Query | Response | Total | Privacy |
|---------|-------|----------|-------|---------|
| InsPIRe^0 (baseline) | 192 KB | 545 KB | 737 KB | Single-server PIR |
| InsPIRe^1 (OnePacking) | 192 KB | 32 KB | 224 KB | Single-server PIR |
| **InsPIRe^2 (Seeded+Packed)** | **96 KB** | **32 KB** | **128 KB** | **Single-server PIR** |
| InsPIRe^2+ (Switched+Packed) | 48 KB | 32 KB | 80 KB | Single-server PIR |

CRS download (one-time): 80 MB. Sizes for d=2048, 128-bit security.

### Production Benchmarks (Sepolia, 6.4M entries)

| Metric | Value | Notes |
|--------|-------|-------|
| Server latency | 2.5-3.3s | Per query, 3133 shards |
| Query generation | ~4 ms | Client-side |
| Extraction | ~0.5 ms | Client-side |
| Throughput | 2.5 qps | 10 concurrent queries |
| Memory (server) | 450-750 MB | Mmap mode |

Latency scales with database size. For smaller databases (~10K entries), expect ~10-50ms server latency.

## Architecture

```
+---------------------------------------------------------------------+
|                         inspire-exex                                |
+---------------------------------------------------------------------+
|                                                                     |
|  +-----------------+    +-------------------------+                 |
|  | reth            |--->| ubt-exex (ExEx plugin)  |                 |
|  | (chain sync)    |    |                         |                 |
|  +-----------------+    |  - NOMT (values)        |                 |
|                         |  - key-index.redb       |                 |
|                         |  - MDBX (deltas only)   |                 |
|                         +-----------+-------------+                 |
|                                     | ubt_exportState               |
|                                     v                               |
|                           state.bin + stem-index.bin                |
|                                     |                               |
|                                     v                               |
|  +--------------------------------------------------------------+   |
|  |                lane-builder (state-to-pir)                   |   |
|  |                Encode PIR DB + CRS + config                  |   |
|  +--------------------------------------------------------------+   |
|                              |                                      |
|                              v                                      |
|  +--------------------------------------------------------------+   |
|  |                    inspire-server                            |   |
|  |                    (PIR query endpoint)                      |   |
|  +--------------------------------------------------------------+   |
|                              |                                      |
|                              v                                      |
|  +--------------------------------------------------------------+   |
|  |  inspire-client / inspire-client-wasm                         |   |
|  |  Client computes index = stem_offset(stem) + subindex          |   |
|  +--------------------------------------------------------------+   |
|                                                                     |
+---------------------------------------------------------------------+

See `docs/ARCHITECTURE.md` for detailed data stores and flow.
```

## Components

### Core Pipeline

| Component | Description | Key Binaries |
|-----------|-------------|--------------|
| **ethrex** | Ethereum client with UBT support ([ethrex](https://github.com/igor53627/ethrex)) | `ethrex`, `ethrex-pir-export` |
| **inspire-pir** | Core PIR library ([inspire-rs](https://github.com/igor53627/inspire-rs)) | `inspire-setup`, `inspire-server`, `inspire-client` |

### Client Libraries

| Crate | Description |
|-------|-------------|
| `inspire-core` | Shared types (Config, PIR params, UBT helpers) |
| `inspire-server` | PIR server with hot-reload, metrics, admin API |
| `inspire-client` | Native Rust client with PIR query generation |
| `inspire-client-wasm` | Browser WASM client (keys remain in browser) |
| `burner-wallet` | Demo wallet UI with PIR + EIP-7702 |

## Data Flow

```
1. Maintain UBT state
   reth + ubt-exex  -->  NOMT (values) + key-index.redb (keys)

2. Export snapshot
   ubt_exportState  -->  state.bin + stem-index.bin

3. Encode PIR Database
   state.bin + state-to-pir  -->  PIR DB + CRS + config

4. Serve Queries
   PIR DB + inspire-server  -->  HTTP endpoint (port 3000)

5. Query Privately
   Client computes stem from (address, slot) using EIP-7864
   inspire-client  -->  PIR query  -->  server  -->  encrypted response
```

### Binary Usage

```bash
# 1. Export state from ubt-exex (NOMT + key-index)
cargo run -p inspire-updater --bin updater -- \
  --rpc-url http://localhost:8545 \
  --ubt-rpc-url /tmp/ubt-exex.ipc \
  --data-dir ./pir-data \
  --one-shot

# 2. Encode PIR database
cargo run -p lane-builder --bin state-to-pir -- \
  --input ./pir-data/state.bin \
  --output ./pir-data

# 3. Start server
cargo run -p inspire-server --bin main

# 4. Query (client computes index from stem + subindex)
cargo run -p inspire-client --bin main -- http://localhost:3000 --stem 0x... --subindex 0
```

### Index Computation (EIP-7864)

With EIP-7864 tree embedding, clients compute PIR indices directly:

```
# Step 1: Compute tree_index from leaf type
# For storage slot:
if slot < 64:
    tree_index = [0; 31] || (64 + slot)  # account stem, subindex 64-127
else:
    tree_index = MAIN_STORAGE_OFFSET + slot  # overflow stem

# Step 2: Compute stem and tree_key
stem = blake3(address32 || tree_index[:31])[:31]
subindex = tree_index[31]
tree_key = stem || subindex

# Step 3: Look up PIR index
index = stem_to_db_offset(stem) + subindex
```

EIP-7864 co-locates account data (headers, code chunks, first 64 storage slots) in a single "account stem" per address, reducing unique stems from ~5.6M to ~30K-60K.

See [EIP-7864](https://eips.ethereum.org/EIPS/eip-7864) and [STATE_FORMAT.md](docs/STATE_FORMAT.md) for details.

## Protocol Variants

```rust
use inspire_pir::pir::{query, query_seeded, query_switched};

// Basic query (192 KB upload)
let q = query(&crs, index, &config, &sk, &mut sampler);

// Seeded query (96 KB upload) - 50% smaller
let q = query_seeded(&crs, index, &config, &sk, &mut sampler);

// Switched query (48 KB upload) - 75% smaller
let q = query_switched(&crs, index, &config, &sk, &mut sampler);
```

## Privacy Model

| Property | Guarantee |
|----------|-----------|
| Query content | Encrypted (RLWE) |
| Target index | Computationally hidden |
| Which contract/slot | Hidden among all N entries |
| Query timing | Visible to server |

**Adversary model**: Single-server, honest-but-curious. Server follows protocol but tries to learn from queries.

**Limitations**: No side-channel protection (cache timing, etc). Application-level query patterns may leak information even though individual indices are hidden. This implementation has not been audited.

## Production Features

```bash
# Prometheus metrics
curl http://localhost:3000/metrics

# Health check (503 if not ready)
curl http://localhost:3000/health

# Hot-reload database (admin port, must be firewalled)
curl -X POST http://localhost:3001/admin/reload
```

## Build

```bash
cargo build --release

# Run tests
cargo test --workspace

# Benchmarks
cargo run --release --example benchmark_large
```

## References

- [inspire-rs](https://github.com/igor53627/inspire-rs) - Core InsPIRe PIR implementation
- [ethrex](https://github.com/igor53627/ethrex) - Ethereum client with UBT support
- [EIP-7864](https://eips.ethereum.org/EIPS/eip-7864) - Unified Binary Trie specification
- [InsPIRe Paper](https://eprint.iacr.org/2025/1352)

## License

MIT OR Apache-2.0
