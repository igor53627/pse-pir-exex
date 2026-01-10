//! UBT Stem Index for WASM client
//!
//! Provides deterministic stem-based indexing per EIP-7864, eliminating
//! the need for bucket index downloads. Clients compute PIR indices locally
//! using only BLAKE3 hashing.
//!
//! ## EIP-7864 Tree Embedding
//!
//! Each address has an "account stem" containing:
//! - subindex 0: basic_data (nonce, balance, code_size)
//! - subindex 1: code_hash
//! - subindex 64-127: storage slots 0-63
//! - subindex 128-255: code chunks 0-127
//!
//! Storage slots >= 64 and code chunks >= 128 use overflow stems.
//!
//! ## Usage
//!
//! ```js
//! // Compute tree_index for a storage slot
//! const treeIndex = computeStorageTreeIndex(slot);
//!
//! // Compute stem for address + tree_index
//! const stem = computeStem(address, treeIndex);
//!
//! // With a stem offset table (downloaded once, rarely changes)
//! const index = stemIndex.lookupStorage(address, slot);
//!
//! // Query using computed index
//! const result = await client.query(index);
//! ```

use inspire_core::ubt::{
    compute_basic_data_tree_index, compute_code_chunk_tree_index, compute_code_hash_tree_index,
    compute_stem, compute_storage_tree_index, compute_storage_tree_key, compute_tree_key,
    get_subindex, Stem,
};
use wasm_bindgen::prelude::*;

/// Compute tree_index for a storage slot per EIP-7864.
///
/// For slots 0-63: returns tree_index with subindex 64-127 (account stem)
/// For slots >= 64: returns tree_index in overflow stems
///
/// # Arguments
/// - `slot`: 32-byte storage slot key (interpreted as big-endian U256)
///
/// # Returns
/// 32-byte tree_index (stem_pos[31] || subindex[1])
#[wasm_bindgen(js_name = computeStorageTreeIndex)]
pub fn compute_storage_tree_index_js(slot: &[u8]) -> Result<Vec<u8>, JsValue> {
    if slot.len() != 32 {
        return Err(JsValue::from_str("Slot must be 32 bytes"));
    }

    let sl: [u8; 32] = slot.try_into().unwrap();
    let tree_index = compute_storage_tree_index(&sl);
    Ok(tree_index.to_vec())
}

/// Compute tree_index for basic_data header (nonce, balance, code_size).
///
/// Returns tree_index with stem_pos=0, subindex=0.
#[wasm_bindgen(js_name = computeBasicDataTreeIndex)]
pub fn compute_basic_data_tree_index_js() -> Vec<u8> {
    compute_basic_data_tree_index().to_vec()
}

/// Compute tree_index for code_hash header.
///
/// Returns tree_index with stem_pos=0, subindex=1.
#[wasm_bindgen(js_name = computeCodeHashTreeIndex)]
pub fn compute_code_hash_tree_index_js() -> Vec<u8> {
    compute_code_hash_tree_index().to_vec()
}

/// Compute tree_index for a code chunk.
///
/// For chunks 0-127: subindex 128-255 (account stem)
/// For chunks >= 128: overflow stems
///
/// # Arguments
/// - `chunk_id`: Code chunk index (0, 1, 2, ...)
#[wasm_bindgen(js_name = computeCodeChunkTreeIndex)]
pub fn compute_code_chunk_tree_index_js(chunk_id: u32) -> Vec<u8> {
    compute_code_chunk_tree_index(chunk_id).to_vec()
}

/// Compute UBT stem from address and tree_index (EIP-7864).
///
/// Returns the 31-byte stem as a Uint8Array.
///
/// # Arguments
/// - `address`: 20-byte contract address
/// - `tree_index`: 32-byte tree index (stem_pos[31] || subindex[1])
#[wasm_bindgen(js_name = computeStem)]
pub fn compute_stem_js(address: &[u8], tree_index: &[u8]) -> Result<Vec<u8>, JsValue> {
    if address.len() != 20 {
        return Err(JsValue::from_str("Address must be 20 bytes"));
    }
    if tree_index.len() != 32 {
        return Err(JsValue::from_str("TreeIndex must be 32 bytes"));
    }

    let addr: [u8; 20] = address.try_into().unwrap();
    let ti: [u8; 32] = tree_index.try_into().unwrap();

    let stem = compute_stem(&addr, &ti);
    Ok(stem.to_vec())
}

