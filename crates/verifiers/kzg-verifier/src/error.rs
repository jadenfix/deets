use aether_types::H256;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VerifierError {
    #[error("challenge is malformed: {0}")]
    InvalidChallenge(&'static str),

    #[error("response vcr id mismatch (expected {expected:?}, received {received:?})")]
    VcrMismatch { expected: H256, received: H256 },

    #[error("missing openings: expected {expected}, received {received}")]
    IncompleteResponse { expected: usize, received: usize },

    #[error("unexpected opening for layer {layer} point {point}")]
    UnexpectedOpening { layer: u32, point: u32 },

    #[error("duplicate opening for layer {layer} point {point}")]
    DuplicateOpening { layer: u32, point: u32 },

    #[error("invalid proof for layer {layer} point {point}: {source}")]
    InvalidProof {
        layer: u32,
        point: u32,
        #[source]
        source: anyhow::Error,
    },
}

pub type Result<T> = std::result::Result<T, VerifierError>;
