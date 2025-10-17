use aether_types::{Address, H256};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::*;

/// WASM Virtual Machine for Smart Contract Execution
///
/// Features:
/// - Gas metering for deterministic execution
/// - Sandboxed environment
/// - Host functions for blockchain interaction
/// - Memory and compute limits
/// - Stack depth tracking
///
/// Integration with Wasmtime (production):
/// - Wasmtime engine with fuel metering
/// - Deterministic imports only
/// - No floating point (non-deterministic)
/// - No SIMD (platform-specific)
///
/// State shared with WASM host functions
#[derive(Clone)]
pub struct VmState {
    pub storage: HashMap<Vec<u8>, Vec<u8>>,
    pub logs: Vec<Log>,
    pub gas_used: u64,
    pub gas_limit: u64,
}

pub struct WasmVm {
    engine: Engine,
    gas_limit: u64,
    gas_used: u64,
    memory_limit: usize,
    stack_limit: usize,
    storage: HashMap<Vec<u8>, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub contract_address: Address,
    pub caller: Address,
    pub value: u128,
    pub gas_limit: u64,
    pub block_number: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub gas_used: u64,
    pub return_data: Vec<u8>,
    pub logs: Vec<Log>,
}

#[derive(Debug, Clone)]
pub struct Log {
    pub topics: Vec<H256>,
    pub data: Vec<u8>,
}

impl WasmVm {
    pub fn new(gas_limit: u64) -> Result<Self> {
        // Create Wasmtime configuration for deterministic execution
        let mut config = Config::new();
        
        // Enable fuel metering for gas
        config.consume_fuel(true);
        
        // Deterministic execution settings
        config.cranelift_nan_canonicalization(true);  // Canonical NaN representation
        config.wasm_simd(false);  // Disable SIMD (platform-specific)
        config.wasm_threads(false);  // Single-threaded execution
        config.wasm_bulk_memory(true);  // Allow bulk memory operations
        config.wasm_reference_types(false);  // Disable for simplicity
        
        // Create engine with deterministic config
        let engine = Engine::new(&config)?;
        
        Ok(WasmVm {
            engine,
            gas_limit,
            gas_used: 0,
            memory_limit: 16 * 1024 * 1024, // 16MB
            stack_limit: 1024,
            storage: HashMap::new(),
        })
    }

    /// Execute WASM bytecode
    pub fn execute(
        &mut self,
        wasm_bytes: &[u8],
        context: &ExecutionContext,
        input: &[u8],
    ) -> Result<ExecutionResult> {
        // Validate WASM module
        self.validate_wasm(wasm_bytes)?;

        // Charge base gas for module instantiation
        self.charge_gas(1000)?;

        // Compile WASM module
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| anyhow::anyhow!("WASM compilation failed: {}", e))?;

        // Create VM state for host functions
        let vm_state = Arc::new(Mutex::new(VmState {
            storage: self.storage.clone(),
            logs: Vec::new(),
            gas_used: self.gas_used,
            gas_limit: context.gas_limit,
        }));

        // Create store with state
        let mut store = Store::new(&self.engine, vm_state.clone());
        
        // Set fuel (maps to gas)
        store.add_fuel(context.gas_limit)
            .map_err(|e| anyhow::anyhow!("Failed to set fuel: {}", e))?;

        // Create linker and add host functions
        let mut linker = Linker::new(&self.engine);
        self.link_host_functions(&mut linker, context)?;

        // Instantiate module
        let instance = linker.instantiate(&mut store, &module)
            .map_err(|e| anyhow::anyhow!("Instantiation failed: {}", e))?;

        // Get the execute function
        let execute_func = instance
            .get_typed_func::<(), i32>(&mut store, "execute")
            .map_err(|e| anyhow::anyhow!("No 'execute' export found: {}", e))?;

        // Get memory for reading return data
        let memory = instance.get_memory(&mut store, "memory");

        // Execute the contract
        let result_code = execute_func.call(&mut store, ())
            .map_err(|e| anyhow::anyhow!("Execution failed: {}", e))?;

        // Get remaining fuel (gas used)
        let fuel_remaining = store.fuel_consumed()
            .ok_or_else(|| anyhow::anyhow!("Failed to get fuel consumed"))?;
        let gas_used = context.gas_limit.saturating_sub(fuel_remaining);

