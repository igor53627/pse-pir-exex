//! Hybrid scoring for hot lane contract selection
//!
//! Combines data-driven gas usage with curated contract lists and category weights.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::contracts::{ContractInfo, HOT_CONTRACTS, KnownContract};

#[cfg(feature = "backfill")]
use crate::gas_tracker::BackfillResult;

/// Category weight multipliers for scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryWeights {
    pub privacy: f64,
    pub defi: f64,
    pub bridge: f64,
    pub stablecoin: f64,
    pub token: f64,
    pub lending: f64,
    pub dex: f64,
    pub nft: f64,
    pub governance: f64,
    pub default: f64,
}

impl Default for CategoryWeights {
    fn default() -> Self {
        Self {
            privacy: 3.0,
            defi: 1.5,
            bridge: 2.0,
            stablecoin: 1.5,
            token: 1.0,
            lending: 1.5,
            dex: 1.5,
            nft: 0.8,
            governance: 1.0,
            default: 1.0,
        }
    }
}

impl CategoryWeights {
    pub fn get(&self, category: &str) -> f64 {
        match category.to_lowercase().as_str() {
            "privacy" => self.privacy,
            "defi" => self.defi,
            "bridge" => self.bridge,
            "stablecoin" => self.stablecoin,
            "token" => self.token,
            "lending" => self.lending,
            "dex" => self.dex,
            "nft" => self.nft,
            "governance" => self.governance,
            _ => self.default,
        }
    }
}

/// A scored contract with combined ranking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredContract {
    #[serde(with = "hex_address")]
    pub address: [u8; 20],
    pub name: Option<String>,
    pub category: Option<String>,
    pub gas_score: u64,
    pub priority_boost: u64,
    pub category_weight: f64,
    pub final_score: u64,
    pub tx_count: u64,
    pub source: ContractSource,
}

/// Where the contract data came from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractSource {
    GasBackfill,
    KnownList,
    Both,
}

/// Configuration for the hybrid scorer
#[derive(Debug, Clone)]
pub struct HybridScorerConfig {
    pub category_weights: CategoryWeights,
    pub known_contract_boost: u64,
    pub max_contracts: usize,
}

impl Default for HybridScorerConfig {
    fn default() -> Self {
        Self {
            category_weights: CategoryWeights::default(),
            known_contract_boost: 100_000_000_000,
            max_contracts: 1000,
        }
    }
}

/// Hybrid scorer that combines gas data with known contracts
pub struct HybridScorer {
    config: HybridScorerConfig,
    known_contracts: HashMap<[u8; 20], &'static KnownContract>,
}

impl HybridScorer {
    pub fn new(config: HybridScorerConfig) -> Self {
        let known_contracts: HashMap<[u8; 20], &'static KnownContract> = HOT_CONTRACTS
            .iter()
            .map(|c| (c.address, c))
            .collect();

        Self {
            config,
            known_contracts,
        }
    }

    /// Score contracts from gas backfill data combined with known contracts
    #[cfg(feature = "backfill")]
    pub fn score_from_backfill(&self, backfill: &BackfillResult) -> Vec<ScoredContract> {
        let mut scored: HashMap<[u8; 20], ScoredContract> = HashMap::new();

        for stats in &backfill.gas_stats {
            let known = self.known_contracts.get(&stats.address);
            let (name, category, priority_boost, source) = if let Some(kc) = known {
                (
                    Some(kc.name.to_string()),
                    Some(kc.category.to_string()),
                    self.config.known_contract_boost,
                    ContractSource::Both,
                )
            } else {
                (None, None, 0, ContractSource::GasBackfill)
            };

            let category_weight = category
                .as_ref()
                .map(|c| self.config.category_weights.get(c))
                .unwrap_or(1.0);

            let final_score = self.calculate_score(stats.total_gas, priority_boost, category_weight);

            scored.insert(stats.address, ScoredContract {
                address: stats.address,
                name,
                category,
                gas_score: stats.total_gas,
                priority_boost,
                category_weight,
                final_score,
                tx_count: stats.tx_count,
                source,
            });
        }

        for (addr, kc) in &self.known_contracts {
            if !scored.contains_key(addr) {
                let category_weight = self.config.category_weights.get(kc.category);
                let final_score = self.calculate_score(0, self.config.known_contract_boost, category_weight);

                scored.insert(*addr, ScoredContract {
                    address: *addr,
                    name: Some(kc.name.to_string()),
                    category: Some(kc.category.to_string()),
                    gas_score: 0,
                    priority_boost: self.config.known_contract_boost,
                    category_weight,
                    final_score,
                    tx_count: 0,
                    source: ContractSource::KnownList,
                });
            }
        }

        let mut result: Vec<ScoredContract> = scored.into_values().collect();
        result.sort_by(|a, b| b.final_score.cmp(&a.final_score));
        result.truncate(self.config.max_contracts);
        result
    }

