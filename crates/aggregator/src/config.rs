use crate::error::AggregatorError;

#[derive(Debug, Clone)]
pub struct AggregatorConfig {
    pub database_url:        String,
    /// Python binary for aggregation — points to .venv-aggr (ezkl v15.6.3)
    pub agg_python_bin:      String,
    pub aggregator_script:   String,
    /// Original artifacts dir (model.compiled, pk.key, vk.key, settings.json)
    pub artifacts_dir:       String,
    /// Separate dir for aggregation keys (agg_vk.key, agg_pk.key)
    pub agg_artifacts_dir:   String,
    pub batch_size:          usize,
    pub flush_interval_secs: u64,
    pub poll_interval_secs:  u64,
}

impl AggregatorConfig {
    pub fn from_env() -> Result<Self, AggregatorError> {
        Ok(Self {
            database_url:        require("DATABASE_URL")?,
            agg_python_bin:      std::env::var("AGG_PYTHON_BIN")
                                     .unwrap_or_else(|_| "prover/.venv-aggr/bin/python3".into()),
            aggregator_script:   std::env::var("AGGREGATOR_SCRIPT")
                                     .unwrap_or_else(|_| "prover/aggregator.py".into()),
            artifacts_dir:       std::env::var("ARTIFACTS_DIR")
                                     .unwrap_or_else(|_| "prover/artifacts".into()),
            agg_artifacts_dir:   std::env::var("AGG_ARTIFACTS_DIR")
                                     .unwrap_or_else(|_| "prover/agg_artifacts".into()),
            batch_size:          std::env::var("BATCH_SIZE")
                                     .ok().and_then(|v| v.parse().ok()).unwrap_or(100),
            flush_interval_secs: std::env::var("FLUSH_INTERVAL_SECS")
                                     .ok().and_then(|v| v.parse().ok()).unwrap_or(60),
            poll_interval_secs:  std::env::var("POLL_INTERVAL_SECS")
                                     .ok().and_then(|v| v.parse().ok()).unwrap_or(5),
        })
    }
}

fn require(key: &str) -> Result<String, AggregatorError> {
    std::env::var(key).map_err(|_| AggregatorError::MissingEnv(key.into()))
}