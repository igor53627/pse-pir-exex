//! Query construction and execution with rotation support

use crate::hint_store::HintStore;
use pir_core::{hint::recover_entry, subset::CompressedQuery, Hint, ENTRY_SIZE};
use serde::{Deserialize, Serialize};

/// PIR client for making private queries
pub struct PirClient {
    /// Local hint store
    pub hints: HintStore,
    /// Query server URL
    pub server_url: String,
    /// HTTP client
    client: reqwest::Client,
    /// Enable hint rotation for multi-query privacy
    pub rotation_enabled: bool,
}

/// Query result
#[derive(Debug)]
pub struct QueryResult {
    pub entry: [u8; ENTRY_SIZE],
    pub query_time_ms: f64,
    pub server_time_ms: f64,
}

/// Server response
#[derive(Debug, Deserialize)]
struct ServerResponse {
    result: String,
    query_time_ms: f64,
}

/// Query request
#[derive(Debug, Serialize)]
struct QueryRequest {
    query: CompressedQuery,
}

impl PirClient {
    /// Create a new PIR client (rotation enabled by default)
    pub fn new(hints: HintStore, server_url: String) -> Self {
        Self {
            hints,
            server_url,
            client: reqwest::Client::new(),
            rotation_enabled: true,
        }
    }

    /// Create without rotation (for testing or when privacy is less critical)
    pub fn without_rotation(hints: HintStore, server_url: String) -> Self {
        Self {
            hints,
            server_url,
            client: reqwest::Client::new(),
            rotation_enabled: false,
        }
    }

    /// Query for a specific database index (uses rotation if enabled)
    pub async fn query(&mut self, target_index: u64) -> anyhow::Result<QueryResult> {
        let start = std::time::Instant::now();
        
        // Find a hint containing the target (with or without rotation)
        let stored_hint = if self.rotation_enabled {
            self.hints
                .find_hint_with_rotation(target_index)
                .ok_or_else(|| anyhow::anyhow!("No hint found for target {}", target_index))?
        } else {
            self.hints
                .find_hint_for_target(target_index)
                .ok_or_else(|| anyhow::anyhow!("No hint found for target {}", target_index))?
        };
        
        // Clone what we need before the borrow ends
        let subset = stored_hint.subset.clone();
        let hint = stored_hint.hint;
        
        // Create compressed query
        let query = CompressedQuery::new(&subset);
        
        // Send to server
        let response: ServerResponse = self
            .client
            .post(format!("{}/query", self.server_url))
            .json(&QueryRequest { query })
            .send()
            .await?
            .json()
            .await?;
        
        // Decode server response
        let server_result: Hint = hex::decode(&response.result)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid response length"))?;
        
        // Recover the entry
        let entry = recover_entry(&server_result, &hint);
        
        let elapsed = start.elapsed();
        
        Ok(QueryResult {
            entry,
            query_time_ms: elapsed.as_secs_f64() * 1000.0,
            server_time_ms: response.query_time_ms,
        })
    }

    /// Query multiple indices (batched, with rotation)
    pub async fn query_batch(&mut self, indices: &[u64]) -> anyhow::Result<Vec<QueryResult>> {
        let mut results = Vec::with_capacity(indices.len());
        
        // TODO: Parallelize queries (need to handle rotation state carefully)
        for &idx in indices {
            results.push(self.query(idx).await?);
        }
        
        Ok(results)
    }

    /// Get privacy statistics
    pub fn privacy_stats(&self, target: u64) -> PrivacyStats {
        let available_hints = self.hints.hint_count_for_target(target);
        let recently_used = self.hints.rotation.hints_to_avoid(target).len();
        
        PrivacyStats {
            available_hints,
            recently_used,
            rotation_enabled: self.rotation_enabled,
        }
    }
}

/// Privacy statistics for a target
#[derive(Debug)]
pub struct PrivacyStats {
    /// How many hints can be used for this target
    pub available_hints: usize,
    /// How many hints were recently used (will be avoided)
    pub recently_used: usize,
    /// Whether rotation is enabled
    pub rotation_enabled: bool,
}
