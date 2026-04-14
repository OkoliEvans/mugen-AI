//! Veil SDK — Live E2E test
//!
//! Usage:
//!   GATEWAY_URL=http://localhost:8080 cargo run --example e2e_verify
//!
//! Optional overrides:
//!   TIMEOUT_MS=180000   — poll timeout in milliseconds (default: 120000)
//!   MODEL_ID=tiny_mlp_v1
//!   RUST_LOG=info       — enable tracing output
// SP1_PROVER=mock GATEWAY_URL=http://localhost:8080 cargo run --manifest-path /Users/MAC/mugen/sdk/Rust/Cargo.toml  e2e_verify

use std::time::Duration;

use veil_sdk::{JobStatus, VeilClient};

const DEFAULT_GATEWAY: &str = "http://localhost:8080";
const DEFAULT_TIMEOUT: u64 = 300_000;
const DEFAULT_MODEL: &str = "tiny_mlp_v1";

// Shape [1, 4] — matches tiny_mlp expected input
const INPUT_DATA: [[f64; 4]; 1] = [[0.1, 0.2, 0.3, 0.4]];

// ── Helpers ───────────────────────────────────────────────────────────────────

macro_rules! pass {
    ($msg:expr) => {
        println!("  ✅  PASS — {}", $msg);
    };
}
macro_rules! fail {
    ($msg:expr) => {
        eprintln!("  ❌  FAIL — {}", $msg);
    };
}

fn section(title: &str) {
    println!(
        "\n── {title} {}",
        "─".repeat(50_usize.saturating_sub(title.len() + 4))
    );
}

