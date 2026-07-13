use thiserror::Error;

#[derive(Debug, Error)]
pub enum KruonError {
    #[error("event store error: {0}")]
    Store(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("path policy violation: {0}")]
    PathPolicy(String),
    #[error("process error: {0}")]
    Process(String),
    #[error("adapter error: {0}")]
    Adapter(String),
    #[error("run not found: {0}")]
    NotFound(String),
    #[error("event conflict: {0}")]
    Conflict(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

impl From<rusqlite::Error> for KruonError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Store(value.to_string())
    }
}

pub type KruonResult<T> = Result<T, KruonError>;
