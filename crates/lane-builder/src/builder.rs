//! Hot lane database builder

use inspire_core::HotLaneManifest;

pub struct HotLaneBuilder {
    manifest: HotLaneManifest,
}

impl HotLaneBuilder {
    pub fn new(block_number: u64) -> Self {
        Self {
            manifest: HotLaneManifest::new(block_number),
        }
    }

    pub fn manifest(&self) -> &HotLaneManifest {
        &self.manifest
    }
}