/// Compute full 32-byte tree key from address and tree_index.
///
/// The tree key is `stem || subindex` where stem = blake3(address32 || tree_index[:31])[:31]
/// and subindex = tree_index[31].
///
/// # Arguments
/// - `address`: 20-byte contract address
/// - `tree_index`: 32-byte tree index
#[wasm_bindgen(js_name = computeTreeKey)]
pub fn compute_tree_key_js(address: &[u8], tree_index: &[u8]) -> Result<Vec<u8>, JsValue> {
    if address.len() != 20 {
        return Err(JsValue::from_str("Address must be 20 bytes"));
    }
    if tree_index.len() != 32 {
        return Err(JsValue::from_str("TreeIndex must be 32 bytes"));
    }

    let addr: [u8; 20] = address.try_into().unwrap();
    let ti: [u8; 32] = tree_index.try_into().unwrap();

    let key = compute_tree_key(&addr, &ti);
    Ok(key.to_vec())
}

/// Compute tree key for a storage slot (convenience function).
///
/// Combines tree_index computation and stem hashing in one step.
///
/// # Arguments
/// - `address`: 20-byte contract address
/// - `slot`: 32-byte storage slot key
#[wasm_bindgen(js_name = computeStorageTreeKey)]
pub fn compute_storage_tree_key_js(address: &[u8], slot: &[u8]) -> Result<Vec<u8>, JsValue> {
    if address.len() != 20 {
        return Err(JsValue::from_str("Address must be 20 bytes"));
    }
    if slot.len() != 32 {
        return Err(JsValue::from_str("Slot must be 32 bytes"));
    }

    let addr: [u8; 20] = address.try_into().unwrap();
    let sl: [u8; 32] = slot.try_into().unwrap();

    let key = compute_storage_tree_key(&addr, &sl);
    Ok(key.to_vec())
}

/// Get subindex (last byte of tree_index).
#[wasm_bindgen(js_name = getSubindex)]
pub fn get_subindex_js(tree_index: &[u8]) -> Result<u8, JsValue> {
    if tree_index.len() != 32 {
        return Err(JsValue::from_str("TreeIndex must be 32 bytes"));
    }

    let ti: [u8; 32] = tree_index.try_into().unwrap();
    Ok(get_subindex(&ti))
}

/// Stem offset table for O(log N) lookup of PIR indices.
///
/// This table maps stems to their starting indices in the PIR database.
/// With EIP-7864 tree embedding, expected ~30K-60K stems vs millions of entries.
#[wasm_bindgen]
pub struct StemIndex {
    stems: Vec<Stem>,
    offsets: Vec<u64>,
}

#[wasm_bindgen]
impl StemIndex {
    /// Create a stem index from binary data.
    ///
    /// Format: `count:8 + (stem:31 + offset:8)*`
    #[wasm_bindgen(constructor)]
    pub fn from_bytes(data: &[u8]) -> Result<StemIndex, JsValue> {
        const HEADER_SIZE: usize = 8;
        const ENTRY_SIZE: usize = 39; // 31 bytes stem + 8 bytes offset

        if data.len() < HEADER_SIZE {
            return Err(JsValue::from_str("Data too short for stem index header"));
        }

        let count = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let count: usize = count
            .try_into()
            .map_err(|_| JsValue::from_str("Stem count overflow"))?;

        // Use checked arithmetic to prevent overflow on 32-bit WASM
        let payload_size = count
            .checked_mul(ENTRY_SIZE)
            .ok_or_else(|| JsValue::from_str("Stem count overflow"))?;
        let expected_size = HEADER_SIZE
            .checked_add(payload_size)
            .ok_or_else(|| JsValue::from_str("Stem index size overflow"))?;

        if data.len() < expected_size {
            return Err(JsValue::from_str(&format!(
                "Data too short: expected {} bytes, got {}",
                expected_size,
                data.len()
            )));
        }

        let mut stems = Vec::with_capacity(count);
        let mut offsets = Vec::with_capacity(count);
        let mut offset = HEADER_SIZE;

        for _ in 0..count {
            let stem: Stem = data[offset..offset + 31].try_into().unwrap();
            let start_idx = u64::from_le_bytes(data[offset + 31..offset + 39].try_into().unwrap());

            // Validate stems are in sorted order for binary search
            if let Some(last) = stems.last() {
                if stem < *last {
                    return Err(JsValue::from_str("Stem index not sorted"));
                }
            }

            stems.push(stem);
            offsets.push(start_idx);
            offset += ENTRY_SIZE;
        }

        Ok(StemIndex { stems, offsets })
    }

    /// Get number of stems in the index.
    #[wasm_bindgen(getter)]
    pub fn count(&self) -> u32 {
        self.stems.len() as u32
    }

