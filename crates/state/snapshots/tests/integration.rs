use aether_state_snapshots::{generate_snapshot, import_snapshot, decode_snapshot};
use aether_state_storage::{Storage, CF_ACCOUNTS, CF_UTXOS, CF_METADATA};
use aether_types::{Account, Address, Utxo, H256};
use anyhow::Result;
use tempfile::tempdir;

#[test]
fn test_snapshot_roundtrip_preserves_state() -> Result<()> {
    // Create storage with known state
    let source_dir = tempdir()?;
    let source = Storage::open(source_dir.path())?;
    
    // Add 100 accounts
    for i in 0..100 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0] = i as u8;
        let addr = Address::from_slice(&addr_bytes)?;
        
        let account = Account {
            address: addr,
            balance: 1000 + i as u128,
            nonce: i as u64,
            code_hash: [0u8; 32],
            storage_root: [0u8; 32],
        };
        
        let key = addr.as_bytes();
        let value = bincode::serialize(&account)?;
        source.put(CF_ACCOUNTS, key, &value)?;
    }
    
    // Add state root
    let state_root = H256::from_slice(&[42u8; 32])?;
    source.put(CF_METADATA, b"state_root", state_root.as_bytes())?;
    
    // Generate snapshot
    let snapshot_bytes = generate_snapshot(&source, 42)?;
    
    // Verify compression
    let uncompressed_size = 100 * 128; // Rough estimate per account
    let compressed_size = snapshot_bytes.len();
    let ratio = uncompressed_size as f64 / compressed_size as f64;
    
    println!("Compression ratio: {:.2}x", ratio);
    println!("Original size: {} bytes", uncompressed_size);
    println!("Compressed size: {} bytes", compressed_size);
    assert!(ratio >= 5.0, "Compression ratio {:.2}x should be at least 5x", ratio);
    
    // Import to fresh storage
    let target_dir = tempdir()?;
    let target = Storage::open(target_dir.path())?;
    let snapshot = import_snapshot(&target, &snapshot_bytes)?;
    
    // Verify metadata
    assert_eq!(snapshot.metadata.height, 42);
    assert_eq!(snapshot.accounts.len(), 100);
    
    // Verify each account
    for (addr, expected_account) in &snapshot.accounts {
        let actual_bytes = target.get(CF_ACCOUNTS, addr.as_bytes())?.unwrap();
        let actual_account: Account = bincode::deserialize(&actual_bytes)?;
        
        assert_eq!(actual_account.address, expected_account.address);
        assert_eq!(actual_account.balance, expected_account.balance);
        assert_eq!(actual_account.nonce, expected_account.nonce);
    }
    
    // Verify state root
    assert_eq!(snapshot.state_root, state_root);
    
    Ok(())
}

#[test]
fn test_snapshot_with_utxos() -> Result<()> {
    // Create storage with accounts and UTxOs
    let source_dir = tempdir()?;
    let source = Storage::open(source_dir.path())?;
    
    // Add some accounts
    for i in 0..10 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0] = i;
        let addr = Address::from_slice(&addr_bytes)?;
        
        let account = Account {
            address: addr,
            balance: 1000,
            nonce: 0,
            code_hash: [0u8; 32],
            storage_root: [0u8; 32],
        };
        
        let key = addr.as_bytes();
        let value = bincode::serialize(&account)?;
        source.put(CF_ACCOUNTS, key, &value)?;
    }
    
    // Add UTxOs
    for i in 0..50 {
        let mut owner_bytes = [0u8; 20];
        owner_bytes[0] = (i % 10) as u8;
        
        let utxo = Utxo {
            amount: 100 + i as u128,
            owner: Address::from_slice(&owner_bytes)?,
            script_hash: [0u8; 32],
        };
        
        let key = format!("utxo_{}", i).into_bytes();
        let value = bincode::serialize(&utxo)?;
        source.put(CF_UTXOS, &key, &value)?;
    }
    
    // Add state root
    let state_root = H256::from_slice(&[1u8; 32])?;
    source.put(CF_METADATA, b"state_root", state_root.as_bytes())?;
    
    // Generate and import snapshot
    let snapshot_bytes = generate_snapshot(&source, 100)?;
    
    let target_dir = tempdir()?;
    let target = Storage::open(target_dir.path())?;
    let snapshot = import_snapshot(&target, &snapshot_bytes)?;
    
    // Verify counts
    assert_eq!(snapshot.accounts.len(), 10);
    assert_eq!(snapshot.utxos.len(), 50);
    assert_eq!(snapshot.metadata.height, 100);
    
    // Verify UTxOs imported
    for i in 0..50 {
        let key = format!("utxo_{}", i).into_bytes();
        let value = target.get(CF_UTXOS, &key)?;
        assert!(value.is_some(), "UTxO {} should be imported", i);
    }
    
    Ok(())
}

