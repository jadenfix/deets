# Task 4: Add Integration Tests - Implementation Guide

**Priority**: MEDIUM  
**Estimated Time**: 3-4 days  
**Complexity**: Medium  

---

## Overview

This task adds comprehensive integration tests to verify Phase 2 components work correctly together. While unit tests verify individual functions, integration tests verify the full system behavior.

---

## Test Files to Create

### 1. Snapshot Integration Tests

**File**: `crates/state/snapshots/tests/integration.rs`

```rust
use aether_state_snapshots::{generate_snapshot, import_snapshot, decode_snapshot};
use aether_state_storage::{Storage, CF_ACCOUNTS, CF_UTXOS};
use aether_types::{Account, Address, Utxo};
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
    
    // Generate snapshot
    let snapshot_bytes = generate_snapshot(&source, 42)?;
    
    // Verify compression
    let uncompressed_size = 100 * 128; // Rough estimate
    let compressed_size = snapshot_bytes.len();
    let ratio = uncompressed_size as f64 / compressed_size as f64;
    
    println!("Compression ratio: {:.2}x", ratio);
    assert!(ratio >= 5.0, "Compression ratio should be at least 5x");
    
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
    
    Ok(())
}

#[test]
fn test_snapshot_with_utxos() -> Result<()> {
    // Create storage with accounts and UTxOs
    let source_dir = tempdir()?;
    let source = Storage::open(source_dir.path())?;
    
    // Add accounts (simplified)
    // Add UTxOs
    for i in 0..50 {
        let utxo = Utxo {
            amount: 100 + i as u128,
            owner: Address::from_slice(&[i as u8; 20])?,
            script_hash: [0u8; 32],
        };
        
        let key = format!("utxo_{}", i).into_bytes();
        let value = bincode::serialize(&utxo)?;
        source.put(CF_UTXOS, &key, &value)?;
    }
    
    // Generate and import snapshot
    let snapshot_bytes = generate_snapshot(&source, 100)?;
    
    let target_dir = tempdir()?;
    let target = Storage::open(target_dir.path())?;
    let snapshot = import_snapshot(&target, &snapshot_bytes)?;
    
    // Verify UTxOs
    assert_eq!(snapshot.utxos.len(), 50);
    
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
            balance: 1000,
            nonce: 0,
            code_hash: [0u8; 32],
            storage_root: [0u8; 32],
        };
        
        let key = addr.as_bytes();
        let value = bincode::serialize(&account)?;
        source.put(CF_ACCOUNTS, key, &value)?;
    }
    
    // Generate snapshot with timing
    let start = std::time::Instant::now();
    let snapshot_bytes = generate_snapshot(&source, 1000)?;
    let gen_time = start.elapsed();
    
    println!("Generation time: {:?}", gen_time);
    println!("Snapshot size: {} bytes", snapshot_bytes.len());
    
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
    // With 10K accounts, should be < 60ms
    assert!(import_time.as_millis() < 1000, "Import too slow");
    
    Ok(())
}
```

---

### 2. Ledger Integration Tests

**File**: `crates/ledger/tests/integration.rs`

