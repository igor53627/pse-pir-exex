//! lane-builder: Hot lane contract extractor and database builder
//!
//! Extracts top contracts from Ethereum state to build the hot lane database.
//! Uses heuristics based on contract popularity (transaction count, TVL, etc.)
//!
//! ## Features
//!
//! - `exex`: Enable Reth ExEx integration for real-time lane updates

pub mod builder;
pub mod contracts;
pub mod extractor;
pub mod reload;
pub mod setup;

#[cfg(feature = "exex")]
pub mod exex;

pub use builder::HotLaneBuilder;
pub use extractor::ContractExtractor;
pub use reload::ReloadClient;
pub use setup::{TwoLaneSetup, TwoLaneSetupResult, default_params, test_params, load_secret_key};

#[cfg(feature = "exex")]
pub use exex::{lane_updater_exex, LaneUpdaterConfig};
