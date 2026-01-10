//! inspire-client binary: Two-lane PIR client CLI

use std::path::PathBuf;

use inspire_client::client::ClientBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: {} <server_url> <contract_address> [slot] [--switched]", args[0]);
        eprintln!(
            "Example: {} http://localhost:3000 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48 0x00 --switched",
            args[0]
        );
        std::process::exit(1);
    }

    let server_url = &args[1];
    let contract_hex = &args[2];
    let mut slot_hex = "0x00";
    let mut use_switched = false;
    for arg in args.iter().skip(3) {
        if arg == "--switched" {
            use_switched = true;
        } else {
            slot_hex = arg;
        }
    }

    let contract = parse_address(contract_hex)?;
    let slot = parse_slot(slot_hex)?;

    let manifest_path = PathBuf::from("./pir-data/hot/manifest.json");

    let mut client = ClientBuilder::new(server_url)
        .manifest(&manifest_path)
        .switched_query(use_switched)
        .build()?;

    let lane = client.get_lane(&contract);
    tracing::info!(
        contract = contract_hex,
        lane = %lane,
        "Query will be routed to {} lane",
        lane
    );

    client.init().await?;

    let _result = client.query(contract, slot).await?;

    Ok(())
}

fn parse_address(hex: &str) -> anyhow::Result<[u8; 20]> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    let bytes = hex::decode(hex)?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid address length"))
}

fn parse_slot(hex: &str) -> anyhow::Result<[u8; 32]> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    let mut bytes = hex::decode(hex)?;
    if bytes.len() > 32 {
        return Err(anyhow::anyhow!("Slot too long"));
    }
    bytes.resize(32, 0);
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid slot length"))
}