    /// Score only from known contracts (no gas data)
    pub fn score_known_only(&self) -> Vec<ScoredContract> {
        let mut result: Vec<ScoredContract> = self
            .known_contracts
            .iter()
            .map(|(addr, kc)| {
                let category_weight = self.config.category_weights.get(kc.category);
                let final_score = self.calculate_score(0, self.config.known_contract_boost, category_weight);

                ScoredContract {
                    address: *addr,
                    name: Some(kc.name.to_string()),
                    category: Some(kc.category.to_string()),
                    gas_score: 0,
                    priority_boost: self.config.known_contract_boost,
                    category_weight,
                    final_score,
                    tx_count: 0,
                    source: ContractSource::KnownList,
                }
            })
            .collect();

        result.sort_by(|a, b| b.final_score.cmp(&a.final_score));
        result.truncate(self.config.max_contracts);
        result
    }

    fn calculate_score(&self, gas_score: u64, priority_boost: u64, category_weight: f64) -> u64 {
        let base = gas_score.saturating_add(priority_boost);
        (base as f64 * category_weight) as u64
    }

    pub fn known_addresses(&self) -> HashSet<[u8; 20]> {
        self.known_contracts.keys().copied().collect()
    }
}

/// Convert scored contracts to ContractInfo for manifest building
impl ScoredContract {
    pub fn to_contract_info(&self) -> ContractInfo {
        ContractInfo {
            address: self.address,
            name: self.name.clone().unwrap_or_else(|| format!("0x{}", hex::encode(&self.address[..6]))),
            category: self.category.clone().unwrap_or_else(|| "unknown".to_string()),
            tx_count: Some(self.tx_count),
            storage_slots: None,
        }
    }
}

mod hex_address {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(address: &[u8; 20], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_str = format!("0x{}", hex::encode(address));
        serializer.serialize_str(&hex_str)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 20], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.strip_prefix("0x").unwrap_or(&s);
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        bytes.try_into().map_err(|_| serde::de::Error::custom("invalid address length"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_weights() {
        let weights = CategoryWeights::default();
        assert!(weights.get("privacy") > weights.get("nft"));
        assert_eq!(weights.get("unknown"), 1.0);
    }

    #[test]
    fn test_score_known_only() {
        let scorer = HybridScorer::new(HybridScorerConfig::default());
        let scored = scorer.score_known_only();

        assert!(!scored.is_empty());
        for s in &scored {
            assert_eq!(s.source, ContractSource::KnownList);
            assert!(s.name.is_some());
        }

        let privacy: Vec<_> = scored.iter().filter(|s| s.category.as_deref() == Some("privacy")).collect();
        assert!(!privacy.is_empty());
        assert!(privacy[0].final_score > scored.last().unwrap().final_score);
    }

    #[test]
    fn test_hybrid_scorer_includes_known() {
        let scorer = HybridScorer::new(HybridScorerConfig::default());
        let known = scorer.known_addresses();
        assert!(known.len() >= 10);
    }
}
