# Architecture

This document summarizes the current end-to-end architecture and data flow for
the UBT → PIR pipeline.

## Overview

The system keeps Ethereum state in **UBT (EIP-7864)** order and serves PIR
queries over a single-server InsPIRe database. Clients compute a deterministic
index from `(address, storage slot)` and do not need a manifest download.

### Data stores (ubt-exex)

- **NOMT**: authoritative values (per 32-byte `tree_index`)
- **key-index.redb**: authoritative key enumeration and metadata
  - `stem -> {address, bitmap(256)}` for key enumeration
  - `head` metadata: block number, block hash, root, stem count
- **MDBX**: retained for deltas/reorg history (not used for snapshot export)

The export path uses **NOMT for values** and **key-index.redb for enumeration**.
MDBX is not required to build `state.bin`.

## End-to-end flow

```
reth
  └── ubt-exex (ExEx)
       ├── NOMT (values)
       ├── key-index.redb (keys + head metadata)
       └── MDBX (deltas only)

ubt_exportState (RPC)
  └── state.bin + stem-index.bin
        └── state-to-pir
             └── PIR DB + CRS + config
                  └── inspire-server
                       └── inspire-client / wasm
```

## Key index details

The key index stores one record per stem:

```
key:   stem (31 bytes)
value: address (20 bytes) || bitmap (32 bytes)
```

`bitmap` marks which subindices (0–255) are present in that stem. This is
enough to enumerate the full UBT keyspace without MDBX.

The `head` metadata is stored in the same redb file and is used for
`ubt_getRoot` and snapshot metadata.

## Snapshot export

`ubt_exportState` writes:

- `state.bin` — PIR2 format records (address, tree_index, value) in UBT order
- `stem-index.bin` — stem → first offset mapping

