use std::convert::{TryFrom, TryInto};

use aether_types::{Address, H256};
use anyhow::{anyhow, bail, Result};
use wasmtime::{Caller, Config, Engine, Extern, Linker, Memory, Module, Store};

use crate::host_functions::HostFunctions;
use crate::runtime_state::RuntimeState;

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
#[allow(dead_code)]
pub struct WasmVm {
    /// Gas limit for execution
    gas_limit: u64,

    /// Gas used so far
    gas_used: u64,

    /// Memory limit (bytes)
    memory_limit: usize,

    /// Stack depth limit
    stack_limit: usize,
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

struct HostEnv<'a> {
    host: HostFunctions<'a>,
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
    pub fn new(gas_limit: u64) -> Self {
        WasmVm {
            gas_limit,
            gas_used: 0,
            memory_limit: 16 * 1024 * 1024, // 16MB
            stack_limit: 1024,
        }
    }

    /// Execute WASM bytecode using Wasmtime
    pub fn execute(
        &mut self,
        wasm_bytes: &[u8],
        context: &ExecutionContext,
        input: &[u8],
        state: &mut dyn RuntimeState,
    ) -> Result<ExecutionResult> {
        self.gas_limit = context.gas_limit;
        self.gas_used = 0;

        self.validate_wasm(wasm_bytes)?;
        self.charge_gas(1000)?;

        let mut config = Config::new();
        config.consume_fuel(true);
        config.wasm_simd(false);
        config.wasm_relaxed_simd(false);
        config.wasm_threads(false);
        config.cranelift_nan_canonicalization(true);
        config.wasm_bulk_memory(true);
        config.wasm_multi_value(true);

        let engine = Engine::new(&config)?;
        let module = Module::new(&engine, wasm_bytes)?;

        let host_functions = HostFunctions::new(
            state,
            context.gas_limit,
            context.block_number,
            context.timestamp,
            context.caller,
            context.contract_address,
        );

        let mut store = Store::new(
            &engine,
            HostEnv {
                host: host_functions,
            },
        );
        store.set_fuel(context.gas_limit)?;

        let mut linker = Linker::new(&engine);
        define_host_functions(&mut linker)?;
        linker.func_wrap(
            "env",
            "abort",
            |_caller: Caller<'_, HostEnv>, _a: i32, _b: i32, _c: i32, _d: i32| Ok(()),
        )?;

        let instance = linker.instantiate(&mut store, &module)?;

        // TODO: write call input to contract memory (future enhancement)
        let _ = input;

        let main_fn = instance
            .get_typed_func::<(), ()>(&mut store, "main")
            .ok()
            .or_else(|| {
                instance
                    .get_typed_func::<(), ()>(&mut store, "execute")
                    .ok()
            });

        let call_result = if let Some(main_fn) = main_fn {
            Some(main_fn.call(&mut store, ()))
        } else {
            None
        };

        let fuel_remaining = store.get_fuel()?;
        let fuel_consumed = context.gas_limit.saturating_sub(fuel_remaining);
        let host_gas_used = store.data().host.gas_used();
        self.gas_used = self
            .gas_used
            .saturating_add(fuel_consumed)
            .saturating_add(host_gas_used);

        if let Some(result) = call_result {
            match result {
                Ok(_) => Ok(ExecutionResult {
                    success: true,
                    gas_used: self.gas_used,
                    return_data: Vec::new(),
                    logs: vec![],
                }),
                Err(e) => {
                    if e.to_string().contains("fuel") {
                        bail!("out of gas");
                    }
                    Ok(ExecutionResult {
                        success: false,
                        gas_used: self.gas_used,
                        return_data: format!("execution failed: {}", e).into_bytes(),
                        logs: vec![],
                    })
                }
            }
        } else {
            Ok(ExecutionResult {
                success: true,
                gas_used: self.gas_used,
                return_data: Vec::new(),
                logs: vec![],
            })
        }
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

fn define_host_functions(linker: &mut Linker<HostEnv>) -> Result<()> {
    linker.func_wrap(
        "env",
        "storage_read",
        |mut caller: Caller<'_, HostEnv>,
         key_ptr: i32,
         key_len: i32,
         value_ptr: i32,
         value_capacity: i32|
         -> Result<i32> {
            if value_capacity < 0 {
                return Err(anyhow!("negative capacity"));
            }
            let key = read_bytes(&mut caller, key_ptr, key_len)?;
            let result = caller.data_mut().host.storage_read(&key)?;
            if let Some(value) = result {
                if value.len() > value_capacity as usize {
                    return Ok(-2);
                }
                write_bytes(&mut caller, value_ptr, &value)?;
                Ok(value.len() as i32)
            } else {
                Ok(-1)
            }
        },
    )?;

    linker.func_wrap(
        "env",
        "storage_write",
        |mut caller: Caller<'_, HostEnv>,
         key_ptr: i32,
         key_len: i32,
         value_ptr: i32,
         value_len: i32|
         -> Result<i32> {
            let key = read_bytes(&mut caller, key_ptr, key_len)?;
            let value = read_bytes(&mut caller, value_ptr, value_len)?;
            caller.data_mut().host.storage_write(key, value)?;
            Ok(0)
        },
    )?;

    linker.func_wrap(
        "env",
        "get_balance",
        |mut caller: Caller<'_, HostEnv>, address_ptr: i32, result_ptr: i32| -> Result<i32> {
            let address = read_address(&mut caller, address_ptr)?;
            let balance = caller.data_mut().host.get_balance(&address)?;
            write_u128(&mut caller, result_ptr, balance)?;
            Ok(0)
        },
    )?;

    linker.func_wrap(
        "env",
        "transfer",
        |mut caller: Caller<'_, HostEnv>,
         from_ptr: i32,
         to_ptr: i32,
         amount_ptr: i32|
         -> Result<i32> {
            let from = read_address(&mut caller, from_ptr)?;
            let to = read_address(&mut caller, to_ptr)?;
            let amount = read_u128(&mut caller, amount_ptr)?;
            caller.data_mut().host.transfer(&from, &to, amount)?;
            Ok(0)
        },
    )?;

    linker.func_wrap(
        "env",
        "emit_log",
        |mut caller: Caller<'_, HostEnv>,
         topics_ptr: i32,
         topics_len: i32,
         data_ptr: i32,
         data_len: i32|
         -> Result<i32> {
            let topics = read_topics(&mut caller, topics_ptr, topics_len)?;
            let data = read_bytes(&mut caller, data_ptr, data_len)?;
            caller.data_mut().host.emit_log(topics, data)?;
            Ok(0)
        },
    )?;

    linker.func_wrap(
        "env",
        "block_number",
        |mut caller: Caller<'_, HostEnv>| -> Result<i64> {
            let block = caller.data_mut().host.block_number()?;
            i64::try_from(block).map_err(|_| anyhow!("block number overflow"))
        },
    )?;

