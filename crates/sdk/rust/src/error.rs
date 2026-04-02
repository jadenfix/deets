/// Typed error enum for the Aether SDK public API.
///
/// Callers can match on specific variants to handle errors programmatically
/// rather than relying on string inspection of `anyhow::Error`.
#[derive(Debug, thiserror::Error)]
pub enum AetherSdkError {
    /// The transaction or job fields failed validation (missing field, bad value).
    #[error("build error: {0}")]
    Build(String),

    /// The transaction's ed25519 signature is invalid.
    #[error("invalid signature: {0}")]
    InvalidSignature(String),

    /// The transaction fee is too low or the fee calculation overflowed.
    #[error("invalid fee: {0}")]
    InvalidFee(String),

    /// A network I/O error occurred while communicating with the RPC endpoint.
    #[error("network error: {0}")]
    Network(String),

    /// The RPC server returned a JSON-RPC error response.
    #[error("rpc error {code}: {message}")]
    Rpc {
        /// JSON-RPC error code.
        code: i64,
        /// Human-readable error message from the server.
        message: String,
    },

    /// The RPC endpoint URL is malformed or uses an unsupported scheme.
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(String),

    /// (De)serialization of a transaction or response payload failed.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// The HTTP response from the RPC server could not be parsed.
    #[error("invalid response: {0}")]
    InvalidResponse(String),

    /// The tx hash returned by the node did not match the locally computed hash.
    #[error("tx hash mismatch: expected {expected}, got {got}")]
    TxHashMismatch {
        /// Locally computed transaction hash.
        expected: String,
        /// Hash returned by the node.
        got: String,
    },
}

impl AetherSdkError {
    pub(crate) fn build(msg: impl Into<String>) -> Self {
        AetherSdkError::Build(msg.into())
    }

    pub(crate) fn network(msg: impl std::fmt::Display) -> Self {
        AetherSdkError::Network(msg.to_string())
    }

    pub(crate) fn invalid_endpoint(msg: impl std::fmt::Display) -> Self {
        AetherSdkError::InvalidEndpoint(msg.to_string())
    }

    pub(crate) fn serialization(msg: impl std::fmt::Display) -> Self {
        AetherSdkError::Serialization(msg.to_string())
    }

    pub(crate) fn invalid_response(msg: impl std::fmt::Display) -> Self {
        AetherSdkError::InvalidResponse(msg.to_string())
    }
}
