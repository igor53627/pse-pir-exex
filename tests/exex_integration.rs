//! Integration tests for ExEx lane updater
//!
//! Tests the reload client and debouncing logic without a full Reth node.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use lane_builder::ReloadClient;
use lane_builder::reload::ReloadResult;

mod mock_server {
    use axum::{extract::State, routing::post, Router};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    #[derive(Clone)]
    pub struct MockState {
        pub reload_count: Arc<AtomicU64>,
        pub current_block: Arc<AtomicU64>,
    }

    pub async fn spawn_mock_server(port: u16) -> (String, MockState) {
        let state = MockState {
            reload_count: Arc::new(AtomicU64::new(0)),
            current_block: Arc::new(AtomicU64::new(0)),
        };

        let app = Router::new()
            .route("/admin/reload", post(handle_reload))
            .route("/health", axum::routing::get(|| async { "ok" }))
            .with_state(state.clone());

        let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
            .await
            .unwrap();
        let url = format!("http://127.0.0.1:{}", port);

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        (url, state)
    }

    async fn handle_reload(
        State(state): State<MockState>,
    ) -> axum::Json<lane_builder::reload::ReloadResult> {
        let count = state.reload_count.fetch_add(1, Ordering::SeqCst);
        let new_block = state.current_block.fetch_add(1, Ordering::SeqCst) + 1;

        axum::Json(lane_builder::reload::ReloadResult {
            old_block_number: if count == 0 { None } else { Some(new_block - 1) },
            new_block_number: Some(new_block),
            reload_duration_ms: 5,
            hot_loaded: true,
            cold_loaded: true,
            mmap_mode: true,
        })
    }

    use std::time::Duration;
}

#[tokio::test]
async fn test_reload_client_basic() {
    let (url, state) = mock_server::spawn_mock_server(19001).await;
    let client = ReloadClient::new(&url);

    let result = client.reload().await.expect("reload should succeed");
    assert!(result.hot_loaded);
    assert!(result.cold_loaded);
    assert_eq!(result.new_block_number, Some(1));
    assert_eq!(state.reload_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_reload_client_multiple_calls() {
    let (url, state) = mock_server::spawn_mock_server(19002).await;
    let client = ReloadClient::new(&url);

    for i in 1..=5 {
        let result = client.reload().await.expect("reload should succeed");
        assert_eq!(result.new_block_number, Some(i));
    }

    assert_eq!(state.reload_count.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn test_health_check() {
    let (url, _state) = mock_server::spawn_mock_server(19003).await;
    let client = ReloadClient::new(&url);

    let healthy = client.health().await.expect("health check should succeed");
    assert!(healthy);
}

#[tokio::test]
async fn test_reload_client_invalid_url() {
    let client = ReloadClient::new("http://127.0.0.1:19999");

    let result = client.reload().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_debouncing_simulation() {
    let (url, _state) = mock_server::spawn_mock_server(19004).await;
    let client = ReloadClient::new(&url);
    let debounce = Duration::from_millis(100);

    let mut last_reload = std::time::Instant::now() - debounce;
    let reload_count = Arc::new(AtomicU64::new(0));

    for block in 1..=10 {
        let should_reload = last_reload.elapsed() >= debounce;

        if should_reload {
            let result = client.reload().await.expect("reload should succeed");
            reload_count.fetch_add(1, Ordering::SeqCst);
            last_reload = std::time::Instant::now();
            tracing::info!(block, new_block = ?result.new_block_number, "Reloaded");
        } else {
            tracing::info!(block, "Skipped due to debounce");
        }

        tokio::time::sleep(Duration::from_millis(30)).await;
    }

    let actual_reloads = reload_count.load(Ordering::SeqCst);
    assert!(
        actual_reloads >= 2 && actual_reloads <= 4,
        "Expected 2-4 reloads with debouncing, got {}",
        actual_reloads
    );
}

#[tokio::test]
async fn test_concurrent_reloads() {
    let (url, state) = mock_server::spawn_mock_server(19005).await;
    let client = Arc::new(ReloadClient::new(&url));

    let mut handles = vec![];
    for _ in 0..10 {
        let c = client.clone();
        handles.push(tokio::spawn(async move {
            c.reload().await
        }));
    }

    let mut successes = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            successes += 1;
        }
    }

    assert_eq!(successes, 10);
    assert_eq!(state.reload_count.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_reload_result_fields() {
    let (url, _state) = mock_server::spawn_mock_server(19006).await;
    let client = ReloadClient::new(&url);

    let first = client.reload().await.unwrap();
    assert_eq!(first.old_block_number, None);
    assert_eq!(first.new_block_number, Some(1));
    assert!(first.mmap_mode);

    let second = client.reload().await.unwrap();
    assert_eq!(second.old_block_number, Some(1));
    assert_eq!(second.new_block_number, Some(2));
}
