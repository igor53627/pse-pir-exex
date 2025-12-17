//! Two-lane PIR server implementation

use inspire_core::{Lane, TwoLaneConfig};

pub struct TwoLaneServer {
    _config: TwoLaneConfig,
}

impl TwoLaneServer {
    pub fn new(config: TwoLaneConfig) -> Self {
        Self { _config: config }
    }

    pub fn handle_query(&self, _lane: Lane, _query: &[u8]) -> Vec<u8> {
        todo!("Implement PIR query handling")
    }
}
