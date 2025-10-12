use std::io;

use bincode::Error as BincodeError;
use thiserror::Error;

/// Unified error type for serialization helpers.
#[derive(Debug, Error)]
pub enum CodecError {
    #[error("borsh serialization failed: {0}")]
    Borsh(#[from] io::Error),

    #[error("bincode serialization failed: {0}")]
    Bincode(#[from] BincodeError),
}

pub type Result<T> = std::result::Result<T, CodecError>;
