//! inspire-server binary: Two-lane PIR server
//!
//! Usage:
//!   inspire-server [config.json] [port] [admin_port]
//!
//! Examples:
//!   inspire-server                           # defaults: ./pir-data, port 3000, no admin isolation
//!   inspire-server config.json 3000          # public on 3000, admin on same port
//!   inspire-server config.json 3000 3001     # public on 0.0.0.0:3000, admin on 127.0.0.1:3001

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
    let admin_port: Option<u16> = args.get(3).and_then(|s| s.parse().ok());

    let config = if let Some(path) = config_path {
        TwoLaneConfig::load(&path)?
    } else {
        TwoLaneConfig::from_base_dir("./pir-data")
    };

    let mut builder = ServerBuilder::new(config).port(port);
    
    if let Some(admin_port) = admin_port {
        builder = builder.admin_port(admin_port);
        tracing::info!("Admin endpoints on 127.0.0.1:{} (rate limited: 1 req/sec)", admin_port);
    }

    let server = builder.build()?;

    tracing::info!("Public server ready on 0.0.0.0:{}", port);
    server.run().await?;

    Ok(())
}
