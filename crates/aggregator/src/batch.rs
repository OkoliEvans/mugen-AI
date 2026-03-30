//! Batch processor — creates a DB batch record, calls the Python aggregator
//! worker via AGG_PYTHON_BIN (.venv-aggr, ezkl v15.6.3), then settles
//! the aggregated proof on-chain.
//!
//! Two-venv pattern:
//!   PYTHON_BIN     → worker.py     (v23.0.5, individual proofs)
//!   AGG_PYTHON_BIN → aggregator.py (v15.6.3, aggregation only)

use std::sync::Arc;
use std::process::Stdio;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{error, info};
use uuid::Uuid;

use common::{
    models::BatchUpdate,
    repo,
    DbPool,
};
use settler::settler::Settler;

use crate::{config::AggregatorConfig, error::AggregatorError};

// ── Python worker protocol (matches aggregator.py v15.6.3) ───────────────────

/// One input item per job — aggregator.py re-proves each with proof_type="for-aggr"
#[derive(Debug, Serialize)]
struct JobInput {
    job_id:     String,
    input_data: Vec<Vec<f64>>,
}

#[derive(Debug, Serialize)]
struct AggregatorInput {
    batch_id:      String,
    job_inputs:    Vec<JobInput>,    // job_id + raw input_data per job
    artifacts_dir: String,           // original circuit artifacts (v23 compatible)
    agg_artifacts: String,           // aggregation keys dir (agg_vk.key, agg_pk.key)
    output_path:   String,
}

#[derive(Debug, Deserialize)]
struct AggregatorOutput {
    status:                  String,
    aggregated_proof_path:   Option<String>,
    #[serde(default)]
    error:                   Option<String>,
    #[serde(default)]
    proof_count:             Option<usize>,
    #[serde(default)]
    size_kb:                 Option<f64>,
}

// ── Job record with input_data for re-proving ─────────────────────────────────

pub struct JobWithInput {
    pub id:         Uuid,
    pub input_data: Vec<Vec<f64>>,
}

// ── Main batch flow ───────────────────────────────────────────────────────────