fn env_str(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Initialise tracing (respects RUST_LOG)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let gateway_url = env_str("GATEWAY_URL", DEFAULT_GATEWAY);
    let timeout_ms = env_u64("TIMEOUT_MS", DEFAULT_TIMEOUT);
    let model_id = env_str("MODEL_ID", DEFAULT_MODEL);
    let wallet_address = std::env::var("WALLET_ADDRESS").ok();
    let input_data: Vec<Vec<f64>> = INPUT_DATA.iter().map(|r| r.to_vec()).collect();

    println!("\n═══════════════════════════════════════════════");
    println!("  Veil SDK — Live E2E: verify_inference");
    println!("═══════════════════════════════════════════════");
    println!("  Gateway : {gateway_url}");
    println!("  Model   : {model_id}");
    println!("  Input   : {input_data:?}");
    println!("  Timeout : {timeout_ms}ms");

    let client = match VeilClient::builder()
        .base_url(&gateway_url)
        .timeout(Duration::from_millis(timeout_ms))
        .poll_interval(Duration::from_secs(4))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            fail!(format!("failed to build VeilClient: {e}"));
            std::process::exit(1);
        }
    };

    let mut all_passed = true;
    macro_rules! assert_sdk {
        ($label:expr, $condition:expr, $got:expr) => {
            if $condition {
                pass!($label);
            } else {
                fail!(format!("{} — got: {:?}", $label, $got));
                all_passed = false;
            }
        };
    }

    // ── Step 0: health ────────────────────────────────────────────────────────
    section("Step 0 — Health check");

    let health = match client.health_check().await {
        Ok(h) => h,
        Err(e) => {
            fail!(format!(
                "GET /healthz failed — is the gateway running? ({e})"
            ));
            std::process::exit(1);
        }
    };

    assert_sdk!(
        "health.status == \"ok\"",
        health.status == "ok",
        &health.status
    );
    assert_sdk!(
        "health.db == \"connected\"",
        health.db == "connected",
        &health.db
    );
    assert_sdk!(
        "health.version is non-empty",
        !health.version.is_empty(),
        &health.version
    );
    println!("  gateway version : {}", health.version);
    println!("  settle_enabled  : {}", health.settle_enabled);

    // ── Step 1: verify_inference ──────────────────────────────────────────────
    section("Step 1 — Submit + wait (verify_inference)");

    let result = match client.verify_inference(&model_id, input_data.clone(), wallet_address).await {
        Ok(r) => r,
        Err(e) => {
            fail!(format!("verify_inference failed: {e}"));
            std::process::exit(1);
        }
    };

    assert_sdk!(
        "result.job_id is non-empty",
        !result.job_id.is_empty(),
        &result.job_id
    );
    assert_sdk!(
        "result.status is terminal and successful",
        result.status.is_success(),
        &result.status
    );
    assert_sdk!(
        "result.elapsed_ms > 0",
        result.elapsed_ms > 0,
        result.elapsed_ms
    );

    // tx_hash is only present once settled on-chain
    if matches!(result.status, JobStatus::Settled) {
        assert_sdk!(
            "result.tx_hash starts with 0x",
            result
                .tx_hash
                .as_deref()
                .map_or(false, |h| h.starts_with("0x")),
            &result.tx_hash
        );
    } else {
        println!("  ℹ️   status is 'done' (not yet settled) — tx_hash may be absent");
    }

    // attestation_hash — present once gateway exposes it (impl guide §6c)
    if let Some(ref hash) = result.attestation_hash {
        assert_sdk!(
            "attestation_hash is 66 chars (0x + 32 bytes hex)",
            hash.starts_with("0x") && hash.len() == 66,
            hash
        );
    } else {
        println!(
            "  ℹ️   attestation_hash not yet exposed by gateway — add field to JobStatusResponse"
        );
    }

    // ── Step 2: cross-check via get_job ──────────────────────────────────────
    section("Step 2 — Cross-check via get_job");

    let job = match client.get_job(&result.job_id).await {
        Ok(j) => j,
        Err(e) => {
            fail!(format!("get_job failed: {e}"));
            all_passed = false;
            // continue to step 3 rather than exiting
            None::<veil_sdk::Job>.unwrap_or_else(|| unreachable!())
        }
    };

    assert_sdk!(
        "job.job_id matches",
        job.job_id == result.job_id,
        &job.job_id
    );
    assert_sdk!(
        "job.status is terminal",
        job.status.is_terminal(),
        &job.status
    );
    if matches!(result.status, JobStatus::Settled) {
        assert_sdk!(
            "job.tx_hash matches verify_inference result",
            job.tx_hash == result.tx_hash,
            &job.tx_hash
        );
    }

    // ── Step 3: fetch raw proof ───────────────────────────────────────────────
    section("Step 3 — Fetch proof bytes via get_proof");

    let proof = match client.get_proof(&result.job_id).await {
        Ok(p) => Some(p),
        Err(e) => {
            // 202 means done-but-not-proof-ready is an expected transient state
            fail!(format!("get_proof failed: {e}"));
            all_passed = false;
            None
        }
    };

    if let Some(ref p) = proof {
        assert_sdk!(
            "proof.proof_hex is non-empty",
            !p.proof_hex.is_empty() && p.proof_hex.len() >= 64,
            &p.proof_hex[..std::cmp::min(16, p.proof_hex.len())]
        );
        assert_sdk!("proof.size_bytes > 0", p.size_bytes > 0, p.size_bytes);
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    println!("\n═══════════════════════════════════════════════");
    println!("  Result Summary");
    println!("═══════════════════════════════════════════════");
    println!("  Job ID           : {}", result.job_id);
    println!("  Status           : {}", result.status);
    println!(
        "  Tx Hash          : {}",
        result.tx_hash.as_deref().unwrap_or("n/a")
    );
    println!(
        "  Attestation Hash : {}",
        result
            .attestation_hash
            .as_deref()
            .unwrap_or("n/a (gateway not yet exposing field)")
    );
    println!(
        "  Proof Size       : {}",
        proof.map_or("n/a".to_string(), |p| format!("{} bytes", p.size_bytes))
    );
    println!("  Elapsed (SDK)    : {}ms", result.elapsed_ms);
    println!("═══════════════════════════════════════════════");

    if all_passed {
        println!("\n  🎉  All assertions passed\n");
        std::process::exit(0);
    } else {
        eprintln!("\n  💥  Some assertions failed — see above\n");
        std::process::exit(1);
    }
}