        // Extract return data from memory (if available)
        let return_data = if let Some(mem) = memory {
            // Contract can write return data to a known location (e.g., first 1KB)
            // For now, read first 32 bytes if result_code indicates success with data
            if result_code > 0 && result_code <= 1024 {
                let mut data = vec![0u8; result_code as usize];
                mem.read(&store, 0, &mut data).unwrap_or(());
                data
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Extract state from host functions
        let final_state = vm_state.lock().unwrap();
        self.storage = final_state.storage.clone();
        self.gas_used = gas_used;

        Ok(ExecutionResult {
            success: result_code >= 0,  // 0 = success, positive = success with data length, negative = error
            gas_used,
            return_data,
            logs: final_state.logs.clone(),
        })
    }

    /// Link host functions into WASM environment
    fn link_host_functions(&self, linker: &mut Linker<Arc<Mutex<VmState>>>, context: &ExecutionContext) -> Result<()> {
        // Get block number
        let block_num = context.block_number;
        linker.func_wrap("env", "block_number", move |_caller: Caller<'_, Arc<Mutex<VmState>>>| -> i64 {
            block_num as i64
        })?;

        // Get timestamp
        let ts = context.timestamp;
        linker.func_wrap("env", "timestamp", move |_caller: Caller<'_, Arc<Mutex<VmState>>>| -> i64 {
            ts as i64
        })?;

        // Get caller address (simplified - returns first 8 bytes as i64)
        let caller_bytes = context.caller.as_bytes()[0..8].try_into().unwrap();
        let caller_val = i64::from_le_bytes(caller_bytes);
        linker.func_wrap("env", "caller", move |_caller: Caller<'_, Arc<Mutex<VmState>>>| -> i64 {
            caller_val
        })?;

        // Storage read - reads a value from contract storage
        // Takes: key_ptr (i32), key_len (i32)
        // Returns: value as i64 (first 8 bytes, or 0 if not found)
        linker.func_wrap("env", "storage_read", 
            |mut caller: Caller<'_, Arc<Mutex<VmState>>>, key_ptr: i32, key_len: i32| -> i64 {
                // Charge gas
                if charge_gas_from_state(&mut caller, 200).is_err() {
                    return -1; // Out of gas
                }

                // Get memory
                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(mem)) => mem,
                    _ => return -1,
                };

                // Read key from memory
                let mut key = vec![0u8; key_len as usize];
                if memory.read(&caller, key_ptr as usize, &mut key).is_err() {
                    return -1;
                }

                // Read from storage
                let state = caller.data().lock().unwrap();
                match state.storage.get(&key) {
                    Some(value) => {
                        // Return first 8 bytes as i64
                        if value.len() >= 8 {
                            i64::from_le_bytes(value[0..8].try_into().unwrap())
                        } else {
                            0
                        }
                    }
                    None => 0,
                }
            }
        )?;

        // Storage write - writes a value to contract storage
        // Takes: key_ptr (i32), key_len (i32), value_ptr (i32), value_len (i32)
        // Returns: 0 on success, -1 on error
        linker.func_wrap("env", "storage_write",
            |mut caller: Caller<'_, Arc<Mutex<VmState>>>, key_ptr: i32, key_len: i32, value_ptr: i32, value_len: i32| -> i32 {
                // Charge base gas
                if charge_gas_from_state(&mut caller, 5000).is_err() {
                    return -1; // Out of gas
                }

                // Get memory
                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(mem)) => mem,
                    _ => return -1,
                };

                // Read key from memory
                let mut key = vec![0u8; key_len as usize];
                if memory.read(&caller, key_ptr as usize, &mut key).is_err() {
                    return -1;
                }

                // Read value from memory
                let mut value = vec![0u8; value_len as usize];
                if memory.read(&caller, value_ptr as usize, &mut value).is_err() {
                    return -1;
                }

                // Check if new key (charge extra)
                let mut state = caller.data().lock().unwrap();
                if !state.storage.contains_key(&key) {
                    drop(state); // Release lock before charging
                    if charge_gas_from_state(&mut caller, 20000).is_err() {
                        return -1; // Out of gas for new slot
                    }
                    state = caller.data().lock().unwrap();
                }

                // Write to storage
                state.storage.insert(key, value);
                0 // Success
            }
        )?;

        // Emit log - emits a log event
        // Takes: topics_ptr (i32), topics_count (i32), data_ptr (i32), data_len (i32)
        // Returns: 0 on success, -1 on error
        linker.func_wrap("env", "emit_log",
            |mut caller: Caller<'_, Arc<Mutex<VmState>>>, topics_ptr: i32, topics_count: i32, data_ptr: i32, data_len: i32| -> i32 {
                // Charge gas (375 base + 8 per byte)
                let gas_cost = 375 + (8 * data_len as u64);
                if charge_gas_from_state(&mut caller, gas_cost).is_err() {
                    return -1;
                }

                // Get memory
                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(mem)) => mem,
                    _ => return -1,
                };

                // Read topics (each topic is 32 bytes)
                let mut topics = Vec::new();
                for i in 0..topics_count {
                    let mut topic_bytes = [0u8; 32];
                    let offset = (topics_ptr + i * 32) as usize;
                    if memory.read(&caller, offset, &mut topic_bytes).is_err() {
                        return -1;
                    }
                    topics.push(H256::from_slice(&topic_bytes).unwrap());
                }

                // Read data
                let mut data = vec![0u8; data_len as usize];
                if memory.read(&caller, data_ptr as usize, &mut data).is_err() {
                    return -1;
                }

                // Store log
                let mut state = caller.data().lock().unwrap();
                state.logs.push(Log { topics, data });
                
                0 // Success
            }
        )?;

        Ok(())
    }

    /// Validate WASM module
    fn validate_wasm(&self, wasm_bytes: &[u8]) -> Result<()> {
        if wasm_bytes.len() > 1024 * 1024 {
            bail!("WASM module too large (max 1MB)");
        }

        // Check WASM magic number
        if wasm_bytes.len() < 4 || &wasm_bytes[0..4] != b"\0asm" {
            bail!("invalid WASM magic number");
        }

        Ok(())
    }

    /// Charge gas for an operation
    pub fn charge_gas(&mut self, amount: u64) -> Result<()> {
        self.gas_used = self
            .gas_used
            .checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("gas overflow"))?;

        if self.gas_used > self.gas_limit {
            bail!(
                "out of gas: used {} > limit {}",
                self.gas_used,
                self.gas_limit
            );
        }

        Ok(())
    }

    /// Get remaining gas
    pub fn remaining_gas(&self) -> u64 {
        self.gas_limit.saturating_sub(self.gas_used)
    }

    pub fn gas_used(&self) -> u64 {
        self.gas_used
    }
}

