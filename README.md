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

| Variant | Query | Response | Total | Privacy |
|---------|-------|----------|-------|---------|
| InsPIRe^0 (baseline) | 192 KB | 545 KB | 737 KB | Single-server PIR |
| InsPIRe^1 (OnePacking) | 192 KB | 32 KB | 224 KB | Single-server PIR |
| InsPIRe^2 (Seeded+Packed) | 96 KB | 32 KB | 128 KB | Single-server PIR |
| **InsPIRe^2+ (Switched+Packed)** | **48 KB** | **32 KB** | **80 KB** | **Single-server PIR** |

Sizes shown are for default parameters (d=2048, 128-bit security). End-to-end latency (local benchmark): ~12 ms (query gen ~4ms + server ~4ms + extract ~5ms). Network latency not included.

## Architecture

```
+---------------------------------------------------------------------+
|                         inspire-exex                                |
+---------------------------------------------------------------------+
|                                                                     |
|  +-----------------+    +----------------------+                    |
|  | ethrex          |--->| ethrex-pir-export    |                    |
|  | (UBT sync)      |    | (iterate PLAIN_STORAGE)                   |
|  +-----------------+    +----------------------+                    |
|        |                         |                                  |
|        | UBT state               | state.bin (stem-ordered)         |
|        v                         v                                  |
|  +--------------------------------------------------------------+   |
|  |                    inspire-setup                             |   |
|  |                    (encode PIR database)                     |   |
|  +--------------------------------------------------------------+   |
|                              |                                      |
|                              | db.bin (PIR database)                |
|                              v                                      |
|  +--------------------------------------------------------------+   |
|  |                    inspire-server                            |   |
|  |                    (PIR query endpoint)                      |   |
|  +--------------------------------------------------------------+   |
|                              |                                      |
|                              | PIR queries                          |
|                              v                                      |
|  +--------------------------------------------------------------+   |
|  |  inspire-client (native) / inspire-client-wasm (browser)     |   |
|  |  Client computes index = stem_to_db_offset(stem) + subindex  |   |
|  +--------------------------------------------------------------+   |
|                                                                     |
+---------------------------------------------------------------------+
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
1. Export State from ethrex
   ethrex (UBT sync)  -->  ethrex-pir-export  -->  state.bin (stem-ordered)
   
2. Encode PIR Database
   state.bin + inspire-setup  -->  db.bin (PIR-encoded database)
   
3. Serve Queries
   db.bin + inspire-server  -->  HTTP endpoint (port 3000)
   
4. Query Privately
   Client computes stem from (address, slot) using EIP-7864
   inspire-client  -->  PIR query  -->  server  -->  encrypted response
```

### Binary Usage

```bash
# 1. Export state from ethrex
ethrex-pir-export --datadir /path/to/ethrex --output state.bin

# 2. Encode PIR database
inspire-setup state.bin db.bin

# 3. Start server
inspire-server db.bin --port 3000

# 4. Query (client computes index from stem + subindex)
inspire-client http://localhost:3000 --stem 0x... --subindex 0
```

### Index Computation (UBT)

With UBT, clients compute the PIR index directly using EIP-7864 stem derivation:

```
stem = pedersen_hash(address || slot[:31])  # 31 bytes
subindex = slot[31]                          # 1 byte (0-255)
index = stem_to_db_offset(stem) + subindex
```

No manifest download required - the stem algorithm is deterministic.

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
