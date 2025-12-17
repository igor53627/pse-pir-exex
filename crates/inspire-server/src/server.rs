//! Two-lane PIR server implementation

use std::net::SocketAddr;

use inspire_core::TwoLaneConfig;
use tokio::net::TcpListener;

use crate::error::Result;
use crate::routes::create_router;
use crate::state::{create_shared_state, SharedState};

/// Two-lane PIR server
pub struct TwoLaneServer {
    state: SharedState,
    addr: SocketAddr,
}

impl TwoLaneServer {
    /// Create a new server with the given configuration
    pub fn new(config: TwoLaneConfig, addr: SocketAddr) -> Self {
        let state = create_shared_state(config);
        Self { state, addr }
    }

    /// Load both lanes from disk
    pub fn load_lanes(&self) -> Result<()> {
        self.state.load_lanes()
    }

    /// Run the server
    pub async fn run(self) -> Result<()> {
        let router = create_router(self.state);

        tracing::info!("Starting Two-Lane PIR server on {}", self.addr);

        let listener = TcpListener::bind(self.addr).await?;
        axum::serve(listener, router)
            .await
            .map_err(|e| crate::error::ServerError::Internal(e.to_string()))?;

        Ok(())
    }

    /// Get the server state for testing
    pub fn state(&self) -> SharedState {
        self.state.clone()
    }
}

/// Builder for TwoLaneServer
pub struct ServerBuilder {
    config: TwoLaneConfig,
    addr: SocketAddr,
    load_hot: bool,
    load_cold: bool,
}

impl ServerBuilder {
    pub fn new(config: TwoLaneConfig) -> Self {
        Self {
            config,
            addr: ([127, 0, 0, 1], 3000).into(),
            load_hot: true,
            load_cold: true,
        }
    }

    pub fn addr(mut self, addr: SocketAddr) -> Self {
        self.addr = addr;
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.addr = ([0, 0, 0, 0], port).into();
        self
    }

    pub fn hot_only(mut self) -> Self {
        self.load_hot = true;
        self.load_cold = false;
        self
    }

    pub fn cold_only(mut self) -> Self {
        self.load_hot = false;
        self.load_cold = true;
        self
    }

    pub fn build(self) -> Result<TwoLaneServer> {
        let server = TwoLaneServer::new(self.config, self.addr);

        if self.load_hot || self.load_cold {
            server.load_lanes()?;
        }

        Ok(server)
    }
}
