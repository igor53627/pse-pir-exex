# AGENTS.md - inspire-exex

## Project Overview

Two-Lane InsPIRe PIR for private Ethereum state queries.

**Key Insight**: 90% of wallet queries target top 1000 contracts. Split database into hot/cold lanes to reduce average query size from 500 KB to ~60 KB while maintaining full privacy.

## Project Structure

```
inspire-exex/
├── crates/
│   ├── inspire-core/     # Shared types (Lane, Config)
│   ├── inspire-server/   # Two-lane PIR server
│   ├── inspire-client/   # Client with lane routing
│   └── lane-builder/     # Hot lane extractor (ExEx)
├── README.md
├── AGENTS.md
└── docs/
    ├── PROTOCOL.md       # Protocol specification
    └── HOT_CONTRACTS.md  # Hot lane contract list
```

## Dependencies

- `inspire-rs` (../inspire/) - Base InsPIRe PIR implementation
- `reth` - For ExEx integration (lane building)

## Build Commands

```bash
# Build all crates
cargo build --release

# Run tests
cargo test --workspace

# Build lane extractor
cargo build -p lane-builder --release
```

## Key Parameters

| Parameter | Hot Lane | Cold Lane |
|-----------|----------|-----------|
| Contracts | ~1,000 | ~2.7M |
| Entries | ~1M | ~2.7B |
| DB Size | ~32 MB | ~87 GB |
| Query Size | ~10 KB | ~500 KB |
| sqrt(N) | ~1,000 | ~52,000 |

## Comparison

| Approach | 14 Wallet Queries | Privacy |
|----------|-------------------|---------|
| Clearnet RPC | 2 KB | None |
| InsPIRe | 7 MB | Full |
| **Two-Lane InsPIRe** | **176 KB** | **Full** |

## Integration Points

1. **inspire-rs**: Use existing PIR primitives
2. **plinko-extractor**: Use for hot lane contract identification
3. **Reth ExEx**: Real-time hot lane updates

## Related Projects

| Project | Path | Purpose |
|---------|------|---------|
| inspire-rs | ~/pse/inspire | Base PIR implementation |
| plinko-extractor | ~/pse/plinko-extractor | State extraction |
| pse-client | ~/pse/pse-client | Base client |

## Open Tasks

See GitHub issues for current work items.

## Privacy Notes

- Hot vs cold lane choice leaks popularity tier (acceptable)
- Query content fully encrypted (RLWE)
- For max privacy: query both lanes (one real, one decoy)
