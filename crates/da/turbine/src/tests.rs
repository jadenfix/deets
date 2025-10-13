// ============================================================================
// PHASE 4 DATA AVAILABILITY ACCEPTANCE TESTS
// ============================================================================
// Comprehensive testing of Turbine DA layer including:
// - Packet loss resilience (existing)
// - Byzantine/adversarial scenarios
// - Large block stress testing
// - Latency benchmarks
// - Network partition recovery
// ============================================================================

use super::*;
use aether_types::H256;
use rand::{seq::SliceRandom, Rng};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Test that reconstruction succeeds with packet loss up to parity threshold
#[test]
fn phase4_acceptance_turbine_packet_loss_resilience() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;
    const TOTAL_SHREDS: usize = DATA_SHARDS + PARITY_SHARDS;
    const TRIALS: usize = 200;

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
    let mut rng = rand::thread_rng();
    let mut successes = 0usize;

    for trial in 0..TRIALS {
        let payload = format!("phase4 turbine payload {trial}").into_bytes();
        let block_hash = {
            let digest = Sha256::digest(&payload);
            H256::from_slice(&digest).unwrap()
        };

        let shreds = broadcaster
            .make_shreds(1, block_hash, &payload)
            .expect("shards");

        // Randomly drop up to parity shards to simulate <=16% loss
        let drop_count = rng.gen_range(0..=PARITY_SHARDS);
        let mut indices: Vec<usize> = (0..TOTAL_SHREDS).collect();
        indices.shuffle(&mut rng);
        let drop_set: HashSet<_> = indices.into_iter().take(drop_count).collect();

        let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
        let mut recovered = false;

        for (idx, shred) in shreds.into_iter().enumerate() {
            if drop_set.contains(&idx) {
                continue;
            }
            if let Some(block) = receiver.ingest_shred(shred).unwrap() {
                assert_eq!(block, payload);
                recovered = true;
                successes += 1;
                break;
            }
        }

        assert!(
            recovered,
            "failed to reconstruct block despite <= parity loss (trial {trial})"
        );
    }

    let success_rate = successes as f64 / TRIALS as f64;
    assert!(
        success_rate >= 0.999,
        "success rate {} below acceptance threshold",
        success_rate
    );
}

/// Test reconstruction with adversarial shred ordering
/// Simulates out-of-order delivery common in real networks
#[test]
fn test_out_of_order_shred_delivery() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;
    const TRIALS: usize = 50;

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
    let mut rng = rand::thread_rng();

    for trial in 0..TRIALS {
        let payload = format!("out of order test {trial}").into_bytes();
        let block_hash = H256::from_slice(&Sha256::digest(&payload)).unwrap();

        let mut shreds = broadcaster.make_shreds(1, block_hash, &payload).unwrap();

        // Randomly shuffle shreds (simulate network reordering)
        shreds.shuffle(&mut rng);

        let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

        let mut recovered = false;
        for shred in shreds {
            if let Some(block) = receiver.ingest_shred(shred).unwrap() {
                assert_eq!(block, payload, "reconstruction mismatch on trial {}", trial);
                recovered = true;
                break;
            }
        }

        assert!(
            recovered,
            "failed to reconstruct with shuffled shreds (trial {})",
            trial
        );
    }
}

/// Test large block stress scenario (simulate 4MB blocks)
#[test]
fn test_large_block_stress() {
    const DATA_SHARDS: usize = 20;
    const PARITY_SHARDS: usize = 4;
    const BLOCK_SIZE: usize = 4_000_000; // 4MB

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();

    // Generate large payload
    let mut payload = Vec::with_capacity(BLOCK_SIZE);
    for i in 0..BLOCK_SIZE {
        payload.push((i % 256) as u8);
    }

    let block_hash = H256::from_slice(&Sha256::digest(&payload)).unwrap();
    let shreds = broadcaster.make_shreds(1, block_hash, &payload).unwrap();

    assert_eq!(shreds.len(), DATA_SHARDS + PARITY_SHARDS);

    let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

    let mut recovered = false;
    for shred in shreds {
        if let Some(block) = receiver.ingest_shred(shred).unwrap() {
            assert_eq!(block.len(), payload.len());
            assert_eq!(block, payload);
            recovered = true;
            break;
        }
    }

    assert!(recovered, "failed to reconstruct large 4MB block");
}

/// Test minimal shred set reconstruction
/// Verifies we can reconstruct with exactly k shreds (no redundancy)
#[test]
fn test_minimal_shred_reconstruction() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
    let payload = b"minimal shred test payload".to_vec();
    let block_hash = H256::from_slice(&Sha256::digest(&payload)).unwrap();

    let shreds = broadcaster.make_shreds(1, block_hash, &payload).unwrap();

    // Take exactly k shreds (minimum needed)
    let minimal_shreds: Vec<_> = shreds.into_iter().take(DATA_SHARDS).collect();

    let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

    let mut recovered = false;
    for shred in minimal_shreds {
        if let Some(block) = receiver.ingest_shred(shred).unwrap() {
            assert_eq!(block, payload);
            recovered = true;
            break;
        }
    }

    assert!(recovered, "failed to reconstruct with minimal k shreds");
}

