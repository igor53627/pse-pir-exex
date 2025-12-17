//! inspire-core: Core types and routing logic for Two-Lane InsPIRe PIR
//!
//! This crate defines the foundational types for the two-lane architecture:
//! - Hot Lane: ~1M entries from top ~1000 contracts (~10 KB queries)
//! - Cold Lane: ~2.7B entries for everything else (~500 KB queries)

mod lane;
mod config;
mod manifest;
mod routing;
mod error;

pub use lane::Lane;
pub use config::TwoLaneConfig;
pub use manifest::{HotLaneManifest, HotContract};
pub use routing::{LaneRouter, QueryTarget, RoutedQuery};
pub use error::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// 20-byte Ethereum address
pub type Address = [u8; 20];

/// 32-byte storage slot key
pub type StorageKey = [u8; 32];

/// 32-byte storage value (entry size)
pub type StorageValue = [u8; 32];

/// Constants for the two-lane architecture
pub mod constants {
    /// Entry size in bytes (Ethereum storage slot)
    pub const ENTRY_SIZE: usize = 32;
    
    /// Target hot lane size (~1M entries)
    pub const HOT_LANE_TARGET_ENTRIES: u64 = 1_000_000;
    
    /// Approximate number of top contracts in hot lane
    pub const HOT_LANE_CONTRACT_COUNT: usize = 1_000;
    
    /// Expected hot lane query size in bytes
    pub const HOT_LANE_QUERY_SIZE: usize = 10_000;
    
    /// Expected cold lane query size in bytes
    pub const COLD_LANE_QUERY_SIZE: usize = 500_000;
}
