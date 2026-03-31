import { useState } from "react";
import { useGateway } from "./hooks/useGateway";
import InferencePanel from "./components/InferencePanel";
import JobHistory from "./components/JobHistory";
import styles from "./App.module.css";

const INSTALL_CMD = "npm install @mugen/sdk";
const USAGE_SNIPPET = `import { ElenxisClient } from '@mugen/sdk'

const client = new ElenxisClient({
  gatewayUrl: 'https://your-gateway.xyz',
  timeoutMs:  600_000,
})

const result = await client.verifyInference({
  modelId:   'tiny_mlp_v1',
  inputData: [[0.1, 0.2, 0.3, 0.4]],
})

console.log(result.txHash)          // on-chain settlement tx
console.log(result.attestationHash) // keccak256 fingerprint`;

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

function HealthDot({
  status,
}: {
  status: "checking" | "healthy" | "unreachable";
}) {
  const color = {
    checking: "#888882",
    healthy: "#c8f060",
    unreachable: "#f06060",
  }[status];
  const label = {
    checking: "checking",
    healthy: "online",
    unreachable: "offline",
  }[status];
  return (
    <span className={styles.health}>
      <span
        className={styles.healthDot}
        style={{
          background: color,
          animation:
            status === "checking"
              ? "pulse-dot 1.2s ease-in-out infinite"
              : "none",
        }}
      />
      {label}
    </span>
  );
}

export default function App() {
  const { health, checkHealth, history, addJob, clearHistory } = useGateway();

  return (
    <div className={styles.root}>
      {/* ── Nav ── */}
      <nav className={styles.nav}>
        <span className={styles.wordmark}>mugen</span>
        <div className={styles.navRight}>
          <HealthDot status={health.status} />
          <button className={styles.navBtn} onClick={checkHealth}>
            refresh
          </button>
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
          Prove any ML inference.
          <br />
          <span className={styles.heroAccent}>On-chain.</span>
        </h1>
        <p className={styles.heroSub}>
          Mugen generates ZK proofs of model inference using EZKL, verifies them
          on-chain via Halo2 KZG, and settles the attestation on StarkNet — all
          from a single SDK call.
        </p>
      </header>

      <main className={styles.main}>
        {/* ── Left col: SDK docs ── */}
        <section className={styles.docs}>
          <div className={styles.sectionLabel}>Install</div>
          <div className={styles.codeBlock}>
            <code className={styles.code}>{INSTALL_CMD}</code>
            <CopyButton text={INSTALL_CMD} />
          </div>

          <div className={styles.sectionLabel} style={{ marginTop: "2rem" }}>
            Usage
          </div>
          <div className={styles.codeBlock}>
            <pre className={styles.pre}>{USAGE_SNIPPET}</pre>
            <CopyButton text={USAGE_SNIPPET} />
          </div>

          <div className={styles.sectionLabel} style={{ marginTop: "2rem" }}>
            How it works
          </div>
          <ol className={styles.steps}>
            {[
              ["Submit", "POST inference input to the Elenxis gateway"],
              ["Prove", "EZKL generates a Halo2 ZK proof of the model run"],
              ["Verify", "KZG pairing check on Ethereum Sepolia"],
              [
                "Settle",
                "Result bridged to InferenceVerifier.cairo on StarkNet",
              ],
            ].map(([title, desc], i) => (
              <li key={i} className={styles.step}>
                <span className={styles.stepNum}>
                  {String(i + 1).padStart(2, "0")}
                </span>
                <div>
                  <div className={styles.stepTitle}>{title}</div>
                  <div className={styles.stepDesc}>{desc}</div>
                </div>
              </li>
            ))}
          </ol>

          {health.version && (
            <div className={styles.gatewayInfo}>
              <span className={styles.gwLabel}>Gateway</span>
              <span className={styles.gwVal}>v{health.version}</span>
              <span className={styles.gwLabel}>Settlement</span>
              <span className={styles.gwVal}>
                {health.settleEnabled ? "enabled" : "disabled"}
              </span>
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
        <a
          href="https://www.npmjs.com/package/@mugen/sdk"
          target="_blank"
          rel="noreferrer"
        >
          @mugen/sdk ↗
        </a>
      </footer>
    </div>
  );
}
