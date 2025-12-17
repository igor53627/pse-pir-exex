//! End-to-end integration tests for Two-Lane PIR
//!
//! Tests the full pipeline: lane-builder -> server -> client

use inspire_core::{HotLaneManifest, Lane, LaneRouter, TwoLaneConfig};
use inspire_client::TwoLaneClient;
use lane_builder::{HotLaneBuilder, TwoLaneSetup, test_params};


/// Test that lane routing works correctly
#[test]
fn test_lane_routing_e2e() {
    let mut manifest = HotLaneManifest::new(12345);
    
    let usdc: [u8; 20] = [0xa0, 0xb8, 0x69, 0x91, 0xc6, 0x21, 0x8b, 0x36, 0xc1, 0xd1,
                          0x9d, 0x4a, 0x2e, 0x9e, 0xb0, 0xce, 0x36, 0x06, 0xeb, 0x48];
    let weth: [u8; 20] = [0xc0, 0x2a, 0xaa, 0x39, 0xb2, 0x23, 0xfe, 0x8d, 0x0a, 0x0e,
                          0x5c, 0x4f, 0x27, 0xea, 0xd9, 0x08, 0x3c, 0x75, 0x6c, 0xc2];
    let unknown: [u8; 20] = [0x99; 20];
    
    manifest.add_contract(usdc, "USDC".into(), 1000, "stablecoin".into());
    manifest.add_contract(weth, "WETH".into(), 500, "token".into());
    
    let router = LaneRouter::new(manifest);
    
    assert_eq!(router.route(&usdc), Lane::Hot);
    assert_eq!(router.route(&weth), Lane::Hot);
    assert_eq!(router.route(&unknown), Lane::Cold);
    
    assert!(router.is_hot(&usdc));
    assert!(!router.is_hot(&unknown));
}

