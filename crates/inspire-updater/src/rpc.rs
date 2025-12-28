use alloy_primitives::{Address, B256, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::BlockNumberOrTag;
use serde::{Deserialize, Serialize};

/// Storage entry from pir_dumpStorage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageEntry {
    pub address: Address,
    pub slot: B256,
    pub value: U256,
}

/// Response from pir_dumpStorage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DumpStorageResponse {
    pub entries: Vec<StorageEntry>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// Response from ubt_getRoot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UbtRootResponse {
    pub block_number: u64,
    pub root: B256,
}

/// Block deltas from pir_getStateDelta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDeltas {
    pub block_number: u64,
    pub deltas: Vec<StorageEntry>,
}

/// Response from pir_getStateDelta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDeltaResponse {
    pub from_block: u64,
    pub to_block: u64,
    pub blocks: Vec<BlockDeltas>,
    pub total_deltas: u64,
}

/// Client for ethrex RPC
pub struct EthrexClient {
    rpc_url: String,
    http: reqwest::Client,
}

impl EthrexClient {
    pub async fn new(rpc_url: &str, _admin_url: Option<String>) -> anyhow::Result<Self> {
        let provider = ProviderBuilder::new().connect(rpc_url).await?;
        let _ = provider.get_block_number().await?;
        Ok(Self {
            rpc_url: rpc_url.to_string(),
            http: reqwest::Client::new(),
        })
    }

    async fn provider(&self) -> anyhow::Result<impl Provider> {
        Ok(ProviderBuilder::new().connect(&self.rpc_url).await?)
    }

    /// Raw JSON-RPC call
    async fn rpc_call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<T> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let resp = self
            .http
            .post(&self.rpc_url)
            .json(&body)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        if let Some(error) = resp.get("error") {
            anyhow::bail!("RPC error: {}", error);
        }

        let result = resp
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("Missing result in RPC response"))?;

        Ok(serde_json::from_value(result.clone())?)
    }

    /// Get current block number
    pub async fn block_number(&self) -> anyhow::Result<u64> {
        Ok(self.provider().await?.get_block_number().await?)
    }

    /// Get storage at specific slot
    pub async fn get_storage_at(
        &self,
        address: Address,
        slot: B256,
        block: BlockNumberOrTag,
    ) -> anyhow::Result<U256> {
        Ok(self
            .provider()
            .await?
            .get_storage_at(address, slot.into())
            .block_id(block.into())
            .await?)
    }

    /// Get UBT root hash for block (public endpoint)
    pub async fn ubt_get_root(&self, block: u64) -> anyhow::Result<UbtRootResponse> {
        self.rpc_call("ubt_getRoot", serde_json::json!([block]))
            .await
    }

    /// Dump storage with pagination
    /// cursor: 52-byte hex (address || slot) or null for first page
    /// limit: max entries per page (up to 10000)
    pub async fn pir_dump_storage(
        &self,
        cursor: Option<&str>,
        limit: u64,
    ) -> anyhow::Result<DumpStorageResponse> {
        self.rpc_call("pir_dumpStorage", serde_json::json!([cursor, limit]))
            .await
    }

    /// Get state deltas for block range (max 100 blocks)
    pub async fn pir_get_state_delta(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> anyhow::Result<StateDeltaResponse> {
        self.rpc_call("pir_getStateDelta", serde_json::json!([from_block, to_block]))
            .await
    }

    /// Iterate all storage entries
    /// Returns an async iterator over all storage entries
    pub async fn dump_all_storage(
        &self,
        limit_per_page: u64,
        mut on_page: impl FnMut(usize, &[StorageEntry]),
    ) -> anyhow::Result<Vec<StorageEntry>> {
        let mut all_entries = Vec::new();
        let mut cursor: Option<String> = None;
        let mut page = 0;

        loop {
            let resp = self
                .pir_dump_storage(cursor.as_deref(), limit_per_page)
                .await?;

            on_page(page, &resp.entries);
            all_entries.extend(resp.entries);

            if !resp.has_more {
                break;
            }

            cursor = resp.next_cursor;
            page += 1;
        }

        Ok(all_entries)
    }
}
