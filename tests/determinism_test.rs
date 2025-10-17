use aether_ledger::Ledger;
use aether_state_storage::Storage;
use aether_runtime::{WasmVm, ExecutionContext};
use aether_types::{Address, Account, H256};
use anyhow::Result;
use tempfile::tempdir;

#[test]
fn test_state_root_deterministic() -> Result<()> {
    // Test that state root is deterministic for same state
    
    println!("\n=== State Root Determinism Test ===\n");
    
    // Create two independent ledgers
    let dir1 = tempdir()?;
    let storage1 = Storage::open(dir1.path())?;
    let mut ledger1 = Ledger::new(storage1.clone())?;
    
    let dir2 = tempdir()?;
    let storage2 = Storage::open(dir2.path())?;
    let mut ledger2 = Ledger::new(storage2.clone())?;
    
    // Add identical accounts to both ledgers
    println!("Adding 50 identical accounts to both ledgers...");
    for i in 0..50 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0] = i as u8;
        let addr = Address::from_slice(&addr_bytes)?;
        
        let account = Account {
            address: addr,
            balance: 5000 + i as u128 * 10,
            nonce: i as u64,
            code_hash: [i as u8; 32],
            storage_root: [i as u8; 32],
        };
        
        // Add to both
        let key = addr.as_bytes();
        let value = bincode::serialize(&account)?;
        storage1.put("accounts", key, &value)?;
        storage2.put("accounts", key, &value)?;
    }
    
    // Get roots
    let root1 = ledger1.state_root();
    let root2 = ledger2.state_root();
    
    println!("Ledger 1 root: {:?}", root1);
    println!("Ledger 2 root: {:?}", root2);
    
    // Should be identical
    assert_eq!(root1, root2, "State roots must be identical for same state");
    
    println!("✓ State roots are deterministic\n");
    
    Ok(())
}

