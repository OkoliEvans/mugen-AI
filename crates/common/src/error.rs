use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommonError {
    #[error("database error: {0}")]
    Diesel(#[from] diesel::result::Error),

    #[error("pool error: {0}")]
    Pool(#[from] deadpool_diesel::PoolError),

    #[error("interact error: {0}")]
    Interact(String),

    #[error("record not found: {0}")]
    NotFound(String),

    #[error("environment variable missing: {0}")]
    MissingEnv(String),
}
