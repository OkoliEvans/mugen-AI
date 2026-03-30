/// Integration test — requires artifacts to be built first:
///   cd prover && python3 export_model.py && python3 setup_circuit.py
///
/// Run with:
///   cargo test -p prover-manager -- --nocapture

#[cfg(test)]
mod tests {
    use prover_manager::manager::{ProverConfig, ProverManager};
    use prover_manager::job::JobState;
    use std::time::Duration;

    fn test_config() -> ProverConfig {
        ProverConfig {
            python_bin: "python3".into(),
            worker_script: "../../prover/worker.py".into(),
            artifacts_dir: "../../prover/artifacts".into(),
            max_concurrent: 2,
            timeout_secs: 30,
        }
    }

    #[tokio::test]
    async fn test_single_proof_job() {
        let _ = tracing_subscriber::fmt::try_init();

        let manager = ProverManager::new(test_config());

        // tiny_mlp takes 4 floats as input
        let input_data = vec![vec![0.1_f64, 0.2, 0.3, 0.4]];

        let job_id = manager.submit(input_data).await.expect("submit failed");
        println!("submitted job: {job_id}");

        // Poll until done (max 30s)
        let mut attempts = 0;
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let state = manager.status(&job_id).await.expect("status failed");
            match state {
                JobState::Done { ref proof_path } => {
                    println!("job done — proof at: {proof_path}");
                    let proof = manager.read_proof(&job_id).await.expect("read_proof failed");
                    assert!(!proof.is_empty(), "proof bytes should not be empty");
                    break;
                }
                JobState::Failed { ref reason } => {
                    panic!("job failed: {reason}");
                }
                state => {
                    println!("state: {state:?}");
                }
            }
            attempts += 1;
            assert!(attempts < 60, "job timed out after 30s");
        }
    }

    #[tokio::test]
    async fn test_concurrent_jobs() {
        let _ = tracing_subscriber::fmt::try_init();

        let manager = ProverManager::new(test_config());
        let mut job_ids = vec![];

        // Submit 4 jobs — 2 will run immediately, 2 will queue
        for i in 0..4 {
            let input_data = vec![vec![i as f64, 0.1, 0.2, 0.3]];
            let job_id = manager.submit(input_data).await.expect("submit failed");
            println!("submitted job {i}: {job_id}");
            job_ids.push(job_id);
        }

        // Wait for all to complete
        for job_id in &job_ids {
            let mut attempts = 0;
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let state = manager.status(job_id).await.expect("status failed");
                match state {
                    JobState::Done { .. } => break,
                    JobState::Failed { ref reason } => panic!("job {job_id} failed: {reason}"),
                    _ => {}
                }
                attempts += 1;
                assert!(attempts < 120, "job {job_id} timed out");
            }
            println!("job {job_id} complete");
        }

        println!("all 4 concurrent jobs completed successfully");
    }
}