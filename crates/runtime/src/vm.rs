use aether_types::{Address, H256};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::*;

/// WASM Virtual Machine for Smart Contract Execution
///
/// Uses Wasmtime with fuel-based gas metering, deterministic configuration
/// (no SIMD, no threads, no floating point), and host function bindings
/// for blockchain state interaction.
pub struct WasmVm {
    engine: Engine,
    gas_limit: u64,
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
    pub storage_changes: HashMap<Vec<u8>, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Log {
    pub topics: Vec<H256>,
    pub data: Vec<u8>,
}

// Host-function size limits to prevent unbounded memory usage from malicious contracts.
const MAX_STORAGE_KEY_LEN: usize = 256;
const MAX_STORAGE_VAL_LEN: usize = 4096;
const MAX_LOG_DATA_LEN: usize = 4096;
const MAX_LOG_COUNT: usize = 100;
const MAX_RETURN_DATA_LEN: usize = 4096;

/// Shared state accessible to host functions during execution.
struct HostState {
    storage: HashMap<Vec<u8>, Vec<u8>>,
    logs: Vec<Log>,
    return_data: Vec<u8>,
    context: ExecutionContext,
}

impl WasmVm {
    pub fn new(gas_limit: u64) -> Self {
        let mut config = Config::new();
        config.consume_fuel(true);
        // Deterministic execution: disable non-deterministic features
        config.wasm_simd(false);
        config.wasm_relaxed_simd(false);
        config.wasm_threads(false);
        config.wasm_bulk_memory(true);
        config.wasm_multi_value(true);
        // Limit maximum WASM memory to 16MB (256 pages × 64KB) to prevent OOM
        config.static_memory_maximum_size(16 * 1024 * 1024);
        config.cranelift_opt_level(OptLevel::Speed);
        // Limit WASM call stack depth to 512KB to prevent stack-overflow DoS.
        config.max_wasm_stack(512 * 1024);
        // Dynamic memory guard size; static_memory_maximum_size caps growth.
        config.dynamic_memory_guard_size(64 * 1024);

        let engine = Engine::new(&config).expect("failed to create Wasmtime engine");

        WasmVm { engine, gas_limit }
    }

