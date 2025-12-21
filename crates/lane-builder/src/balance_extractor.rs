//! Balance extraction for ETH/USDC hot lane
//!
//! Fetches ETH and USDC balances for a list of addresses at a snapshot block.

use std::path::Path;

use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use alloy_rpc_types::BlockId;
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

use inspire_core::{BalanceDbMetadata, BalanceRecord, BALANCE_RECORD_SIZE};

const USDC_BALANCE_OF_SELECTOR: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceExtractorConfig {
    pub chain_id: u64,
    pub usdc_contract: Address,
    pub batch_size: usize,
    pub max_concurrent: usize,
}

impl Default for BalanceExtractorConfig {
    fn default() -> Self {
        Self {
            chain_id: 1,
            usdc_contract: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
                .parse()
                .unwrap(),
            batch_size: 100,
            max_concurrent: 10,
        }
    }
}

impl BalanceExtractorConfig {
    pub fn holesky() -> Self {
        Self {
            chain_id: 17000,
            usdc_contract: "0x94a9D9AC8a22534E3FaCa9F4e7F2E2cf85d5E4C8"
                .parse()
                .unwrap(),
            batch_size: 100,
            max_concurrent: 10,
        }
    }

    pub fn sepolia() -> Self {
        Self {
            chain_id: 11155111,
            usdc_contract: "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238"
                .parse()
                .unwrap(),
            batch_size: 100,
            max_concurrent: 10,
        }
    }
}

pub struct BalanceExtractor<P> {
    provider: P,
    config: BalanceExtractorConfig,
}

impl<P: Provider + Clone + Send + Sync + 'static> BalanceExtractor<P> {
    pub fn new(provider: P, config: BalanceExtractorConfig) -> Self {
        Self { provider, config }
    }

    pub async fn extract_balances(
        &self,
        addresses: &[Address],
        block: BlockId,
    ) -> anyhow::Result<Vec<BalanceRecord>> {
        let pb = ProgressBar::new(addresses.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} ({eta})")?
                .progress_chars("##-"),
        );

        let mut records = Vec::with_capacity(addresses.len());

        for chunk in addresses.chunks(self.config.batch_size) {
            let futures: Vec<_> = chunk
                .iter()
                .map(|addr| self.fetch_balance(*addr, block))
                .collect();

            let results = join_all(futures).await;

            for result in results {
                match result {
                    Ok(record) => records.push(record),
                    Err(e) => {
                        tracing::warn!("Failed to fetch balance: {}", e);
                        records.push(BalanceRecord::zero());
                    }
                }
                pb.inc(1);
            }
        }

        pb.finish_with_message("Done");
        Ok(records)
    }

    async fn fetch_balance(
        &self,
        address: Address,
        block: BlockId,
    ) -> anyhow::Result<BalanceRecord> {
        let eth_future = self.provider.get_balance(address).block_id(block);
        let usdc_future = self.fetch_usdc_balance(address, block);

        let (eth_result, usdc_result) = tokio::join!(eth_future, usdc_future);

        let eth_balance = eth_result.unwrap_or(U256::ZERO);
        let usdc_balance = usdc_result.unwrap_or(U256::ZERO);

        Ok(BalanceRecord::new(
            eth_balance.to_be_bytes(),
            usdc_balance.to_be_bytes(),
        ))
    }

    async fn fetch_usdc_balance(&self, address: Address, block: BlockId) -> anyhow::Result<U256> {
        let mut calldata = Vec::with_capacity(36);
        calldata.extend_from_slice(&USDC_BALANCE_OF_SELECTOR);
        calldata.extend_from_slice(&[0u8; 12]);
        calldata.extend_from_slice(address.as_slice());

        let tx = alloy_rpc_types::TransactionRequest::default()
            .to(self.config.usdc_contract)
            .input(calldata.into());

        let result = self.provider.call(tx).block(block).await?;

        if result.len() >= 32 {
            Ok(U256::from_be_slice(&result[..32]))
        } else {
            Ok(U256::ZERO)
        }
    }

    pub async fn build_database(
        &self,
        addresses: &[Address],
        block_number: u64,
        block_hash: &str,
        output_dir: &Path,
    ) -> anyhow::Result<BalanceDbMetadata> {
        std::fs::create_dir_all(output_dir)?;

        let block = BlockId::number(block_number);
        let records = self.extract_balances(addresses, block).await?;

        let db_path = output_dir.join("balances.bin");
        let mut db_data = Vec::with_capacity(records.len() * BALANCE_RECORD_SIZE);
        for record in &records {
            db_data.extend_from_slice(&record.to_bytes());
        }
        std::fs::write(&db_path, &db_data)?;

        let metadata = BalanceDbMetadata {
            chain_id: self.config.chain_id,
            snapshot_block: block_number,
            snapshot_block_hash: block_hash.to_string(),
            usdc_contract: format!("{:?}", self.config.usdc_contract),
            record_size: BALANCE_RECORD_SIZE,
            num_records: records.len(),
            addresses: addresses.iter().map(|a| format!("{:?}", a)).collect(),
        };

        let metadata_path = output_dir.join("metadata.json");
        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        std::fs::write(&metadata_path, &metadata_json)?;

        tracing::info!(
            records = records.len(),
            db_size = db_data.len(),
            path = %db_path.display(),
            "Balance database built"
        );

        Ok(metadata)
    }
}

pub fn load_addresses_from_file(path: &Path) -> anyhow::Result<Vec<Address>> {
    let content = std::fs::read_to_string(path)?;
    let addresses: Vec<String> = serde_json::from_str(&content)?;

    addresses
        .iter()
        .map(|s| s.parse().map_err(|e| anyhow::anyhow!("Invalid address {}: {}", s, e)))
        .collect()
}

pub fn default_hot_addresses() -> Vec<Address> {
    vec![
        "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap(),
        "0xdAC17F958D2ee523a2206206994597C13D831ec7".parse().unwrap(),
        "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".parse().unwrap(),
        "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".parse().unwrap(),
        "0x6B175474E89094C44Da98b954EedeAC495271d0F".parse().unwrap(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_record_size() {
        assert_eq!(BALANCE_RECORD_SIZE, 64);
    }

    #[test]
    fn test_config_defaults() {
        let config = BalanceExtractorConfig::default();
        assert_eq!(config.chain_id, 1);
        assert_eq!(config.batch_size, 100);
    }
}
