//! Updater service for syncing PIR database from ethrex node
//!
//! This crate provides a polling service that:
//! 1. Monitors ethrex for new blocks
//! 2. Fetches storage deltas via RPC
//! 3. Updates PIR shard files
//! 4. Triggers PIR server reload
//!
//! ## Usage
//!
//! ```no_run
//! use inspire_updater::{UpdaterConfig, UpdaterService};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let config = UpdaterConfig::default();
//!     let mut service = UpdaterService::new(config).await?;
//!     service.run().await
//! }
//! ```

mod config;
mod delta_writer;
mod rpc;
mod service;
mod state;
mod writer;

pub use config::UpdaterConfig;
pub use delta_writer::RangeDeltaWriter;
pub use rpc::{
    BlockDeltas, DumpStorageResponse, EthrexClient, StateDeltaResponse, StateRpcMode, StorageEntry,
    UbtExportStateResult, UbtRootResponse, UbtStateDeltaResult,
};
pub use service::{ReloadClient, UpdaterService};
pub use state::StateTracker;
pub use writer::ShardWriter;
