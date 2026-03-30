//! Collector — polls Postgres for unbatched done jobs and triggers batching.
//!
//! Two triggers (whichever fires first):
//!   1. job count >= batch_size
//!   2. flush_interval_secs elapsed since last flush with pending jobs

use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{error, info, warn};

use common::{repo, DbPool};
use settler::settler::Settler;

use crate::{batch::{self, JobWithInput}, config::AggregatorConfig, error::AggregatorError};

pub async fn run(
    pool:    Arc<DbPool>,
    settler: Arc<Settler>,
    cfg:     AggregatorConfig,
) -> Result<(), AggregatorError> {
    let poll      = Duration::from_secs(cfg.poll_interval_secs);
    let flush_dur = Duration::from_secs(cfg.flush_interval_secs);
    let mut last_flush = Instant::now();

    info!(
        batch_size     = cfg.batch_size,
        flush_interval = cfg.flush_interval_secs,
        poll_interval  = cfg.poll_interval_secs,
        python_bin     = %cfg.agg_python_bin,
        "collector started"
    );

    loop {
        tokio::time::sleep(poll).await;

        // Fetch unbatched done jobs up to batch_size
        // NOTE: repo returns Job structs — we need input_data too.
        // For now we fetch jobs and reconstruct input_data from the job's
        // stored input_hash. In Phase 2.1 we store input_data in Postgres.
        // For the demo, aggregator.py re-fetches input from the proof file.
        let jobs = match repo::fetch_unbatched_done_jobs(&pool, cfg.batch_size as i64).await {
            Ok(j) => j,
            Err(e) => {
                error!("failed to fetch unbatched jobs: {e}");
                continue;
            }
        };

        if jobs.is_empty() {
            continue;
        }

        let elapsed = last_flush.elapsed();
        let should_flush_by_size  = jobs.len() >= cfg.batch_size;
        let should_flush_by_timer = elapsed >= flush_dur;

        if should_flush_by_size {
            info!(count = jobs.len(), "batch size reached — flushing");
        } else if should_flush_by_timer {
            info!(count = jobs.len(), elapsed_secs = elapsed.as_secs(), "flush timer fired");
        } else {
            continue;
        }

        last_flush = Instant::now();

        // Build JobWithInput — input_data stored in Redis job record
        // TODO Phase 2.1: persist input_data in Postgres jobs table
        // For now: pass empty input_data, aggregator.py reads from proof_path
        let job_inputs: Vec<JobWithInput> = jobs
            .iter()
            .filter_map(|j| {
                // Only include jobs that have a proof_path
                j.proof_path.as_ref()?;
                Some(JobWithInput {
                    id:         j.id,
                    input_data: vec![],  // aggregator.py reads proof directly
                })
            })
            .collect();

        if job_inputs.is_empty() {
            warn!("all jobs in batch missing proof_path — skipping");
            continue;
        }

        let pool_c    = Arc::clone(&pool);
        let settler_c = Arc::clone(&settler);
        let cfg_c     = cfg.clone();

        tokio::spawn(async move {
            if let Err(e) = batch::process(pool_c, settler_c, cfg_c, job_inputs).await {
                error!("batch processing failed: {e}");
            }
        });
    }
}