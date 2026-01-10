#!/usr/bin/env python3
"""
Extract USDC storage slots from Sepolia and build state.bin for PIR demo.

Usage:
    python3 extract-usdc-state.py --rpc-url https://sepolia.drpc.org --output usdc-state.bin
"""

import argparse
import hashlib
import json
import struct
import sys
from typing import List, Tuple

import requests

# Sepolia USDC contract
USDC_CONTRACT = "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238"
USDC_ADDRESS_BYTES = bytes.fromhex(USDC_CONTRACT[2:])
BALANCE_SLOT = 9  # balances mapping slot


def keccak256(data: bytes) -> bytes:
    """Compute Keccak-256 hash (for storage slot computation)."""
    from Crypto.Hash import keccak
    k = keccak.new(digest_bits=256)
    k.update(data)
    return k.digest()


def compute_balance_slot(wallet: bytes) -> bytes:
    """Compute storage slot for balances[wallet] mapping."""
    # abi.encode: address left-padded to 32 bytes + slot as uint256
    encoded = bytes(12) + wallet + bytes(28) + struct.pack(">I", BALANCE_SLOT)
    return keccak256(encoded)


def get_storage_at(rpc_url: str, contract: str, slot: str, block: str = "latest") -> str:
    """Fetch storage value at slot."""
    resp = requests.post(rpc_url, json={
        "jsonrpc": "2.0",
        "method": "eth_getStorageAt",
        "params": [contract, slot, block],
        "id": 1
    }, timeout=30)
    result = resp.json()
    if "error" in result:
        raise Exception(f"RPC error: {result['error']}")
    return result["result"]


def get_block_number(rpc_url: str) -> int:
    """Get latest block number."""
    resp = requests.post(rpc_url, json={
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    }, timeout=10)
    return int(resp.json()["result"], 16)


def fetch_usdc_balances(rpc_url: str, wallets: List[str]) -> List[Tuple[bytes, bytes, bytes]]:
    """Fetch USDC balances for wallets, return (wallet, slot, value) tuples."""
    entries = []
    for wallet_hex in wallets:
        wallet = bytes.fromhex(wallet_hex[2:] if wallet_hex.startswith("0x") else wallet_hex)
        slot = compute_balance_slot(wallet)
        slot_hex = "0x" + slot.hex()
        
        try:
            value_hex = get_storage_at(rpc_url, USDC_CONTRACT, slot_hex)
            value = bytes.fromhex(value_hex[2:].zfill(64))
            
            # Only include non-zero balances
            if value != bytes(32):
                balance = int.from_bytes(value, "big")
                print(f"  {wallet_hex[:10]}... balance: {balance / 1e6:.2f} USDC")
                entries.append((wallet, slot, value))
        except Exception as e:
            print(f"  {wallet_hex[:10]}... error: {e}")
    
    return entries


