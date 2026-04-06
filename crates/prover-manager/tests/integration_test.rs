/// Integration test — requires guest ELF to exist at target/elf-compilation/...
/// Run with:
///   cargo test -p prover-manager -- --nocapture

#[cfg(test)]
mod tests {
    use prover_manager::{
        manager::{ProverConfig, ProverManager},
        JobState,
    };
    use std::time::Duration;
    use tiny_mlp;
    use tokio;

    fn test_config() -> ProverConfig {
        ProverConfig {
            proofs_dir: "/tmp".into(),
            max_concurrent: 1,
            timeout_secs: 300,
            guest_elf_path: "/Users/MAC/mugen/target/elf-compilation/riscv64im-succinct-zkvm-elf/release/inference-guest".into(),
            weights_path: "/tmp/test_weights.bin".into(),
            job_ttl_secs: 3600,
        }
    }

    async fn write_test_weights(path: &str) {
        let weights = vec![0.1f32; tiny_mlp::WEIGHTS_LEN];
        let weight_bytes: Vec<u8> = weights.iter().flat_map(|f: &f32| f.to_le_bytes()).collect();
        tokio::fs::write(path, &weight_bytes)
            .await
            .expect("failed to write test weights");
    }

    #[tokio::test]
    async fn test_single_proof_job() {
        let _ = tracing_subscriber::fmt::try_init();

        let config = test_config();
        write_test_weights(&config.weights_path).await;

        let manager = ProverManager::new(config.clone());

        let input_data = vec![vec![0.1_f64, 0.2, 0.3, 0.4]];
        let job_id = manager.submit(input_data).await.expect("submit failed");
        println!("submitted job: {job_id}");

        let mut attempts = 0;
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let state = manager.status(&job_id).await.expect("status failed");
            match &state {
                JobState::Done { proof_path } => {
                    println!("job done — proof at: {proof_path}");
                    let proof = manager
                        .read_proof(&job_id)
                        .await
                        .expect("read_proof failed");
                    assert!(!proof.is_empty(), "proof bytes should not be empty");
                    break;
                }
                JobState::Failed { reason } => panic!("job failed: {reason}"),
                _ => println!("state: {state:?}"),
            }
            attempts += 1;
            assert!(attempts < 300, "job timed out after 150s");
        }
    }

    #[tokio::test]
    async fn test_concurrent_jobs() {
        let _ = tracing_subscriber::fmt::try_init();

        let config = test_config();
        write_test_weights(&config.weights_path).await;

        let manager = ProverManager::new(config.clone());
        let mut job_ids = vec![];

        for i in 0..2 {
            let input_data = vec![vec![i as f64, 0.1, 0.2, 0.3]];
            let job_id = manager.submit(input_data).await.expect("submit failed");
            println!("submitted job {i}: {job_id}");
            job_ids.push(job_id);
        }

        for job_id in &job_ids {
            let mut attempts = 0;
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let state = manager.status(job_id).await.expect("status failed");
                match &state {
                    JobState::Done { .. } => break,
                    JobState::Failed { reason } => panic!("job {job_id} failed: {reason}"),
                    _ => {}
                }
                attempts += 1;
                assert!(attempts < 300, "job {job_id} timed out");
            }
            println!("job {job_id} complete");
        }

        println!("all 4 concurrent jobs completed successfully");
    }
}
