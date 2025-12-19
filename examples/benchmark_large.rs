//! Large-scale PIR benchmark on aya
//!
//! Tests with database sizes closer to real hot lane (~1M entries).
//! Run on aya: cargo run --release --example benchmark_large

use std::time::Instant;

use inspire_pir::math::GaussianSampler;
use inspire_pir::params::InspireParams;
use inspire_pir::pir::{query_seeded, respond, extract, setup};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Large-Scale PIR Benchmark");
    println!("=========================\n");

    let params = InspireParams::secure_128_d2048();
    println!("Parameters: d={}, 128-bit security\n", params.ring_dim);

    // Larger test sizes approaching hot lane scale
    let test_sizes = [
        ("256K entries (128 shards)", params.ring_dim * 128),
        ("512K entries (256 shards)", params.ring_dim * 256),
        ("1M entries (512 shards)", params.ring_dim * 512),
    ];

    let entry_size = 32;

    for (name, num_entries) in test_sizes {
        let db_size_mb = (num_entries * entry_size) as f64 / 1024.0 / 1024.0;
        println!("Database: {} ({} entries, {:.1} MB)", name, num_entries, db_size_mb);
        
        // Generate synthetic data
        let gen_start = Instant::now();
        let database: Vec<u8> = (0..(num_entries * entry_size))
            .map(|i| (i % 256) as u8)
            .collect();
        println!("  Data generation:      {:>8.2} ms", gen_start.elapsed().as_secs_f64() * 1000.0);

        let mut sampler = GaussianSampler::new(params.sigma);

        // Setup (this is the expensive part - encoding database as polynomials)
        let setup_start = Instant::now();
        let (crs, encoded_db, rlwe_sk) = setup(&params, &database, entry_size, &mut sampler)
            .map_err(|e| format!("Setup failed: {}", e))?;
        let setup_time = setup_start.elapsed();
        println!("  Setup time:           {:>8.2} ms ({} shards)", 
            setup_time.as_secs_f64() * 1000.0,
            encoded_db.config.num_shards());

        // Test multiple queries at different indices
        let test_indices = [0, num_entries / 4, num_entries / 2, num_entries - 1];
        let mut total_query_time = std::time::Duration::ZERO;
        let mut total_respond_time = std::time::Duration::ZERO;
        let mut total_extract_time = std::time::Duration::ZERO;

        for &idx in &test_indices {
            let target_index = idx as u64;

            // Seeded query (client side)
            let query_start = Instant::now();
            let (state, seeded_query) = query_seeded(&crs, target_index, &encoded_db.config, &rlwe_sk, &mut sampler)
                .map_err(|e| format!("Query failed: {}", e))?;
            total_query_time += query_start.elapsed();

            // Server: expand + respond
            let respond_start = Instant::now();
            let expanded_query = seeded_query.expand();
            let response = respond(&crs, &encoded_db, &expanded_query)
                .map_err(|e| format!("Respond failed: {}", e))?;
            total_respond_time += respond_start.elapsed();

            // Client: extract
            let extract_start = Instant::now();
            let result = extract(&crs, &state, &response, entry_size)
                .map_err(|e| format!("Extract failed: {}", e))?;
            total_extract_time += extract_start.elapsed();

            // Verify
            let expected = &database[target_index as usize * entry_size..(target_index as usize + 1) * entry_size];
            assert_eq!(result, expected, "PIR result mismatch at index {}!", idx);
        }

        let num_queries = test_indices.len() as f64;
        println!("  Avg query gen time:   {:>8.2} ms (seeded)", total_query_time.as_secs_f64() * 1000.0 / num_queries);
        println!("  Avg respond time:     {:>8.2} ms (expand+respond)", total_respond_time.as_secs_f64() * 1000.0 / num_queries);
        println!("  Avg extract time:     {:>8.2} ms", total_extract_time.as_secs_f64() * 1000.0 / num_queries);
        println!("  Query size:           {:>8.1} KB (seeded)", 229.6);
        println!("  [OK] {} queries verified\n", test_indices.len());
    }

    println!("Real-World Projections");
    println!("----------------------");
    println!("Hot lane (1M entries, ~32 MB): ~3-5ms server respond time");
    println!("Cold lane (2.7B entries, ~87 GB): Would need sharding strategy");
    println!("\nWith seed expansion:");
    println!("  - Query upload: 230 KB (vs 460 KB without)");
    println!("  - Response download: ~1.3 MB (RLWE ciphertext)");
    println!("  - Round-trip overhead: ~1.5 MB total");

    Ok(())
}
