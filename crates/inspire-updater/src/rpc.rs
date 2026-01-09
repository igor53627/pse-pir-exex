use alloy_primitives::{Address, B256, U256, U64};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::BlockNumberOrTag;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::Path;

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

/// Response from ubt_getRoot (ubt-exex format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UbtRootResult {
    #[serde(rename = "blockNumber")]
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

/// Params for ubt_exportState
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UbtExportStateParams {
    pub output_path: String,
    #[serde(default)]
    pub chain_id: Option<u64>,
}

/// Result from ubt_exportState
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UbtExportStateResult {
    #[serde(rename = "blockNumber")]
    pub block_number: u64,
    #[serde(rename = "blockHash")]
    pub block_hash: B256,
    pub root: B256,
    #[serde(rename = "entryCount")]
    pub entry_count: u64,
    #[serde(rename = "stemCount")]
    pub stem_count: u64,
    #[serde(rename = "stateFile")]
    pub state_file: String,
    #[serde(rename = "stemIndexFile")]
    pub stem_index_file: String,
}

/// Params for ubt_getStateDelta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UbtStateDeltaParams {
    pub from_block: u64,
    pub to_block: u64,
    pub output_path: String,
    #[serde(default)]
    pub chain_id: Option<u64>,
}

/// Result from ubt_getStateDelta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UbtStateDeltaResult {
    #[serde(rename = "fromBlock")]
    pub from_block: u64,
    #[serde(rename = "toBlock")]
    pub to_block: u64,
    #[serde(rename = "headBlock")]
    pub head_block: u64,
    #[serde(rename = "entryCount")]
    pub entry_count: u64,
    #[serde(rename = "deltaFile")]
    pub delta_file: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateRpcMode {
    LegacyPir,
    UbtExex,
}

/// Client for ethrex RPC
pub struct EthrexClient {
    chain: RpcClient,
    state: RpcClient,
    state_mode: StateRpcMode,
}

impl EthrexClient {
    pub async fn new(
        rpc_url: &str,
        admin_url: Option<String>,
        ubt_rpc_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let chain = ClientBuilder::default().connect(rpc_url).await?;

        let (state_url, state_mode) = if let Some(url) = ubt_rpc_url {
            (url, StateRpcMode::UbtExex)
        } else if let Some(url) = admin_url {
            (url, StateRpcMode::LegacyPir)
        } else {
            (rpc_url.to_string(), StateRpcMode::LegacyPir)
        };

        let state = if state_url == rpc_url {
            chain.clone()
        } else {
            ClientBuilder::default().connect(&state_url).await?
        };

        Ok(Self {
            chain,
            state,
            state_mode,
        })
    }

    pub fn state_mode(&self) -> StateRpcMode {
        self.state_mode
    }

    /// Raw JSON-RPC call
    async fn rpc_call_state<Params, Resp>(
        &self,
        method: &str,
        params: Params,
    ) -> anyhow::Result<Resp>
    where
        Params: Serialize + Clone + std::fmt::Debug + Send + Sync + Unpin + 'static,
        Resp: DeserializeOwned + std::fmt::Debug + Send + Sync + Unpin + 'static,
    {
        Ok(self.state.request(method.to_string(), params).await?)
    }

    async fn rpc_call_chain<Params, Resp>(
        &self,
        method: &str,
        params: Params,
    ) -> anyhow::Result<Resp>
    where
        Params: Serialize + Clone + std::fmt::Debug + Send + Sync + Unpin + 'static,
        Resp: DeserializeOwned + std::fmt::Debug + Send + Sync + Unpin + 'static,
    {
        Ok(self.chain.request(method.to_string(), params).await?)
    }

    /// Get current block number
    pub async fn block_number(&self) -> anyhow::Result<u64> {
        let block: U64 = self.chain.request_noparams("eth_blockNumber").await?;
        Ok(block.to::<u64>())
    }

