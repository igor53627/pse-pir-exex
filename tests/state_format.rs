//! Tests for STATE_FORMAT parsing and generation
//!
//! These tests verify the state.bin format defined in docs/STATE_FORMAT.md

use inspire_core::bucket_index::compute_bucket_id;
use inspire_core::state_format::{
    StateHeader, StateFormatError, StorageEntry,
    STATE_ENTRY_SIZE, STATE_HEADER_SIZE, STATE_MAGIC,
};

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
    let slot = [0x01; 32];
    let value = [0xff; 32];
    
    let entry = StorageEntry::new(address, slot, value);
    let bytes = entry.to_bytes();
    
    assert_eq!(bytes.len(), STATE_ENTRY_SIZE);
    
    let recovered = StorageEntry::from_bytes(&bytes).unwrap();
    assert_eq!(recovered.address, address);
    assert_eq!(recovered.slot, slot);
    assert_eq!(recovered.value, value);
}

#[test]
fn test_full_file_format() {
    // Simulate a complete state.bin file
    let entry_count = 3u64;
    let block_number = 20_000_000u64;
    let chain_id = 1u64;
    let block_hash = [0xde; 32];
    
    let header = StateHeader::new(entry_count, block_number, chain_id, block_hash);
    
    let entries = vec![
        StorageEntry::new([0x11; 20], [0x01; 32], [0xaa; 32]),
        StorageEntry::new([0x22; 20], [0x02; 32], [0xbb; 32]),
        StorageEntry::new([0x33; 20], [0x03; 32], [0xcc; 32]),
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
    assert!(matches!(result, Err(StateFormatError::HeaderTooShort { actual: 32 })));
}

#[test]
fn test_entries_sorted_by_bucket() {
    // Generate entries and verify they can be sorted by bucket ID
    let entries = vec![
        StorageEntry::new([0x11; 20], [0x01; 32], [0xaa; 32]),
        StorageEntry::new([0x22; 20], [0x02; 32], [0xbb; 32]),
        StorageEntry::new([0x33; 20], [0x03; 32], [0xcc; 32]),
    ];
    
    let mut sorted = entries.clone();
    sorted.sort_by_key(|e| compute_bucket_id(&e.address, &e.slot));
    
    // Verify bucket IDs are non-decreasing
    let mut prev_bucket = 0;
    for entry in &sorted {
        let bucket = compute_bucket_id(&entry.address, &entry.slot);
        assert!(bucket >= prev_bucket || prev_bucket == 0);
        prev_bucket = bucket;
    }
}

#[test]
fn test_has_magic_detection() {
    assert!(StateHeader::has_magic(b"PIR2abcdefgh"));
    assert!(!StateHeader::has_magic(b"XXXXabcdefgh"));
    assert!(!StateHeader::has_magic(b"PIR")); // Too short
}
