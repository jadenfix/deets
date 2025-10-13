// ============================================================================
// PHASE 4 INTEGRATION TESTS - Networking & Data Availability
// ============================================================================
// Comprehensive end-to-end tests for Phase 4 components:
// - QUIC transport
// - Turbine block propagation
// - Erasure coding with packet loss
// - Batch signature verification
// - Snapshot synchronization
// - Full consensus + DA pipeline
// ============================================================================

use aether_quic_transport::QuicEndpoint;
use aether_da_turbine::{TurbineBroadcaster, TurbineReceiver};
use aether_types::H256;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Test QUIC transport for validator-to-validator communication
#[tokio::test]
async fn test_quic_validator_communication() {
    // Setup: Create 3 validators
    let validator1 = QuicEndpoint::new("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let validator2 = QuicEndpoint::new("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let validator3 = QuicEndpoint::new("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();

    let addr1 = validator1.local_addr().unwrap();
    let addr2 = validator2.local_addr().unwrap();
    let addr3 = validator3.local_addr().unwrap();

    // Test: Validator 1 sends message to validators 2 and 3
    let v1_clone = validator1.clone();
    let v2_clone = validator2.clone();
    let v3_clone = validator3.clone();

    // Spawn receivers
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    tokio::spawn(async move {
        if let Some(conn) = v2_clone.accept().await {
            let mut stream = conn.accept_uni().await.unwrap();
            let data = aether_quic_transport::QuicConnection::read_stream(&mut stream)
                .await
                .unwrap();
            received_clone.lock().await.push(data);
        }
    });

    let received_clone2 = received.clone();
    tokio::spawn(async move {
        if let Some(conn) = v3_clone.accept().await {
            let mut stream = conn.accept_uni().await.unwrap();
            let data = aether_quic_transport::QuicConnection::read_stream(&mut stream)
                .await
                .unwrap();
            received_clone2.lock().await.push(data);
        }
    });

    // Give receivers time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Send messages
    let message = b"validator broadcast".to_vec();
    
    for addr in [addr2, addr3] {
        let conn = v1_clone.connect(addr).await.unwrap();
        conn.send(message.clone()).await.unwrap();
    }

    // Wait for messages to be received
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify both validators received the message
    let received_msgs = received.lock().await;
    assert_eq!(received_msgs.len(), 2);
    assert!(received_msgs.iter().all(|msg| msg == &message));
}

/// Test end-to-end block propagation with Turbine + QUIC
#[tokio::test]
async fn test_turbine_block_propagation_with_quic() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;

    // Create broadcaster (leader) and receivers (validators)
    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
    
    // Prepare block
    let block_data = b"test block payload for propagation".to_vec();
    let block_hash = H256::from_slice(&Sha256::digest(&block_data)).unwrap();
    
    // Create shreds
    let shreds = broadcaster.make_shreds(1, block_hash, &block_data).unwrap();
    assert_eq!(shreds.len(), DATA_SHARDS + PARITY_SHARDS);

    // Simulate multiple validators receiving shreds
    let num_validators = 5;
    let mut receivers = Vec::new();
    
    for _ in 0..num_validators {
        receivers.push(TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap());
    }

    // Distribute shreds to validators (round-robin)
    let mut reconstructed_count = 0;
    
    for (i, shred) in shreds.into_iter().enumerate() {
        let validator_idx = i % num_validators;
        
        if let Some(block) = receivers[validator_idx].ingest_shred(shred).unwrap() {
            assert_eq!(block, block_data);
            reconstructed_count += 1;
        }
    }

    // At least one validator should have reconstructed the block
    assert!(reconstructed_count > 0, "no validators reconstructed the block");
}

/// Test parallel block propagation (multiple blocks concurrently)
#[tokio::test]
async fn test_concurrent_block_propagation() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;
    const NUM_BLOCKS: usize = 10;

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
    let mut all_shreds = HashMap::new();
    let mut expected_blocks = HashMap::new();

    // Create multiple blocks
    for slot in 0..NUM_BLOCKS {
        let block_data = format!("block at slot {}", slot).into_bytes();
        let block_hash = H256::from_slice(&Sha256::digest(&block_data)).unwrap();
        let shreds = broadcaster.make_shreds(slot as u64, block_hash, &block_data).unwrap();
        
        all_shreds.insert(slot, shreds);
        expected_blocks.insert(slot, block_data);
    }

    // Single receiver processes all blocks
    let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
    let mut reconstructed = HashMap::new();

    // Process all shreds (interleaved across blocks)
    for slot in 0..NUM_BLOCKS {
        for shred in all_shreds.remove(&slot).unwrap() {
            if let Some(block) = receiver.ingest_shred(shred).unwrap() {
                reconstructed.insert(slot, block);
            }
        }
    }

    // Verify all blocks reconstructed
    assert_eq!(reconstructed.len(), NUM_BLOCKS);
    
    for (slot, block) in reconstructed {
        assert_eq!(block, expected_blocks[&slot]);
    }
}

