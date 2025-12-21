# inspire-exex

Private Ethereum storage queries using InsPIRe PIR (Private Information Retrieval).

## Overview

Query Ethereum storage slots via single-server PIR so the server cannot determine which database index you requested. The server sees that a query occurred (timing, size, variant), but not which storage index it targets.

A public manifest maps `(contract, storage slot)` pairs to database indices; the PIR layer hides only the index, not which contracts exist in the database.

**Current deployment**: Sepolia testnet snapshot with ~79M `(address, slot)` entries (internal host `hsiao:3000`)

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
┌─────────────────────────────────────────────────────────────────────┐
│                         inspire-exex                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────────┐  │
│  │ inspire-exex │───>│ lane-builder │───>│ inspire-pir (setup)  │  │
│  │ (Reth ExEx)  │    │ (state-dump) │    │ (inspire-setup)      │  │
│  └──────────────┘    └──────────────┘    └──────────────────────┘  │
│        │                    │                      │                │
│        │ Periodic           │ state.bin            │ db.bin         │
│        │ state exports      │ (raw storage)        │ (PIR database) │
│        v                    v                      v                │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    inspire-server                             │  │
│  │                    (PIR query endpoint)                       │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                              │                                      │
│                              │ PIR queries                          │
│                              v                                      │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  inspire-client (native) / inspire-client-wasm (browser)      │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Components

### Core Pipeline

| Component | Description | Key Binaries |
|-----------|-------------|--------------|
| **inspire-exex** | Reth ExEx for Ethereum state tracking | `inspire-exex` |
| **lane-builder** | State extraction and preparation | `state-dump` |
| **inspire-pir** | Core PIR library (external: [inspire-rs](https://github.com/igor53627/inspire-rs)) | `inspire-setup`, `inspire-server`, `inspire-client` |

### Client Libraries

| Crate | Description |
|-------|-------------|
| `inspire-core` | Shared types (Config, Manifest, PIR params) |
| `inspire-server` | PIR server with hot-reload, metrics, admin API |
| `inspire-client` | Native Rust client with PIR query generation |
| `inspire-client-wasm` | Browser WASM client (keys remain in browser) |
| `burner-wallet` | Demo wallet UI with PIR + EIP-7702 |

## Data Flow

```
1. Extract State
   Reth node + inspire-exex  -->  state.bin (raw storage slots)
   
2. Encode PIR Database
   state.bin + inspire-setup  -->  db.bin (PIR-encoded database)
   
3. Serve Queries
   db.bin + inspire-server  -->  HTTP endpoint (port 3000)
   
4. Query Privately
   inspire-client  -->  PIR query  -->  server  -->  encrypted response
```

### Binary Usage

```bash
# 1. Dump state from Reth node
state-dump --datadir /path/to/reth --output state.bin

# 2. Encode PIR database
inspire-setup state.bin db.bin

# 3. Start server
inspire-server db.bin --port 3000

# 4. Query (look up index in manifest first)
inspire-client http://localhost:3000 --index 12345
```

### Index Mapping

The PIR index is a 0-based integer in `[0, N)` where `N` is the number of storage entries. A manifest file maps `(address, slot)` to indices:

```json
{
  "entries": [
    { "address": "0xA0b8...", "slot": "0x0", "index": 0 },
    { "address": "0xA0b8...", "slot": "0x1", "index": 1 }
  ]
}
```

Clients use this manifest to translate `(contract, slot)` to the `--index` parameter.

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

## Future Work

- **Two-lane optimization**: Split database into hot (popular contracts) and cold lanes for faster server response
- **Multi-lane extension**: Further lane splitting for different access patterns
- See [docs/HOT_CONTRACTS.md](docs/HOT_CONTRACTS.md) for hot lane design

## References

- [inspire-rs](https://github.com/igor53627/inspire-rs) - Core InsPIRe PIR implementation
- [InsPIRe Paper](https://eprint.iacr.org/2025/1352)

## License

MIT OR Apache-2.0
