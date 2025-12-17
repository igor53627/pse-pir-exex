# pse-pir-exex

Dummy Subsets PIR implementation with DHT-based hint distribution for private Ethereum state queries.

## Overview

This project implements the [Dummy Subsets PIR protocol](https://eprint.iacr.org/2023/1072) (CCS 2024) with optimizations for Ethereum state queries:

- **Query compression**: 150 KB -> 1-2 KB using PRF seed expansion
- **DHT hint storage**: Decentralized hint distribution via IPFS
- **Delta updates**: Efficient hint synchronization via XOR updates

## Architecture

```
+-----------------+     +------------------+     +------------------+
|    DHT/IPFS     |     |     Client       |     |   Query Server   |
|   (Server A)    |<--->|     Wallet       |<--->|   (Server B)     |
|   Hint Storage  |     |   214 MB hints   |     |   87 GB state    |
+-----------------+     +------------------+     +------------------+
        |                       |                        |
   No collusion            Full privacy            No collusion
```

## Performance

| Metric | Value |
|--------|-------|
| Database size | 87 GB (Ethereum state) |
| Client storage | 214 MB |
| Query size | 1-2 KB (compressed) |
| Response size | 32-64 bytes |
| Query time | 2.7-4.5 ms |

## Crates

| Crate | Description |
|-------|-------------|
| `pir-core` | Shared primitives (PRF, XOR, subsets) |
| `pir-seeder` | Hint generator (Reth ExEx integration) |
| `pir-server-b` | Query server (HTTP API) |
| `pir-client` | Client library and CLI |

## Quick Start

```bash
# Build all crates
cargo build --release

# Generate hints from database
./target/release/pir-seeder /path/to/database.bin --publish

# Start query server
./target/release/pir-server-b /path/to/database.bin --port 3000

# Query from client (after downloading hints)
./target/release/pir-client query --target 12345 --server http://localhost:3000
```

## Protocol Flow

### 1. Setup (One-time)

```
Seeder:
  1. Generate 6.7M hints from 87 GB Ethereum state
  2. Publish hints to IPFS
  3. Store Merkle root on-chain (optional)
```

### 2. Client Initialization

```
Client:
  1. Download manifest from IPFS/ENS
  2. Fetch 214 MB of hints from DHT
  3. Verify against Merkle root
  4. Store locally
```

### 3. Query

```
Client:
  1. Find hint containing target index
  2. Send PRF seed to query server (1-2 KB)
  
Server:
  3. Expand seed to subset indices
  4. XOR database entries at indices
  5. Return result (32 bytes)
  
Client:
  6. Recover: answer = response XOR stored_hint
```

### 4. Updates

```
Every block:
  1. State diff published
  2. Affected hints updated: new = old XOR old_value XOR new_value
  3. Delta published to IPFS
  4. Clients sync periodically
```

## Security

| Property | Guarantee |
|----------|-----------|
| Per-server privacy | Information-theoretic |
| Subset generation | Computational (PRF/AES) |
| Non-collusion | DHT decentralization |

### Hint Rotation (Multi-Query Privacy)

Each target index is covered by ~128 different hints. To prevent query correlation:

```
Query 1 for index 42: use hint_7   → subset [3, 17, 42, 89, ...]
Query 2 for index 42: use hint_193 → subset [42, 55, 61, 200, ...]
Query 3 for index 42: use hint_847 → subset [1, 42, 99, 150, ...]
```

Server sees **different subsets** each time - no correlation possible.

| Without Rotation | With Rotation |
|------------------|---------------|
| Same subset repeated | Random subset each query |
| Pattern analysis possible | Queries look independent |
| Single-query privacy only | **Multi-query privacy** |

## References

- [Dummy Subsets PIR Paper](https://eprint.iacr.org/2023/1072) (CCS 2024)
- [Ethereum State Size Analysis](../plinko-extractor/findings.md)
- [PIR Comparison](../inspire/docs/PIR_COMPARISON.md)

## License

MIT OR Apache-2.0
