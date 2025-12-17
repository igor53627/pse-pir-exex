//! inspire-server: Two-lane PIR server
//!
//! Serves PIR queries for both hot and cold lanes, routing based on
//! the lane specified in the request.

pub mod server;
pub mod state;
pub mod routes;
pub mod error;

pub use server::{TwoLaneServer, ServerBuilder};
pub use state::{ServerState, DbSnapshot, SharedState, LaneStats, ReloadResult, LaneData, LaneDatabase};
pub use error::ServerError;
