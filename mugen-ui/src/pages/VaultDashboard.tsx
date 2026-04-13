// mugen-ui/src/pages/VaultDashboard.tsx

import { useState } from "react";
import { Link } from "react-router-dom";
import { useAccount, useConnect, useDisconnect } from "wagmi";
import { injected } from "wagmi/connectors";
import { useVault } from "../hooks/useVault";
import styles from "./VaultDashboard.module.css";

const HASHKEY_EXPLORER = "https://testnet-explorer.hsk.xyz/tx";

// ── Helpers ───────────────────────────────────────────────────────────────────

function shortAddr(addr: string, n = 6) {
  return `${addr.slice(0, n)}...${addr.slice(-4)}`;
}

function formatHsk(val: string | undefined) {
  if (!val) return "0.00";
  return parseFloat(val).toFixed(2);
}

function formatTxError(message: string) {
  if (
    message.includes("User rejected") ||
    message.includes("rejected") ||
    message.includes("denied")
  ) {
    return "Transaction was rejected in your wallet.";
  }
  if (message.includes("insufficient funds")) {
    return "Your wallet does not have enough HSK to pay for this deposit and gas.";
  }
  return message;
}

function timeAgo(iso: string) {
  const diff = Date.now() - new Date(iso).getTime();
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return new Date(iso).toLocaleDateString();
}

// ── Deposit modal ─────────────────────────────────────────────────────────────

