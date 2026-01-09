# State Format Specification

This document specifies the `state.bin` format for PIR database generation.

See `docs/ARCHITECTURE.md` for the end-to-end pipeline that produces this file.

## Format Overview

```
+------------------+
| Header (64 bytes)|
+------------------+
| Entry 0 (84 B)   |
+------------------+
| Entry 1 (84 B)   |
+------------------+
| ...              |
+------------------+
| Entry N-1 (84 B) |
+------------------+
```

## Header (64 bytes)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | magic | `0x50495232` ("PIR2" in ASCII) |
| 4 | 2 | version | Format version (1) |
| 6 | 2 | entry_size | Bytes per entry (84) |
| 8 | 8 | entry_count | Number of entries |
| 16 | 8 | block_number | Snapshot block number |
| 24 | 8 | chain_id | Ethereum chain ID |
| 32 | 32 | block_hash | UBT root hash for verification (or block hash, zero if unknown) |

All integers are little-endian.

### Magic Number

The magic `0x50495232` ("PIR2") identifies this as an inspire state file. Future formats would use different magic bytes.

## Entry Format (84 bytes)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 20 | address | Contract address |
| 20 | 32 | tree_index | EIP-7864 tree index (stem_pos[31] \|\| subindex[1]) |
| 52 | 32 | value | Leaf value (storage value, basic_data, code_hash, or code chunk) |

**Note:** The `tree_index` field is NOT the raw storage slot. It is the EIP-7864 tree index computed from the leaf type and logical position. See "EIP-7864 Tree Embedding" below.

## EIP-7864 Tree Embedding

EIP-7864 co-locates account data to reduce unique stems. Each address has an "account stem" (stem_pos=0) that contains up to 256 leaves:

| Subindex | Content | Value Format |
|----------|---------|--------------|
| 0 | basic_data | See "basic_data Format" |
| 1 | code_hash | 32-byte keccak256 of bytecode |
| 2-63 | reserved | - |
| 64-127 | storage slots 0-63 | 32-byte storage value |
| 128-255 | code chunks 0-127 | See "Code Chunk Format" |

### Tree Index Computation

The tree_index is a 32-byte value: `stem_pos[31] || subindex[1]`

Where:
- `stem_pos` (31 bytes, big-endian): Determines which stem this leaf belongs to
- `subindex` (1 byte): Position within the stem's 256-entry subtree

#### Constants

```
BASIC_DATA_LEAF_KEY = 0
CODE_HASH_LEAF_KEY = 1
HEADER_STORAGE_OFFSET = 64
CODE_OFFSET = 128
STEM_SUBTREE_WIDTH = 256
MAIN_STORAGE_OFFSET = 256^31  (= 2^248)
```

#### For Storage Slots

```python
def compute_storage_tree_index(slot: int) -> bytes[32]:
    if slot < 64:
        # Small slots: account stem at subindex 64-127
        tree_index = bytes(31) + bytes([64 + slot])
    else:
        # Large slots: overflow stems
        pos = MAIN_STORAGE_OFFSET + slot
        stem_pos = pos // 256
        subindex = pos % 256
        tree_index = stem_pos.to_bytes(31, 'big') + bytes([subindex])
    return tree_index
```

#### For Code Chunks

```python
def compute_code_chunk_tree_index(chunk_id: int) -> bytes[32]:
    pos = CODE_OFFSET + chunk_id
    stem_pos = pos // 256
    subindex = pos % 256
    tree_index = stem_pos.to_bytes(31, 'big') + bytes([subindex])
    return tree_index
```

### basic_data Format (32 bytes)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 1 | version | Always 0 |
| 1 | 4 | reserved | All zeros |
| 5 | 3 | code_size | Big-endian bytecode length |
| 8 | 8 | nonce | Big-endian account nonce |
| 16 | 16 | balance | Big-endian account balance (wei) |

### Code Chunk Format (32 bytes)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 1 | leading_pushdata | Bytes of PUSH data from previous chunk (0-31) |
| 1 | 31 | code_bytes | Bytecode slice for this chunk |

Code is split into 31-byte chunks. The `leading_pushdata` field indicates how many bytes at the start of this chunk are push data from a PUSH instruction that started in a previous chunk.

## Tree Key Computation (UBT Stem Ordering)

Entries must be sorted by tree_key for PIR database generation:

```
stem = blake3(address32 || tree_index[:31])[:31]   // 31 bytes
tree_key = stem || tree_index[31]                  // 32 bytes
```

Where `address32` is the 20-byte address left-padded with 12 zero bytes.

### Benefits of EIP-7864 Ordering