    /// Look up the PIR database index for an (address, tree_index) pair.
    ///
    /// Returns the index if the stem exists in the database, or -1 if not found.
    pub fn lookup(&self, address: &[u8], tree_index: &[u8]) -> Result<i64, JsValue> {
        if address.len() != 20 {
            return Err(JsValue::from_str("Address must be 20 bytes"));
        }
        if tree_index.len() != 32 {
            return Err(JsValue::from_str("TreeIndex must be 32 bytes"));
        }

        let addr: [u8; 20] = address.try_into().unwrap();
        let ti: [u8; 32] = tree_index.try_into().unwrap();

        let stem = compute_stem(&addr, &ti);
        let subindex = get_subindex(&ti) as u64;

        // Binary search for the stem
        match self.stems.binary_search(&stem) {
            Ok(idx) => {
                let global_idx = self.offsets[idx]
                    .checked_add(subindex)
                    .and_then(|v| i64::try_from(v).ok())
                    .ok_or_else(|| JsValue::from_str("Index overflow"))?;
                Ok(global_idx)
            }
            Err(_) => Ok(-1), // Not found
        }
    }

    /// Look up PIR database index for a storage slot.
    ///
    /// Convenience method that computes tree_index internally.
    ///
    /// # Arguments
    /// - `address`: 20-byte contract address
    /// - `slot`: 32-byte storage slot key
    pub fn lookup_storage(&self, address: &[u8], slot: &[u8]) -> Result<i64, JsValue> {
        if address.len() != 20 {
            return Err(JsValue::from_str("Address must be 20 bytes"));
        }
        if slot.len() != 32 {
            return Err(JsValue::from_str("Slot must be 32 bytes"));
        }

        let sl: [u8; 32] = slot.try_into().unwrap();
        let tree_index = compute_storage_tree_index(&sl);

        self.lookup(address, &tree_index)
    }

    /// Look up PIR database index for basic_data header.
    pub fn lookup_basic_data(&self, address: &[u8]) -> Result<i64, JsValue> {
        if address.len() != 20 {
            return Err(JsValue::from_str("Address must be 20 bytes"));
        }

        let tree_index = compute_basic_data_tree_index();
        self.lookup(address, &tree_index)
    }

    /// Look up PIR database index for code_hash header.
    pub fn lookup_code_hash(&self, address: &[u8]) -> Result<i64, JsValue> {
        if address.len() != 20 {
            return Err(JsValue::from_str("Address must be 20 bytes"));
        }

        let tree_index = compute_code_hash_tree_index();
        self.lookup(address, &tree_index)
    }

    /// Look up PIR database index for a code chunk.
    pub fn lookup_code_chunk(&self, address: &[u8], chunk_id: u32) -> Result<i64, JsValue> {
        if address.len() != 20 {
            return Err(JsValue::from_str("Address must be 20 bytes"));
        }

        let tree_index = compute_code_chunk_tree_index(chunk_id);
        self.lookup(address, &tree_index)
    }

