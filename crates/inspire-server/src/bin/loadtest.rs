//! Load testing binary for Two-Lane PIR server
//!
//! Tests server performance under various load conditions.
//!
//! Usage:
//!   loadtest [OPTIONS] <SERVER_URL>
//!
//! Examples:
//!   loadtest http://localhost:3000                       # Default: 32 clients, 100 queries each
//!   loadtest http://localhost:3000 -c 128 -q 50         # 128 clients, 50 queries each
//!   loadtest http://localhost:3000 --with-reloads       # Trigger reloads during test
//!   loadtest http://localhost:3000 --lane cold          # Test cold lane only

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use inspire_pir::math::GaussianSampler;
use inspire_pir::rlwe::RlweSecretKey;
use inspire_pir::{extract, query as pir_query, ServerCrs};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

#[derive(Parser, Debug)]
#[command(name = "loadtest")]
#[command(about = "Load test the Two-Lane PIR server")]
struct Args {
    /// Server URL (e.g., http://localhost:3000)
    server_url: String,

    /// Number of concurrent clients
    #[arg(short = 'c', long, default_value = "32")]
    clients: usize,

    /// Number of queries per client
    #[arg(short = 'q', long, default_value = "100")]
    queries: usize,

    /// Lane to test (hot or cold)
    #[arg(short = 'l', long, default_value = "hot")]
    lane: String,

    /// Trigger periodic reloads during test
    #[arg(long)]
    with_reloads: bool,

    /// Reload interval in seconds (only with --with-reloads)
    #[arg(long, default_value = "5")]
    reload_interval: u64,

    /// Maximum concurrent requests (limits parallelism)
    #[arg(long, default_value = "64")]
    max_concurrent: usize,

    /// Warmup queries before timing starts
    #[arg(long, default_value = "10")]
    warmup: usize,
}

#[derive(Deserialize)]
struct CrsResponse {
    crs: String,
    entry_count: u64,
    shard_config: inspire_pir::params::ShardConfig,
}

#[derive(Serialize)]
struct QueryRequest {
    query: inspire_pir::ClientQuery,
}

#[derive(Deserialize)]
struct QueryResponse {
    response: inspire_pir::ServerResponse,
}

struct Stats {
    total_queries: AtomicU64,
    successful_queries: AtomicU64,
    failed_queries: AtomicU64,
    total_latency_us: AtomicU64,
    min_latency_us: AtomicU64,
    max_latency_us: AtomicU64,
}

impl Stats {
    fn new() -> Self {
        Self {
            total_queries: AtomicU64::new(0),
            successful_queries: AtomicU64::new(0),
            failed_queries: AtomicU64::new(0),
            total_latency_us: AtomicU64::new(0),
            min_latency_us: AtomicU64::new(u64::MAX),
            max_latency_us: AtomicU64::new(0),
        }
    }

    fn record_success(&self, latency_us: u64) {
        self.total_queries.fetch_add(1, Ordering::Relaxed);
        self.successful_queries.fetch_add(1, Ordering::Relaxed);
        self.total_latency_us.fetch_add(latency_us, Ordering::Relaxed);
        self.min_latency_us.fetch_min(latency_us, Ordering::Relaxed);
        self.max_latency_us.fetch_max(latency_us, Ordering::Relaxed);
    }

    fn record_failure(&self) {
        self.total_queries.fetch_add(1, Ordering::Relaxed);
        self.failed_queries.fetch_add(1, Ordering::Relaxed);
    }

    fn report(&self, duration: Duration) {
        let total = self.total_queries.load(Ordering::Relaxed);
        let success = self.successful_queries.load(Ordering::Relaxed);
        let failed = self.failed_queries.load(Ordering::Relaxed);
        let total_latency = self.total_latency_us.load(Ordering::Relaxed);
        let min_latency = self.min_latency_us.load(Ordering::Relaxed);
        let max_latency = self.max_latency_us.load(Ordering::Relaxed);

        let avg_latency = if success > 0 {
            total_latency / success
        } else {
            0
        };

        let qps = if duration.as_secs_f64() > 0.0 {
            total as f64 / duration.as_secs_f64()
        } else {
            0.0
        };

        println!("\n=== Load Test Results ===");
        println!("Duration:     {:?}", duration);
        println!("Total:        {} queries", total);
        println!("Successful:   {} ({:.1}%)", success, (success as f64 / total as f64) * 100.0);
        println!("Failed:       {}", failed);
        println!("Throughput:   {:.1} queries/sec", qps);
        println!("\nLatency (successful queries):");
        if min_latency != u64::MAX {
            println!("  Min:        {:.2} ms", min_latency as f64 / 1000.0);
            println!("  Avg:        {:.2} ms", avg_latency as f64 / 1000.0);
            println!("  Max:        {:.2} ms", max_latency as f64 / 1000.0);
        } else {
            println!("  (no successful queries)");
        }
    }
}

async fn fetch_crs(client: &Client, url: &str, lane: &str) -> anyhow::Result<(ServerCrs, inspire_pir::params::ShardConfig, u64)> {
    let resp: CrsResponse = client
        .get(format!("{}/crs/{}", url, lane))
        .send()
        .await?
        .json()
        .await?;

    let crs: ServerCrs = serde_json::from_str(&resp.crs)?;
    Ok((crs, resp.shard_config, resp.entry_count))
}

