//! Two-lane PIR server implementation

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use inspire_core::TwoLaneConfig;
use tokio::net::TcpListener;

use crate::error::Result;
use crate::routes::{create_admin_router, create_public_router, create_router};
use crate::state::{create_shared_state, SharedState};

/// Rate limiter state for admin endpoints
#[derive(Clone)]
struct RateLimiter {
    last_request: Arc<AtomicU64>,
    min_interval: Duration,
}

impl RateLimiter {
    fn new(min_interval: Duration) -> Self {
        Self {
            last_request: Arc::new(AtomicU64::new(0)),
            min_interval,
        }
    }

    fn check(&self) -> bool {
        let now = Instant::now().elapsed().as_millis() as u64;
        let last = self.last_request.load(Ordering::Relaxed);
        let min_ms = self.min_interval.as_millis() as u64;
        
        if now.saturating_sub(last) >= min_ms {
            self.last_request.store(now, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

async fn rate_limit_middleware(
    State(limiter): State<RateLimiter>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if !limiter.check() {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded. Try again in 1 second.",
        )
            .into_response();
    }
    next.run(request).await
}

use axum::extract::State;

/// Two-lane PIR server
pub struct TwoLaneServer {
    state: SharedState,
    public_addr: SocketAddr,
    admin_addr: Option<SocketAddr>,
}

impl TwoLaneServer {
    /// Create a new server with the given configuration
    pub fn new(config: TwoLaneConfig, public_addr: SocketAddr, admin_addr: Option<SocketAddr>) -> Self {
        let state = create_shared_state(config);
        Self { state, public_addr, admin_addr }
    }

    /// Load both lanes from disk
    pub fn load_lanes(&self) -> Result<()> {
        self.state.load_lanes()
    }

    /// Run the server (single listener mode for backwards compatibility)
    pub async fn run(self) -> Result<()> {
        if self.admin_addr.is_some() {
            self.run_dual().await
        } else {
            self.run_combined().await
        }
    }

    /// Run with combined router (backwards compatible)
    async fn run_combined(self) -> Result<()> {
        let router = create_router(self.state);

        tracing::info!("Starting Two-Lane PIR server on {}", self.public_addr);

        let listener = TcpListener::bind(self.public_addr).await?;
        axum::serve(listener, router)
            .await
            .map_err(|e| crate::error::ServerError::Internal(e.to_string()))?;

        Ok(())
    }

    /// Run with separate public and admin listeners
    async fn run_dual(self) -> Result<()> {
        let admin_addr = self.admin_addr.expect("admin_addr required for dual mode");
        
        let public_router = create_public_router(self.state.clone());
        
        let rate_limiter = RateLimiter::new(Duration::from_secs(1));
        let admin_router = create_admin_router(self.state.clone())
            .layer(middleware::from_fn_with_state(rate_limiter, rate_limit_middleware));

        tracing::info!("Starting public PIR server on {}", self.public_addr);
        tracing::info!("Starting admin server on {} (localhost only)", admin_addr);

        let public_listener = TcpListener::bind(self.public_addr).await?;
        let admin_listener = TcpListener::bind(admin_addr).await?;

        let public_handle = tokio::spawn(async move {
            axum::serve(public_listener, public_router).await
        });

        let admin_handle = tokio::spawn(async move {
            axum::serve(admin_listener, admin_router).await
        });

        tokio::select! {
            res = public_handle => {
                res.map_err(|e| crate::error::ServerError::Internal(e.to_string()))?
                   .map_err(|e| crate::error::ServerError::Internal(e.to_string()))?;
            }
            res = admin_handle => {
                res.map_err(|e| crate::error::ServerError::Internal(e.to_string()))?
                   .map_err(|e| crate::error::ServerError::Internal(e.to_string()))?;
            }
        }

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
    public_addr: SocketAddr,
    admin_addr: Option<SocketAddr>,
    load_lanes: bool,
}

impl ServerBuilder {
    pub fn new(config: TwoLaneConfig) -> Self {
        Self {
            config,
            public_addr: ([127, 0, 0, 1], 3000).into(),
            admin_addr: None,
            load_lanes: true,
        }
    }

    pub fn addr(mut self, addr: SocketAddr) -> Self {
        self.public_addr = addr;
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.public_addr = ([0, 0, 0, 0], port).into();
        self
    }

    /// Set admin port (binds to 127.0.0.1 only for security)
    /// 
    /// When set, admin endpoints (/admin/*) are served on a separate
    /// listener bound to localhost, providing network isolation.
    pub fn admin_port(mut self, port: u16) -> Self {
        self.admin_addr = Some(([127, 0, 0, 1], port).into());
        self
    }

    /// Skip loading lanes on build (useful for testing)
    pub fn skip_load(mut self) -> Self {
        self.load_lanes = false;
        self
    }

    pub fn build(self) -> Result<TwoLaneServer> {
        let server = TwoLaneServer::new(self.config, self.public_addr, self.admin_addr);

        if self.load_lanes {
            server.load_lanes()?;
        }

        Ok(server)
    }
}
