//! lane-builder: Hot lane contract extractor and database builder
//!
//! Extracts top contracts from Ethereum state to build the hot lane database.
//! Uses heuristics based on contract popularity (transaction count, TVL, etc.)

pub mod builder;
pub mod contracts;
pub mod extractor;
pub mod setup;

pub use builder::HotLaneBuilder;
pub use extractor::ContractExtractor;
pub use setup::{TwoLaneSetup, TwoLaneSetupResult, default_params, test_params, load_secret_key};
