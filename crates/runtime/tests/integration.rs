use aether_ledger::Ledger;
use aether_runtime::{ExecutionContext, LedgerRuntimeState, WasmVm};
use aether_state_storage::Storage;
use aether_types::{Address, H256};
use tempfile::TempDir;
use wat::parse_str;

#[test]
fn wasm_contract_updates_contract_storage_via_host_functions() {
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::open(temp_dir.path()).unwrap();
    let mut ledger = Ledger::new(storage).unwrap();

    let contract = Address::from_slice(&[0xAAu8; 20]).unwrap();
    let caller = Address::from_slice(&[0xBBu8; 20]).unwrap();
    ledger.apply_balance_delta(&caller, 5_000).unwrap();

    let wasm = parse_str(
        r#"
        (module
          (import "env" "storage_read" (func $storage_read (param i32 i32 i32 i32) (result i32)))
          (import "env" "storage_write" (func $storage_write (param i32 i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "ping")
          (data (i32.const 32) "pong")
          (func (export "main")
            ;; write key "ping" -> "pong"
            i32.const 0     ;; key ptr
            i32.const 4     ;; key len
            i32.const 32    ;; value ptr
            i32.const 4     ;; value len
            call $storage_write
            drop
            ;; read back into offset 64
            i32.const 0
            i32.const 4
            i32.const 64
            i32.const 16
            call $storage_read
            drop)
        )
        "#,
    )
    .unwrap();

    let mut runtime_state = LedgerRuntimeState::new(&mut ledger).unwrap();
    let context = ExecutionContext {
        contract_address: contract,
        caller,
        value: 0,
        gas_limit: 200_000,
        block_number: 10,
        timestamp: 123_456,
    };

    let mut vm = WasmVm::new(context.gas_limit);
    let result = vm
        .execute(&wasm, &context, &[], &mut runtime_state)
        .expect("vm execution succeeds");

    assert!(result.success, "expected contract execution success");

    let logs = runtime_state.commit().expect("commit succeeds");
    assert!(logs.is_empty(), "no logs emitted by contract");

    let stored = ledger
        .get_contract_storage(&contract, b"ping")
        .unwrap()
        .expect("value stored in ledger");
    assert_eq!(stored, b"pong");

    let account = ledger.get_or_create_account(&contract).unwrap();
    assert_ne!(account.storage_root, H256::zero());
}
