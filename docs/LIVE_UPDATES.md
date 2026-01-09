# Live Database Updates

The server supports hot-reloading lane databases without restart or query interruption.

See `docs/ARCHITECTURE.md` for the broader pipeline context.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      ServerState                            │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              ArcSwap<DbSnapshot>                     │   │
│  │  ┌─────────────────────────────────────────────┐    │   │
│  │  │  DbSnapshot (immutable)                      │    │   │
│  │  │  - hot_lane: Option<LaneData>               │    │   │
│  │  │  - cold_lane: Option<LaneData>              │    │   │
│  │  │  - router: Option<LaneRouter>               │    │   │
│  │  │  - block_number: Option<u64>                │    │   │
│  │  └─────────────────────────────────────────────┘    │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## How It Works

### Lock-Free Reads (ArcSwap)

Queries use atomic snapshot loading with zero lock contention:

```rust
async fn handle_query(state: SharedState, query: Query) -> Response {
    let snapshot = state.load_snapshot_full();  // atomic, O(1)
    snapshot.process_query(lane, &query)        // uses cloned Arc
}
```

### Atomic Updates

Updates build a new snapshot off to the side, then atomically swap:

```rust
fn reload(&self) -> Result<ReloadResult> {
    let new_snapshot = Arc::new(DbSnapshot { ... });
    self.snapshot.store(new_snapshot);  // atomic swap
    // Old snapshot freed when last in-flight query drops its Arc
}
```

### Query Consistency

If a query starts before a swap and completes after:
- Query clones `Arc<old_snapshot>` at start
- Continues using that snapshot even after swap
- **Guaranteed consistent view** of one block

## Storage Modes

### Mmap Mode (Default)

Memory-maps binary shard files for O(1) swap time:

| Operation | Time |
|-----------|------|
| `mmap()` syscall | ~1ms |
| ArcSwap pointer swap | ~1μs |
| **Total** | **~1-5ms** |

Enable with config (default):
```rust
let config = TwoLaneConfig::from_base_dir("./pir-data");
// use_mmap: true by default
```

Requires binary shard files in `{lane}/shards/` directory.

### In-Memory Mode

Loads entire database into RAM from JSON:

| Lane | Load Time |
|------|-----------|
| Hot (~32 MB) | ~100ms |
| Cold (~87 GB) | ~60s |

Enable with:
```rust
let config = TwoLaneConfig::from_base_dir("./pir-data")
    .with_mmap(false);
```

## Admin Endpoint

Trigger reload via HTTP:

```bash
curl -X POST http://localhost:3000/admin/reload
```

Response:
```json
{
  "old_block_number": 19000000,
  "new_block_number": 19000001,
  "reload_duration_ms": 3,
  "hot_loaded": true,
  "cold_loaded": true,
  "mmap_mode": true
}
```

## Memory Considerations

During swap, temporarily need 2x memory for the swapped lane:

| Lane | Normal | During Swap |
|------|--------|-------------|
| Hot | ~32 MB | ~64 MB |
| Cold | ~87 GB | ~174 GB |

With mmap mode, this is virtual memory (not resident) until pages are accessed.

## Configuration

```rust
TwoLaneConfig {
    // Paths
    hot_lane_db: PathBuf,
    cold_lane_db: PathBuf,
    hot_lane_crs: PathBuf,
    cold_lane_crs: PathBuf,
    hot_lane_manifest: PathBuf,
    
    // Mmap settings
    use_mmap: bool,                    // default: true
    hot_lane_shards: Option<PathBuf>,  // default: {base}/hot/shards
    cold_lane_shards: Option<PathBuf>, // default: {base}/cold/shards
    shard_size_bytes: u64,             // default: 128KB
    
    // Validation
    hot_entries: u64,
    cold_entries: u64,
    entry_size: usize,
}
```

## Integration with Reth ExEx

Future: The lane-builder ExEx will call `/admin/reload` automatically when new blocks arrive:

```
Reth ExEx --> lane-builder --> write new shards --> POST /admin/reload
```

See issue #17 for ExEx integration status.