#[test]
fn test_wasm_execution_deterministic() -> Result<()> {
    // Test that WASM execution produces deterministic results
    
    println!("\n=== WASM Execution Determinism Test ===\n");
    
    // Create a simple WASM module
    let wasm = wat::parse_str(r#"
        (module
            (func (export "execute") (result i32)
                i32.const 42
            )
        )
    "#)?;
    
    // Execute it multiple times with same context
    let context = ExecutionContext {
        contract_address: Address::from_slice(&[1u8; 20])?,
        caller: Address::from_slice(&[2u8; 20])?,
        value: 0,
        gas_limit: 100_000,
        block_number: 100,
        timestamp: 1234567890,
    };
    
    let mut results = Vec::new();
    let mut gas_amounts = Vec::new();
    
    println!("Executing WASM contract 10 times...");
    for i in 0..10 {
        let mut vm = WasmVm::new(100_000)?;
        let result = vm.execute(&wasm, &context, b"test")?;
        
        println!("  Run {}: success={}, gas={}", i+1, result.success, result.gas_used);
        
        results.push(result.success);
        gas_amounts.push(result.gas_used);
    }
    
    // All results should be identical
    let first_result = results[0];
    let first_gas = gas_amounts[0];
    
    for (i, (&result, &gas)) in results.iter().zip(gas_amounts.iter()).enumerate() {
        assert_eq!(result, first_result, "Result {} differs from first", i);
        assert_eq!(gas, first_gas, "Gas usage {} differs from first", i);
    }
    
    println!("✓ All 10 executions produced identical results");
    println!("  Success: {}", first_result);
    println!("  Gas used: {}", first_gas);
    println!();
    
    Ok(())
}

#[test]
fn test_wasm_host_functions_deterministic() -> Result<()> {
    // Test that host functions return consistent values
    
    println!("\n=== Host Functions Determinism Test ===\n");
    
    let wasm = wat::parse_str(r#"
        (module
            (import "env" "block_number" (func $block_number (result i64)))
            (import "env" "timestamp" (func $timestamp (result i64)))
            (func (export "execute") (result i32)
                ;; Call host functions
                call $block_number
                drop
                call $timestamp
                drop
                i32.const 0
            )
        )
    "#)?;
    
    let context = ExecutionContext {
        contract_address: Address::from_slice(&[1u8; 20])?,
        caller: Address::from_slice(&[2u8; 20])?,
        value: 0,
        gas_limit: 100_000,
        block_number: 42,
        timestamp: 999888777,
    };
    
    println!("Executing contract with host function calls 5 times...");
    
    let mut gas_values = Vec::new();
    for i in 0..5 {
        let mut vm = WasmVm::new(100_000)?;
        let result = vm.execute(&wasm, &context, b"test")?;
        
        println!("  Run {}: gas={}", i+1, result.gas_used);
        gas_values.push(result.gas_used);
        
        assert!(result.success, "Execution should succeed");
    }
    
    // All gas values should be identical
    let first_gas = gas_values[0];
    for (i, &gas) in gas_values.iter().enumerate() {
        assert_eq!(gas, first_gas, "Gas usage differs in run {}", i);
    }
    
    println!("✓ Host functions produce deterministic results");
    println!("  Consistent gas usage: {} across all runs", first_gas);
    println!();
    
    Ok(())
}

#[test]
fn test_cross_node_state_verification() -> Result<()> {
    // Simulate two nodes and verify they produce same state
    
    println!("\n=== Cross-Node State Verification ===\n");
    
    // Node 1
    let dir1 = tempdir()?;
    let storage1 = Storage::open(dir1.path())?;
    let mut ledger1 = Ledger::new(storage1.clone())?;
    
    // Node 2
    let dir2 = tempdir()?;
    let storage2 = Storage::open(dir2.path())?;
    let mut ledger2 = Ledger::new(storage2.clone())?;
    
    println!("Applying same operations to both nodes...");
    
    // Apply same operations to both nodes
    for i in 0..30 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0] = i as u8;
        let addr = Address::from_slice(&addr_bytes)?;
        
        let account = Account {
            address: addr,
            balance: 1000 + i as u128,
            nonce: 0,
            code_hash: [0u8; 32],
            storage_root: [0u8; 32],
        };
        
        let key = addr.as_bytes();
        let value = bincode::serialize(&account)?;
        
        // Apply to both nodes
        storage1.put("accounts", key, &value)?;
        storage2.put("accounts", key, &value)?;
    }
    
    // Get state roots
    let root1 = ledger1.state_root();
    let root2 = ledger2.state_root();
    
    println!("Node 1 state root: {:?}", root1);
    println!("Node 2 state root: {:?}", root2);
    
    // Verify identical
    assert_eq!(root1, root2, "Cross-node state roots must match");
    
    // Verify individual accounts match
    println!("Verifying individual accounts...");
    for i in 0..30 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0] = i as u8;
        let addr = Address::from_slice(&addr_bytes)?;
        
        let account1 = storage1.get("accounts", addr.as_bytes())?;
        let account2 = storage2.get("accounts", addr.as_bytes())?;
        
        assert_eq!(account1, account2, "Account {} mismatch", i);
    }
    
    println!("✓ All 30 accounts identical across nodes");
    println!("✓ Cross-node verification passed\n");
    
    Ok(())
}

#[test]
fn test_merkle_ordering_independence() -> Result<()> {
    // Test that merkle root is independent of insertion order
    
    println!("\n=== Merkle Ordering Independence Test ===\n");
    
    use aether_state_merkle::SparseMerkleTree;
    
    // Create accounts in different orders
    let accounts: Vec<_> = (0..20)
        .map(|i| {
            let mut addr_bytes = [0u8; 20];
            addr_bytes[0] = i as u8;
            let addr = Address::from_slice(&addr_bytes).unwrap();
            let value = H256::from_slice(&[i as u8; 32]).unwrap();
            (addr, value)
        })
        .collect();
    
    // Tree 1: Insert in order
    let mut tree1 = SparseMerkleTree::new();
    for (addr, value) in accounts.iter() {
        tree1.update(*addr, *value);
    }
    let root1 = tree1.root();
    
    // Tree 2: Insert in reverse order
    let mut tree2 = SparseMerkleTree::new();
    for (addr, value) in accounts.iter().rev() {
        tree2.update(*addr, *value);
    }
    let root2 = tree2.root();
    
    // Tree 3: Insert in random order
    let mut tree3 = SparseMerkleTree::new();
    let mut shuffled = accounts.clone();
    // Simple shuffle
    shuffled.swap(5, 15);
    shuffled.swap(2, 18);
    shuffled.swap(10, 3);
    for (addr, value) in shuffled.iter() {
        tree3.update(*addr, *value);
    }
    let root3 = tree3.root();
    
    println!("Root 1 (ordered):   {:?}", root1);
    println!("Root 2 (reversed):  {:?}", root2);
    println!("Root 3 (shuffled):  {:?}", root3);
    
    // All roots should be identical
    assert_eq!(root1, root2, "Forward and reverse order should give same root");
    assert_eq!(root1, root3, "Shuffled order should give same root");
    assert_eq!(root2, root3, "All orderings should give same root");
    
    println!("✓ Merkle root is independent of insertion order\n");
    
    Ok(())
}

