# inspire-exex

Two-Lane InsPIRe PIR for private Ethereum state queries.

## Problem

Wallets need to query Ethereum state (balances, positions) privately. Current approaches:

| Approach | Query Size | Response Size | Total | Privacy |
|----------|------------|---------------|-------|---------|
| Clearnet RPC | 0.1 KB | 0.1 KB | 0.2 KB | None |
| InsPIRe^0 (baseline) | 192 KB | 545 KB | 737 KB | Full |
| InsPIRe^1 (OnePacking) | 192 KB | 32 KB | 224 KB | Full |
| InsPIRe^2 (Seeded+Packed) | 96 KB | 32 KB | 128 KB | Full |
| **InsPIRe^2+ (Switched+Packed)** | **48 KB** | **32 KB** | **80 KB** | **Full** |

The InspiRING 2-matrix packing algorithm provides up to **9.2x bandwidth reduction** vs baseline.

## Solution: Two-Lane InsPIRe

Split the database into lanes by popularity:

```
HOT LANE:  Top 1,000 contracts -> 1M entries  -> Fast server response
COLD LANE: Everything else    -> 2.7B entries -> Slower server response
```

Query size depends on variant (48-192 KB), but the hot lane has faster
server-side processing due to smaller database polynomial evaluation.

## Key Benefits (InspiRING 2-Matrix Packing)

- **226x faster** online packing (InspiRING vs tree packing)
- **16,000x smaller** CRS key material (64 bytes seeds vs 1056 KB)
- Only 2 key-switching matrices vs log(d)=11 matrices

## Architecture

```
HOT LANE  (~32 MB, 1M entries)
  - Top 1,000 contracts by query frequency
  - USDC, WETH, USDT, DAI, Uniswap, Aave, ...
  - Query size: 48-192 KB (depending on variant)
  - Faster server response (smaller DB)

COLD LANE (~87 GB, 2.7B entries)
  - All other contracts and accounts
  - Query size: 48-192 KB (depending on variant)
  - Same query size, slower server processing
```

Note: InsPIRe communication is O(d) where d=ring dimension (2048), **not** O(√N).
Query size is the same regardless of database size. The benefit of the hot lane
is faster server-side computation, not smaller queries.

## Performance (Benchmarked)

Measured on AMD/Intel x64 server with d=2048, 128-bit security:

### Communication by Variant

| Variant | Query (upload) | Response (download) | Total |
|---------|----------------|---------------------|-------|
| InsPIRe^0 (baseline) | 192 KB | 545 KB | 737 KB |
| InsPIRe^1 (OnePacking) | 192 KB | 32 KB | 224 KB |
| InsPIRe^2 (Seeded+Packed) | 96 KB | 32 KB | 128 KB |
| **InsPIRe^2+ (Switched+Packed)** | **48 KB** | **32 KB** | **80 KB** |

### Server Response Time

| Database Size | Shards | Respond Time |
|---------------|--------|--------------|
| 256K entries (8 MB) | 128 | 3.8 ms |
| 512K entries (16 MB) | 256 | 3.1 ms |
| 1M entries (32 MB) | 512 | 3.3 ms |

### End-to-End Latency

| Phase | Time |
|-------|------|
| Client: Query gen (switched) | ~4 ms |
| Server: Expand + Respond | ~3-4 ms |
| Client: Extract | ~5 ms |
| **Total** | **~12 ms** |

### Wallet Open (14 queries)

| Approach | Upload | Download | Total | Privacy |
|----------|--------|----------|-------|---------|
| Clearnet RPC | 2 KB | 2 KB | 4 KB | **None** |
| InsPIRe^0 (baseline) | 2.7 MB | 7.6 MB | 10.3 MB | Full |
| InsPIRe^1 (OnePacking) | 2.7 MB | 0.4 MB | 3.1 MB | Full |
| InsPIRe^2 (Seeded+Packed) | 1.3 MB | 0.4 MB | 1.8 MB | Full |
| **InsPIRe^2+ (Switched+Packed)** | **0.7 MB** | **0.4 MB** | **1.1 MB** | **Full** |

Run benchmarks: `cargo run --release --example benchmark_large`

## Privacy & Threat Model

### Adversary Model

- **Server model**: Single-server, honest-but-curious
  - The server follows the protocol but tries to learn as much as possible from queries
- **Security goal**: Protect *query index confidentiality* within each lane under standard RLWE assumptions
- **Non-goals**:
  - Hiding which user (IP/account) is querying
  - Hiding that a user is using this service
  - Protecting integrity/availability of responses
  - Defending against side-channel attacks

### Privacy Guarantees

| Property | Guarantee | Notes |
|----------|-----------|-------|
| Query content | Encrypted (RLWE) | Computationally secure |
| Target index within lane | Computationally hidden | PIR property |
| Within-lane popularity signal | **Hidden** | Server cannot distinguish which contract/slot |
| **Which lane queried** | **Visible to server** | Deliberate trade-off |
| Cross-query metadata | **Not addressed** | Lane, timing, IP can be correlated |

### What the Server Learns

The two-lane architecture leaks **which lane** is being accessed:

| Information | Server Knowledge |
|-------------|------------------|
| Query lane (hot/cold) | **YES** - endpoint path reveals lane |
| Target contract | NO - encrypted by PIR |
| Target storage slot | NO - encrypted by PIR |
| Target index within lane | NO - PIR property |
| Query timing | YES - observable |

**Example**: If you query the hot lane, the server learns your target is among the ~1000 popular contracts, but not which one or which slot (~1-in-1M slot-level anonymity).

### Lane Privacy Trade-off

This is a **deliberate trade-off** for faster server response times:

```
Privacy "cost" (per query):  Server learns hot vs cold (~1 bit of information)
Latency gain:                ~10x faster response for hot lane queries
```

This is acceptable because:
1. Knowing someone queries "a popular DeFi contract" reveals little
2. The target contract and slot remain hidden (~1-in-1M anonymity set)
3. Multi-query correlation **at the index level** is still impossible (ciphertexts are semantically secure)

### Maximum Privacy Mode

For applications requiring maximum privacy, query **both lanes every time**:
- One lane receives the real query
- Other lane receives a decoy query (random index)

This hides which lane contains your actual target, at the cost of 2x queries (~160 KB with InsPIRe^2+).

### Public Information

The following information is **intentionally public**:
- Hot lane manifest (list of ~1000 contracts, their categories)
- Lane CRS (cryptographic reference strings)
- Lane entry counts

## Hot Lane Contract Selection

Updated weekly based on on-chain analytics using **hybrid scoring**:

### Data Sources

1. **Gas Backfill** - Scan last 100k blocks to find "gas guzzlers"
2. **Curated List** - Known DeFi protocols, privacy tools, bridges
3. **Category Weights** - Privacy protocols boosted 3x, bridges 2x

```bash
# Run gas backfill
cargo run --bin lane-backfill --features backfill -- \
    --rpc-url http://localhost:8545 \
    --blocks 100000

# Build hot lane from scored contracts
lane-builder ./pir-data/hot 21000000 --scored hot-contracts.json
```

See [docs/GAS_BACKFILL.md](docs/GAS_BACKFILL.md) for details.

### Categories

| Category | Weight | Example Contracts |
|----------|--------|-------------------|
| Privacy | 3.0x | Tornado Cash, Railgun |
| Bridges | 2.0x | Arbitrum, Optimism, Polygon |
| Stablecoins | 1.5x | USDC, USDT, DAI, FRAX, LUSD |
| Wrapped assets | 1.0x | WETH, WBTC, stETH, rETH |
| DEX | 1.5x | Uniswap V2/V3, Curve, Balancer |
| Lending | 1.5x | Aave V2/V3, Compound, Maker |

## Live Database Updates

The server supports hot-reloading databases without restart:

- **Lock-free reads** via ArcSwap - zero query blocking during updates
- **Mmap mode** (default) - O(1) swap time (~1-5ms) regardless of database size
- **Atomic swaps** - in-flight queries continue on old snapshot until complete

```bash
# Trigger reload
curl -X POST http://localhost:3000/admin/reload
```

See [docs/LIVE_UPDATES.md](docs/LIVE_UPDATES.md) for details.

## Crates

| Crate | Description |
|-------|-------------|
| `inspire-core` | Shared types (Lane, Config, Manifest) |
| `inspire-server` | Two-lane PIR server with live updates |
| `inspire-client` | Client with lane routing |
| `lane-builder` | Hot lane extractor (Reth ExEx) |

## Integration with inspire-rs

This project extends [inspire-rs](https://github.com/igor53627/inspire-rs) with:
1. Two-lane database splitting
2. Lane routing logic
3. Hot lane manifest management
4. Reth ExEx for lane building

### Protocol Variant Selection

```rust
// Client query generation
use inspire_pir::pir::{query, query_seeded, query_switched};

// Basic query (192 KB)
let q = query(&crs, index, &config, &sk, &mut sampler);

// Seeded query (96 KB) - 50% smaller
let q = query_seeded(&crs, index, &config, &sk, &mut sampler);

// Switched query (48 KB) - 75% smaller
let q = query_switched(&crs, index, &config, &sk, &mut sampler);
```

```rust
// Server response with InspiRING 2-matrix packing
use inspire_pir::pir::{respond_with_variant, respond_inspiring};
use inspire_pir::InspireVariant;

// OnePacking variant (32 KB response)
let resp = respond_with_variant(&crs, db, query, InspireVariant::OnePacking);

// InspiRING 2-matrix variant (32 KB response, fastest packing)
let resp = respond_inspiring(&crs, db, query);
```

```rust
// Client extraction
use inspire_pir::pir::extract_with_variant;

let data = extract_with_variant(&crs, &state, &response, 32, InspireVariant::OnePacking);
```

## Open Questions

1. **Lane update frequency**: How often to rebuild hot lane list?
2. **Lane boundary**: Is 1000 contracts the right cutoff?
3. **Cross-lane queries**: Handle contracts that move between lanes?
4. **Multi-lane extension**: Would 3+ lanes help further?

## References

- [inspire-rs](https://github.com/igor53627/inspire-rs) - Base InsPIRe implementation with InspiRING 2-matrix packing
- [InsPIRe Paper](https://eprint.iacr.org/2024/XXX)
- [Google InsPIRe Reference](https://github.com/google/private-membership/tree/main/research/InsPIRe)

## License

MIT OR Apache-2.0