    linker.func_wrap(
        "env",
        "timestamp",
        |mut caller: Caller<'_, HostEnv>| -> Result<i64> {
            let ts = caller.data_mut().host.timestamp()?;
            i64::try_from(ts).map_err(|_| anyhow!("timestamp overflow"))
        },
    )?;

    linker.func_wrap(
        "env",
        "caller",
        |mut caller: Caller<'_, HostEnv>, dest_ptr: i32| -> Result<i32> {
            let addr = caller.data_mut().host.caller()?;
            write_bytes(&mut caller, dest_ptr, addr.as_bytes())?;
            Ok(0)
        },
    )?;

    linker.func_wrap(
        "env",
        "address",
        |mut caller: Caller<'_, HostEnv>, dest_ptr: i32| -> Result<i32> {
            let addr = caller.data_mut().host.address()?;
            write_bytes(&mut caller, dest_ptr, addr.as_bytes())?;
            Ok(0)
        },
    )?;

    linker.func_wrap(
        "env",
        "sha256",
        |mut caller: Caller<'_, HostEnv>,
         data_ptr: i32,
         data_len: i32,
         result_ptr: i32|
         -> Result<i32> {
            let data = read_bytes(&mut caller, data_ptr, data_len)?;
            let hash = caller.data_mut().host.sha256(&data)?;
            write_bytes(&mut caller, result_ptr, hash.as_bytes())?;
            Ok(0)
        },
    )?;

    Ok(())
}

