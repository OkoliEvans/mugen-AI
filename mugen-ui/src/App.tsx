// mugen-ui/src/App.tsx

import { useState } from "react";
import { useGateway } from "./hooks/useGateway";
import InferencePanel from "./components/InferencePanel";
import JobHistory from "./components/JobHistory";
import styles from "./App.module.css";
import { Link } from "react-router-dom";

const TS_INSTALL = `npm install @mugen-ai/sdk`;

const TS_USAGE = `import { VeilClient } from '@mugen-ai/sdk'

const client = new VeilClient({
  gatewayUrl: 'https://your-gateway.xyz',
  timeoutMs:  600_000,
})

const job = await client.verifyInference({
  modelId:   'polymarket_mlp_v1',
  inputData: [[0.6, 0.4, 12000, 0.2]],
})

console.log(job.attestationHash) // keccak256 proof fingerprint
console.log(job.txHash)          // HashKey testnet settlement tx`;

const RUST_INSTALL = `# Cargo.toml
[dependencies]
veil-sdk = "0.1"`;

const RUST_USAGE = `use veil_sdk::{VeilClient, VeilConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = VeilClient::new(VeilConfig {
        gateway_url:      "https://your-gateway.xyz".into(),
        timeout_ms:       600_000,
        poll_interval_ms: 3_000,
    });

    let job = client
        .verify_inference("polymarket_mlp_v1", vec![
            vec![0.6, 0.4, 12000.0, 0.2],
        ])
        .await?;

    println!("attestation: {}", job.attestation_hash);
    println!("tx:          {}", job.tx_hash);
    Ok(())
}`;

const STEPS = [
  ["Submit",  "POST input to the Veil gateway — job ID returned immediately"],
  ["Prove",   "SP1 zkVM generates a compressed STARK proof of model inference"],
  ["Attest",  "keccak256(model_id ∥ input_hash ∥ output_hash) committed on-chain"],
  ["Settle",  "Aggregated Groth16 proof verified on HashKey testnet"],
];

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  function copy() {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1800);
  }
  return (
    <button className={styles.copyBtn} onClick={copy}>
      {copied ? "✓ copied" : "copy"}
    </button>
  );
}

function HealthDot({ status }: { status: "checking" | "healthy" | "unreachable" }) {
  const color = { checking: "#888882", healthy: "#c8f060", unreachable: "#f06060" }[status];
  const label = { checking: "checking", healthy: "online",  unreachable: "offline"  }[status];
  return (
    <span className={styles.health}>
      <span
        className={styles.healthDot}
        style={{
          background: color,
          animation: status === "checking" ? "pulse-dot 1.2s ease-in-out infinite" : "none",
        }}
      />
      {label}
    </span>
  );
}

