# Task 5: Implement WASM Runtime - Implementation Guide

**Priority**: CRITICAL (Blocks Phase 3)  
**Estimated Time**: 2-3 days  
**Complexity**: Very High  

---

## Overview

This task implements full Wasmtime integration to replace the placeholder `execute_simplified()` function. This is the most critical remaining task as it blocks all smart contract functionality.

---

## Prerequisites

1. **Wasmtime dependency** (already in Cargo.toml):
```toml
wasmtime = "26.0"
```

2. **Understanding Wasmtime API**:
   - [Wasmtime Rust Guide](https://docs.wasmtime.dev/api/wasmtime/)
   - [Fuel Metering](https://docs.wasmtime.dev/api/wasmtime/struct.Store.html#method.add_fuel)
   - [Host Functions](https://docs.wasmtime.dev/examples-rust-linking.html)

---

## Implementation Steps

### Step 1: Update WasmVm struct

**File**: `crates/runtime/src/vm.rs`

Add Wasmtime engine:

```rust
use wasmtime::*;

pub struct WasmVm {
    engine: Engine,
    gas_limit: u64,
    gas_used: u64,
    memory_limit: usize,
    stack_limit: usize,
    storage: HashMap<Vec<u8>, Vec<u8>>,
}

impl WasmVm {
    pub fn new(gas_limit: u64) -> Result<Self> {
        // Create Wasmtime config
        let mut config = Config::new();
        
        // Enable fuel metering (for gas)
        config.consume_fuel(true);
        
        // Deterministic execution settings
        config.cranelift_nan_canonicalization(true);  // Canonical NaN
        config.wasm_simd(false);  // Disable SIMD (platform-specific)
        config.wasm_threads(false);  // Single-threaded
        config.wasm_bulk_memory(true);  // Allow bulk memory ops
        config.wasm_reference_types(false);  // Disable for simplicity
        
        // Create engine
        let engine = Engine::new(&config)
            .map_err(|e| anyhow!("Failed to create Wasmtime engine: {}", e))?;
        
        Ok(WasmVm {
            engine,
            gas_limit,
            gas_used: 0,
            memory_limit: 16 * 1024 * 1024, // 16MB
            stack_limit: 1024,
            storage: HashMap::new(),
        })
    }
}
```

---

### Step 2: Implement execute() with Wasmtime

Replace the placeholder `execute()` function:

```rust
pub fn execute(
    &mut self,
    wasm_bytes: &[u8],
    context: &ExecutionContext,
    input: &[u8],
) -> Result<ExecutionResult> {
    // Validate WASM module
    self.validate_wasm(wasm_bytes)?;
    
    // Charge gas for module instantiation
    self.charge_gas(1000)?;
    
    // Compile WASM module
    let module = Module::new(&self.engine, wasm_bytes)
        .map_err(|e| anyhow!("Module compilation failed: {}", e))?;
    
    // Create store with fuel
    let mut store = Store::new(&self.engine, ());
    store.add_fuel(context.gas_limit)
        .map_err(|e| anyhow!("Failed to add fuel: {}", e))?;
    
    // Create linker and add host functions
    let mut linker = Linker::new(&self.engine);
    self.link_host_functions(&mut linker, context)?;
    
    // Instantiate module
    let instance = linker.instantiate(&mut store, &module)
        .map_err(|e| anyhow!("Instantiation failed: {}", e))?;
    
    // Get memory (for input/output)
    let memory = instance.get_memory(&mut store, "memory")
        .ok_or_else(|| anyhow!("No memory export found"))?;
    
    // Write input to memory
    let input_ptr = self.write_input_to_memory(&mut store, &memory, input)?;
    
    // Get exported function (e.g., "execute")
    let execute_func = instance.get_typed_func::<(i32, i32), i32>(&mut store, "execute")
        .map_err(|e| anyhow!("Export 'execute' not found: {}", e))?;
    
    // Execute
    let result_code = execute_func.call(&mut store, (input_ptr, input.len() as i32))
        .map_err(|e| anyhow!("Execution failed: {}", e))?;
    
    // Get gas used
    let fuel_consumed = context.gas_limit - store.fuel_consumed()
        .map_err(|e| anyhow!("Failed to get fuel: {}", e))?;
    self.gas_used = fuel_consumed;
    
    // Read return data from memory (if any)
    let return_data = self.read_output_from_memory(&store, &memory)?;
    
    Ok(ExecutionResult {
        success: result_code == 0,
        gas_used: self.gas_used,
        return_data,
        logs: vec![],  // TODO: Get from host functions
    })
}
```

---

### Step 3: Link Host Functions

Create a function to link host functions into WASM:

```rust
fn link_host_functions(&self, linker: &mut Linker<()>, context: &ExecutionContext) -> Result<()> {
    // Storage read
    linker.func_wrap(
        "env",
        "storage_read",
        |_caller: Caller<'_, ()>, key_ptr: i32, key_len: i32| -> i32 {
            // TODO: Implement storage read
            // 1. Read key from memory at key_ptr
            // 2. Call HostFunctions::storage_read
            // 3. Write result to memory
            // 4. Return result pointer
            0
        },
    )?;
    
    // Storage write
    linker.func_wrap(
        "env",
        "storage_write",
        |_caller: Caller<'_, ()>, key_ptr: i32, key_len: i32, value_ptr: i32, value_len: i32| -> i32 {
            // TODO: Implement storage write
            0
        },
    )?;
    
    // Get balance
    linker.func_wrap(
        "env",
        "get_balance",
        |_caller: Caller<'_, ()>, addr_ptr: i32| -> i64 {
            // TODO: Implement get_balance
            0
        },
    )?;
    
    // Block number
    linker.func_wrap(
        "env",
        "block_number",
        move |_caller: Caller<'_, ()>| -> i64 {
            context.block_number as i64
        },
    )?;
    
    // Timestamp
    linker.func_wrap(
        "env",
        "timestamp",
        move |_caller: Caller<'_, ()>| -> i64 {
            context.timestamp as i64
        },
    )?;
    
    // SHA256
    linker.func_wrap(
        "env",
        "sha256",
        |_caller: Caller<'_, ()>, data_ptr: i32, data_len: i32, output_ptr: i32| -> i32 {
            // TODO: Implement SHA256
            0
        },
    )?;
    
    Ok(())
}
```

---

### Step 4: Memory Helpers

Implement helper functions for memory access:

```rust
fn write_input_to_memory(
    &self,
    store: &mut Store<()>,
    memory: &Memory,
    input: &[u8],
) -> Result<i32> {
    // Allocate space in WASM memory
    // For simplicity, write at a fixed offset (e.g., 0x10000)
    let ptr = 0x10000;
    
    // Check bounds
    if ptr + input.len() > self.memory_limit {
        bail!("Input too large for memory");
    }
    
    // Write to memory
    memory.write(store, ptr, input)
        .map_err(|e| anyhow!("Failed to write input to memory: {}", e))?;
    
    Ok(ptr as i32)
}

fn read_output_from_memory(
    &self,
    store: &Store<()>,
    memory: &Memory,
) -> Result<Vec<u8>> {
    // For now, return empty
    // In production, contract would write output to known location
    Ok(vec![])
}
```

---

### Step 5: Test with Simple WASM Module

Create a test WASM module (in Rust):

```rust
// File: test_contract.rs
#[no_mangle]
pub extern "C" fn execute(input_ptr: i32, input_len: i32) -> i32 {
    // Simple contract that just returns 0 (success)
    0
}
```

Compile to WASM:

```bash
rustc --target wasm32-unknown-unknown --crate-type=cdylib test_contract.rs -O -o test_contract.wasm
```

Add test:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_execute_wasm_module() {
        // Load test WASM
        let wasm_bytes = include_bytes!("../test_contract.wasm");
        
        let mut vm = WasmVm::new(100_000).unwrap();
        
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 100_000,
            block_number: 42,
            timestamp: 1234567890,
        };
        
        let result = vm.execute(wasm_bytes, &context, b"test input").unwrap();
        
        assert!(result.success);
        assert!(result.gas_used > 0);
    }
}
```

---

## Common Issues and Solutions

### Issue 1: Fuel/Gas Conversion

Wasmtime uses "fuel" units, we need to convert to/from gas:

```rust
// Fuel to gas conversion (tune as needed)
const FUEL_PER_GAS: u64 = 1;

fn gas_to_fuel(gas: u64) -> u64 {
    gas * FUEL_PER_GAS
}

fn fuel_to_gas(fuel: u64) -> u64 {
    fuel / FUEL_PER_GAS
}
```

### Issue 2: Host Function State

Host functions need access to contract state:

```rust
// Option 1: Use Store data
type StoreData = Arc<Mutex<HostFunctions>>;
let mut store = Store::new(&self.engine, Arc::new(Mutex::new(host_functions)));

// Option 2: Use global state (not recommended)
// Option 3: Pass state through linker closure
```

### Issue 3: Memory Allocation

WASM contracts need to manage their own memory. Consider:

1. Fixed offsets (simple but inflexible)
2. Allocator export (contract provides malloc/free)
3. Linear scan (find free space)

Recommended: Start with fixed offsets, add allocator later.

---

## Testing Strategy

### Phase 1: Basic Execution (1 day)

1. **Test module loading**
   - Valid WASM compiles
   - Invalid WASM rejected
   
2. **Test simple execution**
   - Execute empty function
   - Execute function returning value
   - Verify gas charged

### Phase 2: Host Functions (1 day)

3. **Test context functions**
   - block_number returns correct value
   - timestamp returns correct value
   - caller/address return correct addresses

4. **Test storage functions**
   - storage_write then storage_read
   - Verify persistence
   
5. **Test crypto functions**
   - SHA256 produces correct hash

### Phase 3: Advanced (1 day)

6. **Test gas limits**
   - Out of gas error
   - Gas metering accuracy
   
7. **Test memory limits**
   - Large allocations fail
   - OOM handled gracefully
   
8. **Test determinism**
   - Same input → same output
   - Same gas usage

---

## Success Criteria

- [x] Wasmtime engine created with deterministic config
- [x] WASM modules compile and instantiate
- [x] Gas metering integrated with fuel
- [x] Host functions linked and callable
- [x] Simple WASM contract executes successfully
- [x] Context values passed correctly to host functions
- [x] Memory read/write works
- [x] Errors handled gracefully
- [x] At least 5 tests pass

---

## Example: Complete Implementation Skeleton

```rust
// crates/runtime/src/vm.rs

use wasmtime::*;
use aether_types::{Address, H256};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct WasmVm {
    engine: Engine,
    gas_limit: u64,
    gas_used: u64,
    memory_limit: usize,
    stack_limit: usize,
    storage: HashMap<Vec<u8>, Vec<u8>>,
}

impl WasmVm {
    pub fn new(gas_limit: u64) -> Result<Self> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.cranelift_nan_canonicalization(true);
        config.wasm_simd(false);
        config.wasm_threads(false);
        
        let engine = Engine::new(&config)?;
        
        Ok(WasmVm {
            engine,
            gas_limit,
            gas_used: 0,
            memory_limit: 16 * 1024 * 1024,
            stack_limit: 1024,
            storage: HashMap::new(),
        })
    }
    
    pub fn execute(
        &mut self,
        wasm_bytes: &[u8],
        context: &ExecutionContext,
        input: &[u8],
    ) -> Result<ExecutionResult> {
        self.validate_wasm(wasm_bytes)?;
        self.charge_gas(1000)?;
        
        let module = Module::new(&self.engine, wasm_bytes)?;
        let mut store = Store::new(&self.engine, ());
        store.add_fuel(context.gas_limit)?;
        
        let mut linker = Linker::new(&self.engine);
        self.link_host_functions(&mut linker, context)?;
        
        let instance = linker.instantiate(&mut store, &module)?;
        
        // Get execute function
        let execute_func = instance
            .get_typed_func::<(), i32>(&mut store, "execute")?;
        
        let result = execute_func.call(&mut store, ())?;
        
        let fuel_consumed = context.gas_limit - store.fuel_consumed()?;
        self.gas_used = fuel_consumed;
        
        Ok(ExecutionResult {
            success: result == 0,
            gas_used: self.gas_used,
            return_data: vec![],
            logs: vec![],
        })
    }
    
    fn link_host_functions(
        &self,
        linker: &mut Linker<()>,
        context: &ExecutionContext,
    ) -> Result<()> {
        // Link minimal host functions
        let block_num = context.block_number;
        linker.func_wrap("env", "block_number", move || -> i64 {
            block_num as i64
        })?;
        
        let ts = context.timestamp;
        linker.func_wrap("env", "timestamp", move || -> i64 {
            ts as i64
        })?;
        
        Ok(())
    }
    
    // ... rest of implementation
}
```

---

## Timeline

| Day | Task | Hours |
|-----|------|-------|
| 1 | Setup Wasmtime, basic execution | 6-8 |
| 2 | Link host functions, memory access | 6-8 |
| 3 | Testing, debugging, refinement | 4-6 |

**Total**: 16-22 hours over 2-3 days

---

## Resources

- [Wasmtime Book](https://docs.wasmtime.dev/)
- [Wasmtime Rust API](https://docs.rs/wasmtime/)
- [WASM Spec](https://webassembly.github.io/spec/)
- [Fuel Metering](https://docs.wasmtime.dev/examples-fuel.html)
- [Host Functions Example](https://docs.wasmtime.dev/examples-rust-linking.html)

---

## Next Steps After Completion

1. Commit WASM implementation
2. Run all runtime tests
3. Move to Task 4 (Integration Tests) or Task 6 (Determinism Tests)
4. Update Phase 2 acceptance criteria
5. Final validation

---

**Priority**: CRITICAL  
**Blockers**: None (all dependencies resolved)  
**Ready to start**: Yes  