#[test]
fn test_snapshot_compression_ratio() -> Result<()> {
    // Create storage with repetitive data (should compress well)
    let source_dir = tempdir()?;
    let source = Storage::open(source_dir.path())?;
    
    // Add 1000 accounts with similar data
    for i in 0..1000 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0..2].copy_from_slice(&i.to_le_bytes()[0..2]);
        let addr = Address::from_slice(&addr_bytes)?;
        
        let account = Account {
            address: addr,
            balance: 1000000,  // Same balance for all
            nonce: 0,          // Same nonce
            code_hash: [0u8; 32],  // Same code hash
            storage_root: [0u8; 32],  // Same storage root
        };
        
        let key = addr.as_bytes();
        let value = bincode::serialize(&account)?;
        source.put(CF_ACCOUNTS, key, &value)?;
    }
    
    let state_root = H256::from_slice(&[0u8; 32])?;
    source.put(CF_METADATA, b"state_root", state_root.as_bytes())?;
    
    // Generate snapshot
    let snapshot_bytes = generate_snapshot(&source, 1000)?;
    
    // Calculate compression ratio
    let estimated_uncompressed = 1000 * 128; // ~128 bytes per account
    let compressed_size = snapshot_bytes.len();
    let ratio = estimated_uncompressed as f64 / compressed_size as f64;
    
    println!("Compression test:");
    println!("  Accounts: 1000");
    println!("  Estimated uncompressed: {} bytes", estimated_uncompressed);
    println!("  Compressed: {} bytes", compressed_size);
    println!("  Ratio: {:.2}x", ratio);
    
    // Repetitive data should achieve high compression
    assert!(ratio >= 10.0, "Compression ratio {:.2}x should be at least 10x for repetitive data", ratio);
    
    Ok(())
}

#[test]
fn test_empty_snapshot() -> Result<()> {
    // Create empty storage
    let source_dir = tempdir()?;
    let source = Storage::open(source_dir.path())?;
    
    // Just add state root
    let state_root = H256::zero();
    source.put(CF_METADATA, b"state_root", state_root.as_bytes())?;
    
    // Generate snapshot
    let snapshot_bytes = generate_snapshot(&source, 0)?;
    
    // Import to target
    let target_dir = tempdir()?;
    let target = Storage::open(target_dir.path())?;
    let snapshot = import_snapshot(&target, &snapshot_bytes)?;
    
    // Verify empty
    assert_eq!(snapshot.accounts.len(), 0);
    assert_eq!(snapshot.utxos.len(), 0);
    assert_eq!(snapshot.metadata.height, 0);
    
    Ok(())
}

#[test]
#[ignore] // Long-running test
fn test_snapshot_large_state() -> Result<()> {
    // Create 10,000 accounts
    let source_dir = tempdir()?;
    let source = Storage::open(source_dir.path())?;
    
    println!("Creating 10,000 accounts...");
    for i in 0..10_000 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0..2].copy_from_slice(&i.to_le_bytes()[0..2]);
        let addr = Address::from_slice(&addr_bytes)?;
        
        let account = Account {
            address: addr,
            balance: 1000 + (i as u128),
            nonce: i as u64,
            code_hash: [0u8; 32],
            storage_root: [0u8; 32],
        };
        
        let key = addr.as_bytes();
        let value = bincode::serialize(&account)?;
        source.put(CF_ACCOUNTS, key, &value)?;
    }
    
    let state_root = H256::from_slice(&[99u8; 32])?;
    source.put(CF_METADATA, b"state_root", state_root.as_bytes())?;
    
    // Generate snapshot with timing
    let start = std::time::Instant::now();
    let snapshot_bytes = generate_snapshot(&source, 1000)?;
    let gen_time = start.elapsed();
    
    println!("Generation time: {:?}", gen_time);
    println!("Snapshot size: {} bytes ({:.2} MB)", 
        snapshot_bytes.len(), 
        snapshot_bytes.len() as f64 / 1_000_000.0
    );
    
    // Import with timing
    let target_dir = tempdir()?;
    let target = Storage::open(target_dir.path())?;
    
    let start = std::time::Instant::now();
    let snapshot = import_snapshot(&target, &snapshot_bytes)?;
    let import_time = start.elapsed();
    
    println!("Import time: {:?}", import_time);
    
    // Verify
    assert_eq!(snapshot.accounts.len(), 10_000);
    
    // Check performance requirements
    // For 50GB state (estimated 5M accounts), should be < 5 min
    // With 10K accounts, should be reasonable
    assert!(import_time.as_secs() < 10, "Import took too long: {:?}", import_time);
    
    Ok(())
}

#[test]
fn test_snapshot_decode_only() -> Result<()> {
    // Create a snapshot
    let source_dir = tempdir()?;
    let source = Storage::open(source_dir.path())?;
    
    for i in 0..10 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0] = i;
        let addr = Address::from_slice(&addr_bytes)?;
        
        let account = Account {
            address: addr,
            balance: 1000,
            nonce: 0,
            code_hash: [0u8; 32],
            storage_root: [0u8; 32],
        };
        
        let key = addr.as_bytes();
        let value = bincode::serialize(&account)?;
        source.put(CF_ACCOUNTS, key, &value)?;
    }
    
    let state_root = H256::zero();
    source.put(CF_METADATA, b"state_root", state_root.as_bytes())?;
    
    let snapshot_bytes = generate_snapshot(&source, 50)?;
    
    // Decode without importing
    let snapshot = decode_snapshot(&snapshot_bytes)?;
    
    // Verify can read metadata
    assert_eq!(snapshot.metadata.height, 50);
    assert_eq!(snapshot.accounts.len(), 10);
    
    Ok(())
}