    /// Look up just the stem's starting offset (without adding subindex).
    pub fn lookup_stem_offset(&self, address: &[u8], tree_index: &[u8]) -> Result<i64, JsValue> {
        if address.len() != 20 {
            return Err(JsValue::from_str("Address must be 20 bytes"));
        }
        if tree_index.len() != 32 {
            return Err(JsValue::from_str("TreeIndex must be 32 bytes"));
        }

        let addr: [u8; 20] = address.try_into().unwrap();
        let ti: [u8; 32] = tree_index.try_into().unwrap();

        let stem = compute_stem(&addr, &ti);

        match self.stems.binary_search(&stem) {
            Ok(idx) => {
                i64::try_from(self.offsets[idx]).map_err(|_| JsValue::from_str("Offset overflow"))
            }
            Err(_) => Ok(-1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_compute_storage_tree_index_small() {
        // Slot 0 -> subindex 64
        let slot = [0u8; 32];
        let tree_index = compute_storage_tree_index_js(&slot).unwrap();
        assert_eq!(tree_index.len(), 32);
        assert_eq!(tree_index[31], 64);
    }

    #[wasm_bindgen_test]
    fn test_compute_storage_tree_index_large() {
        // Slot 64 -> overflow stem
        let mut slot = [0u8; 32];
        slot[31] = 64;
        let tree_index = compute_storage_tree_index_js(&slot).unwrap();
        assert_eq!(tree_index[0], 1); // MAIN_STORAGE_OFFSET
        assert_eq!(tree_index[31], 64);
    }

    #[wasm_bindgen_test]
    fn test_compute_basic_data_tree_index() {
        let tree_index = compute_basic_data_tree_index_js();
        assert_eq!(tree_index.len(), 32);
        assert_eq!(tree_index[31], 0);
    }

    #[wasm_bindgen_test]
    fn test_compute_code_hash_tree_index() {
        let tree_index = compute_code_hash_tree_index_js();
        assert_eq!(tree_index.len(), 32);
        assert_eq!(tree_index[31], 1);
    }

    #[wasm_bindgen_test]
    fn test_compute_code_chunk_tree_index() {
        // Chunk 0 -> subindex 128
        let tree_index = compute_code_chunk_tree_index_js(0);
        assert_eq!(tree_index[31], 128);

        // Chunk 127 -> subindex 255
        let tree_index = compute_code_chunk_tree_index_js(127);
        assert_eq!(tree_index[31], 255);
    }

    #[wasm_bindgen_test]
    fn test_compute_stem_js() {
        let address = [0x42u8; 20];
        let tree_index = [0u8; 32]; // account stem

        let stem = compute_stem_js(&address, &tree_index).unwrap();
        assert_eq!(stem.len(), 31);
    }

    #[wasm_bindgen_test]
    fn test_compute_tree_key_js() {
        let address = [0x42u8; 20];
        let mut tree_index = [0u8; 32];
        tree_index[31] = 42;

        let key = compute_tree_key_js(&address, &tree_index).unwrap();
        assert_eq!(key.len(), 32);
        assert_eq!(key[31], 42); // subindex preserved
    }

    #[wasm_bindgen_test]
    fn test_compute_storage_tree_key_js() {
        let address = [0x42u8; 20];
        let slot = [0u8; 32]; // slot 0

        let key = compute_storage_tree_key_js(&address, &slot).unwrap();
        assert_eq!(key.len(), 32);
        assert_eq!(key[31], 64); // slot 0 -> subindex 64
    }

    #[wasm_bindgen_test]
    fn test_get_subindex_js() {
        let mut tree_index = [0u8; 32];
        tree_index[31] = 255;

        let subindex = get_subindex_js(&tree_index).unwrap();
        assert_eq!(subindex, 255);
    }

    #[wasm_bindgen_test]
    fn test_stem_index_from_bytes() {
        // Create test data: 2 stems
        let mut data = Vec::new();
        data.extend_from_slice(&2u64.to_le_bytes()); // count

        // Stem 1: all zeros, offset 0
        data.extend_from_slice(&[0u8; 31]);
        data.extend_from_slice(&0u64.to_le_bytes());

        // Stem 2: all 0xff, offset 1000
        data.extend_from_slice(&[0xffu8; 31]);
        data.extend_from_slice(&1000u64.to_le_bytes());

        let index = StemIndex::from_bytes(&data).unwrap();
        assert_eq!(index.count(), 2);
    }

    #[wasm_bindgen_test]
    fn test_stem_deterministic() {
        let address = [0x42u8; 20];
        let tree_index = [0u8; 32];

        let stem1 = compute_stem_js(&address, &tree_index).unwrap();
        let stem2 = compute_stem_js(&address, &tree_index).unwrap();

        assert_eq!(stem1, stem2);
    }

    #[wasm_bindgen_test]
    fn test_invalid_address_length() {
        let address = [0x42u8; 19]; // Wrong size
        let tree_index = [0u8; 32];

        let result = compute_stem_js(&address, &tree_index);
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn test_invalid_tree_index_length() {
        let address = [0x42u8; 20];
        let tree_index = [0u8; 31]; // Wrong size

        let result = compute_stem_js(&address, &tree_index);
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn test_stem_index_overflow_rejected() {
        // Create malicious data with u64::MAX count
        let mut data = vec![0u8; 8];
        data[0..8].copy_from_slice(&u64::MAX.to_le_bytes());

        let result = StemIndex::from_bytes(&data);
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn test_stem_index_truncated_rejected() {
        // Claim 10 entries but provide only header
        let mut data = vec![0u8; 8];
        data[0..8].copy_from_slice(&10u64.to_le_bytes());

        let result = StemIndex::from_bytes(&data);
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn test_same_account_stem_for_all_account_leaves() {
        let address = [0x42u8; 20];

        // basic_data, code_hash, slot 0, chunk 0 should all share the same stem
        let basic_data = compute_basic_data_tree_index_js();
        let code_hash = compute_code_hash_tree_index_js();
        let slot_0_idx = compute_storage_tree_index_js(&[0u8; 32]).unwrap();
        let chunk_0 = compute_code_chunk_tree_index_js(0);

        let stem_basic = compute_stem_js(&address, &basic_data).unwrap();
        let stem_code = compute_stem_js(&address, &code_hash).unwrap();
        let stem_slot = compute_stem_js(&address, &slot_0_idx).unwrap();
        let stem_chunk = compute_stem_js(&address, &chunk_0).unwrap();

        assert_eq!(stem_basic, stem_code);
        assert_eq!(stem_code, stem_slot);
        assert_eq!(stem_slot, stem_chunk);
    }
}