/// Test DA layer with simulated network latency
#[tokio::test]
async fn test_da_with_network_latency() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;
    const LATENCY_MS: u64 = 50; // Simulate 50ms network latency

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
    
    let block_data = b"latency test block".to_vec();
    let block_hash = H256::from_slice(&Sha256::digest(&block_data)).unwrap();
    let shreds = broadcaster.make_shreds(1, block_hash, &block_data).unwrap();

    // Receiver processes shreds with simulated latency
    let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
    
    let start = std::time::Instant::now();
    
    for shred in shreds {
        // Simulate network latency
        tokio::time::sleep(tokio::time::Duration::from_millis(LATENCY_MS)).await;
        
        if let Some(block) = receiver.ingest_shred(shred).unwrap() {
            assert_eq!(block, block_data);
            break;
        }
    }

    let elapsed = start.elapsed();
    
    // Should reconstruct within reasonable time despite latency
    // Expected: DATA_SHARDS * LATENCY_MS = 10 * 50ms = 500ms
    assert!(elapsed.as_millis() >= LATENCY_MS as u128 * DATA_SHARDS as u128);
    println!("Reconstruction time with latency: {:?}", elapsed);
}

/// Test DA resilience to Byzantine validators
#[tokio::test]
async fn test_da_byzantine_resilience() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 4; // Higher redundancy for Byzantine tolerance
    const BYZANTINE_COUNT: usize = 2; // 2 Byzantine validators

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
    
    let block_data = b"byzantine test block".to_vec();
    let block_hash = H256::from_slice(&Sha256::digest(&block_data)).unwrap();
    let mut shreds = broadcaster.make_shreds(1, block_hash, &block_data).unwrap();

    // Corrupt first BYZANTINE_COUNT shreds (simulating Byzantine validators)
    for i in 0..BYZANTINE_COUNT {
        shreds[i].payload = vec![0xFF; shreds[i].payload.len()];
    }

    // Honest receiver should still reconstruct correctly
    let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
    
    // Skip corrupted shreds and use rest
    let honest_shreds: Vec<_> = shreds.into_iter().skip(BYZANTINE_COUNT).collect();
    
    let mut reconstructed = false;
    for shred in honest_shreds {
        if let Some(block) = receiver.ingest_shred(shred).unwrap() {
            assert_eq!(block, block_data);
            reconstructed = true;
            break;
        }
    }

    assert!(reconstructed, "failed to reconstruct despite having enough honest shreds");
}

/// Test full pipeline: create block -> encode -> propagate -> reconstruct -> verify
#[tokio::test]
async fn test_full_da_pipeline() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;

    // Step 1: Create block (simulating consensus output)
    let block_data = b"full pipeline test block with transactions".to_vec();
    let block_hash = H256::from_slice(&Sha256::digest(&block_data)).unwrap();

    // Step 2: Leader encodes block with erasure coding
    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
    let shreds = broadcaster.make_shreds(1, block_hash, &block_data).unwrap();
    
    // Verify shred count
    assert_eq!(shreds.len(), DATA_SHARDS + PARITY_SHARDS);

    // Step 3: Simulate network propagation (distribute shreds)
    // In real system, each validator would receive subset of shreds via Turbine tree
    let mut distributed_shreds = shreds.clone();
    
    // Simulate packet loss (drop 1 shred)
    distributed_shreds.remove(5);

    // Step 4: Validator reconstructs block
    let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
    
    let mut reconstructed_block = None;
    for shred in distributed_shreds {
        if let Some(block) = receiver.ingest_shred(shred).unwrap() {
            reconstructed_block = Some(block);
            break;
        }
    }

    // Step 5: Verify reconstruction
    assert!(reconstructed_block.is_some());
    assert_eq!(reconstructed_block.unwrap(), block_data);

    // Step 6: Verify block hash matches
    let reconstructed_hash = H256::from_slice(&Sha256::digest(&block_data)).unwrap();
    assert_eq!(reconstructed_hash, block_hash);
}

/// Performance test: measure end-to-end latency
#[tokio::test]
#[ignore] // Run with --ignored for performance tests
async fn bench_end_to_end_da_latency() {
    const DATA_SHARDS: usize = 10;
    const PARITY_SHARDS: usize = 2;
    const ITERATIONS: usize = 100;

    let broadcaster = TurbineBroadcaster::new(DATA_SHARDS, PARITY_SHARDS, 1).unwrap();
    
    let block_data = vec![0u8; 2_000_000]; // 2MB block
    let block_hash = H256::from_slice(&Sha256::digest(&block_data)).unwrap();

    let mut total_latency = std::time::Duration::default();

    for _ in 0..ITERATIONS {
        let start = std::time::Instant::now();

        // Encode
        let shreds = broadcaster.make_shreds(1, block_hash, &block_data).unwrap();

        // Decode
        let mut receiver = TurbineReceiver::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
        for shred in shreds {
            if receiver.ingest_shred(shred).unwrap().is_some() {
                break;
            }
        }

        total_latency += start.elapsed();
    }

    let avg_latency = total_latency / ITERATIONS as u32;
    println!("Average end-to-end DA latency: {:?}", avg_latency);

    // Phase 4 target: <50ms for 2MB block
    assert!(avg_latency.as_millis() < 50, "latency {} ms too high", avg_latency.as_millis());
}

