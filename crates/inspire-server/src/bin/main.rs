//! inspire-server binary: Two-lane PIR server

use std::path::PathBuf;

use inspire_core::TwoLaneConfig;
use inspire_server::ServerBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args: Vec<String> = std::env::args().collect();
    
    let config_path = args.get(1).map(PathBuf::from);
    let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3000);

    let config = if let Some(path) = config_path {
        TwoLaneConfig::load(&path)?
    } else {
        TwoLaneConfig::from_base_dir("./pir-data")
    };

    let server = ServerBuilder::new(config)
        .port(port)
        .build()
        .await?;

    tracing::info!("Server ready on port {}", port);
    server.run().await?;

    Ok(())
}
