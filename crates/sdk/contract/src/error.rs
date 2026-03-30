use std::fmt;

/// Errors that can occur during contract execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    /// Insufficient balance for the operation.
    InsufficientBalance,
    /// Unauthorized caller.
    Unauthorized,
    /// Storage read/write failure.
    StorageError(String),
    /// Invalid input data.
    InvalidInput(String),
    /// Arithmetic overflow.
    Overflow,
    /// Custom error with message.
    Custom(String),
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsufficientBalance => write!(f, "insufficient balance"),
            Self::Unauthorized => write!(f, "unauthorized"),
            Self::StorageError(msg) => write!(f, "storage error: {}", msg),
            Self::InvalidInput(msg) => write!(f, "invalid input: {}", msg),
            Self::Overflow => write!(f, "arithmetic overflow"),
            Self::Custom(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ContractError {}

/// Result type for contract operations.
pub type ContractResult<T> = Result<T, ContractError>;
