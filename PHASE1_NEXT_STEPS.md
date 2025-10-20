# Phase 1: Next Steps - Implementation Guide

## Current Status: Task 1 Complete ✅

**What We Just Finished:**
- Refactored runtime host functions to use `RuntimeState` abstraction
- Created `MockRuntimeState` for testing
- All 25 runtime tests passing
- Clean architecture ready for ledger integration

---

## TASK 2: Implement Ledger-Backed RuntimeState

### Goal
Connect the runtime to real blockchain state so WASM contracts can read/write persistent data.

### Step-by-Step Implementation

#### Step 1: Create the File (1 minute)
```bash
cd /Users/jadenfix/deets
touch crates/runtime/src/ledger_state.rs
```

#### Step 2: Add Basic Structure (10 minutes)
```rust
use aether_ledger::Ledger;
use aether_types::{Address, H256};
use anyhow::Result;
use std::collections::HashMap;

use crate::runtime_state::RuntimeState;

/// Ledger-backed Runtime State
///
/// Provides contract execution with access to persistent blockchain state.
/// Changes are cached and applied atomically after successful execution.
pub struct LedgerRuntimeState<'a> {
    /// Reference to the ledger
    ledger: &'a mut Ledger,
    
    /// Pending contract storage writes (not yet committed)
    pending_storage: HashMap<(Address, Vec<u8>), Vec<u8>>,
    
    /// Pending account balance changes
    pending_balances: HashMap<Address, i128>, // delta
    
    /// Emitted logs (collected during execution)
    logs: Vec<(Address, Vec<H256>, Vec<u8>)>,
}

impl<'a> LedgerRuntimeState<'a> {
    pub fn new(ledger: &'a mut Ledger) -> Self {
        Self {
            ledger,
            pending_storage: HashMap::new(),
            pending_balances: HashMap::new(),
            logs: Vec::new(),
        }
    }
    
    /// Commit all pending changes to the ledger
    pub fn commit(self) -> Result<Vec<(Address, Vec<H256>, Vec<u8>)>> {
        // TODO: Apply storage writes
        // TODO: Apply balance changes
        // Return logs
        Ok(self.logs)
    }
    
    /// Get logs without committing
    pub fn get_logs(&self) -> &[(Address, Vec<H256>, Vec<u8>)] {
        &self.logs
    }
}
```

#### Step 3: Implement RuntimeState Trait (30 minutes)
```rust
impl<'a> RuntimeState for LedgerRuntimeState<'a> {
    fn storage_read(&mut self, contract: &Address, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Check pending writes first
        if let Some(value) = self.pending_storage.get(&(*contract, key.to_vec())) {
            return Ok(Some(value.clone()));
        }
        
        // For Phase 1: contract storage not implemented in ledger yet
        // Return None (will implement in Phase 2)
        Ok(None)
    }
    
    fn storage_write(&mut self, contract: &Address, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        // Cache the write
        self.pending_storage.insert((*contract, key), value);
        Ok(())
    }
    
    fn get_balance(&self, address: &Address) -> Result<u128> {
        // Get base balance from ledger
        let account = self.ledger.get_account(address)?
            .unwrap_or_else(|| Account::new(*address));
        let mut balance = account.balance;
        
        // Apply pending delta
        if let Some(delta) = self.pending_balances.get(address) {
            if *delta < 0 {
                balance = balance.saturating_sub((-*delta) as u128);
            } else {
                balance = balance.saturating_add(*delta as u128);
            }
        }
        
        Ok(balance)
    }
    
    fn transfer(&mut self, from: &Address, to: &Address, amount: u128) -> Result<()> {
        // Check balance
        let from_balance = self.get_balance(from)?;
        if from_balance < amount {
            anyhow::bail!("insufficient balance");
        }
        
        // Record deltas
        let from_delta = self.pending_balances.entry(*from).or_insert(0);
        *from_delta -= amount as i128;
        
        let to_delta = self.pending_balances.entry(*to).or_insert(0);
        *to_delta += amount as i128;
        
        Ok(())
    }
    
    fn emit_log(&mut self, contract: &Address, topics: Vec<H256>, data: Vec<u8>) -> Result<()> {
        self.logs.push((*contract, topics, data));
        Ok(())
    }
}
```

