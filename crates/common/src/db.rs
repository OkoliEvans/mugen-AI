//! Database connection pool (deadpool-diesel + Postgres).

use deadpool_diesel::postgres::{Manager, Pool};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use tracing::info;

use crate::error::CommonError;

pub type DbPool = Pool;

/// Embedded migrations — compiled into the binary at build time.
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("../../migrations");

/// Build a connection pool from DATABASE_URL.
pub fn build_pool(database_url: &str) -> Result<DbPool, CommonError> {
    let manager = Manager::new(database_url, deadpool_diesel::Runtime::Tokio1);
    Pool::builder(manager)
        .max_size(10)
        .build()
        .map_err(|e| CommonError::MissingEnv(e.to_string()))
}

/// Run any pending Diesel migrations at startup.
pub async fn run_migrations(pool: &DbPool) -> Result<(), CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(|conn| {
        conn.run_pending_migrations(MIGRATIONS)
            .map(|versions| {
                for v in versions {
                    info!("applied migration: {v}");
                }
            })
            .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}
