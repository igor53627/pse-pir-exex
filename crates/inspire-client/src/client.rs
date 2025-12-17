//! Two-lane PIR client implementation

use inspire_core::{Address, LaneRouter, StorageKey, StorageValue};

pub struct TwoLaneClient {
    router: LaneRouter,
    _server_url: String,
}

impl TwoLaneClient {
    pub fn new(router: LaneRouter, server_url: String) -> Self {
        Self {
            router,
            _server_url: server_url,
        }
    }

    pub async fn query(&self, _contract: Address, _slot: StorageKey) -> anyhow::Result<StorageValue> {
        let _lane = self.router.route(&_contract);
        todo!("Implement PIR query")
    }
}
