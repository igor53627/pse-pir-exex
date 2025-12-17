//! Reload client for triggering server database updates

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadResult {
    pub old_block_number: Option<u64>,
    pub new_block_number: Option<u64>,
    pub reload_duration_ms: u64,
    pub hot_loaded: bool,
    pub cold_loaded: bool,
    pub mmap_mode: bool,
}

#[derive(Debug, Clone)]
pub struct ReloadClient {
    client: reqwest::Client,
    server_url: String,
}

impl ReloadClient {
    pub fn new(server_url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");
        
        Self {
            client,
            server_url: server_url.into(),
        }
    }

    pub async fn reload(&self) -> anyhow::Result<ReloadResult> {
        let url = format!("{}/admin/reload", self.server_url);
        
        let response = self
            .client
            .post(&url)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Reload failed with status {}: {}", status, body);
        }

        let result: ReloadResult = response.json().await?;
        Ok(result)
    }

    pub async fn health(&self) -> anyhow::Result<bool> {
        let url = format!("{}/health", self.server_url);
        
        let response = self.client.get(&url).send().await?;
        Ok(response.status().is_success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reload_client_new() {
        let client = ReloadClient::new("http://localhost:3000");
        assert_eq!(client.server_url, "http://localhost:3000");
    }
}
