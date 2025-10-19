use aether_ledger::Ledger;
use aether_runtime::{ledger_state::LedgerRuntimeState, ExecutionContext, RuntimeState, WasmVm};
use aether_state_storage::Storage;
use aether_types::{Address, H256};
use tempfile::TempDir;
use wat::parse_str;

fn apply_sequence(order: &[(&[u8], &[u8])]) -> H256 {
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::open(temp_dir.path()).unwrap();
    let mut ledger = Ledger::new(storage).unwrap();

    let contract = Address::from_slice(&[0xABu8; 20]).unwrap();
    let caller = Address::from_slice(&[0xCDu8; 20]).unwrap();

    ledger.apply_balance_delta(&caller, 1_000).unwrap();

    for (key, value) in order {
        let mut state = LedgerRuntimeState::new(&mut ledger).unwrap();
        state
            .storage_write(&contract, (*key).to_vec(), (*value).to_vec())
            .unwrap();
        state.transfer(&caller, &contract, 0).unwrap();
        state.commit().unwrap();
    }

    ledger.state_root()
}

#[test]
fn ledger_state_root_consistent_across_operation_orders() {
    let first = apply_sequence(&[("alpha".as_bytes(), b"one"), ("beta".as_bytes(), b"two")]);
    let second = apply_sequence(&[("beta".as_bytes(), b"two"), ("alpha".as_bytes(), b"one")]);
    assert_eq!(first, second, "state root must be order independent");
}

fn execute_wasm_and_measure_gas() -> u64 {
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::open(temp_dir.path()).unwrap();
    let mut ledger = Ledger::new(storage).unwrap();
    let contract = Address::from_slice(&[0x77u8; 20]).unwrap();
    let caller = Address::from_slice(&[0x55u8; 20]).unwrap();
    ledger.apply_balance_delta(&caller, 5_000).unwrap();

    let wasm = parse_str(
        r#"
        (module
          (import "env" "storage_write" (func $storage_write (param i32 i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "key!")
          (data (i32.const 32) "data")
          (func (export "main")
            i32.const 0
            i32.const 4
            i32.const 32
            i32.const 4
            call $storage_write
            drop))
        "#,
    )
    .unwrap();

    let mut state = LedgerRuntimeState::new(&mut ledger).unwrap();
    let context = ExecutionContext {
        contract_address: contract,
        caller,
        value: 0,
        gas_limit: 150_000,
        block_number: 0,
        timestamp: 42,
    };

    let mut vm = WasmVm::new(context.gas_limit);
    let result = vm
        .execute(&wasm, &context, &[], &mut state)
        .expect("wasm execution succeeds");
    assert!(
        result.success,
        "contract execution should succeed deterministically"
    );
    state.commit().unwrap();
    result.gas_used
}

#[test]
fn wasm_execution_gas_is_deterministic() {
    let first = execute_wasm_and_measure_gas();
    let second = execute_wasm_and_measure_gas();
    assert_eq!(first, second, "gas usage must be deterministic");
}
