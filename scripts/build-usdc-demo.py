#!/usr/bin/env python3
"""
Build USDC demo database with proper PIR2 format and stem index.
Creates state.bin (64-byte header + 84-byte entries) and stem-index.bin for PIR.

This script fetches real USDC balances from Sepolia and builds a database
that can be used with the PIR demo server.

Usage:
    pip install requests pycryptodome blake3
    python3 scripts/build-usdc-demo.py

Output:
    /mnt/sepolia/usdc-demo/
        state.bin           - PIR2 format state file
        stem-index.bin      - Stem index for O(log N) lookup
        wallet-mapping.json - Maps wallet addresses to PIR indices
"""

import json
import struct
import sys
import os

try:
    import requests
except ImportError:
    print("pip install requests")
    sys.exit(1)

try:
    from Crypto.Hash import keccak
    def keccak256(data: bytes) -> bytes:
        k = keccak.new(digest_bits=256)
        k.update(data)
        return k.digest()
except ImportError:
    print("pip install pycryptodome")
    sys.exit(1)

try:
    import blake3 as blake3_lib
    def blake3_hash(data: bytes) -> bytes:
        return blake3_lib.blake3(data).digest()
except ImportError:
    try:
        import hashlib
        def blake3_hash(data: bytes) -> bytes:
            return hashlib.blake3(data).digest()
    except AttributeError:
        print("pip install blake3 (for Python < 3.11)")
        sys.exit(1)

# Sepolia USDC
USDC_CONTRACT = "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238"
USDC_ADDRESS = bytes.fromhex(USDC_CONTRACT[2:])
BALANCE_SLOT = 9
DECIMALS = 6
CHAIN_ID = 11155111  # Sepolia

RPC_URL = "https://sepolia.drpc.org"

STATE_HEADER_SIZE = 64
STATE_ENTRY_SIZE = 84

def compute_balance_slot(wallet: bytes) -> bytes:
    """Compute keccak256(abi.encode(wallet, BALANCE_SLOT))."""
    encoded = bytes(12) + wallet + bytes(28) + struct.pack(">I", BALANCE_SLOT)
    return keccak256(encoded)

def add_with_offset(slot: bytes, offset: bytes) -> bytes:
    """Add offset to slot, return 32-byte result (big-endian)."""
    result = bytearray(32)
    carry = 0
    for i in range(31, -1, -1):
        s = slot[i] + offset[i] + carry
        result[i] = s & 0xff
        carry = s >> 8
    return bytes(result)

# MAIN_STORAGE_OFFSET_BYTES = 256^31 = 0x01 followed by 31 zeros (big-endian)
MAIN_STORAGE_OFFSET_BYTES = bytes([1]) + bytes(31)

def compute_storage_tree_index(slot: bytes) -> bytes:
    """Compute EIP-7864 tree_index for storage slot (matches Rust impl)."""
    # Check if slot < 64 (fits in account stem)
    is_small = all(b == 0 for b in slot[:31]) and slot[31] < 64
    
    if is_small:
        # Small slot: place in account stem at subindex 64 + slot
        return bytes(31) + bytes([64 + slot[31]])
    else:
        # Large slot: add MAIN_STORAGE_OFFSET to slot
        return add_with_offset(slot, MAIN_STORAGE_OFFSET_BYTES)

def compute_stem(address: bytes, tree_index: bytes) -> bytes:
    """Compute 31-byte stem from address and tree_index per EIP-7864."""
    address32 = bytes(12) + address
    stem_pos = tree_index[:31]
    return blake3_hash(address32 + stem_pos)[:31]

def get_storage_at(contract: str, slot: str) -> str:
    resp = requests.post(RPC_URL, json={
        "jsonrpc": "2.0",
        "method": "eth_getStorageAt", 
        "params": [contract, slot, "latest"],
        "id": 1
    }, timeout=30)
    result = resp.json()
    if "error" in result:
        raise Exception(f"RPC error: {result['error']}")
    return result["result"]

def get_block_number() -> int:
    resp = requests.post(RPC_URL, json={
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    }, timeout=10)
    return int(resp.json()["result"], 16)

# Known Sepolia USDC holders
KNOWN_WALLETS = [
    "0xf08A50178dfcDe18524640EA6618a1f965821715",
    "0x5B38Da6a701c568545dCfcB03FcB875f56beddC4",
    "0xAb8483F64d9C6d1EcF9b849Ae677dD3315835cb2",
    "0x4B20993Bc481177ec7E8f571ceCaE8A9e22C02db",
    "0x78731D3Ca6b7E34aC0F824c42a7cC18A495cabaB",
    "0x617F2E2fD72FD9D5503197092aC168c91465E7f2",
    "0x17F6AD8Ef982297579C203069C1DbfFE4348c372",
    "0x467d543e5e4e41aeddf3b6d1997350dd9820a173",
    "0x0000000000000000000000000000000000000001",
    "0x0000000000000000000000000000000000000002",
    "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",  # vitalik.eth
]