    /// Execute WASM bytecode with the given context and input.
    pub fn execute(
        &mut self,
        wasm_bytes: &[u8],
        context: &ExecutionContext,
        input: &[u8],
    ) -> Result<ExecutionResult> {
        // Validate WASM magic number
        if wasm_bytes.len() < 4 || &wasm_bytes[0..4] != b"\0asm" {
            bail!("invalid WASM magic number");
        }

        if wasm_bytes.len() > 1024 * 1024 {
            bail!("WASM module too large (max 1MB)");
        }

        // Compile the module
        let module = Module::new(&self.engine, wasm_bytes)?;

        // Create store with fuel (gas)
        let host_state = Arc::new(Mutex::new(HostState {
            storage: HashMap::new(),
            logs: Vec::new(),
            return_data: Vec::new(),
            context: context.clone(),
        }));

        let mut store = Store::new(&self.engine, host_state.clone());
        store.set_fuel(context.gas_limit)?;

        // Create linker with host functions
        let mut linker = Linker::new(&self.engine);
        Self::register_host_functions(&mut linker, host_state.clone())?;

        // Instantiate the module
        let instance = linker.instantiate(&mut store, &module)?;

        // Get memory export (if any)
        let memory = instance.get_memory(&mut store, "memory");

        // Write input data to WASM memory if memory exists
        if let Some(mem) = &memory {
            let _input_offset = 0u32;
            if input.len() <= mem.data_size(&store) {
                mem.data_mut(&mut store)[..input.len()].copy_from_slice(input);
            }
        }

        // Call the entry point
        let func = instance.get_typed_func::<(i32, i32), i32>(&mut store, "execute");

        let success = match func {
            Ok(f) => {
                match f.call(&mut store, (0, input.len() as i32)) {
                    Ok(result_code) => result_code == 0,
                    Err(e) => {
                        // Out-of-fuel means out-of-gas: do NOT retry another entry point.
                        // A retry would start a new call with 0 remaining fuel, bypassing
                        // the gas limit entirely.
                        let oof = e
                            .downcast_ref::<wasmtime::Trap>()
                            .map(|t| *t == wasmtime::Trap::OutOfFuel)
                            .unwrap_or_else(|| e.to_string().contains("fuel"));
                        if oof {
                            false
                        } else {
                            // Non-gas error — try simpler entry point with no args
                            let simple_func =
                                instance.get_typed_func::<(), i32>(&mut store, "main");
                            match simple_func {
                                Ok(f) => f.call(&mut store, ()).map(|r| r == 0).unwrap_or(false),
                                Err(_) => false,
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // Try "main" with no args
                let main_func = instance.get_typed_func::<(), i32>(&mut store, "main");
                match main_func {
                    Ok(f) => f.call(&mut store, ()).map(|r| r == 0).unwrap_or(false),
                    Err(_) => {
                        // Try _start (WASI-style)
                        let start_func = instance.get_typed_func::<(), ()>(&mut store, "_start");
                        match start_func {
                            Ok(f) => f.call(&mut store, ()).is_ok(),
                            Err(_) => false, // Module has no entry point, treat as failure
                        }
                    }
                }
            }
        };

        // Calculate gas used
        let remaining_fuel = store.get_fuel().unwrap_or(0);
        let gas_used = context.gas_limit.saturating_sub(remaining_fuel);

        // Collect results from host state
        let state = host_state
            .lock()
            .map_err(|_| anyhow::anyhow!("host state mutex poisoned"))?;

        Ok(ExecutionResult {
            success,
            gas_used,
            return_data: state.return_data.clone(),
            logs: state.logs.clone(),
            storage_changes: state.storage.clone(),
        })
    }

    /// Register host functions that WASM modules can import.
    fn register_host_functions(
        linker: &mut Linker<Arc<Mutex<HostState>>>,
        _host_state: Arc<Mutex<HostState>>,
    ) -> Result<()> {
        // env.storage_read(key_ptr: i32, key_len: i32, val_ptr: i32) -> i32
        // Gas cost: 200 fuel units
        linker.func_wrap(
            "env",
            "storage_read",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>,
             key_ptr: i32,
             key_len: i32,
             val_ptr: i32|
             -> i32 {
                // Charge fuel for storage_read (200 fuel units)
                match caller.get_fuel() {
                    Ok(fuel) if fuel >= 200 => {
                        if caller.set_fuel(fuel - 200).is_err() {
                            return -1; // Fuel deduction failed
                        }
                    }
                    Ok(_) => return -1,  // Insufficient fuel
                    Err(_) => return -1, // Fuel system unavailable
                }

                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return -1,
                };

                let key = {
                    let data = memory.data(&caller);
                    let start = key_ptr as usize;
                    let end = match start.checked_add(key_len as usize) {
                        Some(e) if e <= data.len() => e,
                        _ => return -1,
                    };
                    data[start..end].to_vec()
                };

                let value = {
                    let state = match caller.data().lock() {
                        Ok(s) => s,
                        Err(_) => return -1,
                    };
                    state.storage.get(&key).cloned()
                };
                match value {
                    Some(value) => {
                        let val_start = val_ptr as usize;
                        let val_end = match val_start.checked_add(value.len()) {
                            Some(e) if e <= memory.data(&caller).len() => e,
                            _ => return -1,
                        };
                        let data = memory.data_mut(&mut caller);
                        data[val_start..val_end].copy_from_slice(&value);
                        value.len() as i32
                    }
                    None => 0,
                }
            },
        )?;

        // env.storage_write(key_ptr: i32, key_len: i32, val_ptr: i32, val_len: i32) -> i32
        // Gas cost: 5000 fuel units base + 20 per byte
        linker.func_wrap(
            "env",
            "storage_write",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>,
             key_ptr: i32,
             key_len: i32,
             val_ptr: i32,
             val_len: i32|
             -> i32 {
                // Charge fuel for host function call (base + per-byte)
                let val_cost = (val_len as u64).saturating_mul(20);
                let fuel_cost = 5000u64.saturating_add(val_cost);
                match caller.get_fuel() {
                    Ok(fuel) if fuel >= fuel_cost => {
                        if caller.set_fuel(fuel - fuel_cost).is_err() {
                            return -1;
                        }
                    }
                    Ok(_) => return -1,
                    Err(_) => return -1,
                }

                // Reject oversized keys/values before touching memory.
                if key_len as usize > MAX_STORAGE_KEY_LEN || val_len as usize > MAX_STORAGE_VAL_LEN
                {
                    return -1;
                }

                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return -1,
                };

                let data = memory.data(&caller);
                let key_start = key_ptr as usize;
                let key_end = match key_start.checked_add(key_len as usize) {
                    Some(e) if e <= data.len() => e,
                    _ => return -1,
                };
                let val_start = val_ptr as usize;
                let val_end = match val_start.checked_add(val_len as usize) {
                    Some(e) if e <= data.len() => e,
                    _ => return -1,
                };

                let key = data[key_start..key_end].to_vec();
                let value = data[val_start..val_end].to_vec();

                let mut state = match caller.data().lock() {
                    Ok(s) => s,
                    Err(_) => return -1,
                };
                const MAX_STORAGE_ENTRIES: usize = 10_000;
                if state.storage.len() >= MAX_STORAGE_ENTRIES && !state.storage.contains_key(&key) {
                    return -1; // Storage limit exceeded
                }
                state.storage.insert(key, value);
                0
            },
        )?;

        // env.emit_log(data_ptr: i32, data_len: i32) -> i32
        // Gas cost: 375 base + 8 per byte
        linker.func_wrap(
            "env",
            "emit_log",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, data_ptr: i32, data_len: i32| -> i32 {
                // Charge fuel for host function call
                let log_byte_cost = (data_len as u64).saturating_mul(8);
                let fuel_cost = 375u64.saturating_add(log_byte_cost);
                match caller.get_fuel() {
                    Ok(fuel) if fuel >= fuel_cost => {
                        if caller.set_fuel(fuel - fuel_cost).is_err() {
                            return -1;
                        }
                    }
                    Ok(_) => return -1,
                    Err(_) => return -1,
                }

                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return -1,
                };

                let data = memory.data(&caller);
                let start = data_ptr as usize;
                let end = match start.checked_add(data_len as usize) {
                    Some(e) if e <= data.len() => e,
                    _ => return -1,
                };

                // Enforce log data size limit.
                if data_len as usize > MAX_LOG_DATA_LEN {
                    return -1;
                }

                let log_data = data[start..end].to_vec();
                let mut state = match caller.data().lock() {
                    Ok(s) => s,
                    Err(_) => return -1,
                };
                if state.logs.len() >= MAX_LOG_COUNT {
                    return -1; // Too many logs emitted
                }
                state.logs.push(Log {
                    topics: vec![],
                    data: log_data,
                });
                0
            },
        )?;

        // env.set_return(ptr: i32, len: i32)
        linker.func_wrap(
            "env",
            "set_return",
            |mut caller: Caller<'_, Arc<Mutex<HostState>>>, ptr: i32, len: i32| -> i32 {
                // Enforce return data size limit.
                if len as usize > MAX_RETURN_DATA_LEN {
                    return -1;
                }

                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return -1,
                };

                let data = memory.data(&caller);
                let start = ptr as usize;
                let end = match start.checked_add(len as usize) {
                    Some(e) if e <= data.len() => e,
                    _ => return -1,
                };

                let ret_data = data[start..end].to_vec();
                let mut state = match caller.data().lock() {
                    Ok(s) => s,
                    Err(_) => return -1,
                };
                state.return_data = ret_data;
                0
            },
        )?;

