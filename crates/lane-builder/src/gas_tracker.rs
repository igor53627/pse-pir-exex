//! Gas usage tracking for data-driven hot lane selection
//!
//! Tracks gas consumption by contract address to identify "gas guzzlers" -
//! contracts that consume the most gas and should be prioritized in the hot lane.

#![cfg(feature = "backfill")]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::{BlockNumberOrTag, TransactionTrait};
use futures::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Gas usage statistics for a single contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasStats {
    pub address: [u8; 20],
    pub total_gas: u64,
    pub tx_count: u64,
    pub first_seen_block: u64,
    pub last_seen_block: u64,
}

impl GasStats {
    fn new(address: [u8; 20], block: u64, gas: u64) -> Self {
        Self {
            address,
            total_gas: gas,
            tx_count: 1,
            first_seen_block: block,
            last_seen_block: block,
        }
    }

    fn add_tx(&mut self, block: u64, gas: u64) {
        self.total_gas = self.total_gas.saturating_add(gas);
        self.tx_count += 1;
        self.last_seen_block = self.last_seen_block.max(block);
        self.first_seen_block = self.first_seen_block.min(block);
    }
}

/// Configuration for gas backfill
#[derive(Debug, Clone)]
pub struct BackfillConfig {
    pub rpc_url: String,
    pub block_count: u64,
    pub batch_size: usize,
    pub concurrency: usize,
}

impl Default for BackfillConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8545".to_string(),
            block_count: 100_000,
            batch_size: 100,
            concurrency: 10,
        }
    }
}

/// Result of a gas backfill operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillResult {
    pub start_block: u64,
    pub end_block: u64,
    pub blocks_processed: u64,
    pub total_transactions: u64,
    pub unique_contracts: usize,
    pub gas_stats: Vec<GasStats>,
}

impl BackfillResult {
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let result: Self = serde_json::from_str(&content)?;
        Ok(result)
    }

    pub fn top_contracts(&self, n: usize) -> Vec<&GasStats> {
        let mut sorted: Vec<_> = self.gas_stats.iter().collect();
        sorted.sort_by(|a, b| b.total_gas.cmp(&a.total_gas));
        sorted.into_iter().take(n).collect()
    }
}

/// Gas tracker for backfilling historical gas usage
pub struct GasTracker {
    rpc_url: String,
    gas_by_contract: Arc<Mutex<HashMap<[u8; 20], GasStats>>>,
    config: BackfillConfig,
}

impl GasTracker {
    pub async fn new(config: BackfillConfig) -> anyhow::Result<Self> {
        Ok(Self {
            rpc_url: config.rpc_url.clone(),
            gas_by_contract: Arc::new(Mutex::new(HashMap::new())),
            config,
        })
    }

    pub async fn backfill(&self) -> anyhow::Result<BackfillResult> {
        let provider = ProviderBuilder::new()
            .connect(&self.rpc_url)
            .await?;
        
        let latest = provider.get_block_number().await?;
        let start_block = latest.saturating_sub(self.config.block_count);
        let end_block = latest;

        info!(
            start = start_block,
            end = end_block,
            blocks = self.config.block_count,
            "Starting gas backfill"
        );

        let pb = ProgressBar::new(self.config.block_count);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} blocks ({eta})")?
                .progress_chars("#>-"),
        );

        let total_txs = Arc::new(Mutex::new(0u64));

        let block_numbers: Vec<u64> = (start_block..=end_block).collect();
        let chunks: Vec<Vec<u64>> = block_numbers
            .chunks(self.config.batch_size)
            .map(|c| c.to_vec())
            .collect();

        let rpc_url = self.rpc_url.clone();
        let gas_map = self.gas_by_contract.clone();

        stream::iter(chunks)
            .for_each_concurrent(self.config.concurrency, |batch| {
                let rpc_url = rpc_url.clone();
                let gas_map = gas_map.clone();
                let total_txs = total_txs.clone();
                let pb = pb.clone();

                async move {
                    let Ok(provider) = ProviderBuilder::new().connect(&rpc_url).await else {
                        warn!("Failed to connect to RPC");
                        return;
                    };
                    
                    for block_num in batch {
                        if let Err(e) = Self::process_block(&provider, &gas_map, &total_txs, block_num).await {
                            warn!(block = block_num, error = %e, "Failed to process block");
                        }
                        pb.inc(1);
                    }
                }
            })
            .await;

        pb.finish_with_message("Backfill complete");

        let gas_stats: Vec<GasStats> = {
            let map = self.gas_by_contract.lock().await;
            map.values().cloned().collect()
        };

        let total_transactions = *total_txs.lock().await;

        let result = BackfillResult {
            start_block,
            end_block,
            blocks_processed: end_block - start_block + 1,
            total_transactions,
            unique_contracts: gas_stats.len(),
            gas_stats,
        };

        info!(
            blocks = result.blocks_processed,
            transactions = result.total_transactions,
            contracts = result.unique_contracts,
            "Backfill complete"
        );

        Ok(result)
    }

    async fn process_block<P: Provider>(
        provider: &P,
        gas_map: &Arc<Mutex<HashMap<[u8; 20], GasStats>>>,
        total_txs: &Arc<Mutex<u64>>,
        block_num: u64,
    ) -> anyhow::Result<()> {
        let block = provider
            .get_block_by_number(BlockNumberOrTag::Number(block_num))
            .full()
            .await?;

        let Some(block) = block else {
            return Ok(());
        };

        let txs = block.transactions.into_transactions();

        for tx in txs {
            let Some(to) = tx.to() else {
                continue;
            };

            let gas_used = tx.gas_limit();
            let to_bytes: [u8; 20] = to.0.into();

            let mut map = gas_map.lock().await;
            map.entry(to_bytes)
                .and_modify(|stats| stats.add_tx(block_num, gas_used))
                .or_insert_with(|| GasStats::new(to_bytes, block_num, gas_used));

            let mut count = total_txs.lock().await;
            *count += 1;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_stats_accumulation() {
        let addr = [0x11u8; 20];
        let mut stats = GasStats::new(addr, 1000, 50000);

        stats.add_tx(1001, 30000);
        stats.add_tx(1002, 20000);

        assert_eq!(stats.total_gas, 100000);
        assert_eq!(stats.tx_count, 3);
        assert_eq!(stats.first_seen_block, 1000);
        assert_eq!(stats.last_seen_block, 1002);
    }

    #[test]
    fn test_backfill_result_top_contracts() {
        let result = BackfillResult {
            start_block: 0,
            end_block: 100,
            blocks_processed: 100,
            total_transactions: 1000,
            unique_contracts: 3,
            gas_stats: vec![
                GasStats {
                    address: [0x11u8; 20],
                    total_gas: 1000,
                    tx_count: 10,
                    first_seen_block: 0,
                    last_seen_block: 100,
                },
                GasStats {
                    address: [0x22u8; 20],
                    total_gas: 5000,
                    tx_count: 20,
                    first_seen_block: 0,
                    last_seen_block: 100,
                },
                GasStats {
                    address: [0x33u8; 20],
                    total_gas: 3000,
                    tx_count: 15,
                    first_seen_block: 0,
                    last_seen_block: 100,
                },
            ],
        };

        let top2 = result.top_contracts(2);
        assert_eq!(top2.len(), 2);
        assert_eq!(top2[0].total_gas, 5000);
        assert_eq!(top2[1].total_gas, 3000);
    }
}
