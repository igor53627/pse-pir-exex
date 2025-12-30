//! Tests for STATE_FORMAT parsing and generation
//!
//! These tests verify the state.bin format defined in docs/STATE_FORMAT.md

use inspire_core::state_format::{
    StateFormatError, StateHeader, StorageEntry, STATE_ENTRY_SIZE, STATE_HEADER_SIZE, STATE_MAGIC,
};
use inspire_core::ubt::compute_tree_key;

#[test]
fn test_header_magic() {
    assert_eq!(STATE_MAGIC, *b"PIR2");
    assert_eq!(STATE_HEADER_SIZE, 64);
    assert_eq!(STATE_ENTRY_SIZE, 84);
}

#[test]
fn test_header_roundtrip() {
    let block_hash = [0xab; 32];
    let header = StateHeader::new(12345, 20_000_000, 1, block_hash);

    let bytes = header.to_bytes();
    assert_eq!(bytes.len(), STATE_HEADER_SIZE);

    // Check magic bytes
    assert_eq!(&bytes[0..4], b"PIR2");

    let recovered = StateHeader::from_bytes(&bytes).unwrap();
    assert_eq!(recovered.magic, STATE_MAGIC);
    assert_eq!(recovered.version, 1);
    assert_eq!(recovered.entry_size, 84);
    assert_eq!(recovered.entry_count, 12345);
    assert_eq!(recovered.block_number, 20_000_000);
    assert_eq!(recovered.chain_id, 1);
    assert_eq!(recovered.block_hash, block_hash);
}

#[test]
fn test_entry_roundtrip() {
    let address = [0x42; 20];
    let tree_index = [0x01; 32];
    let value = [0xff; 32];

    let entry = StorageEntry::new(address, tree_index, value);
    let bytes = entry.to_bytes();

    assert_eq!(bytes.len(), STATE_ENTRY_SIZE);

    let recovered = StorageEntry::from_bytes(&bytes).unwrap();
    assert_eq!(recovered.address, address);
    assert_eq!(recovered.tree_index, tree_index);
    assert_eq!(recovered.value, value);
}

#[test]
fn test_from_storage_slot() {
    let address = [0x42; 20];
    let slot = [0u8; 32]; // Slot 0
    let value = [0xff; 32];

    let entry = StorageEntry::from_storage_slot(address, slot, value);

    // Slot 0 should map to tree_index with subindex 64 (HEADER_STORAGE_OFFSET)
    assert_eq!(entry.tree_index[..31], [0u8; 31]);
    assert_eq!(entry.tree_index[31], 64);
}

#[test]
fn test_full_file_format() {
    // Simulate a complete state.bin file
    let entry_count = 3u64;
    let block_number = 20_000_000u64;
    let chain_id = 1u64;
    let block_hash = [0xde; 32];

    let header = StateHeader::new(entry_count, block_number, chain_id, block_hash);

    // Create entries with proper tree_index (using from_storage_slot)
    let entries = vec![
        StorageEntry::from_storage_slot([0x11; 20], [0x01; 32], [0xaa; 32]),
        StorageEntry::from_storage_slot([0x22; 20], [0x02; 32], [0xbb; 32]),
        StorageEntry::from_storage_slot([0x33; 20], [0x03; 32], [0xcc; 32]),
    ];

    // Build file bytes
    let mut file_bytes = Vec::with_capacity(STATE_HEADER_SIZE + entries.len() * STATE_ENTRY_SIZE);
    file_bytes.extend_from_slice(&header.to_bytes());
    for entry in &entries {
        file_bytes.extend_from_slice(&entry.to_bytes());
    }

    let expected_size = STATE_HEADER_SIZE + (entry_count as usize * STATE_ENTRY_SIZE);
    assert_eq!(file_bytes.len(), expected_size);

    // Parse back
    let recovered_header = StateHeader::from_bytes(&file_bytes).unwrap();
    assert_eq!(recovered_header.entry_count, entry_count);

    // Parse entries
    for (i, entry) in entries.iter().enumerate() {
        let offset = STATE_HEADER_SIZE + i * STATE_ENTRY_SIZE;
        let recovered_entry = StorageEntry::from_bytes(&file_bytes[offset..]).unwrap();
        assert_eq!(recovered_entry, *entry);
    }
}

#[test]
fn test_invalid_magic_rejected() {
    let mut bytes = [0u8; STATE_HEADER_SIZE];
    bytes[0..4].copy_from_slice(b"XXXX");

    let result = StateHeader::from_bytes(&bytes);
    assert!(matches!(result, Err(StateFormatError::InvalidMagic { .. })));
}

