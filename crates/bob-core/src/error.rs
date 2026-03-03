#[derive(Debug, thiserror::Error)]
pub enum BobError {
    #[error("database error: {0}")]
    Db(#[from] tokio_postgres::Error),
    #[error("connection pool error: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),
    #[error("ollama error: {0}")]
    Ollama(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

pub type BobResult<T> = Result<T, BobError>;