function DepositModal({
  onConfirm,
  onClose,
  pending,
  hskPrice,
}: {
  onConfirm: (amount: string) => void;
  onClose: () => void;
  pending: boolean;
  hskPrice: number | null;
}) {
  const [amount, setAmount] = useState("20");
  const parsed = parseFloat(amount) || 0;
  // FIX: proofs display rounds down — 1 HSK shows 0 standard, that's correct
  // but we should NOT block the deposit. Any amount > 0 is valid.
  // Users can accumulate partial balances.
  const proofs = Math.floor(parsed / 2);
  const usdVal = hskPrice ? (parsed * hskPrice).toFixed(2) : null;

  // FIX: deposit is valid for any amount > 0, not just multiples of 2.
  // A user might want to top up an odd balance (e.g. they have 1 HSK left
  // and deposit 1 more to make it 2 = 1 proof).
  const isValid = parsed > 0;

  return (
    <div className={styles.modalOverlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.modalHeader}>
          <span className={styles.modalTitle}>deposit hsk</span>
          <button className={styles.modalClose} onClick={onClose}>
            ✕
          </button>
        </div>

        <div className={styles.modalBody}>
          <div className={styles.inputGroup}>
            <label className={styles.inputLabel}>amount (hsk)</label>
            <div className={styles.inputRow}>
              <input
                className={styles.amountInput}
                type="number"
                min="0.01" // FIX: was "2", now any positive amount
                step="1"
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
                disabled={pending}
              />
              <span className={styles.inputSuffix}>HSK</span>
            </div>
          </div>

          <div className={styles.depositPreview}>
            <div className={styles.previewRow}>
              <span className={styles.previewLabel}>proofs you'll get</span>
              <span className={styles.previewVal}>
                {proofs > 0 ? `${proofs} standard` : `< 1 standard`}
              </span>
            </div>
            <div className={styles.previewRow}>
              <span className={styles.previewLabel}>cost per proof</span>
              <span className={styles.previewVal}>2 HSK</span>
            </div>
            {usdVal && (
              <div className={styles.previewRow}>
                <span className={styles.previewLabel}>approx. USD value</span>
                <span className={styles.previewVal}>≈ ${usdVal}</span>
              </div>
            )}
          </div>

          <div className={styles.quickAmounts}>
            {["10", "20", "50", "100"].map((v) => (
              <button
                key={v}
                className={`${styles.quickBtn} ${amount === v ? styles.quickBtnActive : ""}`}
                onClick={() => setAmount(v)}
                disabled={pending}
              >
                {v}
              </button>
            ))}
          </div>
        </div>

        <div className={styles.modalFooter}>
          <button
            className={styles.cancelBtn}
            onClick={onClose}
            disabled={pending}
          >
            cancel
          </button>
          <button
            className={styles.confirmBtn}
            onClick={() => onConfirm(amount)}
            disabled={pending || !isValid} 
          >
            {pending ? (
              <>
                <span className={styles.spinner} /> confirming…
              </>
            ) : (
              `deposit ${amount} hsk →`
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Withdraw modal ────────────────────────────────────────────────────────────

function WithdrawModal({
  maxBalance,
  onConfirm,
  onClose,
  pending,
}: {
  maxBalance: string;
  onConfirm: (amount: string) => void;
  onClose: () => void;
  pending: boolean;
}) {
  const [amount, setAmount] = useState(maxBalance);

  return (
    <div className={styles.modalOverlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.modalHeader}>
          <span className={styles.modalTitle}>withdraw hsk</span>
          <button className={styles.modalClose} onClick={onClose}>
            ✕
          </button>
        </div>

        <div className={styles.modalBody}>
          <div className={styles.inputGroup}>
            <label className={styles.inputLabel}>amount (hsk)</label>
            <div className={styles.inputRow}>
              <input
                className={styles.amountInput}
                type="number"
                min="0"
                max={maxBalance}
                step="0.01"
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
                disabled={pending}
              />
              <button
                className={styles.maxBtn}
                onClick={() => setAmount(maxBalance)}
                disabled={pending}
              >
                max
              </button>
            </div>
          </div>
          <p className={styles.withdrawNote}>
            Available: {parseFloat(maxBalance).toFixed(4)} HSK
          </p>
        </div>

        <div className={styles.modalFooter}>
          <button
            className={styles.cancelBtn}
            onClick={onClose}
            disabled={pending}
          >
            cancel
          </button>
          <button
            className={styles.confirmBtn}
            onClick={() => onConfirm(amount)}
            disabled={pending || parseFloat(amount) <= 0}
          >
            {pending ? (
              <>
                <span className={styles.spinner} /> confirming…
              </>
            ) : (
              `withdraw ${parseFloat(amount).toFixed(2)} hsk →`
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Main page ─────────────────────────────────────────────────────────────────

export default function VaultDashboard() {
  const { connect } = useConnect();
  const { disconnect } = useDisconnect();
  const vault = useVault();

  const [showDeposit, setShowDeposit] = useState(false);
  const [showWithdraw, setShowWithdraw] = useState(false);

  function handleDeposit(amount: string) {
    vault.deposit(amount);
  }

  function handleWithdraw(amount: string) {
    vault.withdraw(amount);
    setShowWithdraw(false);
  }

  // Close modals on success
  if (vault.depositSuccess && showDeposit) setShowDeposit(false);
  if (vault.withdrawSuccess && showWithdraw) setShowWithdraw(false);

  const balance = vault.stats?.vaultBalance ?? "0";
  const walletBalance = vault.walletBalance ?? "0";
  const hskUsd = vault.hskPrice
    ? (parseFloat(balance) * vault.hskPrice).toFixed(2)
    : null;
  const headerBalance = vault.walletBalanceLoading
    ? "—"
    : parseFloat(walletBalance).toFixed(2);
  const statusMessage =
    vault.txError
      ? formatTxError(vault.txError)
      : !vault.VAULT_ADDRESS
        ? "Vault contract address is not configured in the frontend."
        : vault.isWrongNetwork
          ? "Switch your wallet to HashKey Testnet (chain 133) to deposit or withdraw."
          : vault.balanceError
            ? `Vault balance read failed: ${vault.balanceError}`
            : vault.statsError
              ? vault.statsError
              : vault.txSuccessMessage;
  const statusTone =
    vault.txError || !vault.VAULT_ADDRESS || vault.isWrongNetwork || vault.balanceError
      ? "error"
      : "success";

  return (
    <div className={styles.root}>
      {/* ── Modals ── */}
      {showDeposit && (
        <DepositModal
          onConfirm={handleDeposit}
          onClose={() => setShowDeposit(false)}
          pending={vault.depositPending}
          hskPrice={vault.hskPrice}
        />
      )}
      {showWithdraw && (
        <WithdrawModal
          maxBalance={balance}
          onConfirm={handleWithdraw}
          onClose={() => setShowWithdraw(false)}
          pending={vault.withdrawPending}
        />
      )}

      {/* ── Header ── */}
      <header className={styles.header}>
        <div className={styles.headerLeft}>
          <Link to="/" className={styles.breadcrumb}>
            MUGEN
          </Link>
          <span className={styles.breadcrumbSep}>/</span>
          <span className={styles.breadcrumbCurrent}>VEIL</span>
          {vault.address && (
            <button
              className={styles.walletChip}
              onClick={() => disconnect()}
              title="click to disconnect"
            >
              {shortAddr(vault.address)}
            </button>
          )}
        </div>

        <div className={styles.headerRight}>
          {vault.isConnected ? (
            <div className={styles.proveBalance}>
              <span className={styles.proveAmount}>{headerBalance}</span>
              <span className={styles.proveLabel}>HSK</span>
            </div>
          ) : (
            <button
              className={styles.connectBtn}
              onClick={() => connect({ connector: injected() })}
            >
              connect wallet
            </button>
          )}
        </div>
      </header>

      {statusMessage && (
        <div
          className={`${styles.statusBanner} ${
            statusTone === "error" ? styles.statusBannerError : styles.statusBannerSuccess
          }`}
        >
          <div className={styles.statusBannerRow}>
            <span>{statusMessage}</span>
            {vault.isWrongNetwork && (
              <button
                className={styles.switchChainBtn}
                onClick={vault.switchToHashKeyTestnet}
                disabled={vault.switchChainPending}
              >
                {vault.switchChainPending
                  ? "switching…"
                  : "switch to HashKey Testnet"}
              </button>
            )}
          </div>
        </div>
      )}

      {!vault.isConnected ? (
        <div className={styles.connectPrompt}>
          <div className={styles.connectPromptTitle}>connect your wallet</div>
          <p className={styles.connectPromptSub}>
            Connect to view your Veil vault balance and proof history.
          </p>
          <button
            className={styles.connectBtnLarge}
            onClick={() => connect({ connector: injected() })}
          >
            connect wallet →
          </button>
        </div>
      ) : (
        <>
          {/* ── Stat cards ── */}
          <div className={styles.statGrid}>
            <div className={styles.statCard}>
              <div className={styles.statLabel}>vault balance</div>
              <div className={styles.statValue}>{formatHsk(balance)}</div>
              <div className={styles.statSub}>
                HSK{hskUsd ? ` ≈ $${hskUsd}` : ""}
              </div>
            </div>

            <div className={styles.statCard}>
              <div className={styles.statLabel}>proof count</div>
              <div className={styles.statValue}>
                {vault.statsLoading ? "—" : (vault.stats?.proofCount ?? 0)}
              </div>
              <div className={styles.statSub}>total proofs paid</div>
            </div>

            <div className={styles.statCard}>
              <div className={styles.statLabel}>hsk spent</div>
              <div className={styles.statValue}>
                {vault.statsLoading ? "—" : formatHsk(vault.stats?.hskSpent)}
              </div>
              <div className={styles.statSub}>
                {vault.stats
                  ? `${vault.stats.proofCount} × 2 HSK standard`
                  : "loading…"}
              </div>
            </div>
          </div>

          {/* ── Action buttons ── */}
          <div className={styles.actions}>
            <button
              className={styles.actionBtn}
              onClick={() => setShowDeposit(true)}
              disabled={vault.isWrongNetwork}
            >
              <span className={styles.actionIcon}>↓</span>
              <span>
                deposit
                <br />
                hsk
              </span>
            </button>
            <button
              className={styles.actionBtn}
              onClick={() => setShowWithdraw(true)}
              disabled={vault.isWrongNetwork || parseFloat(balance) === 0}
            >
              <span className={styles.actionIcon}>↑</span>
              <span>withdraw</span>
            </button>
            <Link to="/explorer" className={styles.actionBtn}>
              <span className={styles.actionIcon}>≡</span>
              <span>
                view proofs <span className={styles.extIcon}>↗</span>
              </span>
            </Link>
            <button className={styles.actionBtn} onClick={vault.refetchStats}>
              <span className={styles.actionIcon}>◷</span>
              <span>
                history <span className={styles.extIcon}>↗</span>
              </span>
            </button>
          </div>

          {/* ── Main grid ── */}
          <div className={styles.mainGrid}>
            {/* ── Balance history ── */}
            <div className={styles.panel}>
              <div className={styles.panelHeader}>
                <span className={styles.panelTitle}>balance history</span>
                <span className={styles.panelMeta}>view all</span>
              </div>

              <div className={styles.filterRow}>
                <span className={styles.filterLabel}>op:</span>
                {(["all", "deduct", "deposit"] as const).map((f) => (
                  <button
                    key={f}
                    className={`${styles.filterBtn} ${vault.historyFilter === f ? styles.filterBtnActive : ""}`}
                    onClick={() => vault.changeFilter(f)}
                  >
                    {f}
                  </button>
                ))}
              </div>

              <div className={styles.historyTable}>
                <div className={styles.historyHead}>
                  <span>tx hash</span>
                  <span>operation</span>
                  <span className={styles.historyAmountCol}>amount</span>
                </div>

                {vault.historyLoading && vault.history.length === 0 && (
                  <div className={styles.historyEmpty}>
                    <span className={styles.spinner} /> loading…
                  </div>
                )}

                {!vault.historyLoading && vault.history.length === 0 && (
                  <div className={styles.historyEmpty}>no transactions yet</div>
                )}

                {vault.history.map((entry, i) => (
                  <div
                    key={`${entry.txHash}-${i}`}
                    className={styles.historyRow}
                  >
                    <a
                      className={styles.txLink}
                      href={`${HASHKEY_EXPLORER}/${entry.txHash}`}
                      target="_blank"
                      rel="noreferrer"
                    >
                      {shortAddr(entry.txHash, 8)}
                    </a>
                    <span
                      className={`${styles.opBadge} ${
                        entry.operation === "deposit"
                          ? styles.opDeposit
                          : styles.opDeduct
                      }`}
                    >
                      {entry.operation}
                    </span>
                    <span
                      className={`${styles.historyAmount} ${
                        entry.operation === "deposit"
                          ? styles.amountPositive
                          : styles.amountNegative
                      }`}
                    >
                      {entry.amount} HSK
                    </span>
                  </div>
                ))}
              </div>

              {vault.hasMoreHistory && (
                <button
                  className={styles.loadMore}
                  onClick={vault.loadMoreHistory}
                  disabled={vault.historyLoading}
                >
                  load more
                </button>
              )}
            </div>

            {/* ── Account panel ── */}
            <div className={styles.panel}>
              <div className={styles.panelHeader}>
                <span className={styles.panelTitle}>account</span>
              </div>

              <div className={styles.accountGrid}>
                <div className={styles.accountRow}>
                  <span className={styles.accountLabel}>vault address</span>
                  <span className={styles.accountVal}>
                    {vault.VAULT_ADDRESS
                      ? shortAddr(vault.VAULT_ADDRESS)
                      : "not deployed"}
                  </span>
                </div>
                <div className={styles.accountRow}>
                  <span className={styles.accountLabel}>wallet</span>
                  <span className={styles.accountVal}>
                    {vault.address ? shortAddr(vault.address) : "—"}
                  </span>
                </div>
                <div className={styles.accountRow}>
                  <span className={styles.accountLabel}>network</span>
                  <span className={styles.accountVal}>
                    <span className={styles.networkDot} />
                    {vault.chainId
                      ? vault.isWrongNetwork
                        ? `Wrong network · connected to ${vault.chainId}`
                        : "HashKey Testnet · 133"
                      : "HashKey Testnet · 133"}
                  </span>
                </div>
                <div className={styles.accountRow}>
                  <span className={styles.accountLabel}>proofs remaining</span>
                  <span
                    className={`${styles.accountVal} ${styles.proofsRemaining}`}
                  >
                    {vault.stats
                      ? `${vault.stats.proofsRemaining} standard`
                      : "—"}
                  </span>
                </div>
              </div>

              <div className={styles.feeCard}>
                <div className={styles.feeCardLabel}>proof fees</div>
                <div className={styles.feeRow}>
                  <span className={styles.feeTier}>standard</span>
                  <span className={styles.feeVal}>2 HSK</span>
                </div>
                <div className={styles.feeRow}>
                  <span className={styles.feeTier}>priority</span>
                  <span className={styles.feeVal}>5 HSK</span>
                </div>
              </div>

              <button
                className={styles.depositHskBtn}
                onClick={() => setShowDeposit(true)}
                disabled={vault.isWrongNetwork}
              >
                deposit hsk
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
