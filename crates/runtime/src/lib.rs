// ============================================================================
// AETHER RUNTIME - WASM VM & Smart Contract Execution
// ============================================================================
// PURPOSE: Execute smart contracts with gas metering and sandboxing
//
// FEATURES:
// - WASM VM using Wasmtime
// - Deterministic execution
// - Gas metering per instruction
// - Host functions for blockchain interaction
// - Memory and stack limits
// - Parallel execution scheduling (R/W sets)
//
// HOST FUNCTIONS:
// - storage_read/storage_write: Contract storage
// - get_balance/transfer: Account operations
// - sha256: Cryptographic hashing
// - emit_log: Event logging
// - block_number/timestamp/caller/address: Context info
//
// GAS COSTS (per spec):
// - Base: 100
// - Memory: 1 per byte
// - Storage read: 200
// - Storage write: 5000 (+ 20000 for new slot)
// - Transfer: 9000
// - SHA256: 60 + 12 per word
// - Log: 375 + 8 per byte
//
// EXECUTION FLOW:
// 1. Load WASM module
// 2. Validate bytecode
// 3. Instantiate with gas limit
// 4. Inject host functions
// 5. Execute entry point
// 6. Return result + gas used
// ============================================================================

pub mod host_functions;
pub mod scheduler;
pub mod vm;

pub use host_functions::HostFunctions;
pub use scheduler::ParallelScheduler;
pub use vm::{gas_costs, ExecutionContext, ExecutionResult, Log, WasmVm};