/// Test manifest building with the lane builder
#[test]
fn test_manifest_building_e2e() {
    let temp_dir = std::env::temp_dir().join("pir-test-e2e");
    let _ = std::fs::remove_dir_all(&temp_dir);
    
    let manifest = HotLaneBuilder::new(&temp_dir)
        .at_block(20_000_000)
        .load_known_contracts()
        .max_contracts(100)
        .max_entries(100_000)
        .build()
        .unwrap();
    
    assert!(manifest.contract_count() > 0);
    assert!(manifest.contract_count() <= 100);
    assert!(manifest.total_entries <= 100_000);
    assert_eq!(manifest.block_number, 20_000_000);
    
    let manifest_path = temp_dir.join("manifest.json");
    assert!(manifest_path.exists());
    
    let loaded = HotLaneManifest::load(&manifest_path).unwrap();
    assert_eq!(loaded.contract_count(), manifest.contract_count());
    
    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Test config creation and serialization
#[test]
fn test_config_e2e() {
    let config = TwoLaneConfig::from_base_dir("/data/pir")
        .with_entries(1_000_000, 2_700_000_000);
    
    assert_eq!(config.hot_entries, 1_000_000);
    assert_eq!(config.cold_entries, 2_700_000_000);
    assert_eq!(config.total_entries(), 2_701_000_000);
    
    let avg_query = config.estimated_avg_query_size();
    assert!(avg_query < 100_000);
}

/// Test client routing without server
#[test]
fn test_client_routing_without_server() {
    let mut manifest = HotLaneManifest::new(12345);
    manifest.add_contract([0x11u8; 20], "Test1".into(), 100, "token".into());
    manifest.add_contract([0x22u8; 20], "Test2".into(), 200, "defi".into());
    
    let router = LaneRouter::new(manifest);
    let client = TwoLaneClient::new(router, "http://localhost:9999".into());
    
    assert!(client.is_hot(&[0x11u8; 20]));
    assert!(!client.is_hot(&[0x33u8; 20]));
    assert_eq!(client.get_lane(&[0x11u8; 20]), Lane::Hot);
    assert_eq!(client.get_lane(&[0x33u8; 20]), Lane::Cold);
    assert_eq!(client.hot_contract_count(), 2);
}

/// Test query size estimation
#[test]
fn test_query_size_estimation() {
    assert_eq!(Lane::Hot.expected_query_size(), 10_000);
    assert_eq!(Lane::Cold.expected_query_size(), 500_000);
    
    let improvement = Lane::Cold.expected_query_size() as f64 / Lane::Hot.expected_query_size() as f64;
    assert!(improvement >= 50.0);
}

/// Test privacy contracts are included in hot lane
#[test]
fn test_privacy_contracts_in_hot_lane() {
    use lane_builder::ContractExtractor;
    
    let mut extractor = ContractExtractor::new();
    extractor.load_known_contracts();
    
    let manifest = extractor.build_manifest(0);
    
    let privacy_contracts: Vec<_> = manifest.contracts
        .iter()
        .filter(|c| c.category == "privacy")
        .collect();
    
    assert!(!privacy_contracts.is_empty(), "Privacy contracts should be in hot lane");
}

/// Test full PIR setup and database creation
#[test]
fn test_pir_database_setup() {
    let temp_dir = std::env::temp_dir().join("pir-setup-test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    
    let hot_data: Vec<u8> = (0..256 * 32).map(|i| (i % 256) as u8).collect();
    let cold_data: Vec<u8> = (0..256 * 32).map(|i| ((i + 128) % 256) as u8).collect();
    
    let result = TwoLaneSetup::new(&temp_dir)
        .hot_data(hot_data)
        .cold_data(cold_data)
        .entry_size(32)
        .params(test_params())
        .build()
        .expect("Setup should succeed");
    
    assert_eq!(result.config.hot_entries, 256);
    assert_eq!(result.config.cold_entries, 256);
    assert_eq!(result.config.entry_size, 32);
    
    assert!(temp_dir.join("hot/crs.json").exists());
    assert!(temp_dir.join("hot/encoded.json").exists());
    assert!(temp_dir.join("cold/crs.json").exists());
    assert!(temp_dir.join("cold/encoded.json").exists());
    assert!(temp_dir.join("config.json").exists());
    
    let loaded_config = TwoLaneConfig::load(&temp_dir.join("config.json"))
        .expect("Config should load");
    assert_eq!(loaded_config.hot_entries, 256);
    
    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// Test actual PIR query/response cycle with encryption and decryption
#[test]
fn test_pir_query_response_cycle() {
    use inspire_pir::{query, respond, extract};
    use inspire_pir::math::GaussianSampler;
    use inspire_pir::rlwe::RlweSecretKey;
    
    let temp_dir = std::env::temp_dir().join("pir-query-test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    
    let params = test_params();
    let entry_size = 32;
    let num_entries = params.ring_dim;
    
    // Create test data with predictable pattern
    let mut hot_data = vec![0u8; num_entries * entry_size];
    for i in 0..num_entries {
        let start = i * entry_size;
        for j in 0..entry_size {
            hot_data[start + j] = ((i + j) % 256) as u8;
        }
    }
    
    let cold_data = hot_data.clone();
    
    // Setup two-lane PIR databases
    let result = TwoLaneSetup::new(&temp_dir)
        .hot_data(hot_data.clone())
        .cold_data(cold_data)
        .entry_size(entry_size)
        .params(params)
        .build()
        .expect("Setup should succeed");
    
    // Query for entry at index 42
    let target_index = 42u64;
    let mut sampler = GaussianSampler::new(result.hot_crs.params.sigma);
    
    // Client generates their own secret key (as they would in production)
    let client_secret_key = RlweSecretKey::generate(&result.hot_crs.params, &mut sampler);
    
    // Client creates encrypted query
    let (client_state, client_query) = query(
        &result.hot_crs,
        target_index,
        &result.hot_db.config,
        &client_secret_key,
        &mut sampler,
    ).expect("Query should succeed");
    
    // Server processes query homomorphically
    let server_response = respond(
        &result.hot_crs,
        &result.hot_db,
        &client_query,
    ).expect("Respond should succeed");
    
    // Client decrypts response
    let retrieved = extract(
        &result.hot_crs,
        &client_state,
        &server_response,
        entry_size,
    ).expect("Extract should succeed");
    
    // Verify correctness
    let expected_start = (target_index as usize) * entry_size;
    let expected = &hot_data[expected_start..expected_start + entry_size];
    
    assert_eq!(&retrieved[..], expected, "Retrieved entry should match original");
    
    let _ = std::fs::remove_dir_all(&temp_dir);
}