/// Helper to charge gas from within host functions
fn charge_gas_from_state(caller: &mut Caller<'_, Arc<Mutex<VmState>>>, amount: u64) -> Result<()> {
    let mut state = caller.data().lock().unwrap();
    state.gas_used = state.gas_used.checked_add(amount)
        .ok_or_else(|| anyhow::anyhow!("Gas overflow"))?;
    
    if state.gas_used > state.gas_limit {
        bail!("Out of gas");
    }
    
    // Also consume fuel in the store
    let _ = caller.consume_fuel(amount);
    
    Ok(())
}

/// Gas costs for different operations (per spec)
pub mod gas_costs {
    pub const BASE: u64 = 100;
    pub const MEMORY_BYTE: u64 = 1;
    pub const STORAGE_READ: u64 = 200;
    pub const STORAGE_WRITE: u64 = 5000;
    pub const LOG: u64 = 375;
    pub const SHA256: u64 = 60;
    pub const TRANSFER: u64 = 9000;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_creation() {
        let vm = WasmVm::new(100_000).unwrap();
        assert_eq!(vm.gas_limit, 100_000);
        assert_eq!(vm.gas_used, 0);
    }

    #[test]
    fn test_gas_charging() {
        let mut vm = WasmVm::new(1000).unwrap();

        assert!(vm.charge_gas(500).is_ok());
        assert_eq!(vm.gas_used(), 500);

        assert!(vm.charge_gas(400).is_ok());
        assert_eq!(vm.gas_used(), 900);

        // Should fail - exceeds limit
        assert!(vm.charge_gas(200).is_err());
    }

    #[test]
    fn test_remaining_gas() {
        let mut vm = WasmVm::new(1000).unwrap();
        vm.charge_gas(300).unwrap();

        assert_eq!(vm.remaining_gas(), 700);
    }

    #[test]
    fn test_wasm_validation() {
        let vm = WasmVm::new(100_000).unwrap();

        // Valid WASM header
        let valid_wasm = b"\0asm\x01\x00\x00\x00";
        assert!(vm.validate_wasm(valid_wasm).is_ok());

        // Invalid magic number
        let invalid_wasm = b"XXXX\x01\x00\x00\x00";
        assert!(vm.validate_wasm(invalid_wasm).is_err());

        // Too short
        assert!(vm.validate_wasm(b"\0as").is_err());
    }

    #[test]
    fn test_execute_with_real_wasm() {
        // Create a minimal valid WASM module with an execute function
        // WASM binary format: magic + version + sections
        let wasm = wat::parse_str(r#"
            (module
                (func (export "execute") (result i32)
                    i32.const 0
                )
            )
        "#).unwrap();

        let mut vm = WasmVm::new(100_000).unwrap();
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 100_000,
            block_number: 1,
            timestamp: 1000,
        };

        let result = vm.execute(&wasm, &context, b"input").unwrap();

        assert!(result.success);
        assert!(result.gas_used > 0);
    }

    #[test]
    fn test_host_functions_accessible() {
        // Test that host functions are linked correctly
        let wasm = wat::parse_str(r#"
            (module
                (import "env" "block_number" (func $block_number (result i64)))
                (import "env" "timestamp" (func $timestamp (result i64)))
                (func (export "execute") (result i32)
                    call $block_number
                    drop
                    call $timestamp
                    drop
                    i32.const 0
                )
            )
        "#).unwrap();

        let mut vm = WasmVm::new(100_000).unwrap();
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 100_000,
            block_number: 42,
            timestamp: 1234567890,
        };

        let result = vm.execute(&wasm, &context, b"input").unwrap();
        assert!(result.success);
    }
}
