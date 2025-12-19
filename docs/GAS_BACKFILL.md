# Gas Backfill: Data-Driven Hot Lane Selection

The `lane-backfill` tool analyzes historical gas usage to identify "gas guzzlers" - contracts that consume the most gas and should be prioritized in the hot lane.

## Overview

Instead of relying solely on a static list of known contracts, gas backfill provides **data-driven** hot lane selection by:

1. Scanning the last N blocks (default: 100,000)
2. Aggregating gas usage per contract address
3. Combining with curated contract lists using hybrid scoring
4. Outputting ranked contracts for hot lane inclusion

## Installation

Build with the `backfill` feature:

```bash
cargo build --release --features backfill --bin lane-backfill
```

## Usage

### Basic Backfill

```bash
lane-backfill \
    --rpc-url http://localhost:8545 \
    --blocks 100000 \
    --output gas-rankings.json
```

### Full Options

```bash
lane-backfill \
    --rpc-url http://localhost:8545 \    # Ethereum RPC (archive node recommended)
    --blocks 100000 \                     # Number of blocks to scan
    --batch-size 100 \                    # Blocks per batch
    --concurrency 10 \                    # Parallel RPC connections
    --top-n 1000 \                        # Max contracts for hot lane
    --known-boost 100000000000 \          # Priority boost for known contracts
    --output gas-rankings.json \          # Raw gas data output
    --scored-output hot-contracts.json    # Hybrid-scored output
```

### Building Hot Lane from Scored Contracts

After running backfill, use the scored output with `lane-builder`:

```bash
# Generate hot lane manifest from scored contracts
lane-builder ./pir-data/hot 21000000 --scored hot-contracts.json
```

## Hybrid Scoring

The hybrid scorer combines three signals:

### 1. Gas Score (Empirical)
Total gas consumed by the contract over the backfill period.

### 2. Priority Boost (Curated)
Contracts in the known list (`HOT_CONTRACTS`) receive a large boost to ensure inclusion regardless of recent gas usage.

### 3. Category Weight (Strategic)
Different contract categories receive multipliers based on PIR use-case priority:

| Category | Weight | Rationale |
|----------|--------|-----------|
| Privacy | 3.0x | Core PIR use case |
| Bridge | 2.0x | High-value, privacy-sensitive |
| DeFi | 1.5x | Common wallet queries |
| Lending | 1.5x | Position tracking |
| DEX | 1.5x | Swap activity |
| Stablecoin | 1.5x | Balance checks |
| Token | 1.0x | Standard priority |
| Governance | 1.0x | Standard priority |
| NFT | 0.8x | Lower priority for PIR |

### Scoring Formula

```
final_score = (gas_score + priority_boost) * category_weight
```

### Contract Sources

Each scored contract is tagged with its source:
- `GasBackfill` - Discovered from gas usage data
- `KnownList` - From curated contract list only
- `Both` - In both gas data and known list

## Example Output

```
Top 20 Gas Guzzlers (Hybrid Ranked)
------------------------------------
  1. Tornado Cash 0.1 ETH (privacy) - score: 300000000000, gas: 0, txs: 0 [known]
  2. Tornado Cash 1 ETH (privacy) - score: 300000000000, gas: 0, txs: 0 [known]
  3. Uniswap Universal Router (dex) - score: 245000000000, gas: 63333333333, txs: 1200000 [both]
  4. USDC (stablecoin) - score: 180000000000, gas: 20000000000, txs: 5000000 [both]
  5. 0x7a3e... (unknown) - score: 85000000000, gas: 85000000000, txs: 200000 [gas]
  ...
```

## Performance

For 100,000 blocks with default settings:
- **Time**: 15-30 minutes (depends on RPC latency)
- **RPC calls**: ~1,000 batched requests
- **Memory**: ~100 MB peak

### Optimization Tips

1. **Use an archive node** - Required for historical block data
2. **Increase concurrency** - `--concurrency 20` for fast nodes
3. **Reduce batch size** - `--batch-size 50` if hitting rate limits
4. **Use local node** - Lowest latency, no rate limits

## Integration with ExEx

For production deployments, combine backfill with real-time updates:

1. **Weekly backfill**: Run `lane-backfill` to update hot lane rankings
2. **Real-time reload**: ExEx triggers `/admin/reload` on new blocks
3. **Graceful migration**: Contracts moving hot<->cold handled by dual-lane queries

```
┌─────────────────┐     Weekly      ┌─────────────────┐
│  Archive Node   │ ───────────────>│  lane-backfill  │
└─────────────────┘                 └────────┬────────┘
                                             │
                                    hot-contracts.json
                                             │
                                             v
┌─────────────────┐   Real-time    ┌─────────────────┐
│   Reth ExEx     │ ──────────────>│   PIR Server    │
│  (lane-exex)    │   /reload      │                 │
└─────────────────┘                └─────────────────┘
```

## Output Files

### gas-rankings.json
Raw backfill data with per-contract gas statistics:

```json
{
  "start_block": 21000000,
  "end_block": 21100000,
  "blocks_processed": 100000,
  "total_transactions": 15000000,
  "unique_contracts": 250000,
  "gas_stats": [
    {
      "address": "0x7a250d5630b4cf539739df2c5dacb4c659f2488d",
      "total_gas": 85000000000,
      "tx_count": 1200000,
      "first_seen_block": 21000001,
      "last_seen_block": 21099999
    }
  ]
}
```

### hot-contracts.json
Hybrid-scored contracts ready for hot lane building:

```json
[
  {
    "address": "0x910cbd523d972eb0a6f4cae4618ad62622b39dbf",
    "name": "Tornado Cash 0.1 ETH",
    "category": "privacy",
    "gas_score": 0,
    "priority_boost": 100000000000,
    "category_weight": 3.0,
    "final_score": 300000000000,
    "tx_count": 0,
    "source": "KnownList"
  }
]
```

## Limitations

1. **Gas limit vs gas used**: Currently uses `gas_limit` from transactions, not actual `gas_used` from receipts (would require additional RPC calls)
2. **Contract creation**: Transactions creating contracts (no `to` address) are excluded
3. **Internal calls**: Only counts top-level transaction targets, not internal contract calls

## Future Improvements

- [ ] Use `eth_getBlockReceipts` for accurate gas_used
- [ ] Track internal calls via debug_traceBlock
- [ ] Incremental updates (append to existing rankings)
- [ ] Weighted time decay (recent blocks count more)