    /// Get head block number (ubt_getRoot for ubt-exex mode)
    pub async fn head_block(&self) -> anyhow::Result<u64> {
        match self.state_mode {
            StateRpcMode::LegacyPir => self.block_number().await,
            StateRpcMode::UbtExex => Ok(self.ubt_get_root(0).await?.block_number),
        }
    }

    /// Get storage at specific slot
    pub async fn get_storage_at(
        &self,
        address: Address,
        slot: B256,
        block: BlockNumberOrTag,
    ) -> anyhow::Result<U256> {
        Ok(self
            .rpc_call_chain("eth_getStorageAt", (address, slot, block))
            .await?)
    }

    /// Get UBT root hash for block (public endpoint)
    /// Note: ubt_getRoot returns just the root hash as a hex string
    pub async fn ubt_get_root(&self, block: u64) -> anyhow::Result<UbtRootResponse> {
        match self.state_mode {
            StateRpcMode::LegacyPir => {
                let root: B256 = self.rpc_call_state("ubt_getRoot", (block,)).await?;
                Ok(UbtRootResponse {
                    block_number: block,
                    root,
                })
            }
            StateRpcMode::UbtExex => {
                let result: UbtRootResult =
                    self.state.request_noparams("ubt_getRoot").await?;
                Ok(UbtRootResponse {
                    block_number: result.block_number,
                    root: result.root,
                })
            }
        }
    }

    /// Dump storage with pagination
    /// cursor: 52-byte hex (address || slot) or null for first page
    /// limit: max entries per page (up to 10000)
    pub async fn pir_dump_storage(
        &self,
        cursor: Option<&str>,
        limit: u64,
    ) -> anyhow::Result<DumpStorageResponse> {
        if self.state_mode == StateRpcMode::UbtExex {
            anyhow::bail!("pir_dumpStorage is not available in ubt-exex mode");
        }
        let cursor = cursor.map(|value| value.to_string());
        self.rpc_call_state("pir_dumpStorage", (cursor, limit)).await
    }

    /// Get state deltas for block range (max 100 blocks)
    pub async fn pir_get_state_delta(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> anyhow::Result<StateDeltaResponse> {
        if self.state_mode == StateRpcMode::UbtExex {
            anyhow::bail!("pir_getStateDelta is not available in ubt-exex mode");
        }
        self.rpc_call_state("pir_getStateDelta", (from_block, to_block))
            .await
    }

    /// Export full UBT state via ubt-exex (writes state.bin + stem_index.bin)
    pub async fn ubt_export_state(
        &self,
        output_dir: &Path,
        chain_id: u64,
    ) -> anyhow::Result<UbtExportStateResult> {
        if self.state_mode != StateRpcMode::UbtExex {
            anyhow::bail!("ubt_exportState requires ubt-exex mode");
        }
        let params = UbtExportStateParams {
            output_path: output_dir.display().to_string(),
            chain_id: Some(chain_id),
        };
        self.rpc_call_state("ubt_exportState", (params,)).await
    }

    /// Export state deltas via ubt-exex (writes delta file)
    pub async fn ubt_get_state_delta(
        &self,
        from_block: u64,
        to_block: u64,
        output_dir: &Path,
        chain_id: u64,
    ) -> anyhow::Result<UbtStateDeltaResult> {
        if self.state_mode != StateRpcMode::UbtExex {
            anyhow::bail!("ubt_getStateDelta requires ubt-exex mode");
        }
        let params = UbtStateDeltaParams {
            from_block,
            to_block,
            output_path: output_dir.display().to_string(),
            chain_id: Some(chain_id),
        };
        self.rpc_call_state("ubt_getStateDelta", (params,)).await
    }

    /// Iterate all storage entries
    /// Returns an async iterator over all storage entries
    pub async fn dump_all_storage(
        &self,
        limit_per_page: u64,
        mut on_page: impl FnMut(usize, &[StorageEntry]),
    ) -> anyhow::Result<Vec<StorageEntry>> {
        if self.state_mode == StateRpcMode::UbtExex {
            anyhow::bail!("dump_all_storage is not available in ubt-exex mode");
        }
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
