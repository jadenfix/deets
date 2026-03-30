use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KesError {
    #[error("requested period {requested} exceeds supported range ({max_periods})")]
    PeriodOutOfRange { requested: u32, max_periods: u32 },

    #[error("cannot sign for past period {requested} (current {current})")]
    PeriodRegression { current: u32, requested: u32 },

    #[error("key has been evolved past period {period}, forward secrecy prevents signing")]
    KeyErased { period: u32 },

    #[error("invalid signature")]
    InvalidSignature,

    #[error("key generation failed: {0}")]
    KeyGeneration(String),
}

pub type Result<T> = std::result::Result<T, KesError>;
