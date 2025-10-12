// ============================================================================
// AETHER JSON-RPC - Query Interface for Clients
// ============================================================================
// PURPOSE: Standard JSON-RPC 2.0 API for wallets, explorers, indexers
//
// METHODS:
// - aeth_sendRawTransaction: Submit signed transaction
// - aeth_getBlockByNumber: Get block by slot number
// - aeth_getBlockByHash: Get block by hash
// - aeth_getTransactionReceipt: Get transaction receipt
// - aeth_getStateRoot: Get state root (Merkle root)
// - aeth_getAccount: Get account state
// - aeth_getSlotNumber: Get current slot
// - aeth_getFinalizedSlot: Get last finalized slot
//
// ENDPOINT: http://localhost:8545
// ============================================================================

pub mod server;

pub use server::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, JsonRpcServer, RpcBackend};
