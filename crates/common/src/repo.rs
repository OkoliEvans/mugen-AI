//! Repository layer — all DB queries go here, never in handlers.

use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;

use crate::{
    DbPool,
    error::CommonError,
    models::{
        AccountStats, Batch, BatchUpdate, Job, JobUpdate, Model, NewBatch, NewJob, NewModel,
        VaultEvent,
    },
    schema::{batches, jobs, models, vault_events},
};

// ── Models ────────────────────────────────────────────────────────────────────

pub async fn insert_model(pool: &DbPool, new: NewModel) -> Result<Model, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        diesel::insert_into(models::table)
            .values(&new)
            .returning(Model::as_returning())
            .get_result(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

pub async fn find_model_by_id(pool: &DbPool, id: Uuid) -> Result<Model, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        models::table
            .find(id)
            .select(Model::as_select())
            .first(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(|e| match e {
        diesel::result::Error::NotFound => CommonError::NotFound(id.to_string()),
        other => CommonError::Diesel(other),
    })
}

pub async fn find_model_by_name(pool: &DbPool, name: &str) -> Result<Model, CommonError> {
    let name = name.to_string();
    let query_name = name.clone();

    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        models::table
            .filter(models::name.eq(&query_name))
            .select(Model::as_select())
            .first(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(|e| match e {
        diesel::result::Error::NotFound => CommonError::NotFound(name.clone()),
        other => CommonError::Diesel(other),
    })
}

/// Patch the input_shape for a model row that was previously inserted as a
/// placeholder with an empty shape. Called by upsert_model on first real job.
pub async fn update_model_shape(
    pool: &DbPool,
    id: Uuid,
    shape: serde_json::Value,
) -> Result<Model, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        diesel::update(models::table.find(id))
            .set(models::input_shape.eq(shape))
            .returning(Model::as_returning())
            .get_result(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

/// Update ipfs_cid and on_chain_hash after successful model registration.
/// Called by the POST /v1/models handler once IPFS pin and on-chain
/// registerModel() have both completed.
pub async fn update_model_registration(
    pool: &DbPool,
    id: Uuid,
    ipfs_cid: String,
    on_chain_hash: String,
) -> Result<Model, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        diesel::update(models::table.find(id))
            .set((
                models::ipfs_cid.eq(ipfs_cid),
                models::on_chain_hash.eq(on_chain_hash),
            ))
            .returning(Model::as_returning())
            .get_result(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

// ── Jobs ──────────────────────────────────────────────────────────────────────

pub async fn insert_job(pool: &DbPool, new: NewJob) -> Result<Job, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        diesel::insert_into(jobs::table)
            .values(&new)
            .returning(Job::as_returning())
            .get_result(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

pub async fn find_job(pool: &DbPool, id: Uuid) -> Result<Job, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| jobs::table.find(id).select(Job::as_select()).first(conn))
        .await
        .map_err(|e| CommonError::Interact(e.to_string()))?
        .map_err(|e| match e {
            diesel::result::Error::NotFound => CommonError::NotFound(id.to_string()),
            other => CommonError::Diesel(other),
        })
}

pub async fn update_job(pool: &DbPool, id: Uuid, update: JobUpdate) -> Result<Job, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        diesel::update(jobs::table.find(id))
            .set(&update)
            .returning(Job::as_returning())
            .get_result(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

/// Fetch all done jobs not yet assigned to a batch — up to `limit`.
pub async fn fetch_unbatched_done_jobs(pool: &DbPool, limit: i64) -> Result<Vec<Job>, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        jobs::table
            .filter(jobs::status.eq("done"))
            .filter(jobs::batch_id.is_null())
            .limit(limit)
            .select(Job::as_select())
            .load(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

// ── Batches ───────────────────────────────────────────────────────────────────

pub async fn create_batch(pool: &DbPool) -> Result<Batch, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    let new = NewBatch {
        id: Uuid::new_v4(),
        status: "pending".into(),
    };
    conn.interact(move |conn| {
        diesel::insert_into(batches::table)
            .values(&new)
            .returning(Batch::as_returning())
            .get_result(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

pub async fn update_batch(
    pool: &DbPool,
    id: Uuid,
    update: BatchUpdate,
) -> Result<Batch, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        diesel::update(batches::table.find(id))
            .set(&update)
            .returning(Batch::as_returning())
            .get_result(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

/// Assign a list of job IDs to a batch.
pub async fn assign_jobs_to_batch(
    pool: &DbPool,
    job_ids: Vec<Uuid>,
    batch_id: Uuid,
) -> Result<usize, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        diesel::update(jobs::table.filter(jobs::id.eq_any(&job_ids)))
            .set((
                jobs::batch_id.eq(Some(batch_id)),
                jobs::status.eq("settled"),
                jobs::settled_at.eq(Some(Utc::now())),
            ))
            .execute(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

/// List jobs ordered by created_at DESC — most recent first.
/// Used by GET /v1/proofs for the explorer paginated feed.
pub async fn list_jobs(pool: &DbPool, limit: u64, offset: u64) -> Result<Vec<Job>, CommonError> {
    let conn = pool.get().await.map_err(CommonError::Pool)?;

    conn.interact(move |conn| -> Result<Vec<Job>, diesel::result::Error> {
        jobs::table
            .order(jobs::submitted_at.desc())
            .limit(limit as i64)
            .offset(offset as i64)
            .select(Job::as_select())
            .load::<Job>(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

pub async fn find_job_by_attestation_hash(
    pool: &DbPool,
    attestation_hash: &str,
) -> Result<Job, CommonError> {
    let hash = attestation_hash.to_string();
    let conn = pool.get().await.map_err(CommonError::Pool)?;
    conn.interact(move |conn| {
        jobs::table
            .filter(jobs::attestation_hash.eq(&hash))
            .select(Job::as_select())
            .first(conn)
            .map_err(|e| match e {
                diesel::result::Error::NotFound => {
                    CommonError::NotFound(format!("job with attestation_hash {hash} not found"))
                }
                other => CommonError::Diesel(other),
            })
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
}

/// Count proof jobs settled for a given wallet address.
/// Used by GET /v1/account/:wallet to compute proof_count and hsk_spent.
pub async fn get_account_stats(pool: &DbPool, wallet: &str) -> Result<AccountStats, CommonError> {
    let wallet = wallet.to_lowercase();
    let conn = pool.get().await.map_err(CommonError::Pool)?;

    let proof_count = conn
        .interact(move |conn| {
            vault_events::table
                .filter(vault_events::wallet.eq(wallet))
                .filter(vault_events::operation.eq("deduct"))
                .count()
                .get_result::<i64>(conn)
        })
        .await
        .map_err(|e| CommonError::Interact(e.to_string()))?
        .map_err(CommonError::Diesel)?;

    Ok(AccountStats { proof_count })
}

/// Paginated vault event history for a wallet.
/// Used by GET /v1/account/:wallet/history.
pub async fn get_account_history(
    pool: &DbPool,
    wallet: &str,
    limit: u64,
    offset: u64,
    operation: Option<&str>,
) -> Result<Vec<VaultEvent>, CommonError> {
    use crate::schema::vault_events;
    use diesel::pg::Pg;

    let wallet = wallet.to_lowercase();
    let operation = operation.map(|s| s.to_string());
    let conn = pool.get().await.map_err(CommonError::Pool)?;

    conn.interact(
        move |conn| -> Result<Vec<VaultEvent>, diesel::result::Error> {
            let mut query = vault_events::table
                .filter(vault_events::wallet.eq(wallet))
                .into_boxed::<Pg>();

            if let Some(op) = operation {
                query = query.filter(vault_events::operation.eq(op));
            }

            query
                .order(vault_events::created_at.desc())
                .limit(limit as i64)
                .offset(offset as i64)
                .select(VaultEvent::as_select())
                .load::<VaultEvent>(conn)
        },
    )
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)
}

/// Insert a vault event (deposit or deduct) into the DB.
/// Called by the VaultClient after each on-chain tx confirms.
pub async fn insert_vault_event(
    pool: &DbPool,
    wallet: &str,
    tx_hash: &str,
    operation: &str,
    amount_wei: &str,
    job_id: Option<Uuid>,
) -> Result<(), CommonError> {
    // Create a temporary struct or use the model if it supports insertion
    // For alignment, we'll assume a NewVaultEvent struct exists or use values directly
    let wallet = wallet.to_lowercase();
    let tx_hash = tx_hash.to_string();
    let operation = operation.to_string();
    let amount_wei = amount_wei.to_string();

    let conn = pool.get().await.map_err(CommonError::Pool)?;

    conn.interact(move |conn| {
        diesel::insert_into(vault_events::table)
            .values((
                vault_events::wallet.eq(wallet),
                vault_events::tx_hash.eq(tx_hash),
                vault_events::operation.eq(operation),
                vault_events::amount_wei.eq(amount_wei),
                vault_events::job_id.eq(job_id),
            ))
            .on_conflict(vault_events::tx_hash)
            .do_nothing()
            .execute(conn)
    })
    .await
    .map_err(|e| CommonError::Interact(e.to_string()))?
    .map_err(CommonError::Diesel)?;

    Ok(())
}