pub async fn process(
    pool:      Arc<DbPool>,
    settler:   Arc<Settler>,
    cfg:       AggregatorConfig,
    jobs:      Vec<JobWithInput>,
) -> Result<(), AggregatorError> {
    let job_ids: Vec<Uuid> = jobs.iter().map(|j| j.id).collect();

    // 1. Create batch record in Postgres
    let batch = repo::create_batch(&pool).await?;
    info!(batch_id = %batch.id, jobs = jobs.len(), "batch created");

    // 2. Assign jobs to this batch
    repo::assign_jobs_to_batch(&pool, job_ids.clone(), batch.id).await?;

    // 3. Update batch status → aggregating
    repo::update_batch(&pool, batch.id, BatchUpdate {
        status:                "aggregating".into(),
        job_count:             jobs.len() as i32,
        aggregated_proof_path: None,
        tx_hash:               None,
        gas_used:              None,
        aggregated_at:         None,
        settled_at:            None,
    }).await?;

    // 4. Build input for aggregator.py
    let output_path = format!("/tmp/batch_{}_aggregated.json", batch.id);

    let job_inputs: Vec<JobInput> = jobs.iter().map(|j| JobInput {
        job_id:     j.id.to_string(),
        input_data: j.input_data.clone(),
    }).collect();

    let input = AggregatorInput {
        batch_id:      batch.id.to_string(),
        job_inputs,
        artifacts_dir: cfg.artifacts_dir.clone(),
        agg_artifacts: cfg.agg_artifacts_dir.clone(),
        output_path:   output_path.clone(),
    };

    let input_json = serde_json::to_string(&input)?;

    // 5. Spawn aggregator.py via AGG_PYTHON_BIN (.venv-aggr, ezkl v15.6.3)
    info!(
        batch_id    = %batch.id,
        python_bin  = %cfg.agg_python_bin,
        script      = %cfg.aggregator_script,
        "invoking aggregator worker"
    );

    let child = Command::new(&cfg.agg_python_bin)
        .arg(&cfg.aggregator_script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let output = {
        use tokio::io::AsyncWriteExt;
        let mut child = child;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(input_json.as_bytes()).await?;
        }
        child.wait_with_output().await?
    };

    // Log stderr for debugging (aggregator.py writes progress there)
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        for line in stderr.lines() {
            info!(batch_id = %batch.id, "{}", line);
        }
    }

    if !output.status.success() {
        error!(batch_id = %batch.id, "aggregator worker exited non-zero");
        mark_failed(&pool, batch.id, jobs.len()).await?;
        return Err(AggregatorError::Worker(
            format!("aggregator exited {:?}: {}", output.status.code(), stderr)
        ));
    }

    let result: AggregatorOutput = serde_json::from_slice(&output.stdout)
        .map_err(|e| AggregatorError::Worker(format!("bad worker output: {e}")))?;

    if result.status != "ok" {
        let reason = result.error.unwrap_or_else(|| "unknown".into());
        error!(batch_id = %batch.id, %reason, "aggregator worker reported failure");
        mark_failed(&pool, batch.id, jobs.len()).await?;
        return Err(AggregatorError::Worker(reason));
    }

    let aggregated_proof_path = result.aggregated_proof_path
        .ok_or_else(|| AggregatorError::Worker("ok status but no proof path".into()))?;

    info!(
        batch_id    = %batch.id,
        path        = %aggregated_proof_path,
        proof_count = result.proof_count.unwrap_or(0),
        size_kb     = result.size_kb.unwrap_or(0.0),
        "aggregation complete"
    );

    // 6. Mark batch aggregated
    repo::update_batch(&pool, batch.id, BatchUpdate {
        status:                "done".into(),
        job_count:             jobs.len() as i32,
        aggregated_proof_path: Some(aggregated_proof_path.clone()),
        tx_hash:               None,
        gas_used:              None,
        aggregated_at:         Some(Utc::now()),
        settled_at:            None,
    }).await?;

    // 7. Settle aggregated proof on-chain
    //
    // model_name = "batch", model_version = batch UUID string.
    // The on-chain modelId for aggregated proofs is therefore:
    //   keccak256(abi.encodePacked("batch", batch_id_str))
    // This is distinct from any individual model's modelId and uniquely
    // identifies each aggregated settlement on the AggregatedVerifier contract.
    info!(batch_id = %batch.id, "settling aggregated proof on-chain");

    let batch_id_str = batch.id.to_string();
    let input_bytes  = serde_json::to_vec(
        &job_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>()
    ).unwrap_or_default();
    let output_bytes = batch.id.as_bytes().to_vec();

    match settler
        .submit(
            &aggregated_proof_path,
            "batch",          // model_name
            &batch_id_str,    // model_version — uniquely identifies this batch
            &input_bytes,
            &output_bytes,
        )
        .await
    {
        Ok(tx_hash) if tx_hash == "already-verified" => {
            info!(batch_id = %batch.id, "batch already verified on-chain");
            finalize_batch(&pool, batch.id, jobs.len(), aggregated_proof_path, "already-verified".into()).await?;
        }
        Ok(tx_hash) => {
            info!(batch_id = %batch.id, %tx_hash, "batch settled on-chain");
            finalize_batch(&pool, batch.id, jobs.len(), aggregated_proof_path, tx_hash).await?;
        }
        Err(e) => {
            error!(batch_id = %batch.id, "settlement failed: {e}");
            return Err(AggregatorError::Settlement(e.to_string()));
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn mark_failed(
    pool:      &DbPool,
    batch_id:  Uuid,
    job_count: usize,
) -> Result<(), AggregatorError> {
    repo::update_batch(pool, batch_id, BatchUpdate {
        status:                "failed".into(),
        job_count:             job_count as i32,
        aggregated_proof_path: None,
        tx_hash:               None,
        gas_used:              None,
        aggregated_at:         Some(Utc::now()),
        settled_at:            None,
    }).await?;
    Ok(())
}

async fn finalize_batch(
    pool:       &DbPool,
    batch_id:   Uuid,
    job_count:  usize,
    proof_path: String,
    tx_hash:    String,
) -> Result<(), AggregatorError> {
    repo::update_batch(pool, batch_id, BatchUpdate {
        status:                "done".into(),
        job_count:             job_count as i32,
        aggregated_proof_path: Some(proof_path),
        tx_hash:               Some(tx_hash),
        gas_used:              None,
        aggregated_at:         Some(Utc::now()),
        settled_at:            Some(Utc::now()),
    }).await?;
    Ok(())
}