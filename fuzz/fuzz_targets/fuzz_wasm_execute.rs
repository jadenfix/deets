#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz WASM module loading and execution — must never panic
    // (should return errors gracefully for invalid WASM)
    let mut vm = match aether_runtime::WasmVm::new(10_000) {
        Ok(vm) => vm,
        Err(_) => return,
    };
    let context = aether_runtime::ExecutionContext {
        contract_address: aether_types::Address::from_slice(&[1u8; 20]).unwrap(),
        caller: aether_types::Address::from_slice(&[2u8; 20]).unwrap(),
        value: 0,
        gas_limit: 10_000,
        block_number: 1,
        timestamp: 1000,
    };
    let _ = vm.execute(data, &context, &[]);
});