#[test]
fn test_truncated_header_rejected() {
    let bytes = [0u8; 32]; // Too short

    let result = StateHeader::from_bytes(&bytes);
    assert!(matches!(
        result,
        Err(StateFormatError::HeaderTooShort { actual: 32 })
    ));
}

#[test]
fn test_entries_sorted_by_tree_key() {
    // Generate entries and verify they can be sorted by tree_key (EIP-7864)
    let entries = vec![
        StorageEntry::from_storage_slot([0x11; 20], [0x01; 32], [0xaa; 32]),
        StorageEntry::from_storage_slot([0x22; 20], [0x02; 32], [0xbb; 32]),
        StorageEntry::from_storage_slot([0x33; 20], [0x03; 32], [0xcc; 32]),
    ];

    let mut sorted = entries.clone();
    sorted.sort_by_key(|e| compute_tree_key(&e.address, &e.tree_index));

    // Verify tree_keys are non-decreasing
    let mut prev_key = [0u8; 32];
    for entry in &sorted {
        let key = compute_tree_key(&entry.address, &entry.tree_index);
        assert!(key >= prev_key, "Entries should be sorted by tree_key");
        prev_key = key;
    }
}

#[test]
fn test_has_magic_detection() {
    assert!(StateHeader::has_magic(b"PIR2abcdefgh"));
    assert!(!StateHeader::has_magic(b"XXXXabcdefgh"));
    assert!(!StateHeader::has_magic(b"PIR")); // Too short
}

#[test]
fn test_known_entry_lookup_with_tree_key() {
    // Create entries with known addresses/slots using EIP-7864 tree_index
    let known_address: [u8; 20] = [
        0xda, 0xc1, 0x7f, 0x95, 0x8d, 0x2e, 0xe5, 0x23, 0xa2, 0x20, 0x62, 0x06, 0x99, 0x45, 0x97,
        0xc1, 0x3d, 0x83, 0x1e, 0xc7, // USDT address
    ];
    let known_slot: [u8; 32] = [0u8; 32]; // slot 0
    let known_value: [u8; 32] = {
        let mut v = [0u8; 32];
        v[31] = 0x42; // some value
        v
    };

    // Create entries using from_storage_slot (computes proper tree_index)
    let mut entries = vec![
        StorageEntry::from_storage_slot(known_address, known_slot, known_value),
        StorageEntry::from_storage_slot([0x11; 20], [0x01; 32], [0xaa; 32]),
        StorageEntry::from_storage_slot([0x22; 20], [0x02; 32], [0xbb; 32]),
    ];

    // Sort by tree_key (EIP-7864 ordering)
    entries.sort_by_key(|e| compute_tree_key(&e.address, &e.tree_index));

    // Compute the expected tree_index for our known slot
    let expected_tree_index = inspire_core::ubt::compute_storage_tree_index(&known_slot);

    // Find our entry in the sorted list
    let found = entries
        .iter()
        .any(|e| e.address == known_address && e.tree_index == expected_tree_index);

    assert!(found, "Known entry should be found in sorted list");

    // Verify the value matches
    let entry = entries
        .iter()
        .find(|e| e.address == known_address && e.tree_index == expected_tree_index)
        .unwrap();

    assert_eq!(entry.value, known_value);
}

#[test]
fn test_slots_0_63_share_same_stem() {
    // EIP-7864: slots 0-63 should all have stem_pos = 0
    let address = [0x42; 20];

    for slot_num in 0..64u8 {
        let mut slot = [0u8; 32];
        slot[31] = slot_num;
        let entry = StorageEntry::from_storage_slot(address, slot, [0xff; 32]);

        // stem_pos is tree_index[0..31], should be all zeros
        assert_eq!(
            entry.tree_index[..31],
            [0u8; 31],
            "Slot {} should have zero stem_pos",
            slot_num
        );
        // subindex should be 64 + slot_num
        assert_eq!(
            entry.tree_index[31],
            64 + slot_num,
            "Slot {} should have subindex {}",
            slot_num,
            64 + slot_num
        );
    }
}

#[test]
fn test_slot_64_goes_to_overflow() {
    let address = [0x42; 20];
    let mut slot = [0u8; 32];
    slot[31] = 64;

    let entry = StorageEntry::from_storage_slot(address, slot, [0xff; 32]);

    // Slot 64 should go to overflow stem (MAIN_STORAGE_OFFSET + 64)
    // tree_index[0] should be 1 (high byte of 256^31)
    assert_eq!(entry.tree_index[0], 1);
    assert_eq!(entry.tree_index[31], 64);
}
