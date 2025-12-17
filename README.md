# pse-pir-exex

Two-Lane InsPIRe PIR for private Ethereum state queries.

## Problem

Wallets need to query Ethereum state (balances, positions) privately. Existing approaches have tradeoffs:

| Approach | Query Size | Privacy | Issues |
|----------|-----------|---------|--------|
| Direct RPC | 0.1 KB | None | Leaks all queries |
| Single-lane InsPIRe | 500 KB | Full | High bandwidth |
| Dummy Subsets | 2 KB | Weak | Intersection attacks, popularity attacks |

## Solution: Two-Lane InsPIRe

Split the database into lanes by popularity:

```
HOT LANE:  Top 1,000 contracts -> 1M entries  -> 10 KB queries
COLD LANE: Everything else    -> 2.7B entries -> 500 KB queries
```

Since 90% of queries target popular contracts, average bandwidth drops from 500 KB to ~60 KB.

## Why Not Dummy Subsets?

We initially explored Dummy Subsets PIR but found critical issues:

| Attack | Description | Mitigation Complexity |
|--------|-------------|----------------------|
| **Intersection** | Server intersects query subsets, reveals target | Requires 10x dummy queries |
| **Popularity** | Server knows 90% queries are popular contracts | Requires popularity-weighted dummies |
| **Correlation** | Wallet queries are correlated (USDC + WETH) | Requires correlated dummy bundles |

InsPIRe is immune to all these attacks because queries are encrypted.

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
| Direct RPC | 2 KB | **None** |
| Single InsPIRe | 7 MB | Full |
| Dummy Subsets + 10x dummies | 308 KB | Partial |
| **Two-Lane InsPIRe** | **176 KB** | **Full** |

## Privacy Analysis

| Property | Guarantee |
|----------|-----------|
| Query content | Encrypted (RLWE) |
| Target index | Computationally hidden |
| Multi-query correlation | **Immune** |
| Popularity analysis | **Immune** |
| Intersection attack | **Immune** |

### Lane Privacy

Which lane you query reveals popularity tier:
- Hot lane -> target is in top 1000 contracts
- Cold lane -> target is elsewhere

This is acceptable: knowing someone queries "a popular DeFi contract" doesn't reveal which one (1-in-1M).

For maximum privacy, query both lanes every time (one real, one dummy).

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
| Privacy | Tornado Cash, Railgun, Privacy Pools, YOLO |

## Crates

| Crate | Description |
|-------|-------------|
| `inspire-core` | Shared types (Lane, Config, Manifest) |
| `inspire-server` | Two-lane PIR server |
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
- [PIR Comparison](../inspire/docs/PIR_COMPARISON.md)

## License

MIT OR Apache-2.0