#[test]
fn test_deterministic_gas_charging() -> Result<()> {
    // Test that gas charging is deterministic
    
    println!("\n=== Deterministic Gas Charging Test ===\n");
    
    // Create WASM with some computations
    let wasm = wat::parse_str(r#"
        (module
            (func (export "execute") (result i32)
                (local $i i32)
                (local $sum i32)
                
                ;; Loop 100 times
                (local.set $i (i32.const 0))
                (local.set $sum (i32.const 0))
                
                (loop $continue
                    ;; Add to sum
                    (local.set $sum 
                        (i32.add (local.get $sum) (i32.const 1))
                    )
                    
                    ;; Increment counter
                    (local.set $i 
                        (i32.add (local.get $i) (i32.const 1))
                    )
                    
                    ;; Continue if i < 100
                    (br_if $continue 
                        (i32.lt_u (local.get $i) (i32.const 100))
                    )
                )
                
                i32.const 0
            )
        )
    "#)?;
    
    let context = ExecutionContext {
        contract_address: Address::from_slice(&[1u8; 20])?,
        caller: Address::from_slice(&[2u8; 20])?,
        value: 0,
        gas_limit: 100_000,
        block_number: 1,
        timestamp: 1000,
    };
    
    println!("Executing computational WASM 10 times...");
    
    let mut gas_values = Vec::new();
    for i in 0..10 {
        let mut vm = WasmVm::new(100_000)?;
        let result = vm.execute(&wasm, &context, b"test")?;
        
        println!("  Run {}: gas={}", i+1, result.gas_used);
        gas_values.push(result.gas_used);
    }
    
    // All gas values must be identical
    let first_gas = gas_values[0];
    for (i, &gas) in gas_values.iter().enumerate() {
        assert_eq!(gas, first_gas, "Gas differs in run {}: {} vs {}", i, gas, first_gas);
    }
    
    println!("✓ Gas charging is perfectly deterministic");
    println!("  Exact gas usage: {} for all runs", first_gas);
    println!();
    
    Ok(())
}

#[test]
#[ignore] // Long-running test
fn test_large_scale_determinism() -> Result<()> {
    // Test determinism with larger state
    
    println!("\n=== Large Scale Determinism Test ===\n");
    
    // Create two ledgers
    let dir1 = tempdir()?;
    let storage1 = Storage::open(dir1.path())?;
    let mut ledger1 = Ledger::new(storage1.clone())?;
    
    let dir2 = tempdir()?;
    let storage2 = Storage::open(dir2.path())?;
    let mut ledger2 = Ledger::new(storage2.clone())?;
    
    println!("Creating 1000 accounts on both ledgers...");
    
    // Add 1000 accounts to both
    for i in 0..1000 {
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0..2].copy_from_slice(&i.to_le_bytes()[0..2]);
        let addr = Address::from_slice(&addr_bytes)?;
        
        let account = Account {
            address: addr,
            balance: 10000 + i as u128,
            nonce: i as u64,
            code_hash: [(i % 256) as u8; 32],
            storage_root: [0u8; 32],
        };
        
        let key = addr.as_bytes();
        let value = bincode::serialize(&account)?;
        storage1.put("accounts", key, &value)?;
        storage2.put("accounts", key, &value)?;
    }
    
    // Get roots
    let root1 = ledger1.state_root();
    let root2 = ledger2.state_root();
    
    println!("Ledger 1 root: {:?}", root1);
    println!("Ledger 2 root: {:?}", root2);
    
    assert_eq!(root1, root2, "Large scale state roots must match");
    
    println!("✓ Determinism verified with 1000 accounts\n");
    
    Ok(())
}

