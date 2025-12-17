//! Known hot lane contracts
//!
//! Curated list of top Ethereum contracts for the hot lane.

use inspire_core::Address;

pub struct KnownContract {
    pub address: Address,
    pub name: &'static str,
    pub category: &'static str,
}

pub const HOT_CONTRACTS: &[KnownContract] = &[
    KnownContract {
        address: hex_literal("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        name: "USDC",
        category: "token",
    },
    KnownContract {
        address: hex_literal("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
        name: "WETH",
        category: "token",
    },
    KnownContract {
        address: hex_literal("0xdAC17F958D2ee523a2206206994597C13D831ec7"),
        name: "USDT",
        category: "token",
    },
    KnownContract {
        address: hex_literal("0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984"),
        name: "UNI",
        category: "token",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usdc_address() {
        assert_eq!(HOT_CONTRACTS[0].name, "USDC");
        assert_eq!(HOT_CONTRACTS[0].address[0], 0xa0);
    }
}
