use thiserror::Error;

#[derive(Debug, Error)]
pub enum AggregatorError {
    #[error("missing env var: {0}")]
    MissingEnv(String),

    #[error("database error: {0}")]
    Db(#[from] common::CommonError),

    #[error("aggregator worker failed: {0}")]
    Worker(String),

    #[error("settlement failed: {0}")]
    Settlement(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