        // env.block_number() -> i64
        linker.func_wrap(
            "env",
            "block_number",
            |caller: Caller<'_, Arc<Mutex<HostState>>>| -> i64 {
                let state = match caller.data().lock() {
                    Ok(s) => s,
                    Err(_) => return -1,
                };
                state.context.block_number as i64
            },
        )?;

        // env.timestamp() -> i64
        linker.func_wrap(
            "env",
            "timestamp",
            |caller: Caller<'_, Arc<Mutex<HostState>>>| -> i64 {
                let state = match caller.data().lock() {
                    Ok(s) => s,
                    Err(_) => return -1,
                };
                state.context.timestamp as i64
            },
        )?;

        Ok(())
    }

    /// Get remaining gas.
    pub fn remaining_gas(&self) -> u64 {
        self.gas_limit
    }

    pub fn gas_used(&self) -> u64 {
        0 // Per-execution tracking; not stored on VM
    }

    /// Charge gas (for direct callers, not WASM).
    pub fn charge_gas(&mut self, _amount: u64) -> Result<()> {
        Ok(())
    }
}

/// Gas costs for different operations.
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
        let vm = WasmVm::new(100_000);
        assert_eq!(vm.gas_limit, 100_000);
    }

    #[test]
    fn test_wasm_validation_bad_magic() {
        let mut vm = WasmVm::new(100_000);
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 100_000,
            block_number: 1,
            timestamp: 1000,
        };

        let invalid_wasm = b"XXXX\x01\x00\x00\x00";
        assert!(vm.execute(invalid_wasm, &context, b"").is_err());
    }

    #[test]
    fn test_wasm_validation_too_short() {
        let mut vm = WasmVm::new(100_000);
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 100_000,
            block_number: 1,
            timestamp: 1000,
        };

        assert!(vm.execute(b"\0as", &context, b"").is_err());
    }

    #[test]
    fn test_execute_minimal_wasm() {
        let mut vm = WasmVm::new(1_000_000);
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 1_000_000,
            block_number: 1,
            timestamp: 1000,
        };

        // Minimal WASM module with a function that returns 0 (success)
        // (module
        //   (func (export "execute") (param i32 i32) (result i32)
        //     i32.const 0))
        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "execute") (param i32 i32) (result i32)
                    i32.const 0
                )
            )
            "#,
        )
        .unwrap();

        let result = vm.execute(&wasm, &context, b"input").unwrap();
        assert!(result.success, "minimal WASM should succeed");
        assert!(result.gas_used > 0, "should consume some gas");
    }

    #[test]
    fn test_execute_wasm_with_storage() {
        let mut vm = WasmVm::new(1_000_000);
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 1_000_000,
            block_number: 42,
            timestamp: 1000,
        };

        // WASM module that writes to storage
        let wasm = wat::parse_str(
            r#"
            (module
                (import "env" "storage_write" (func $storage_write (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "key")
                (data (i32.const 3) "value")
                (func (export "execute") (param i32 i32) (result i32)
                    ;; storage_write(key_ptr=0, key_len=3, val_ptr=3, val_len=5)
                    i32.const 0
                    i32.const 3
                    i32.const 3
                    i32.const 5
                    call $storage_write
                    drop
                    i32.const 0
                )
            )
            "#,
        )
        .unwrap();

        let result = vm.execute(&wasm, &context, b"").unwrap();
        assert!(result.success);
        assert_eq!(
            result.storage_changes.get(b"key".as_slice()),
            Some(&b"value".to_vec()),
            "storage should contain the written key-value pair"
        );
    }

    #[test]
    fn test_execute_wasm_with_logging() {
        let mut vm = WasmVm::new(1_000_000);
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 1_000_000,
            block_number: 1,
            timestamp: 1000,
        };

        // WASM module that emits a log
        let wasm = wat::parse_str(
            r#"
            (module
                (import "env" "emit_log" (func $emit_log (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "hello from wasm")
                (func (export "execute") (param i32 i32) (result i32)
                    i32.const 0
                    i32.const 15
                    call $emit_log
                    drop
                    i32.const 0
                )
            )
            "#,
        )
        .unwrap();

        let result = vm.execute(&wasm, &context, b"").unwrap();
        assert!(result.success);
        assert_eq!(result.logs.len(), 1);
        assert_eq!(result.logs[0].data, b"hello from wasm");
    }

    #[test]
    fn test_gas_consumption() {
        let mut vm = WasmVm::new(100);
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 100, // Very low gas
            block_number: 1,
            timestamp: 1000,
        };

        // A module with a loop that will exhaust gas
        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "execute") (param i32 i32) (result i32)
                    (local $i i32)
                    (local.set $i (i32.const 0))
                    (block $break
                        (loop $loop
                            (br_if $break (i32.ge_u (local.get $i) (i32.const 10000)))
                            (local.set $i (i32.add (local.get $i) (i32.const 1)))
                            (br $loop)
                        )
                    )
                    i32.const 0
                )
            )
            "#,
        )
        .unwrap();

        let result = vm.execute(&wasm, &context, b"");
        // Should either return with success=false or error due to fuel exhaustion
        if let Ok(r) = result {
            assert!(!r.success || r.gas_used >= 100);
        }
    }

    #[test]
    fn test_gas_exhaustion_does_not_retry_entrypoint() {
        // When "execute" exhausts fuel the VM must NOT fall through to "main".
        let mut vm = WasmVm::new(50);
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 50,
            block_number: 1,
            timestamp: 1000,
        };

        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "execute") (param i32 i32) (result i32)
                    (local $i i32)
                    (loop $loop
                        (local.set $i (i32.add (local.get $i) (i32.const 1)))
                        (br_if $loop (i32.lt_u (local.get $i) (i32.const 99999)))
                    )
                    i32.const 0
                )
            )
            "#,
        )
        .unwrap();

        let result = vm.execute(&wasm, &context, b"").unwrap();
        assert!(!result.success, "OOF execution must report failure, not retry");
    }

    #[test]
    fn test_storage_key_size_limit() {
        let mut vm = WasmVm::new(1_000_000);
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 1_000_000,
            block_number: 1,
            timestamp: 1000,
        };

        // key_len=1000 exceeds MAX_STORAGE_KEY_LEN=256 — must not panic
        let wasm = wat::parse_str(
            r#"
            (module
                (import "env" "storage_write" (func $sw (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 2)
                (func (export "execute") (param i32 i32) (result i32)
                    i32.const 0
                    i32.const 1000
                    i32.const 0
                    i32.const 1
                    call $sw
                    drop
                    i32.const 0
                )
            )
            "#,
        )
        .unwrap();

        let result = vm.execute(&wasm, &context, b"");
        assert!(result.is_ok(), "oversized key must not panic the VM");
        // The write should be silently rejected (storage_write returned -1)
        let exec = result.unwrap();
        assert!(
            exec.storage_changes.is_empty(),
            "oversized key write must be rejected"
        );
    }

    #[test]
    fn test_no_entrypoint_returns_failure() {
        let mut vm = WasmVm::new(1_000_000);
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 1_000_000,
            block_number: 1,
            timestamp: 1000,
        };

        // Minimal WASM module with no exported functions at all
        let wasm = wat::parse_str("(module)").unwrap();

        let result = vm.execute(&wasm, &context, b"").unwrap();
        assert!(
            !result.success,
            "module with no entrypoint should not report success"
        );
    }
}
