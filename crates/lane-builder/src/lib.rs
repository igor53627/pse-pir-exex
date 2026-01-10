//! lane-builder: Hot lane contract extractor and database builder
//!
//! Extracts top contracts from Ethereum state to build the hot lane database.
//! Uses heuristics based on contract popularity (transaction count, TVL, etc.)
//!
//! ## Features
//!
//! - `exex`: Enable Reth ExEx integration for real-time lane updates
//! - `backfill`: Enable gas backfill for data-driven hot lane selection

pub mod builder;
pub mod contracts;
pub mod extractor;
pub mod hybrid_scorer;
pub mod reload;
pub mod setup;

#[cfg(feature = "exex")]
pub mod exex;

#[cfg(feature = "exex")]
pub mod delta_exex;

#[cfg(feature = "backfill")]
pub mod gas_tracker;

#[cfg(feature = "balance")]
pub mod balance_extractor;

pub use builder::HotLaneBuilder;
pub use extractor::ContractExtractor;
pub use hybrid_scorer::{CategoryWeights, HybridScorer, HybridScorerConfig, ScoredContract};
pub use reload::ReloadClient;
pub use setup::{default_params, load_secret_key, test_params, TwoLaneSetup, TwoLaneSetupResult};

#[cfg(feature = "exex")]
pub use exex::{lane_updater_exex, LaneUpdaterConfig};

#[cfg(feature = "exex")]
pub use delta_exex::{delta_export_exex, DeltaExporterConfig};

#[cfg(feature = "backfill")]
pub use gas_tracker::{BackfillConfig, BackfillResult, GasStats, GasTracker};

#[cfg(feature = "balance")]
pub use balance_extractor::{BalanceExtractor, BalanceExtractorConfig};
