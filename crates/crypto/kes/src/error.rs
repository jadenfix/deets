use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KesError {
    #[error("requested period {requested} exceeds supported range ({max_periods})")]
    PeriodOutOfRange { requested: u32, max_periods: u32 },

    #[error("cannot sign for past period {requested} (current {current})")]
    PeriodRegression { current: u32, requested: u32 },
}

pub type Result<T> = std::result::Result<T, KesError>;
