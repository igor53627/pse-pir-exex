//! Known hot lane contracts
//!
//! Curated list of top Ethereum contracts for the hot lane.
//! Includes DeFi protocols, stablecoins, DEXes, bridges, and privacy protocols.

use inspire_core::Address;
use serde::{Deserialize, Serialize};

/// Contract information for hot lane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractInfo {
    #[serde(with = "hex_address")]
    pub address: Address,
    pub name: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_slots: Option<u64>,
}

/// Known contract for compile-time inclusion
pub struct KnownContract {
    pub address: Address,
    pub name: &'static str,
    pub category: &'static str,
}

/// Curated list of top Ethereum contracts
pub const HOT_CONTRACTS: &[KnownContract] = &[
    // Stablecoins
    KnownContract {
        address: hex_literal("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        name: "USDC",
        category: "stablecoin",
    },
    KnownContract {
        address: hex_literal("0xdAC17F958D2ee523a2206206994597C13D831ec7"),
        name: "USDT",
        category: "stablecoin",
    },
    KnownContract {
        address: hex_literal("0x6B175474E89094C44Da98b954EescdeCB5BE3d842"),
        name: "DAI",
        category: "stablecoin",
    },
    // Wrapped tokens
    KnownContract {
        address: hex_literal("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
        name: "WETH",
        category: "token",
    },
    KnownContract {
        address: hex_literal("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599"),
        name: "WBTC",
        category: "token",
    },
    // DEX protocols
    KnownContract {
        address: hex_literal("0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D"),
        name: "Uniswap V2 Router",
        category: "dex",
    },
    KnownContract {
        address: hex_literal("0xE592427A0AEce92De3Edee1F18E0157C05861564"),
        name: "Uniswap V3 Router",
        category: "dex",
    },
    KnownContract {
        address: hex_literal("0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45"),
        name: "Uniswap Universal Router",
        category: "dex",
    },
    // Lending protocols
    KnownContract {
        address: hex_literal("0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"),
        name: "Aave V3 Pool",
        category: "lending",
    },
    KnownContract {
        address: hex_literal("0x3d9819210A31b4961b30EF54bE2aeD79B9c9Cd3B"),
        name: "Compound Comptroller",
        category: "lending",
    },
    // Privacy protocols
    KnownContract {
        address: hex_literal("0x910Cbd523D972eb0a6f4cAe4618aD62622b39DbF"),
        name: "Tornado Cash 0.1 ETH",
        category: "privacy",
    },
    KnownContract {
        address: hex_literal("0xA160cdAB225685dA1d56aa342Ad8841c3b53f291"),
        name: "Tornado Cash 1 ETH",
        category: "privacy",
    },
    KnownContract {
        address: hex_literal("0xD4B88Df4D29F5CedD6857912842cff3b20C8Cfa3"),
        name: "Tornado Cash 10 ETH",
        category: "privacy",
    },
    KnownContract {
        address: hex_literal("0xA0B86991C6218B36C1D19D4A2E9EB0cE3606eB48"),
        name: "Railgun",
        category: "privacy",
    },
    // Bridges
    KnownContract {
        address: hex_literal("0x40ec5B33f54e0E8A33A975908C5BA1c14e5BbbDf"),
        name: "Polygon Bridge",
        category: "bridge",
    },
    KnownContract {
        address: hex_literal("0x99C9fc46f92E8a1c0deC1b1747d010903E884bE1"),
        name: "Optimism Bridge",
        category: "bridge",
    },
    KnownContract {
        address: hex_literal("0x8315177aB297bA92A06054cE80a67Ed4DBd7ed3a"),
        name: "Arbitrum Bridge",
        category: "bridge",
    },
    // Governance tokens
    KnownContract {
        address: hex_literal("0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984"),
        name: "UNI",
        category: "governance",
    },
    KnownContract {
        address: hex_literal("0x7Fc66500c84A76Ad7e9c93437bFc5Ac33E2DDaE9"),
        name: "AAVE",
        category: "governance",
    },
    // NFT marketplaces
    KnownContract {
        address: hex_literal("0x00000000006c3852cbEf3e08E8dF289169EdE581"),
        name: "Seaport",
        category: "nft",
    },
];

const fn hex_literal(s: &str) -> Address {
    let bytes = s.as_bytes();
    let mut result = [0u8; 20];
    let mut i = 2;
    let mut j = 0;
    while j < 20 {
        let high = hex_char(bytes[i]);
        let low = hex_char(bytes[i + 1]);
        result[j] = (high << 4) | low;
        i += 2;
        j += 1;
    }
    result
}

const fn hex_char(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
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
    fn test_usdc_address() {
        assert_eq!(HOT_CONTRACTS[0].name, "USDC");
        assert_eq!(HOT_CONTRACTS[0].address[0], 0xa0);
    }

    #[test]
    fn test_contract_count() {
        assert!(HOT_CONTRACTS.len() >= 20);
    }

    #[test]
    fn test_privacy_contracts_included() {
        let privacy: Vec<_> = HOT_CONTRACTS
            .iter()
            .filter(|c| c.category == "privacy")
            .collect();
        assert!(privacy.len() >= 3);
    }

    #[test]
    fn test_contract_info_serialization() {
        let info = ContractInfo {
            address: [0xabu8; 20],
            name: "Test".into(),
            category: "token".into(),
            tx_count: Some(1000),
            storage_slots: None,
        };
        
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("0xabab"));
        assert!(json.contains("tx_count"));
        assert!(!json.contains("storage_slots"));
    }
}