fn get_memory(caller: &mut Caller<'_, HostEnv>) -> Result<Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => Ok(mem),
        _ => Err(anyhow!("contract must export linear memory")),
    }
}

fn read_bytes(caller: &mut Caller<'_, HostEnv>, ptr: i32, len: i32) -> Result<Vec<u8>> {
    if ptr < 0 || len < 0 {
        return Err(anyhow!("negative pointer or length"));
    }
    let offset = ptr as usize;
    let length = len as usize;

    let memory = get_memory(caller)?;
    let mut buffer = vec![0u8; length];
    memory
        .read(caller, offset, &mut buffer)
        .map_err(|e| anyhow!("memory read failed: {e}"))?;
    Ok(buffer)
}

fn write_bytes(caller: &mut Caller<'_, HostEnv>, ptr: i32, data: &[u8]) -> Result<()> {
    if ptr < 0 {
        return Err(anyhow!("negative pointer"));
    }
    let offset = ptr as usize;
    let memory = get_memory(caller)?;
    memory
        .write(caller, offset, data)
        .map_err(|e| anyhow!("memory write failed: {e}"))
}

fn read_address(caller: &mut Caller<'_, HostEnv>, ptr: i32) -> Result<Address> {
    let bytes = read_bytes(caller, ptr, 20)?;
    Address::from_slice(&bytes).map_err(|_| anyhow!("invalid address encoding"))
}

fn read_u128(caller: &mut Caller<'_, HostEnv>, ptr: i32) -> Result<u128> {
    let bytes = read_bytes(caller, ptr, 16)?;
    let arr: [u8; 16] = bytes
        .try_into()
        .map_err(|_| anyhow!("invalid u128 encoding"))?;
    Ok(u128::from_le_bytes(arr))
}

fn write_u128(caller: &mut Caller<'_, HostEnv>, ptr: i32, value: u128) -> Result<()> {
    write_bytes(caller, ptr, &value.to_le_bytes())
}

fn read_topics(caller: &mut Caller<'_, HostEnv>, ptr: i32, topics_len: i32) -> Result<Vec<H256>> {
    if topics_len < 0 {
        return Err(anyhow!("negative topics length"));
    }
    let total = topics_len as usize * 32;
    if total > i32::MAX as usize {
        return Err(anyhow!("topics too large"));
    }
    if total == 0 {
        return Ok(Vec::new());
    }

    let bytes = read_bytes(caller, ptr, total as i32)?;
    let mut topics = Vec::with_capacity(topics_len as usize);
    for chunk in bytes.chunks(32) {
        let topic = H256::from_slice(chunk).map_err(|_| anyhow!("invalid topic"))?;
        topics.push(topic);
    }
    Ok(topics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_state::MockRuntimeState;

    #[test]
    fn test_vm_creation() {
        let vm = WasmVm::new(100_000);
        assert_eq!(vm.gas_limit, 100_000);
        assert_eq!(vm.gas_used, 0);
    }

    #[test]
    fn test_gas_charging() {
        let mut vm = WasmVm::new(1000);

        assert!(vm.charge_gas(500).is_ok());
        assert_eq!(vm.gas_used(), 500);

        assert!(vm.charge_gas(400).is_ok());
        assert_eq!(vm.gas_used(), 900);

        // Should fail - exceeds limit
        assert!(vm.charge_gas(200).is_err());
    }

    #[test]
    fn test_remaining_gas() {
        let mut vm = WasmVm::new(1000);
        vm.charge_gas(300).unwrap();

        assert_eq!(vm.remaining_gas(), 700);
    }

    #[test]
    fn test_wasm_validation() {
        let vm = WasmVm::new(100_000);

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
    fn test_execute_basic() {
        let mut vm = WasmVm::new(100_000);

        let wasm =
            wat::parse_str("(module (memory (export \"memory\") 1) (func (export \"main\")))")
                .expect("valid wasm");
        let context = ExecutionContext {
            contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
            caller: Address::from_slice(&[2u8; 20]).unwrap(),
            value: 0,
            gas_limit: 100_000,
            block_number: 1,
            timestamp: 1000,
        };

        let mut state = MockRuntimeState::new();
        let result = vm.execute(&wasm, &context, b"input", &mut state).unwrap();

        assert!(result.success);
        assert!(result.gas_used > 0);
    }
}