#### Step 4: Add Tests (30 minutes)
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use aether_state_storage::Storage;
    use tempfile::TempDir;
    
    #[test]
    fn test_ledger_state_balance() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();
        
        // Give account some balance through ledger
        let addr = Address::from_slice(&[1u8; 20]).unwrap();
        let account = Account::with_balance(addr, 1000);
        // TODO: Use proper ledger API to set balance
        
        let mut state = LedgerRuntimeState::new(&mut ledger);
        
        assert_eq!(state.get_balance(&addr).unwrap(), 1000);
    }
    
    #[test]
    fn test_ledger_state_transfer() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();
        
        let addr1 = Address::from_slice(&[1u8; 20]).unwrap();
        let addr2 = Address::from_slice(&[2u8; 20]).unwrap();
        
        // Setup initial balance
        // TODO: Use proper ledger API
        
        let mut state = LedgerRuntimeState::new(&mut ledger);
        
        // Transfer
        state.transfer(&addr1, &addr2, 300).unwrap();
        
        // Check pending balances
        assert_eq!(state.get_balance(&addr1).unwrap(), 700);
        assert_eq!(state.get_balance(&addr2).unwrap(), 300);
        
        // Commit and verify
        state.commit().unwrap();
        // TODO: Verify ledger was updated
    }
    
    #[test]
    fn test_ledger_state_storage() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();
        
        let contract = Address::from_slice(&[1u8; 20]).unwrap();
        let mut state = LedgerRuntimeState::new(&mut ledger);
        
        // Write
        state.storage_write(&contract, b"key".to_vec(), b"value".to_vec()).unwrap();
        
        // Read (should get from cache)
        let value = state.storage_read(&contract, b"key").unwrap();
        assert_eq!(value, Some(b"value".to_vec()));
    }
    
    #[test]
    fn test_ledger_state_logs() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::open(temp_dir.path()).unwrap();
        let mut ledger = Ledger::new(storage).unwrap();
        
        let contract = Address::from_slice(&[1u8; 20]).unwrap();
        let mut state = LedgerRuntimeState::new(&mut ledger);
        
        // Emit log
        state.emit_log(&contract, vec![H256::zero()], b"data".to_vec()).unwrap();
        
        let logs = state.get_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].0, contract);
    }
}
```

#### Step 5: Wire into Runtime Module (5 minutes)
```rust
// In crates/runtime/src/lib.rs
pub mod ledger_state;

pub use ledger_state::LedgerRuntimeState;
```

#### Step 6: Update Runtime Dependencies (2 minutes)
```toml
# In crates/runtime/Cargo.toml
[dependencies]
# ... existing deps ...
aether-ledger = { path = "../ledger" }
aether-state-storage = { path = "../state/storage" }
```

#### Step 7: Run Tests (continuous)
```bash
# Test as you go
cargo test --package aether-runtime --lib ledger_state

# Full runtime test suite
cargo test --package aether-runtime --lib

# Integration test with ledger
cargo test --package aether-ledger
```

### Expected Challenges

1. **Ledger API for Account Updates**
   - Current ledger uses transactions, not direct account updates
   - **Solution**: Add helper methods to ledger or use internal batch API

2. **Contract Storage in Ledger**
   - Not yet implemented in ledger
   - **Solution**: For Phase 1, keep in-memory (migrate to RocksDB in Phase 2)

3. **Atomic Commits**
   - Need all-or-nothing semantics
   - **Solution**: Use `StorageBatch` for atomic writes

### Acceptance Criteria

- [ ] Can read account balances from ledger
- [ ] Transfers update pending balances correctly
- [ ] Contract storage read/write works (in-memory for now)
- [ ] Logs are collected
- [ ] `commit()` applies all changes atomically
- [ ] All tests pass
- [ ] No performance regression

### Time Estimate
- Implementation: 1.5-2 hours
- Testing: 30-45 minutes
- Debugging: 30-60 minutes
- **Total**: 2.5-3.5 hours

---

## TASK 3: Wire Host Functions to Wasmtime

### Goal
Make host functions callable from WASM code by injecting them into Wasmtime linker.

### Background
Wasmtime requires host functions to be `'static` or carefully managed with lifetimes. We need to create closures that:
1. Access the host functions
2. Read/write WASM memory
3. Handle errors
4. Track gas

### Implementation Strategy

#### Option A: Pass State as Store Data (Recommended)
```rust
// Store state in Wasmtime's store
struct StoreState<'a> {
    runtime_state: &'a mut dyn RuntimeState,
    gas_used: u64,
    gas_limit: u64,
}

let mut store = Store::new(&engine, StoreState {
    runtime_state: state,
    gas_used: 0,
    gas_limit: context.gas_limit,
});

// Host functions can access store.data()
linker.func_wrap("env", "storage_read",
    |mut caller: Caller<'_, StoreState>, key_ptr: i32, key_len: i32| -> i32 {
        let store_data = caller.data_mut();
        // Read key from memory
        let memory = caller.get_export("memory").unwrap().into_memory().unwrap();
        let key = memory.data(&caller)[key_ptr..key_ptr+key_len].to_vec();
        
        // Call through to state
        match store_data.runtime_state.storage_read(...) {
            Ok(Some(value)) => {
                // Write value to memory
                // Return pointer
            }
            Ok(None) => 0, // NULL
            Err(_) => -1, // Error
        }
    }
)?;
```