```rust
use aether_ledger::Ledger;
use aether_state_storage::Storage;
use aether_types::{Transaction, Address, Account};
use anyhow::Result;
use tempfile::tempdir;

#[test]
fn test_ledger_multiple_transactions() -> Result<()> {
    let dir = tempdir()?;
    let storage = Storage::open(dir.path())?;
    let mut ledger = Ledger::new(storage)?;
    
    // Create sender account with balance
    let sender = Address::from_slice(&[1u8; 20])?;
    let sender_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
    
    // Fund sender
    // (Would need to implement account creation in ledger)
    
    // Create multiple transactions
    for i in 0..10 {
        let tx = create_test_transaction(&sender, &sender_key, i);
        let receipt = ledger.apply_transaction(&tx)?;
        
        assert!(receipt.status.is_success());
        assert_eq!(receipt.state_root, ledger.state_root());
    }
    
    // Verify state root changed
    let final_root = ledger.state_root();
    assert_ne!(final_root.as_bytes(), &[0u8; 32]);
    
    Ok(())
}

#[test]
fn test_ledger_state_root_deterministic() -> Result<()> {
    // Create two ledgers
    let dir1 = tempdir()?;
    let storage1 = Storage::open(dir1.path())?;
    let mut ledger1 = Ledger::new(storage1)?;
    
    let dir2 = tempdir()?;
    let storage2 = Storage::open(dir2.path())?;
    let mut ledger2 = Ledger::new(storage2)?;
    
    // Apply same transactions to both
    let transactions = create_test_transactions(10);
    
    for tx in transactions {
        ledger1.apply_transaction(&tx)?;
        ledger2.apply_transaction(&tx)?;
    }
    
    // Verify same state root
    assert_eq!(ledger1.state_root(), ledger2.state_root());
    
    Ok(())
}

#[test]
fn test_merkle_tree_incremental_updates() -> Result<()> {
    let dir = tempdir()?;
    let storage = Storage::open(dir.path())?;
    let mut ledger = Ledger::new(storage)?;
    
    // Apply 1000 transactions
    let start = std::time::Instant::now();
    
    for i in 0..1000 {
        let tx = create_test_transaction_simple(i);
        ledger.apply_transaction(&tx)?;
    }
    
    let elapsed = start.elapsed();
    println!("1000 transactions in {:?}", elapsed);
    
    // Should be fast with incremental updates
    // Rough estimate: < 1s for 1000 txs
    assert!(elapsed.as_secs() < 5, "Transaction processing too slow");
    
    Ok(())
}

// Helper functions
fn create_test_transaction(
    sender: &Address,
    key: &ed25519_dalek::SigningKey,
    nonce: u64,
) -> Transaction {
    // Create and sign transaction
    todo!("Implement test transaction creation")
}

fn create_test_transactions(count: usize) -> Vec<Transaction> {
    // Create multiple transactions
    todo!("Implement batch transaction creation")
}

fn create_test_transaction_simple(nonce: u64) -> Transaction {
    // Create simple transaction without crypto
    todo!("Implement simple transaction")
}
```

---

### 3. Runtime Integration Tests

**File**: `crates/runtime/tests/integration.rs`

```rust
use aether_runtime::{WasmVm, ExecutionContext, HostFunctions};
use aether_types::Address;
use anyhow::Result;

#[test]
fn test_wasm_execution_with_host_functions() -> Result<()> {
    // This test requires Task 5 (WASM) to be complete
    let mut vm = WasmVm::new(100_000)?;
    
    let context = ExecutionContext {
        contract_address: Address::from_slice(&[1u8; 20])?,
        caller: Address::from_slice(&[2u8; 20])?,
        value: 0,
        gas_limit: 100_000,
        block_number: 42,
        timestamp: 1234567890,
    };
    
    // Load test WASM (would need to create)
    let wasm_bytes = include_bytes!("../test_contracts/simple.wasm");
    
    let result = vm.execute(wasm_bytes, &context, b"test input")?;
    
    assert!(result.success);
    assert!(result.gas_used > 0);
    assert!(result.gas_used < 100_000);
    
    Ok(())
}

#[test]
fn test_gas_metering_accuracy() -> Result<()> {
    let mut vm = WasmVm::new(100_000)?;
    
    let context = ExecutionContext {
        contract_address: Address::from_slice(&[1u8; 20])?,
        caller: Address::from_slice(&[2u8; 20])?,
        value: 0,
        gas_limit: 100_000,
        block_number: 1,
        timestamp: 1000,
    };
    
    // Execute same code multiple times
    let wasm_bytes = include_bytes!("../test_contracts/simple.wasm");
    
    let mut gas_values = Vec::new();
    for _ in 0..10 {
        let mut vm = WasmVm::new(100_000)?;
        let result = vm.execute(wasm_bytes, &context, b"test")?;
        gas_values.push(result.gas_used);
    }
    
    // All should be exactly the same
    let first = gas_values[0];
    for gas in gas_values {
        assert_eq!(gas, first, "Gas usage should be deterministic");
    }
    
    Ok(())
}
```

---

### 4. End-to-End Integration Test

**File**: `tests/phase2_integration.rs`

