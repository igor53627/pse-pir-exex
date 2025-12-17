# pse-pir-exex

Two-Lane InsPIRe PIR for private Ethereum state queries.

## Problem

Wallets need to query Ethereum state (balances, positions) privately. Current approaches:

| Approach | Query Size | Privacy |
|----------|-----------|---------|
| Clearnet RPC | 0.1 KB | None |
| InsPIRe | 500 KB | Full |
| **Two-Lane InsPIRe** | **~60 KB avg** | **Full** |

## Solution: Two-Lane InsPIRe

Split the database into lanes by popularity:

```
HOT LANE:  Top 1,000 contracts -> 1M entries  -> 10 KB queries
COLD LANE: Everything else    -> 2.7B entries -> 500 KB queries
```

Since 90% of queries target popular contracts, average bandwidth drops from 500 KB to ~60 KB.

## Architecture

```
HOT LANE  (~32 MB, 1M entries)
  - Top 1,000 contracts by query frequency
  - USDC, WETH, USDT, DAI, Uniswap, Aave, ...
  - Query size: ~10 KB
  - O(sqrt(1M)) = O(1000) communication

COLD LANE (~87 GB, 2.7B entries)
  - All other contracts and accounts
  - Query size: ~500 KB
  - O(sqrt(2.7B)) = O(52000) communication
```

## Performance

| Scenario | Hot Lane | Cold Lane | Total |
|----------|----------|-----------|-------|
| Query USDC balance | 10 KB | - | 10 KB |
| Query obscure NFT | - | 500 KB | 500 KB |
| Wallet open (14 queries, 90% hot) | 126 KB | 50 KB | 176 KB |

### Comparison

| Approach | 14 Wallet Queries | Privacy |
|----------|-------------------|---------|
| Clearnet RPC | 2 KB | **None** |
| InsPIRe | 7 MB | Full |
| **Two-Lane InsPIRe** | **176 KB** | **Full** |

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

This is a **deliberate trade-off** to reduce average query size from 500 KB to ~60 KB:

```
Privacy "cost" (per query):  Server learns hot vs cold (~1 bit of information)
Bandwidth gain:              90% reduction in average query size (500KB -> 60KB)
```

This is acceptable because:
1. Knowing someone queries "a popular DeFi contract" reveals little
2. The target contract and slot remain hidden (~1-in-1M anonymity set)
3. Multi-query correlation **at the index level** is still impossible (ciphertexts are semantically secure)

### Maximum Privacy Mode

For applications requiring maximum privacy, query **both lanes every time**:
- One lane receives the real query
- Other lane receives a decoy query (random index)

This hides which lane contains your actual target, at the cost of ~510 KB per query.

### Public Information

The following information is **intentionally public**:
- Hot lane manifest (list of ~1000 contracts, their categories)
- Lane CRS (cryptographic reference strings)
- Lane entry counts

## Hot Lane Contract Selection

Updated weekly based on on-chain analytics:

| Category | Example Contracts |
|----------|-------------------|
| Stablecoins | USDC, USDT, DAI, FRAX, LUSD |
| Wrapped assets | WETH, WBTC, stETH, rETH |
| DEX | Uniswap V2/V3, Curve, Balancer, SushiSwap |
| Lending | Aave V2/V3, Compound, Maker, Spark |
| Bridges | Across, Stargate, Hop, Synapse |
| L2 | Arbitrum, Optimism, Base bridges |
| Restaking | EigenLayer, Renzo, EtherFi |

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

This project extends [inspire-rs](../inspire/) with:
1. Two-lane database splitting
2. Lane routing logic
3. Hot lane manifest management
4. Reth ExEx for lane building

## Open Questions

1. **Lane update frequency**: How often to rebuild hot lane list?
2. **Lane boundary**: Is 1000 contracts the right cutoff?
3. **Cross-lane queries**: Handle contracts that move between lanes?
4. **Multi-lane extension**: Would 3+ lanes help further?

## References

- [inspire-rs](../inspire/) - Base InsPIRe implementation
- [InsPIRe Paper](https://eprint.iacr.org/2024/XXX)
- [Ethereum State Analysis](../plinko-extractor/findings.md)

## License

MIT OR Apache-2.0