export default function App() {
  const { health, checkHealth, history, addJob, clearHistory } = useGateway();
  const [sdkTab, setSdkTab] = useState<"ts" | "rust">("ts");

  const installSnippet = sdkTab === "ts" ? TS_INSTALL : RUST_INSTALL;
  const usageSnippet   = sdkTab === "ts" ? TS_USAGE   : RUST_USAGE;

  return (
    <div className={styles.root}>

      {/* ── Nav ── */}
      <nav className={styles.nav}>
        <span className={styles.wordmark}>mugen</span>
        <div className={styles.navRight}>
          <HealthDot status={health.status} />
          <button className={styles.navBtn} onClick={checkHealth}>refresh</button>
          <Link to="/explorer" className={styles.navBtn}>explorer</Link>
          <Link to="/account"  className={styles.navBtn}>account</Link>
          <a
            className={styles.navBtn}
            href="https://github.com/OkoliEvans/mugen-AI"
            target="_blank"
            rel="noreferrer"
          >
            github ↗
          </a>
        </div>
      </nav>

      {/* ── Hero ── */}
      <header className={styles.hero}>
        <div className={styles.heroTag}>verifiable inference network</div>
        <h1 className={styles.heroTitle}>
          Prove any ML inference.<br />
          <span className={styles.heroAccent}>On-chain.</span>
        </h1>
        <p className={styles.heroSub}>
          Mugen generates ZK proofs of model inference using SP1 (Succinct),
          attests the result on-chain via a Groth16 SNARK, and settles on
          HashKey testnet — all from a single SDK call, in Rust or TypeScript.
        </p>
      </header>

      <main className={styles.main}>

        {/* ── Left col: SDK docs ── */}
        <section className={styles.docs}>
          <div className={styles.tabRow}>
            <button
              className={`${styles.tab} ${sdkTab === "ts" ? styles.tabActive : ""}`}
              onClick={() => setSdkTab("ts")}
            >
              TypeScript
            </button>
            <button
              className={`${styles.tab} ${sdkTab === "rust" ? styles.tabActive : ""}`}
              onClick={() => setSdkTab("rust")}
            >
              Rust
            </button>
          </div>

          <div className={styles.sectionLabel}>Install</div>
          <div className={styles.codeBlock}>
            <code className={styles.code}>{installSnippet}</code>
            <CopyButton text={installSnippet} />
          </div>

          <div className={styles.sectionLabel} style={{ marginTop: "2rem" }}>Usage</div>
          <div className={styles.codeBlock}>
            <pre className={styles.pre}>{usageSnippet}</pre>
            <CopyButton text={usageSnippet} />
          </div>

          {sdkTab === "rust" && (
            <div className={styles.sdkNote}>
              The Rust SDK is the same crate used internally by the Veil agent.
              Requires <code>tokio</code> async runtime.
            </div>
          )}
          {sdkTab === "ts" && (
            <div className={styles.sdkNote}>
              Works in Node.js 18+ and all modern browsers. ESM and CJS builds included.
            </div>
          )}

          <div className={styles.sectionLabel} style={{ marginTop: "2rem" }}>How it works</div>
          <ol className={styles.steps}>
            {STEPS.map(([title, desc], i) => (
              <li key={i} className={styles.step}>
                <span className={styles.stepNum}>{String(i + 1).padStart(2, "0")}</span>
                <div>
                  <div className={styles.stepTitle}>{title}</div>
                  <div className={styles.stepDesc}>{desc}</div>
                </div>
              </li>
            ))}
          </ol>

          <div className={styles.archNote}>
            <div className={styles.archTitle}>Proof architecture</div>
            <p className={styles.archBody}>
              Each inference job produces a <strong>compressed STARK</strong> proof
              via the Succinct Prover Network. Jobs are batched — once <code>BATCH_SIZE</code> proofs
              accumulate (or <code>BATCH_FLUSH_SECS</code> elapse), the aggregator wraps them into a
              single <strong>Groth16 SNARK</strong> and settles on-chain. This reduces gas cost to
              one transaction per batch regardless of how many inferences it contains.
            </p>
          </div>

          {health.version && (
            <div className={styles.gatewayInfo}>
              <span className={styles.gwLabel}>Gateway</span>
              <span className={styles.gwVal}>v{health.version}</span>
              <span className={styles.gwLabel}>Settlement</span>
              <span className={styles.gwVal}>{health.settleEnabled ? "enabled" : "disabled"}</span>
            </div>
          )}
        </section>

        {/* ── Right col: live inference + history ── */}
        <section className={styles.playground}>
          <div className={styles.sectionLabel}>Try it live</div>
          <div className={styles.card}>
            <InferencePanel onJobComplete={addJob} />
          </div>
          <div style={{ marginTop: "2rem" }}>
            <JobHistory jobs={history} onClear={clearHistory} />
          </div>
        </section>
      </main>

      <footer className={styles.footer}>
        <span>© 2026 Mist Labs</span>
        <span className={styles.footerDot}>·</span>
        <span>MIT License</span>
        <span className={styles.footerDot}>·</span>
        <a href="https://www.npmjs.com/package/@mugen-ai/sdk" target="_blank" rel="noreferrer">
          @mugen-ai/sdk ↗
        </a>
        <span className={styles.footerDot}>·</span>
        <a href="https://crates.io/crates/veil-sdk" target="_blank" rel="noreferrer">
          veil-sdk ↗
        </a>
      </footer>
    </div>
  );
}