#### Step-by-Step

1. **Update VM Structure** (30 min)
   ```rust
   struct VmStoreData<'a> {
       state: &'a mut dyn RuntimeState,
       context: ExecutionContext,
       gas_used: u64,
   }
   
   impl WasmVm {
       pub fn execute_with_state(
           &mut self,
           wasm_bytes: &[u8],
           context: &ExecutionContext,
           state: &mut dyn RuntimeState,
       ) -> Result<ExecutionResult> {
           // ... setup engine/config ...
           
           let mut store = Store::new(&engine, VmStoreData {
               state,
               context: context.clone(),
               gas_used: 0,
           });
           
           // ... rest of setup ...
       }
   }
   ```

2. **Implement Memory Helpers** (20 min)
   ```rust
   fn read_bytes_from_wasm(
       caller: &Caller<'_, VmStoreData>,
       ptr: i32,
       len: i32,
   ) -> Result<Vec<u8>> {
       let memory = caller.get_export("memory")
           .ok_or_else(|| anyhow!("no memory export"))?
           .into_memory()
           .ok_or_else(|| anyhow!("not a memory"))?;
       
       let data = memory.data(caller);
       let start = ptr as usize;
       let end = start + len as usize;
       
       if end > data.len() {
           anyhow::bail!("memory out of bounds");
       }
       
       Ok(data[start..end].to_vec())
   }
   
   fn write_bytes_to_wasm(
       caller: &mut Caller<'_, VmStoreData>,
       ptr: i32,
       bytes: &[u8],
   ) -> Result<()> {
       // Similar implementation
       Ok(())
   }
   ```

3. **Add Host Functions** (1-2 hours)
   ```rust
   // Storage read
   linker.func_wrap("env", "storage_read",
       |mut caller: Caller<'_, VmStoreData>, 
        key_ptr: i32, key_len: i32,
        out_ptr_ptr: i32| -> i32 {
           // Read key
           let key = read_bytes_from_wasm(&caller, key_ptr, key_len)?;
           
           // Get state
           let data = caller.data_mut();
           let contract = &data.context.contract_address;
           
           // Call storage_read
           match data.state.storage_read(contract, &key) {
               Ok(Some(value)) => {
                   // Allocate in WASM, write value, return ptr
                   // This is complex - might need allocator in WASM
                   todo!("allocate and write")
               }
               Ok(None) => 0,
               Err(_) => -1,
           }
       }
   )?;
   
   // Similar for other functions...
   ```

4. **Handle Gas in Host Calls** (30 min)
   ```rust
   fn charge_gas(caller: &mut Caller<'_, VmStoreData>, amount: u64) -> Result<()> {
       let data = caller.data_mut();
       data.gas_used += amount;
       if data.gas_used > data.context.gas_limit {
           anyhow::bail!("out of gas");
       }
       Ok(())
   }
   ```

### Simplified Approach for Phase 1

Instead of full WASM memory management, implement basic host functions that return via fixed buffers:

```rust
// Simple API: host functions work with pre-allocated buffers
linker.func_wrap("env", "get_caller",
    |mut caller: Caller<'_, VmStoreData>| -> i64 {
        let data = caller.data();
        // Return address as i64 (first 8 bytes)
        i64::from_le_bytes(data.context.caller.as_bytes()[0..8].try_into().unwrap())
    }
)?;

linker.func_wrap("env", "get_balance",
    |mut caller: Caller<'_, VmStoreData>, addr_low: i64, addr_high: i32| -> i64 {
        let data = caller.data_mut();
        // Reconstruct address
        let mut addr_bytes = [0u8; 20];
        addr_bytes[0..8].copy_from_slice(&addr_low.to_le_bytes());
        addr_bytes[8..12].copy_from_slice(&addr_high.to_le_bytes()[0..4]);
        
        let address = Address::from_slice(&addr_bytes).unwrap();
        match data.state.get_balance(&address) {
            Ok(balance) => balance as i64,
            Err(_) => -1,
        }
    }
)?;
```

### Time Estimate
- Implementation: 2-3 hours
- Testing with WASM: 1-2 hours
- Debugging: 1-2 hours
- **Total**: 4-7 hours

### Acceptance Criteria

