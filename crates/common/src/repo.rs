//! Repository layer — all DB queries go here, never in handlers.

use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;

use crate::{
    DbPool,
    error::CommonError,
    models::{Batch, BatchUpdate, Job, JobUpdate, Model, NewBatch, NewJob, NewModel},
    schema::{batches, jobs, models},
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