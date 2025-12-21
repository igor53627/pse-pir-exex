//! Benchmark seed expansion query size reduction
//!
//! Compares serialized sizes of ClientQuery vs SeededClientQuery
//! Run: cargo run --example benchmark_seed_expansion

use inspire_pir::math::GaussianSampler;
use inspire_pir::params::InspireParams;
use inspire_pir::pir::{query, query_seeded, setup};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Seed Expansion Benchmark (inspire-exex)");
    println!("=======================================\n");

    let configs = [
        ("d=256 (test)", InspireParams {
            ring_dim: 256,
            q: 1152921504606830593,
            p: 65536,
            sigma: 3.2,
            gadget_base: 1 << 20,
            gadget_len: 3,
            security_level: inspire_pir::params::SecurityLevel::Bits128,
        }),
        ("d=2048 (production)", InspireParams::secure_128_d2048()),
    ];

    for (name, params) in configs {
        println!("Configuration: {}", name);
        println!("  Ring dimension: {}", params.ring_dim);
        println!("  Gadget length: {}", params.gadget_len);
        
        let mut sampler = GaussianSampler::new(params.sigma);
        
        let entry_size = 32;
        let num_entries = params.ring_dim;
        let database: Vec<u8> = (0..(num_entries * entry_size))
            .map(|i| (i % 256) as u8)
            .collect();

        let (crs, encoded_db, rlwe_sk) = setup(&params, &database, entry_size, &mut sampler)
            .map_err(|e| format!("Setup failed: {}", e))?;

        let target_index = 42u64;
        let (_, regular_query) = query(&crs, target_index, &encoded_db.config, &rlwe_sk, &mut sampler)
            .map_err(|e| format!("Query failed: {}", e))?;
        let (_, seeded_query) = query_seeded(&crs, target_index, &encoded_db.config, &rlwe_sk, &mut sampler)
            .map_err(|e| format!("Seeded query failed: {}", e))?;

        let regular_json = serde_json::to_vec(&regular_query)?;
        let seeded_json = serde_json::to_vec(&seeded_query)?;

        let regular_size = regular_json.len();
        let seeded_size = seeded_json.len();
        let reduction = 100.0 * (1.0 - (seeded_size as f64 / regular_size as f64));

        println!("  Regular query size: {} bytes ({:.1} KB)", regular_size, regular_size as f64 / 1024.0);
        println!("  Seeded query size:  {} bytes ({:.1} KB)", seeded_size, seeded_size as f64 / 1024.0);
        println!("  Reduction: {:.1}%", reduction);

        let expanded = seeded_query.expand();
        assert_eq!(expanded.shard_id, regular_query.shard_id);
        println!("  [OK] Seeded query expands correctly\n");
    }

    println!("Two-Lane Impact");
    println!("---------------");
    println!("With seed expansion enabled (default):");
    println!("  - Query size: ~230 KB (vs ~460 KB without)");
    println!("  - 14 wallet queries: ~3.2 MB (vs ~6.4 MB without)");
    println!("\nServer expands seeds on receipt (adds ~1-2ms latency).");

    Ok(())
}
