//! Balance record types for ETH/USDC hot lane
//!
//! Fixed-size balance records for efficient PIR queries.

use serde::{Deserialize, Serialize};

pub const BALANCE_RECORD_SIZE: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceRecord {
    pub eth_balance: [u8; 32],
    pub usdc_balance: [u8; 32],
}

impl BalanceRecord {
    pub const SIZE: usize = BALANCE_RECORD_SIZE;

    pub fn new(eth_balance: [u8; 32], usdc_balance: [u8; 32]) -> Self {
        Self {
            eth_balance,
            usdc_balance,
        }
    }

    pub fn zero() -> Self {
        Self {
            eth_balance: [0u8; 32],
            usdc_balance: [0u8; 32],
        }
    }

    pub fn from_u256(eth: [u8; 32], usdc: [u8; 32]) -> Self {
        Self::new(eth, usdc)
    }

    pub fn to_bytes(&self) -> [u8; BALANCE_RECORD_SIZE] {
        let mut bytes = [0u8; BALANCE_RECORD_SIZE];
        bytes[..32].copy_from_slice(&self.eth_balance);
        bytes[32..].copy_from_slice(&self.usdc_balance);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < BALANCE_RECORD_SIZE {
            return None;
        }
        let mut eth = [0u8; 32];
        let mut usdc = [0u8; 32];
        eth.copy_from_slice(&bytes[..32]);
        usdc.copy_from_slice(&bytes[32..64]);
        Some(Self::new(eth, usdc))
    }

    pub fn eth_as_u128(&self) -> u128 {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&self.eth_balance[16..32]);
        u128::from_be_bytes(bytes)
    }

    pub fn usdc_as_u128(&self) -> u128 {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&self.usdc_balance[16..32]);
        u128::from_be_bytes(bytes)
    }
}

impl Default for BalanceRecord {
    fn default() -> Self {
        Self::zero()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceDbMetadata {
    pub chain_id: u64,
    pub snapshot_block: u64,
    pub snapshot_block_hash: String,
    pub usdc_contract: String,
    pub record_size: usize,
    pub num_records: usize,
    pub addresses: Vec<String>,
}

impl BalanceDbMetadata {
    pub fn find_index(&self, address: &str) -> Option<usize> {
        let normalized = address.to_lowercase();
        self.addresses
            .iter()
            .position(|a| a.to_lowercase() == normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_record_size() {
        assert_eq!(std::mem::size_of::<BalanceRecord>(), BALANCE_RECORD_SIZE);
    }

    #[test]
    fn test_balance_record_roundtrip() {
        let eth = [1u8; 32];
        let usdc = [2u8; 32];
        let record = BalanceRecord::new(eth, usdc);

        let bytes = record.to_bytes();
        let recovered = BalanceRecord::from_bytes(&bytes).unwrap();

        assert_eq!(record, recovered);
    }

    #[test]
    fn test_balance_as_u128() {
        let mut eth = [0u8; 32];
        eth[31] = 100;
        let record = BalanceRecord::new(eth, [0u8; 32]);
        assert_eq!(record.eth_as_u128(), 100);
    }
}
