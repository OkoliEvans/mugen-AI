//! Elenxis Aggregator
//!
//! Watches for completed proof jobs in Postgres and triggers snark-verifier
//! KZG aggregation once either:
//!   a) batch_size proofs are ready  (default: 100–500, configurable)
//!   b) flush_interval_secs elapsed  (default: 60s)
//!
//! Architecture:
//!   collector task  — polls Postgres for unbatched done jobs
//!   aggregator task — receives batches, calls Python aggregator worker
//!   settler         — submits aggregated proof on-chain

mod config;
mod collector;
mod batch;
mod error;

use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;
use anyhow;

use common::{db, DbPool};
use config::AggregatorConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load workspace root .env
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .parent().unwrap()
        .join(".env");
    let _ = dotenvy::from_path(&env_path);

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cfg = AggregatorConfig::from_env()?;

    info!(
        batch_size     = cfg.batch_size,
        flush_interval = cfg.flush_interval_secs,
        "Elenxis Aggregator starting"
    );

    // Database pool
    let pool: Arc<DbPool> = Arc::new(db::build_pool(&cfg.database_url)?);
    db::run_migrations(&pool).await?;

    // Settler
    let settler_cfg = settler::config::SettlerConfig::from_env()?;
    let settler = Arc::new(settler::settler::Settler::new(settler_cfg));

    // Run the aggregation loop — blocks forever
    collector::run(pool, settler, cfg).await?;

    Ok(())
}