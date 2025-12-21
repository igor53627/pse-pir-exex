//! WASM PIR client for browser
//!
//! Simplified single-lane client optimized for hot lane ETH/USDC queries.

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use inspire_pir::{
    query_seeded as pir_query_seeded, extract,
    SeededClientQuery, ServerCrs, ServerResponse,
};
use inspire_pir::math::GaussianSampler;
use inspire_pir::params::ShardConfig;
use inspire_pir::rlwe::RlweSecretKey;
use inspire_core::PIR_PARAMS_VERSION;

use crate::console_log;
use crate::error::PirError;
use crate::transport::HttpClient;

#[derive(Deserialize)]
struct ServerInfo {
    pir_params_version: u16,
}

#[derive(Deserialize)]
struct CrsResponse {
    crs: String,
    entry_count: u64,
    shard_config: ShardConfig,
}

#[derive(Serialize)]
struct SeededQueryRequest {
    query: SeededClientQuery,
}

#[derive(Deserialize)]
struct QueryResponse {
    response: ServerResponse,
}

struct ClientInner {
    http: HttpClient,
    crs: ServerCrs,
    secret_key: RlweSecretKey,
    entry_count: u64,
    shard_config: ShardConfig,
    lane: String,
}

#[wasm_bindgen]
pub struct PirClient {
    inner: Option<ClientInner>,
    server_url: String,
}

#[wasm_bindgen]
impl PirClient {
    #[wasm_bindgen(constructor)]
    pub fn new(server_url: String) -> PirClient {
        PirClient {
            inner: None,
            server_url,
        }
    }

    #[wasm_bindgen]
    pub async fn init(&mut self, lane: &str) -> Result<(), JsValue> {
        let http = HttpClient::new(self.server_url.clone());
        
        console_log!("Checking server PIR params version...");
        let info: ServerInfo = http
            .get("/info")
            .await
            .map_err(PirError::from)?;

        if info.pir_params_version != PIR_PARAMS_VERSION {
            return Err(PirError::VersionMismatch {
                client: PIR_PARAMS_VERSION,
                server: info.pir_params_version,
            }.into());
        }

        console_log!("Version check passed: v{}", PIR_PARAMS_VERSION);
        console_log!("Fetching CRS for lane: {}", lane);
        
        let crs_resp: CrsResponse = http
            .get(&format!("/crs/{}", lane))
            .await
            .map_err(PirError::from)?;
        
        let crs: ServerCrs = serde_json::from_str(&crs_resp.crs)
            .map_err(|e| PirError::Serialization(e.to_string()))?;
        
        console_log!("Generating secret key...");
        let mut sampler = GaussianSampler::new(crs.params.sigma);
        let secret_key = RlweSecretKey::generate(&crs.params, &mut sampler);
        
        console_log!("Client initialized: {} entries", crs_resp.entry_count);
        
        self.inner = Some(ClientInner {
            http,
            crs,
            secret_key,
            entry_count: crs_resp.entry_count,
            shard_config: crs_resp.shard_config,
            lane: lane.to_string(),
        });
        
        Ok(())
    }

    #[wasm_bindgen]
    pub fn entry_count(&self) -> Result<u64, JsValue> {
        let inner = self.inner.as_ref().ok_or(PirError::NotInitialized)?;
        Ok(inner.entry_count)
    }

    #[wasm_bindgen]
    pub async fn query(&self, index: u64) -> Result<Vec<u8>, JsValue> {
        let inner = self.inner.as_ref().ok_or(PirError::NotInitialized)?;
        
        if index >= inner.entry_count {
            return Err(PirError::IndexOutOfBounds(index).into());
        }
        
        console_log!("Building PIR query for index {}", index);
        
        let mut sampler = GaussianSampler::new(inner.crs.params.sigma);
        let (client_state, seeded_query) = pir_query_seeded(
            &inner.crs,
            index,
            &inner.shard_config,
            &inner.secret_key,
            &mut sampler,
        ).map_err(|e| PirError::Pir(e.to_string()))?;
        
        console_log!("Sending seeded query...");
        
        let response: QueryResponse = inner.http
            .post_json(&format!("/query/{}/seeded", inner.lane), &SeededQueryRequest { query: seeded_query })
            .await
            .map_err(PirError::from)?;
        
        console_log!("Extracting result...");
        
        let entry = extract(
            &inner.crs,
            &client_state,
            &response.response,
            64,
        ).map_err(|e| PirError::Pir(e.to_string()))?;
        
        Ok(entry)
    }

    #[wasm_bindgen]
    pub async fn query_binary(&self, index: u64) -> Result<Vec<u8>, JsValue> {
        let inner = self.inner.as_ref().ok_or(PirError::NotInitialized)?;
        
        if index >= inner.entry_count {
            return Err(PirError::IndexOutOfBounds(index).into());
        }
        
        let mut sampler = GaussianSampler::new(inner.crs.params.sigma);
        let (client_state, seeded_query) = pir_query_seeded(
            &inner.crs,
            index,
            &inner.shard_config,
            &inner.secret_key,
            &mut sampler,
        ).map_err(|e| PirError::Pir(e.to_string()))?;
        
        let bytes = inner.http
            .post_json_binary(&format!("/query/{}/seeded/binary", inner.lane), &SeededQueryRequest { query: seeded_query })
            .await
            .map_err(PirError::from)?;
        
        let response = ServerResponse::from_binary(&bytes)
            .map_err(|e| PirError::Pir(e.to_string()))?;
        
        let entry = extract(
            &inner.crs,
            &client_state,
            &response,
            64,
        ).map_err(|e| PirError::Pir(e.to_string()))?;
        
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_client_creation() {
        let client = PirClient::new("http://localhost:3000".to_string());
        assert!(client.inner.is_none());
    }
}
