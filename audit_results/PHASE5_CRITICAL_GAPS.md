# PHASE 5: CRITICAL GAPS IDENTIFIED

## GAP 1: WASM RUNTIME (CRITICAL - BLOCKS EVERYTHING)

### Location: crates/runtime/src/vm.rs:85-96

### Problem:
- Wasmtime integration is commented out
- execute_simplified() is a stub
- execute() always returns success
- No actual WASM execution

### Evidence:
```rust
// In production: use Wasmtime
// let engine = Engine::new(&config)?;
// let module = Module::new(&engine, wasm_bytes)?;
// let mut store = Store::new(&engine, ());
// store.add_fuel(context.gas_limit)?;

// For now: simplified execution
let result = self.execute_simplified(wasm_bytes, context, input)?;
```

### Impact:
- Cannot run smart contracts
- Gas metering not applied correctly
- All runtime functionality blocked

### Fix Required:
- Uncomment and implement Wasmtime engine
- Add Module compilation
- Store with fuel metering
- Proper error handling
- Test with real contracts

### Effort: 2-3 days
### Risk: HIGH (complex Wasmtime integration)

---

## GAP 2: SNAPSHOT COMPRESSION (HIGH - FAILS SPEC)

### Location: crates/state/snapshots/src/compression.rs

### Problem:
- compress() just copies bytes: Ok(bytes.to_vec())
- No actual compression algorithm
- Spec requires 10x compression (50GB → 5GB)
- Current: 1x compression

### Evidence:
```rust
pub fn compress(bytes: &[u8]) -> Result<Vec<u8>> {
    Ok(bytes.to_vec())  // NO-OP!
}

pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    Ok(bytes.to_vec())  // NO-OP!
}
```

### Impact:
- Snapshots not compressed
- Storage bloated
- Fails acceptance criterion

### Fix Required:
- Implement zstd or snappy compression
- Add compression benchmarks
- Verify 10x+ compression ratio
- Add roundtrip tests

### Effort: 1 day
### Risk: LOW (straightforward)

---

## GAP 3: HOST FUNCTIONS USE PLACEHOLDERS (MEDIUM)

### Location: crates/runtime/src/host_functions.rs:108-133

### Problem:
- block_number() returns hardcoded 1000
- timestamp() returns hardcoded 1234567890
- caller() returns fixed address
- address() returns fixed address

### Evidence:
```rust
pub fn block_number(&mut self) -> Result<u64> {
    Ok(1000)  // HARDCODED!
}

pub fn timestamp(&mut self) -> Result<u64> {
    Ok(1234567890)  // HARDCODED!
}

pub fn caller(&mut self) -> Result<Address> {
    Ok(Address::from_slice(&[1u8; 20]).unwrap())  // FIXED!
}
```

### Impact:
- Contracts get wrong context
- Cannot access actual block info
- Cannot verify caller identity

### Fix Required:
- Accept ExecutionContext in constructor
- Store actual block_number, timestamp, caller
- Remove all hardcoded values
- Pass context through execution

### Effort: 1 day
### Risk: LOW (straightforward)

---

## GAP 4: MERKLE TREE RECOMPUTE (MEDIUM - PERF)

### Location: crates/ledger/src/state.rs:170-195

### Problem:
- recompute_state_root() iterates ALL accounts
- Rebuilds entire tree every transaction
- O(n) per transaction = O(n²) per block
- Performance degrades with state size

### Evidence:
```rust
fn recompute_state_root(&mut self) -> Result<()> {
    let mut accounts = HashMap::new();
    for item in self.storage.iterator(CF_ACCOUNTS)? {  // Full scan!
        let (key_bytes, value_bytes) = item;
        if key_bytes.len() == 20 {
            let address = Address::from_slice(&key_bytes)?;
            let account: Account = bincode::deserialize(&value_bytes)?;
            let account_hash = self.hash_account(&account);
            accounts.insert(address, account_hash);
        }
    }
    // Rebuild entire tree
    self.merkle_tree = SparseMerkleTree::new();
    for (address, hash) in accounts {
        self.merkle_tree.update(address, hash);
    }
    ...
}
```

### Impact:
- Performance O(n²) for blocks
- Doesn't scale with state size
- Bottleneck for high throughput

### Fix Required:
- Implement incremental Merkle updates
- Cache intermediate nodes
- Update only affected nodes
- Benchmark before/after

### Effort: 2-3 days
### Risk: MEDIUM (must maintain correctness)

---

## GAP 5: MISSING INTEGRATION TESTS (MEDIUM)

### Problem:
- No snapshot roundtrip test
- No WASM contract execution test
- No concurrent ledger operations test
- No large state (1M+ accounts) test
- No determinism verification test

### Current Test Count:
- Storage: 2 tests
- Merkle: 3 tests
- Ledger: 3 tests
- Runtime: 11 tests
- **Total: 28 unit tests**

### Missing:
- Snapshot gen → import → verify state
- WASM contract execution
- Concurrent transactions
- Stress test (1M+ accounts)
- Cross-node determinism

### Fix Required:
- Add integration test suite (10-15 tests)
- Snapshot roundtrip verification
- WASM execution with gas metering
- Concurrent ledger operations
- Large state stress test

### Effort: 3-4 days
### Risk: MEDIUM

---

## GAP 6: STATE ROOT DETERMINISM UNVERIFIED (MEDIUM)

### Problem:
- Spec claims state root is deterministic
- No tests verify this
- No property tests for different orderings
- No cross-node verification

### Impact:
- Consensus risk if not truly deterministic
- Unknown correctness under different conditions
- May cause fork under certain scenarios

### Fix Required:
- Add property test: determinism under different tx orderings
- Add cross-node verification test
- Specify serialization order
- Document guarantees

### Effort: 2 days
### Risk: MEDIUM (important for consensus)

