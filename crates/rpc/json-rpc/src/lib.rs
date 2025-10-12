// ============================================================================
// AETHER JSON-RPC - Query Interface for Clients
// ============================================================================
// PURPOSE: Standard JSON-RPC 2.0 API for wallets, explorers, indexers
//
// METHODS:
// - aeth_sendRawTransaction
// - aeth_getBlockByNumber
// - aeth_getTransactionReceipt
// - aeth_getBalance
// - aeth_getStateRoot
// - aeth_getValidatorSet
// - aeth_getJob
//
// ENDPOINT: http://localhost:8545
// ============================================================================

pub mod server;
pub mod methods;
pub mod types;

pub use server::JsonRpcServer;