- [ ] WASM can call `get_caller()`
- [ ] WASM can call `get_balance(address)`
- [ ] WASM can call `transfer(from, to, amount)`
- [ ] Gas is tracked across host calls
- [ ] Errors propagate correctly
- [ ] Tests with simple WASM contracts pass

---

## Testing Strategy

### Unit Tests
```bash
# Test each component
cargo test --package aether-runtime --lib ledger_state
cargo test --package aether-runtime --lib vm
cargo test --package aether-runtime --lib host_functions
```

### Integration Tests
```rust
#[test]
fn test_wasm_execution_with_ledger() {
    // Setup ledger with accounts
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::open(temp_dir.path()).unwrap();
    let mut ledger = Ledger::new(storage).unwrap();
    
    // Create runtime state
    let mut state = LedgerRuntimeState::new(&mut ledger);
    
    // Execute WASM
    let mut vm = WasmVm::new(100_000);
    let wasm = compile_test_contract(); // Calls host functions
    let context = ExecutionContext { /* ... */ };
    
    let result = vm.execute_with_state(&wasm, &context, &mut state).unwrap();
    
    assert!(result.success);
    assert!(result.gas_used > 0);
    
    // Verify state changes
    state.commit().unwrap();
    // Check ledger was updated
}
```

### Debugging Tips
```bash
# Run with output
cargo test --package aether-runtime -- --nocapture

# Run specific test
cargo test --package aether-runtime test_wasm_execution_with_ledger

# Check compilation
cargo check --package aether-runtime
```

---

## Timeline

| Day | Task | Hours | Cumulative |
|-----|------|-------|------------|
| 1 | Task 2: LedgerRuntimeState | 3 | 3 |
| 2 | Task 3: Wasmtime integration (basic) | 4 | 7 |
| 3 | Task 3: Complete host functions | 3 | 10 |
| 4 | Testing & debugging | 2 | 12 |

**Total**: ~12 hours for runtime completion

---

## What Success Looks Like

After Tasks 2 & 3:
```rust
// This should work:
#[test]
fn test_end_to_end_contract_execution() {
    // Setup real ledger
    let mut ledger = setup_test_ledger();
    let addr1 = setup_account_with_balance(&mut ledger, 1000);
    let addr2 = create_empty_account();
    
    // Deploy and execute contract
    let contract_wasm = compile_simple_transfer_contract();
    let mut vm = WasmVm::new(100_000);
    
    // Execute: transfer 300 from addr1 to addr2
    let mut state = LedgerRuntimeState::new(&mut ledger);
    let result = vm.execute_with_state(
        &contract_wasm,
        &ExecutionContext {
            caller: addr1,
            contract_address: contract_addr,
            // ... other context
        },
        &mut state,
    ).unwrap();
    
    // Verify execution
    assert!(result.success);
    assert!(result.gas_used > 0 && result.gas_used < 100_000);
    
    // Verify state changes
    assert_eq!(state.get_balance(&addr1).unwrap(), 700);
    assert_eq!(state.get_balance(&addr2).unwrap(), 300);
    
    // Commit to ledger
    state.commit().unwrap();
    
    // Verify persistence
    assert_eq!(ledger.get_account(&addr1).unwrap().balance, 700);
    assert_eq!(ledger.get_account(&addr2).unwrap().balance, 300);
}
```

---

## Resources

### Wasmtime Documentation
- [Rust Embedding Guide](https://docs.wasmtime.dev/api/wasmtime/)
- [Linker API](https://docs.rs/wasmtime/latest/wasmtime/struct.Linker.html)
- [Store and Caller](https://docs.rs/wasmtime/latest/wasmtime/struct.Caller.html)

### Example Projects
- [Wasmtime Examples](https://github.com/bytecodealliance/wasmtime/tree/main/examples)
- [Hello World Host Functions](https://github.com/bytecodealliance/wasmtime/blob/main/examples/hello.rs)

### Testing
- Compile simple WASM: `wat2wasm simple.wat -o simple.wasm`
- WAT example:
  ```wat
  (module
    (import "env" "get_caller" (func $get_caller (result i64)))
    (export "main" (func $main))
    (func $main
      call $get_caller
      drop
    )
  )
  ```

---

## Next Action

**Start Task 2 now:**
```bash
cd /Users/jadenfix/deets
touch crates/runtime/src/ledger_state.rs
# Open in editor and paste the template above
```

**Command to test:**
```bash
cargo test --package aether-runtime --lib
```

**Expected Result:**
All existing tests continue to pass, new `ledger_state` tests added and passing.

---

**Questions?** Refer to:
- `PHASE1_AUDIT.md` - Overall audit and remaining work
- `PHASE1_PROGRESS_SUMMARY.md` - What we just completed
- This file - Detailed implementation guide

Good luck! 🚀