| Metric | Raw Slot Ordering | EIP-7864 Ordering |
|--------|-------------------|-------------------|
| Unique stems | ~5.6M (for 6.4M entries) | ~30K-60K |
| Stem index size | ~208 MB | ~2 MB |
| Entries per stem | ~1.1 average | Up to 256 |

By co-locating account data, most queries hit the same stem, dramatically reducing the stem index size.

## Ordering Schemes

Two ordering schemes are supported:

### Bucket Index Ordering (Legacy)

Entries sorted by `keccak256(address || slot)` for bucket index compatibility.

The PIR database is laid out by bucket ID:
```
[bucket 0 entries][bucket 1 entries]...[bucket N entries]
```

See [bucket_index.rs](../crates/inspire-core/src/bucket_index.rs) for the 18-bit bucket ID computation.

Clients must download the 512 KB bucket index to compute PIR indices.

### UBT Stem Ordering (EIP-7864)

Entries sorted by tree_key for deterministic client-side index computation.

Database layout groups entries by stem:
```text
[stem 0, subindex 0][stem 0, subindex 1]...[stem 0, subindex 255]
[stem 1, subindex 0]...[stem N, subindex 255]
```

Benefits:
- No bucket index download required (clients compute indices locally)
- Small stem offset table (~2 MB) vs bucket index (512 KB) for large databases
- Compatible with EIP-7864 state proofs

See [ubt.rs](../crates/inspire-core/src/ubt.rs) for tree_index and stem computation.

## Example: Account with Storage and Code

Consider an account at `0x1234...5678` with:
- Nonce: 42
- Balance: 1 ETH (10^18 wei)
- Code: 100 bytes (4 chunks)
- Storage slots: 0, 1, 2, 100 (4 slots)

Entries in state.bin:

| tree_index (hex) | Subindex | Content |
|------------------|----------|---------|
| `00...00` | 0 | basic_data: {nonce=42, balance=10^18, code_size=100} |
| `00...01` | 1 | code_hash: keccak256(bytecode) |
| `00...40` | 64 | storage[0] value |
| `00...41` | 65 | storage[1] value |
| `00...42` | 66 | storage[2] value |
| `00...80` | 128 | code_chunk[0]: {leading=0, bytes=code[0:31]} |
| `00...81` | 129 | code_chunk[1]: {leading=?, bytes=code[31:62]} |
| `00...82` | 130 | code_chunk[2]: {leading=?, bytes=code[62:93]} |
| `00...83` | 131 | code_chunk[3]: {leading=?, bytes=code[93:100]++padding} |
| `01...00...64` | 100 | storage[100] value (overflow stem) |

All entries with `tree_index[:31] == 0` share the same stem (the account stem).

## Rust Types

```rust
/// State file header
#[repr(C, packed)]
pub struct StateHeader {
    pub magic: [u8; 4],        // b"PIR2"
    pub version: u16,          // 1
    pub entry_size: u16,       // 84
    pub entry_count: u64,
    pub block_number: u64,
    pub chain_id: u64,
    pub block_hash: [u8; 32],
}

/// Storage entry with EIP-7864 tree_index
#[repr(C, packed)]
pub struct StateEntry {
    pub address: [u8; 20],
    pub tree_index: [u8; 32],  // NOT raw slot - see EIP-7864 spec
    pub value: [u8; 32],
}

const STATE_MAGIC: [u8; 4] = *b"PIR2";
const STATE_HEADER_SIZE: usize = 64;
const STATE_ENTRY_SIZE: usize = 84;
```

## Example Binary Layout

A file with 1000 entries at block 20000000 on mainnet (chain_id=1):

```
Offset 0x00: 50 49 52 32  # "PIR2"
Offset 0x04: 01 00        # version = 1
Offset 0x06: 54 00        # entry_size = 84
Offset 0x08: e8 03 00 00 00 00 00 00  # entry_count = 1000
Offset 0x10: 00 2d 31 01 00 00 00 00  # block_number = 20000000
Offset 0x18: 01 00 00 00 00 00 00 00  # chain_id = 1
Offset 0x20: [32 bytes block hash or zeros]
Offset 0x40: [first entry starts here]
```

## References

- [EIP-7864](https://eips.ethereum.org/EIPS/eip-7864) - Unified Binary Trie specification
- [ETHREX_INTEGRATION.md](ETHREX_INTEGRATION.md) - ethrex export pipeline
- [ubt.rs](../crates/inspire-core/src/ubt.rs) - Tree index computation
- [inspire-exex#67](https://github.com/igor53627/inspire-exex/issues/67) - EIP-7864 restructuring