def write_state_header(f, entry_count, block_number, chain_id, block_hash):
    """Write 64-byte PIR2 header."""
    buf = bytearray(STATE_HEADER_SIZE)
    buf[0:4] = b"PIR2"
    buf[4:6] = struct.pack("<H", 1)
    buf[6:8] = struct.pack("<H", STATE_ENTRY_SIZE)
    buf[8:16] = struct.pack("<Q", entry_count)
    buf[16:24] = struct.pack("<Q", block_number)
    buf[24:32] = struct.pack("<Q", chain_id)
    buf[32:64] = block_hash
    f.write(buf)

def write_storage_entry(f, address, tree_index, value):
    """Write 84-byte entry: address[20] + tree_index[32] + value[32]."""
    f.write(address)      # 20 bytes
    f.write(tree_index)   # 32 bytes
    f.write(value)        # 32 bytes

def main():
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", default="/mnt/sepolia/usdc-demo",
                        help="Output directory")
    parser.add_argument("--rpc-url", default=RPC_URL,
                        help="Sepolia RPC URL")
    args = parser.parse_args()
    
    global RPC_URL
    RPC_URL = args.rpc_url
    output_dir = args.output
    os.makedirs(output_dir, exist_ok=True)
    
    print(f"Checking {len(KNOWN_WALLETS)} wallets on Sepolia...")
    block = get_block_number()
    print(f"Block: {block}")
    
    entries = []  # (stem, subindex, tree_index, value, wallet_hex)
    
    for wallet_hex in KNOWN_WALLETS:
        wallet = bytes.fromhex(wallet_hex[2:] if wallet_hex.startswith("0x") else wallet_hex)
        storage_slot = compute_balance_slot(wallet)
        slot_hex = "0x" + storage_slot.hex()
        
        try:
            value_hex = get_storage_at(USDC_CONTRACT, slot_hex)
            value = bytes.fromhex(value_hex[2:].zfill(64))
            
            if value != bytes(32):
                balance = int.from_bytes(value, "big")
                balance_fmt = balance / (10 ** DECIMALS)
                
                tree_index = compute_storage_tree_index(storage_slot)
                stem = compute_stem(USDC_ADDRESS, tree_index)
                subindex = tree_index[31]
                
                print(f"  [{len(entries)}] {wallet_hex} = {balance_fmt:,.2f} USDC (subindex={subindex})")
                entries.append((stem, subindex, tree_index, value, wallet_hex.lower()))
        except Exception as e:
            print(f"  {wallet_hex[:10]}... error: {e}")
    
    if not entries:
        print("\nNo non-zero balances found!")
        sys.exit(1)
    
    # Sort by stem for binary search
    entries.sort(key=lambda x: x[0])
    
    # Build wallet mapping after sorting
    wallet_mapping = {}
    for idx, (_, _, _, _, wallet) in enumerate(entries):
        wallet_mapping[wallet] = idx
    
    # Write state.bin
    state_path = f"{output_dir}/state.bin"
    block_hash = bytes(32)
    
    with open(state_path, "wb") as f:
        write_state_header(f, len(entries), block, CHAIN_ID, block_hash)
        
        for stem, subindex, tree_index, value, _ in entries:
            write_storage_entry(f, USDC_ADDRESS, tree_index, value)
    
    file_size = os.path.getsize(state_path)
    print(f"\nWrote {len(entries)} entries to {state_path} ({file_size} bytes)")
    
    # Write stem-index.bin (direct index, not offset+subindex)
    stem_index_path = f"{output_dir}/stem-index.bin"
    
    with open(stem_index_path, "wb") as f:
        f.write(struct.pack("<Q", len(entries)))
        for idx, (stem, subindex, _, _, _) in enumerate(entries):
            f.write(stem)  # 31 bytes
            f.write(struct.pack("<Q", idx))  # 8 bytes - direct PIR index
    
    print(f"Wrote {len(entries)} stems to {stem_index_path}")
    
    # Write wallet-mapping.json
    mapping_path = f"{output_dir}/wallet-mapping.json"
    with open(mapping_path, "w") as f:
        json.dump({
            "block": block,
            "usdc_contract": USDC_CONTRACT,
            "balance_slot": BALANCE_SLOT,
            "entries": len(entries),
            "wallets": wallet_mapping
        }, f, indent=2)
    
    print(f"Wrote wallet mapping to {mapping_path}")
    print(f"\nNext steps:")
    print(f"  1. state-to-pir --input {state_path} --output {output_dir}/pir-data")
    print(f"  2. cp {stem_index_path} {output_dir}/pir-data/")
    print(f"  3. inspire-server {output_dir}/pir-data/config.json")

if __name__ == "__main__":
    main()