/// Test recovery from network partition
/// Simulates a partition where only subset of validators receive shreds
#[test]
fn test_network_partition_recovery() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 4; // Higher redundancy for partition tolerance
    const PARTITION_SIZE: usize = 7; // Partition receives 7 of 14 shreds

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
    let payload = b"partition recovery test".to_vec();
    let block_hash = H256::from_slice(&Sha256::digest(&payload)).unwrap();

    let shreds = broadcaster.make_shreds(1, block_hash, &payload).unwrap();

    // Simulate partition: only receive first PARTITION_SIZE shreds
    let partition_shreds: Vec<_> = shreds.into_iter().take(PARTITION_SIZE).collect();

    // This should fail if PARTITION_SIZE < DATA_SHARDS
    if PARTITION_SIZE < DATA_SHARDS {
        let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

        let mut recovered = false;
        for shred in partition_shreds {
            if let Some(_block) = receiver.ingest_shred(shred).unwrap() {
                recovered = true;
                break;
            }
        }

        assert!(
            !recovered,
            "should not reconstruct with insufficient shreds"
        );
    } else {
        let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

        let mut recovered = false;
        for shred in partition_shreds {
            if let Some(block) = receiver.ingest_shred(shred).unwrap() {
                assert_eq!(block, payload);
                recovered = true;
                break;
            }
        }

        assert!(
            recovered,
            "should reconstruct with sufficient shreds despite partition"
        );
    }
}

/// Benchmark encoding throughput
#[test]
#[ignore] // Run with --ignored for benchmarks
fn bench_encoding_throughput() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;
    const BLOCK_SIZE: usize = 2_000_000; // 2MB
    const ITERATIONS: usize = 100;

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();

    let payload = vec![0u8; BLOCK_SIZE];
    let block_hash = H256::from_slice(&Sha256::digest(&payload)).unwrap();

    let start = std::time::Instant::now();

    for i in 0..ITERATIONS {
        let _shreds = broadcaster
            .make_shreds(i as u64, block_hash, &payload)
            .unwrap();
    }

    let elapsed = start.elapsed();
    let throughput_mbps = (BLOCK_SIZE * ITERATIONS) as f64 / elapsed.as_secs_f64() / 1_000_000.0;

    println!("Encoding throughput: {:.2} MB/s", throughput_mbps);
    println!(
        "Average latency: {:.2} ms",
        elapsed.as_millis() as f64 / ITERATIONS as f64
    );

    // Should be able to encode at least 100 MB/s
    assert!(
        throughput_mbps > 100.0,
        "encoding throughput too low: {:.2} MB/s",
        throughput_mbps
    );
}

/// Benchmark decoding throughput
#[test]
#[ignore] // Run with --ignored for benchmarks
fn bench_decoding_throughput() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;
    const BLOCK_SIZE: usize = 2_000_000; // 2MB
    const ITERATIONS: usize = 100;

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();

    let payload = vec![0u8; BLOCK_SIZE];
    let block_hash = H256::from_slice(&Sha256::digest(&payload)).unwrap();

    // Pre-generate shreds
    let shreds = broadcaster.make_shreds(1, block_hash, &payload).unwrap();

    let start = std::time::Instant::now();

    for _ in 0..ITERATIONS {
        let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

        for shred in shreds.clone() {
            if receiver.ingest_shred(shred).unwrap().is_some() {
                break;
            }
        }
    }

    let elapsed = start.elapsed();
    let throughput_mbps = (BLOCK_SIZE * ITERATIONS) as f64 / elapsed.as_secs_f64() / 1_000_000.0;

    println!("Decoding throughput: {:.2} MB/s", throughput_mbps);
    println!(
        "Average latency: {:.2} ms",
        elapsed.as_millis() as f64 / ITERATIONS as f64
    );

    // Should be able to decode at least 100 MB/s
    assert!(
        throughput_mbps > 100.0,
        "decoding throughput too low: {:.2} MB/s",
        throughput_mbps
    );
}

/// Test concurrent reconstruction from multiple blocks
#[test]
fn test_concurrent_block_reconstruction() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;
    const NUM_BLOCKS: usize = 5;

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();

    // Generate multiple blocks
    let mut all_shreds = Vec::new();
    let mut expected_payloads = Vec::new();

    for i in 0..NUM_BLOCKS {
        let payload = format!("block {}", i).into_bytes();
        let block_hash = H256::from_slice(&Sha256::digest(&payload)).unwrap();
        let shreds = broadcaster
            .make_shreds(i as u64, block_hash, &payload)
            .unwrap();

        all_shreds.extend(shreds);
        expected_payloads.push(payload);
    }

    // Shuffle all shreds together (interleaved delivery)
    let mut rng = rand::thread_rng();
    all_shreds.shuffle(&mut rng);

    // Single receiver processes all shreds
    let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
    let mut reconstructed = Vec::new();

    for shred in all_shreds {
        if let Some(block) = receiver.ingest_shred(shred).unwrap() {
            reconstructed.push(block);
        }
    }

    // Should have reconstructed all blocks
    assert_eq!(reconstructed.len(), NUM_BLOCKS);

    // Verify all payloads match (order may differ)
    for payload in &expected_payloads {
        assert!(
            reconstructed.contains(payload),
            "missing expected payload in reconstruction"
        );
    }
}