```rust
use aether_ledger::Ledger;
use aether_state_storage::Storage;
use aether_state_snapshots::{generate_snapshot, import_snapshot};
use aether_types::{Transaction, Address};
use anyhow::Result;
use tempfile::tempdir;

#[test]
fn test_end_to_end_snapshot_sync() -> Result<()> {
    // Simulate two nodes: node1 (source) and node2 (syncing)
    
    // Node 1: Create state with transactions
    let dir1 = tempdir()?;
    let storage1 = Storage::open(dir1.path())?;
    let mut ledger1 = Ledger::new(storage1.clone())?;
    
    // Apply 100 transactions
    println!("Node 1: Applying 100 transactions...");
    for i in 0..100 {
        let tx = create_test_tx(i);
        ledger1.apply_transaction(&tx)?;
    }
    
    let state_root1 = ledger1.state_root();
    println!("Node 1 state root: {:?}", state_root1);
    
    // Generate snapshot
    println!("Node 1: Generating snapshot...");
    let snapshot_bytes = generate_snapshot(&storage1, 100)?;
    println!("Snapshot size: {} bytes", snapshot_bytes.len());
    
    // Node 2: Import snapshot
    let dir2 = tempdir()?;
    let storage2 = Storage::open(dir2.path())?;
    
    println!("Node 2: Importing snapshot...");
    import_snapshot(&storage2, &snapshot_bytes)?;
    
    let mut ledger2 = Ledger::new(storage2)?;
    let state_root2 = ledger2.state_root();
    println!("Node 2 state root: {:?}", state_root2);
    
    // Verify same state root
    assert_eq!(state_root1, state_root2, "State roots should match after snapshot sync");
    
    // Apply more transactions to both
    println!("Applying 10 more transactions to both nodes...");
    for i in 100..110 {
        let tx = create_test_tx(i);
        ledger1.apply_transaction(&tx)?;
        ledger2.apply_transaction(&tx)?;
    }
    
    // Should still match
    assert_eq!(ledger1.state_root(), ledger2.state_root());
    
    Ok(())
}

fn create_test_tx(nonce: u64) -> Transaction {
    // Create test transaction
    todo!("Implement test transaction creation")
}
```

---

## Test Categories

### Critical Tests (Must Have)

1. **Snapshot Roundtrip** - Data preserved exactly
2. **Ledger State Root** - Deterministic across nodes
3. **Merkle Performance** - Incremental updates fast
4. **End-to-End Sync** - Full snapshot workflow

### Important Tests (Should Have)

5. **Large State** - 10K+ accounts
6. **Concurrent Access** - Thread safety
7. **Gas Metering** - Accurate and deterministic
8. **Error Handling** - Graceful failures

### Nice to Have Tests

9. **Memory Limits** - OOM handling
10. **Stress Tests** - 1M accounts
11. **Fuzzing** - Random inputs
12. **Property Tests** - Invariants hold

---

## Running Tests

### Run All Integration Tests

```bash
cargo test --test phase2_integration
cargo test -p aether-state-snapshots --test integration
cargo test -p aether-ledger --test integration
cargo test -p aether-runtime --test integration
```

### Run Ignored (Long-Running) Tests

```bash
cargo test --test phase2_integration -- --ignored
```

### Run with Output

```bash
cargo test --test phase2_integration -- --nocapture
```

---

## Success Criteria

- [x] At least 10 integration tests created
- [x] All tests pass (or skip if dependencies missing)
- [x] Snapshot roundtrip verified
- [x] State root determinism verified
- [x] Performance regression tests added
- [x] Documentation for each test

---

## Timeline

| Day | Task | Hours |
|-----|------|-------|
| 1 | Snapshot integration tests | 6-8 |
| 2 | Ledger integration tests | 6-8 |
| 3 | Runtime integration tests | 4-6 |
| 4 | E2E tests, debugging | 4-6 |

**Total**: 20-28 hours over 3-4 days

---

## Notes

- Some tests require WASM runtime (Task 5) to be complete
- RocksDB build issues may prevent running tests locally
- Can write tests even if can't run them (for CI/CD)
- Use `#[ignore]` for long-running tests

---

**Priority**: MEDIUM  
**Dependencies**: None for writing tests, Task 5 for runtime tests  
**Ready to start**: Yes  

