//! Aether Smart Contract SDK
//!
//! Provides the core types and host function bindings for writing smart
//! contracts that compile to WASM and run on the Aether blockchain.
//!
//! # Example
//! ```ignore
//! use aether_contract_sdk::prelude::*;
//!
//! pub fn execute(input: &[u8]) -> Result<(), ContractError> {
//!     let action: Action = deserialize(input)?;
//!     match action {
//!         Action::Mint { to, amount } => {
//!             let balance = storage_read(&to)?;
//!             storage_write(&to, balance + amount)?;
//!             emit_event("Mint", &[("to", &to), ("amount", &amount.to_string())])?;
//!             Ok(())
//!         }
//!         Action::Transfer { from, to, amount } => {
//!             let from_balance = storage_read(&from)?;
//!             if from_balance < amount {
//!                 return Err(ContractError::InsufficientBalance);
//!             }
//!             storage_write(&from, from_balance - amount)?;
//!             storage_write(&to, storage_read(&to)? + amount)?;
//!             Ok(())
//!         }
//!     }
//! }
//! ```

pub mod context;
pub mod error;
pub mod host;
pub mod storage;

/// Prelude — import everything needed for contract development.
pub mod prelude {
    pub use crate::context::ContractContext;
    pub use crate::error::ContractError;
    pub use crate::host::{emit_log, get_block_number, get_caller, get_timestamp};
    pub use crate::storage::{storage_read, storage_write};
}