def compute_eip7864_tree_index(slot: bytes) -> bytes:
    """Compute EIP-7864 tree_index for a storage slot."""
    slot_value = int.from_bytes(slot, "big")
    MAIN_STORAGE_OFFSET = 256
    HEADER_STORAGE_OFFSET = 64
    
    if slot_value < HEADER_STORAGE_OFFSET:
        # Account stem
        stem_pos = 0
        subindex = HEADER_STORAGE_OFFSET + slot_value
    else:
        # Overflow stem
        stem_pos = MAIN_STORAGE_OFFSET + (slot_value // 256)
        subindex = slot_value % 256
    
    # tree_index = stem_pos[31 bytes] || subindex[1 byte]
    tree_index = stem_pos.to_bytes(31, "big") + bytes([subindex])
    return tree_index


def compute_stem(address: bytes, tree_index: bytes) -> bytes:
    """Compute 31-byte stem from address and tree_index."""
    # address32 = 12 zero bytes + 20-byte address
    address32 = bytes(12) + address
    # stem_pos = tree_index[:31]
    stem_pos = tree_index[:31]
    
    # stem = blake3(address32 || stem_pos)[:31]
    import hashlib
    try:
        h = hashlib.blake3(address32 + stem_pos)
        return h.digest()[:31]
    except AttributeError:
        # Python < 3.11 doesn't have blake3 in hashlib
        import blake3
        return blake3.blake3(address32 + stem_pos).digest()[:31]


def build_state_bin(entries: List[Tuple[bytes, bytes, bytes]], output_path: str):
    """
    Build state.bin file in PIR2 format.
    
    Format per entry (84 bytes):
    - tree_key[32]: stem[31] || subindex[1]
    - storage_slot[32]: original slot key
    - value[32]: storage value
    
    With header:
    - magic[4]: "PIR2"
    - version[2]: 0x0001
    - entry_size[2]: 84
    - num_entries[8]: count
    - reserved[16]: zeros
    """
    # Sort entries by stem for binary search
    indexed_entries = []
    for wallet, slot, value in entries:
        tree_index = compute_eip7864_tree_index(slot)
        stem = compute_stem(USDC_ADDRESS_BYTES, tree_index)
        subindex = tree_index[31]
        tree_key = stem + bytes([subindex])
        indexed_entries.append((tree_key, slot, value))
    
    # Sort by tree_key
    indexed_entries.sort(key=lambda x: x[0])
    
    # Write file
    with open(output_path, "wb") as f:
        # Header (32 bytes)
        f.write(b"PIR2")                           # magic
        f.write(struct.pack("<H", 1))              # version
        f.write(struct.pack("<H", 84))             # entry_size
        f.write(struct.pack("<Q", len(indexed_entries)))  # num_entries
        f.write(bytes(16))                          # reserved
        
        # Entries
        for tree_key, slot, value in indexed_entries:
            f.write(tree_key)  # 32 bytes
            f.write(slot)      # 32 bytes (original storage slot)
            f.write(value)     # 32 bytes
    
    print(f"\nWrote {len(indexed_entries)} entries to {output_path}")


def build_stem_index(entries: List[Tuple[bytes, bytes, bytes]], output_path: str):
    """Build stem index for O(log N) lookup."""
    # Group entries by stem
    stem_to_offset = {}
    for i, (wallet, slot, value) in enumerate(entries):
        tree_index = compute_eip7864_tree_index(slot)
        stem = compute_stem(USDC_ADDRESS_BYTES, tree_index)
        if stem not in stem_to_offset:
            stem_to_offset[stem] = i
    
    # Sort stems
    sorted_stems = sorted(stem_to_offset.keys())
    
    # Write index
    with open(output_path, "wb") as f:
        f.write(struct.pack("<Q", len(sorted_stems)))
        for stem in sorted_stems:
            f.write(stem)  # 31 bytes
            f.write(struct.pack("<Q", stem_to_offset[stem]))  # 8 bytes
    
    print(f"Wrote {len(sorted_stems)} stems to {output_path}")


def main():
    parser = argparse.ArgumentParser(description="Extract USDC storage for PIR demo")
    parser.add_argument("--rpc-url", default="https://sepolia.drpc.org", help="Sepolia RPC URL")
    parser.add_argument("--output", default="usdc-state.bin", help="Output state.bin path")
    parser.add_argument("--wallets-file", help="File with wallet addresses (one per line)")
    args = parser.parse_args()
    
    # Default test wallets (known to have USDC on Sepolia)
    default_wallets = [
        "0x5B38Da6a701c568545dCfcB03FcB875f56beddC4",  # Remix default
        "0x0000000000000000000000000000000000000001",  # Burn address (123 USDC)
        "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",  # vitalik.eth
        "0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B",  # Vitalik 2
        # Add more known USDC holders on Sepolia
    ]
    
    # Load wallets from file if provided
    if args.wallets_file:
        with open(args.wallets_file) as f:
            wallets = [line.strip() for line in f if line.strip() and not line.startswith("#")]
    else:
        wallets = default_wallets
    
    print(f"Fetching USDC balances from {args.rpc_url}")
    print(f"Contract: {USDC_CONTRACT}")
    print(f"Checking {len(wallets)} wallets...")
    
    block = get_block_number(args.rpc_url)
    print(f"Block: {block}")
    
    entries = fetch_usdc_balances(args.rpc_url, wallets)
    
    if not entries:
        print("\nNo non-zero balances found!")
        sys.exit(1)
    
    print(f"\nFound {len(entries)} wallets with USDC balance")
    
    build_state_bin(entries, args.output)
    
    stem_index_path = args.output.replace(".bin", "-stems.bin")
    build_stem_index(entries, stem_index_path)


if __name__ == "__main__":
    main()
