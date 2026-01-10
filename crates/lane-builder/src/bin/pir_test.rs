//! pir-test: Test PIR query against a running server
//!
//! Usage:
//!   pir-test --server http://localhost:3001 --lane hot --index 42

use clap::Parser;
use inspire_pir::math::GaussianSampler;
use inspire_pir::params::{InspireVariant, ShardConfig};
use inspire_pir::rlwe::RlweSecretKey;
use inspire_pir::{
    extract_with_variant, query as pir_query, query_switched as pir_query_switched, ServerCrs,
    SwitchedClientQuery,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(about = "Test PIR query against a running server")]
struct Args {
    /// Server URL
    #[arg(long, default_value = "http://localhost:3001")]
    server: String,

    /// Lane to query (hot or cold)
    #[arg(long, default_value = "hot")]
    lane: String,

    /// Index to query
    #[arg(long, default_value = "0")]
    index: u64,

    /// Use switched+seeded query (~75% smaller)
    #[arg(long)]
    switched: bool,
}

#[derive(Debug, Deserialize)]
struct CrsResponse {
    crs: String,
    shard_config: ShardConfig,
}

#[derive(Debug, Serialize)]
struct QueryRequest {
    query: inspire_pir::ClientQuery,
}

#[derive(Debug, Serialize)]
struct SwitchedQueryRequest {
    query: SwitchedClientQuery,
}

#[derive(Debug, Deserialize)]
struct QueryResponse {
    response: inspire_pir::ServerResponse,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = Args::parse();
    let client = Client::new();

    tracing::info!(
        server = %args.server,
        lane = %args.lane,
        index = args.index,
        switched = args.switched,
        "Testing PIR query"
    );

    // Fetch CRS
    tracing::info!("Fetching CRS...");
    let crs_resp: CrsResponse = client
        .get(format!("{}/crs/{}", args.server, args.lane))
        .send()
        .await?
        .json()
        .await?;

    let crs: ServerCrs = serde_json::from_str(&crs_resp.crs)?;
    tracing::info!(ring_dim = crs.params.ring_dim, "CRS loaded");

    // Generate secret key
    let mut sampler = GaussianSampler::new(crs.params.sigma);
    let sk = RlweSecretKey::generate(&crs.params, &mut sampler);
    tracing::info!("Secret key generated");

    // Create query
    let (client_state, full_query, switched_query, endpoint) = if args.switched {
        let (state, query) =
            pir_query_switched(&crs, args.index, &crs_resp.shard_config, &sk, &mut sampler)
                .map_err(|e| anyhow::anyhow!("Query generation failed: {}", e))?;
        (
            state,
            None,
            Some(query),
            format!("{}/query/{}/switched", args.server, args.lane),
        )
    } else {
        let (state, query) =
            pir_query(&crs, args.index, &crs_resp.shard_config, &sk, &mut sampler)
                .map_err(|e| anyhow::anyhow!("Query generation failed: {}", e))?;
        (
            state,
            Some(query),
            None,
            format!("{}/query/{}", args.server, args.lane),
        )
    };
    tracing::info!("Query generated");

    // Send query
    tracing::info!("Sending query to server...");
    let start = std::time::Instant::now();
    let resp: QueryResponse = if let Some(query) = switched_query {
        client
            .post(endpoint)
            .json(&SwitchedQueryRequest { query })
            .send()
            .await?
            .json()
            .await?
    } else {
        client
            .post(endpoint)
            .json(&QueryRequest {
                query: full_query.expect("full query missing"),
            })
            .send()
            .await?
            .json()
            .await?
    };
    let elapsed = start.elapsed();
    tracing::info!(elapsed_ms = elapsed.as_millis(), "Response received");

    // Extract result (32 bytes per entry)
    let variant = InspireVariant::OnePacking;
    let entry_size = 32;
    let entry = extract_with_variant(&crs, &client_state, &resp.response, entry_size, variant)
        .map_err(|e| anyhow::anyhow!("Extraction failed: {}", e))?;

    println!("\nPIR Query Result:");
    println!("  Index: {}", args.index);
    println!("  Entry (hex): {}", hex::encode(&entry));
    println!("  Entry size: {} bytes", entry.len());
    println!("  Round-trip time: {:?}", elapsed);

    Ok(())
}
