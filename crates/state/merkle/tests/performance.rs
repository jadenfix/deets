use aether_state_merkle::SparseMerkleTree;
use aether_types::{Address, H256};

#[test]
fn test_merkle_incremental_updates_performance() {
    let mut tree = SparseMerkleTree::new();
    
    // Create 1000 accounts
    let start = std::time::Instant::now();
    
    for i in 0..1000 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0..2].copy_from_slice(&i.to_le_bytes()[0..2]);
        let addr = Address::from_slice(&addr_bytes).unwrap();
        
        let value = H256::from_slice(&[i as u8; 32]).unwrap();
        tree.update(addr, value);
    }
    
    // Root computation should be lazy (deferred until accessed)
    let root_compute_start = std::time::Instant::now();
    let _root = tree.root();
    let root_compute_time = root_compute_start.elapsed();
    
    let total_time = start.elapsed();
    
    println!("Merkle performance test:");
    println!("  Updates: 1000");
    println!("  Update time: {:?} ({:.2}μs per update)", 
        total_time, 
        total_time.as_micros() as f64 / 1000.0
    );
    println!("  Root computation: {:?}", root_compute_time);
    
    // Updates should be fast (O(1) each)
    assert!(total_time.as_millis() < 100, "Updates took too long: {:?}", total_time);
    
    // Root computation should be reasonable (O(n))
    assert!(root_compute_time.as_millis() < 50, "Root computation took too long: {:?}", root_compute_time);
}

#[test]
fn test_batch_update_performance() {
    let mut tree = SparseMerkleTree::new();
    
    // Prepare 1000 updates
    let updates: Vec<_> = (0..1000)
        .map(|i| {
            let mut addr_bytes = [0u8; 20];
            addr_bytes[0..2].copy_from_slice(&i.to_le_bytes()[0..2]);
            let addr = Address::from_slice(&addr_bytes).unwrap();
            let value = H256::from_slice(&[i as u8; 32]).unwrap();
            (addr, value)
        })
        .collect();
    
    // Batch update
    let start = std::time::Instant::now();
    tree.batch_update(updates);
    let _root = tree.root();
    let batch_time = start.elapsed();
    
    println!("Batch update performance:");
    println!("  Batch size: 1000");
    println!("  Time: {:?}", batch_time);
    
    // Batch should be fast
    assert!(batch_time.as_millis() < 100, "Batch update took too long: {:?}", batch_time);
}

#[test]
fn test_lazy_computation_efficiency() {
    let mut tree = SparseMerkleTree::new();
    
    // Do multiple updates without accessing root
    let start = std::time::Instant::now();
    for i in 0..100 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0] = i as u8;
        let addr = Address::from_slice(&addr_bytes).unwrap();
        let value = H256::from_slice(&[i as u8; 32]).unwrap();
        tree.update(addr, value);
    }
    let update_time = start.elapsed();
    
    // Now access root (triggers computation)
    let root_start = std::time::Instant::now();
    let _root1 = tree.root();
    let first_root_time = root_start.elapsed();
    
    // Access root again (should be cached)
    let cached_start = std::time::Instant::now();
    let _root2 = tree.root();
    let cached_time = cached_start.elapsed();
    
    println!("Lazy computation efficiency:");
    println!("  Updates: 100");
    println!("  Update time: {:?}", update_time);
    println!("  First root access: {:?}", first_root_time);
    println!("  Cached root access: {:?}", cached_time);
    
    // Updates should be very fast (just inserts)
    assert!(update_time.as_micros() < 10_000, "Updates should be fast: {:?}", update_time);
    
    // Cached access should be instantaneous
    assert!(cached_time.as_nanos() < 1_000, "Cached access should be instant: {:?}", cached_time);
    
    // First access should do actual computation
    assert!(first_root_time > cached_time, "First access should compute, cached should not");
}

#[test]
#[ignore] // Long-running test
fn test_large_tree_performance() {
    let mut tree = SparseMerkleTree::new();
    
    println!("Large tree performance test (10,000 accounts):");
    
    // Add 10,000 accounts
    let start = std::time::Instant::now();
    for i in 0..10_000 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0..2].copy_from_slice(&i.to_le_bytes()[0..2]);
        let addr = Address::from_slice(&addr_bytes).unwrap();
        let value = H256::from_slice(&[(i % 256) as u8; 32]).unwrap();
        tree.update(addr, value);
    }
    let update_time = start.elapsed();
    
    // Compute root
    let root_start = std::time::Instant::now();
    let _root = tree.root();
    let root_time = root_start.elapsed();
    
    println!("  Update time: {:?} ({:.2}μs per update)", 
        update_time,
        update_time.as_micros() as f64 / 10_000.0
    );
    println!("  Root computation: {:?}", root_time);
    
    // Should scale linearly
    assert!(update_time.as_secs() < 1, "Updates took too long: {:?}", update_time);
    assert!(root_time.as_millis() < 500, "Root computation took too long: {:?}", root_time);
}

#[test]
fn test_delete_performance() {
    let mut tree = SparseMerkleTree::new();
    
    // Add 100 accounts
    let addresses: Vec<_> = (0..100)
        .map(|i| {
            let mut addr_bytes = [0u8; 20];
            addr_bytes[0] = i as u8;
            let addr = Address::from_slice(&addr_bytes).unwrap();
            let value = H256::from_slice(&[i as u8; 32]).unwrap();
            tree.update(addr, value);
            addr
        })
        .collect();
    
    // Get initial root
    let _initial_root = tree.root();
    
    // Delete half
    let start = std::time::Instant::now();
    for addr in addresses.iter().take(50) {
        tree.delete(addr);
    }
    let delete_time = start.elapsed();
    
    // Compute new root
    let root_start = std::time::Instant::now();
    let _new_root = tree.root();
    let root_time = root_start.elapsed();
    
    println!("Delete performance:");
    println!("  Deletes: 50");
    println!("  Delete time: {:?}", delete_time);
    println!("  Root recomputation: {:?}", root_time);
    
    // Deletes should be fast
    assert!(delete_time.as_micros() < 5_000, "Deletes took too long: {:?}", delete_time);
}