async fn run_client(
    client_id: usize,
    server_url: String,
    lane: String,
    queries: usize,
    crs: Arc<ServerCrs>,
    shard_config: inspire_pir::params::ShardConfig,
    entry_count: u64,
    stats: Arc<Stats>,
    semaphore: Arc<Semaphore>,
) {
    let http = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("HTTP client build");

    for q in 0..queries {
        let _permit = semaphore.acquire().await.unwrap();

        let index = ((client_id * queries + q) as u64) % entry_count;
        let start = Instant::now();

        let result = async {
            let mut sampler = GaussianSampler::new(crs.params.sigma);
            let sk = RlweSecretKey::generate(&crs.params, &mut sampler);

            let (client_state, client_query) =
                pir_query(&crs, index, &shard_config, &sk, &mut sampler)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

            let resp: QueryResponse = http
                .post(format!("{}/query/{}", server_url, lane))
                .json(&QueryRequest { query: client_query })
                .send()
                .await?
                .json()
                .await?;

            let _entry = extract(&crs, &client_state, &resp.response, 32)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            Ok::<_, anyhow::Error>(())
        }
        .await;

        let latency_us = start.elapsed().as_micros() as u64;

        match result {
            Ok(()) => stats.record_success(latency_us),
            Err(e) => {
                stats.record_failure();
                if q == 0 {
                    eprintln!("Client {} query {} failed: {}", client_id, q, e);
                }
            }
        }
    }
}

async fn run_reloader(server_url: String, interval: Duration, stop: Arc<AtomicU64>) {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("HTTP client build");
    let mut reload_count = 0u64;

    while stop.load(Ordering::Relaxed) == 0 {
        tokio::time::sleep(interval).await;

        if stop.load(Ordering::Relaxed) != 0 {
            break;
        }

        match client
            .post(format!("{}/admin/reload", server_url))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                reload_count += 1;
                println!("[Reload {}] Success", reload_count);
            }
            Ok(resp) => {
                println!("[Reload] Failed: {}", resp.status());
            }
            Err(e) => {
                println!("[Reload] Error: {}", e);
            }
        }
    }

    println!("Reloader stopped after {} reloads", reload_count);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    println!("Two-Lane PIR Load Test");
    println!("======================");
    println!("Server:       {}", args.server_url);
    println!("Clients:      {}", args.clients);
    println!("Queries/client: {}", args.queries);
    println!("Lane:         {}", args.lane);
    println!("Max concurrent: {}", args.max_concurrent);
    println!("With reloads: {}", args.with_reloads);
    println!();

    let client = Client::new();

    println!("Fetching CRS for {} lane...", args.lane);
    let (crs, shard_config, entry_count) = fetch_crs(&client, &args.server_url, &args.lane).await?;
    println!("  Entry count: {}", entry_count);
    println!("  Ring dim: {}", crs.params.ring_dim);

    let crs = Arc::new(crs);
    let stats = Arc::new(Stats::new());
    let semaphore = Arc::new(Semaphore::new(args.max_concurrent));

    if args.warmup > 0 {
        println!("\nWarmup: {} queries...", args.warmup);
        let warmup_stats = Arc::new(Stats::new());
        let mut warmup_handles = vec![];

        for i in 0..args.warmup {
            let crs = crs.clone();
            let shard_config = shard_config.clone();
            let stats = warmup_stats.clone();
            let url = args.server_url.clone();
            let lane = args.lane.clone();
            let sem = semaphore.clone();

            warmup_handles.push(tokio::spawn(async move {
                run_client(
                    i,
                    url,
                    lane,
                    1,
                    crs,
                    shard_config,
                    entry_count,
                    stats,
                    sem,
                )
                .await;
            }));
        }

        for h in warmup_handles {
            let _ = h.await;
        }

        let warmup_success = warmup_stats.successful_queries.load(Ordering::Relaxed);
        println!("Warmup complete: {}/{} successful", warmup_success, args.warmup);
    }

    let stop_reloader = Arc::new(AtomicU64::new(0));
    let reload_handle = if args.with_reloads {
        let url = args.server_url.clone();
        let interval = Duration::from_secs(args.reload_interval);
        let stop = stop_reloader.clone();
        Some(tokio::spawn(async move {
            run_reloader(url, interval, stop).await;
        }))
    } else {
        None
    };

    println!("\nStarting load test with {} clients x {} queries = {} total...",
             args.clients, args.queries, args.clients * args.queries);

    let start = Instant::now();
    let mut handles = vec![];

    for client_id in 0..args.clients {
        let crs = crs.clone();
        let shard_config = shard_config.clone();
        let stats = stats.clone();
        let url = args.server_url.clone();
        let lane = args.lane.clone();
        let sem = semaphore.clone();
        let queries = args.queries;

        handles.push(tokio::spawn(async move {
            run_client(
                client_id,
                url,
                lane,
                queries,
                crs,
                shard_config,
                entry_count,
                stats,
                sem,
            )
            .await;
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    let duration = start.elapsed();

    stop_reloader.store(1, Ordering::Relaxed);
    if let Some(h) = reload_handle {
        let _ = h.await;
    }

    stats.report(duration);

    Ok(())
}
