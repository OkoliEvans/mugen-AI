// mugen-ui/src/hooks/useVault.ts

import { useCallback, useEffect, useState } from "react";
import {
  useAccount,
  useBalance,
  useChainId,
  useReadContract,
  useSwitchChain,
  useWriteContract,
  useWaitForTransactionReceipt,
} from "wagmi";
import { parseEther, formatEther } from "viem";
import { VAULT_ABI } from "../lib/vaultAbi";
import { hashkeyTestnet } from "../lib/wagmiConfig";

const VAULT_ADDRESS = import.meta.env.VITE_VAULT_ADDRESS as
  | `0x${string}`
  | undefined;
const GATEWAY = import.meta.env.VITE_GATEWAY_URL ?? "/api";
const STANDARD_FEE = parseEther("2");

export interface AccountStats {
  vaultBalance: string; // formatted HSK (e.g. "12.0000")
  proofCount: number;
  hskSpent: string; // formatted HSK (e.g. "24.0000")
  proofsRemaining: number;
}

export interface HistoryEntry {
  txHash: string;
  operation: "deposit" | "deduct";
  amount: string; // signed formatted HSK (e.g. "+20.0000" or "-2.0000")
  timestamp: string;
  jobId?: string;
}

export function useVault() {
  const { address, isConnected } = useAccount();
  const chainId = useChainId();
  const {
    switchChainAsync,
    isPending: switchChainPending,
    error: switchChainError,
  } = useSwitchChain();
  const {
    data: walletBalanceData,
    isLoading: walletBalanceLoading,
    error: walletBalanceError,
    refetch: refetchWalletBalance,
  } = useBalance({
    address,
    chainId: hashkeyTestnet.id,
    query: { enabled: !!address },
  });
  const {
    data: rawBalance,
    refetch: refetchBalance,
    isLoading: balanceLoading,
    error: balanceError,
  } = useReadContract({
    chainId: hashkeyTestnet.id,
    address: VAULT_ADDRESS,
    abi: VAULT_ABI,
    functionName: "balanceOf",
    args: [address!],
    query: { enabled: !!address && !!VAULT_ADDRESS },
  });

  // ── Gateway account stats ─────────────────────────────────────────────────
  const [stats, setStats] = useState<AccountStats | null>(null);
  const [statsLoading, setStatsLoading] = useState(false);
  const [statsError, setStatsError] = useState<string | null>(null);

  const [txError, setTxError] = useState<string | null>(null);
  const [txSuccessMessage, setTxSuccessMessage] = useState<string | null>(null);
  const [lastDepositAmount, setLastDepositAmount] = useState<string | null>(null);
  const [lastWithdrawAmount, setLastWithdrawAmount] = useState<string | null>(null);

  const isWrongNetwork = isConnected && chainId !== 133;

  function formatError(error: unknown) {
    if (error instanceof Error) return error.message;
    if (typeof error === "string") return error;
    return "Transaction failed. Check your wallet and network, then try again.";
  }

  function isUnknownChainError(error: unknown) {
    if (!error || typeof error !== "object") return false;
    const maybeError = error as { code?: number; message?: string };
    return (
      maybeError.code === 4902 ||
      maybeError.message?.toLowerCase().includes("unknown chain") === true ||
      maybeError.message?.toLowerCase().includes("unrecognized chain") === true
    );
  }

  function getEthereumProvider() {
    return (globalThis as {
      ethereum?: { request: (args: { method: string; params?: unknown[] }) => Promise<unknown> };
    }).ethereum;
  }

  async function waitForTargetChain(targetChainId: number, timeoutMs = 10_000) {
    const ethereum = getEthereumProvider();
    if (!ethereum) throw new Error("No injected wallet provider found.");

    const start = Date.now();
    while (Date.now() - start < timeoutMs) {
      const chainHex = (await ethereum.request({
        method: "eth_chainId",
      })) as string;
      if (parseInt(chainHex, 16) === targetChainId) return;
      await new Promise((resolve) => setTimeout(resolve, 400));
    }
    throw new Error("Wallet network did not switch in time. Please retry.");
  }

  async function addHashKeyChain() {
    const ethereum = getEthereumProvider();
    if (!ethereum) throw new Error("No injected wallet provider found.");

    await ethereum.request({
      method: "wallet_addEthereumChain",
      params: [
        {
          chainId: `0x${hashkeyTestnet.id.toString(16)}`,
          chainName: hashkeyTestnet.name,
          nativeCurrency: hashkeyTestnet.nativeCurrency,
          rpcUrls: hashkeyTestnet.rpcUrls.default.http,
          blockExplorerUrls: [hashkeyTestnet.blockExplorers.default.url],
        },
      ],
    });
  }

  async function switchToHashKeyTestnet() {
    try {
      setTxError(null);
      setTxSuccessMessage(null);
      try {
        await switchChainAsync({ chainId: hashkeyTestnet.id });
      } catch (error) {
        if (!isUnknownChainError(error)) throw error;
        await addHashKeyChain();
        await switchChainAsync({ chainId: hashkeyTestnet.id });
      }
      await waitForTargetChain(hashkeyTestnet.id);
      await Promise.allSettled([refetchWalletBalance(), refetchBalance(), fetchStats()]);
    } catch (error) {
      setTxError(formatError(error));
    }
  }

  async function recordVaultEvent(
    wallet: string,
    txHash: string,
    operation: "deposit" | "deduct",
    amountHsk: string,
  ) {
    try {
      await fetch(`${GATEWAY}/v1/account/${wallet}/events`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          tx_hash: txHash,
          amount_wei: parseEther(amountHsk).toString(),
          operation,
        }),
      });
    } catch {
      // The on-chain tx already succeeded; history recording is best-effort.
    }
  }

  const fetchStats = useCallback(async () => {
    if (!address) {
      setStats(null);
      setStatsError(null);
      return;
    }
    setStatsLoading(true);
    setStatsError(null);
    try {
      const res = await fetch(`${GATEWAY}/v1/account/${address}`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();

      // FIX: gateway returns:
      //   vault_balance_wei  — raw U256 string, use this for on-chain balance
      //   vault_balance_hsk  — pre-formatted HSK string ("12.0000")
      //   proof_count        — number (i64)
      //   hsk_spent          — pre-formatted HSK string ("24.0000"), NOT wei
      //   proofs_remaining   — number (i64)
      //
      // Use rawBalance from wagmi for vaultBalance — it's the authoritative
      // on-chain read and doesn't go stale between gateway polls.
      const balanceBigInt = rawBalance as bigint | undefined;
      const balanceHsk =
        balanceBigInt !== undefined
          ? formatEther(balanceBigInt)
          : (data.vault_balance_hsk ?? "0");

      setStats({
        vaultBalance: balanceHsk,
        proofCount: Number(data.proof_count ?? 0),
        hskSpent: String(data.hsk_spent ?? "0"), // already HSK string
        proofsRemaining: Number(data.proofs_remaining ?? 0),
      });
    } catch (e) {
      // Fallback: derive entirely from on-chain rawBalance
      const balanceBigInt = rawBalance as bigint | undefined;
      if (balanceBigInt !== undefined) {
        const balanceHsk = formatEther(balanceBigInt);
        setStats({
          vaultBalance: balanceHsk,
          proofCount: 0,
          hskSpent: "0",
          proofsRemaining: Number(balanceBigInt / STANDARD_FEE),
        });
      }
      setStatsError("Unable to load account stats from the gateway.");
    } finally {
      setStatsLoading(false);
    }
  }, [address, rawBalance]);

  useEffect(() => {
    fetchStats();
  }, [fetchStats]);

  // ── History ───────────────────────────────────────────────────────────────
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyPage, setHistoryPage] = useState(1);
  const [hasMoreHistory, setHasMoreHistory] = useState(true);
  const [historyFilter, setHistoryFilter] = useState<
    "all" | "deduct" | "deposit"
  >("all");

  const fetchHistory = useCallback(
    async (page = 1, filter = historyFilter, reset = false) => {
      if (!address) return;
      setHistoryLoading(true);
      try {
        const params = new URLSearchParams({
          page: String(page),
          limit: "20",
          ...(filter !== "all" ? { operation: filter } : {}),
        });
        const res = await fetch(
          `${GATEWAY}/v1/account/${address}/history?${params}`,
        );
        if (!res.ok) return;
        const data = await res.json();

        // FIX: gateway returns { entries: [...] } with amount_wei per entry.
        // amount_wei is always positive — apply sign based on operation.
        const entries: HistoryEntry[] = (data.entries ?? []).map(
          (e: {
            tx_hash: string;
            operation: string;
            amount_wei: string;
            timestamp: string;
            job_id?: string;
          }) => {
            const hsk = formatEther(BigInt(e.amount_wei ?? "0"));
            const sign = e.operation === "deposit" ? "+" : "-";
            return {
              txHash: e.tx_hash,
              operation: e.operation as "deposit" | "deduct",
              amount: `${sign}${hsk}`,
              timestamp: e.timestamp,
              jobId: e.job_id,
            };
          },
        );

        setHistory((prev) => (reset ? entries : [...prev, ...entries]));
        setHasMoreHistory(entries.length === 20);
        setHistoryPage(page);
      } finally {
        setHistoryLoading(false);
      }
    },
    [address, historyFilter],
  );

  useEffect(() => {
    fetchHistory(1, historyFilter, true);
  }, [address, historyFilter]); // eslint-disable-line react-hooks/exhaustive-deps

  function changeFilter(f: "all" | "deduct" | "deposit") {
    setHistoryFilter(f);
    setHistory([]);
    fetchHistory(1, f, true);
  }

  function loadMoreHistory() {
    fetchHistory(historyPage + 1);
  }

  // ── Deposit ───────────────────────────────────────────────────────────────
  const {
    writeContractAsync,
    data: depositTxHash,
    isPending: depositPending,
    error: depositWriteError,
  } = useWriteContract();
  const {
    isLoading: depositConfirming,
    isSuccess: depositSuccess,
    isError: depositReceiptError,
    error: depositReceiptErrorValue,
  } = useWaitForTransactionReceipt({ hash: depositTxHash });

  useEffect(() => {
    if (depositSuccess) {
      setTxError(null);
      setTxSuccessMessage(
        lastDepositAmount
          ? `Deposit confirmed: ${lastDepositAmount} HSK added to your vault.`
          : "Deposit confirmed.",
      );
      if (address && depositTxHash && lastDepositAmount) {
        recordVaultEvent(address, depositTxHash, "deposit", lastDepositAmount);
      }
      refetchWalletBalance();
      refetchBalance();
      fetchStats();
      fetchHistory(1, historyFilter, true);
    }
  }, [depositSuccess, depositTxHash, lastDepositAmount, address]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (depositWriteError) {
      setTxSuccessMessage(null);
      setTxError(formatError(depositWriteError));
    }
  }, [depositWriteError]);

  useEffect(() => {
    if (depositReceiptError) {
      setTxSuccessMessage(null);
      setTxError(formatError(depositReceiptErrorValue));
    }
  }, [depositReceiptError, depositReceiptErrorValue]);

  async function deposit(amountHsk: string) {
    try {
      setTxError(null);
      setTxSuccessMessage(null);
      setLastDepositAmount(amountHsk);

      if (!VAULT_ADDRESS) throw new Error("Vault contract is not configured.");
      if (isWrongNetwork) throw new Error("Switch your wallet to HashKey Testnet (chain 133).");

      await writeContractAsync({
        chainId: hashkeyTestnet.id,
        address: VAULT_ADDRESS,
        abi: VAULT_ABI,
        functionName: "deposit",
        value: parseEther(amountHsk),
      });
    } catch (error) {
      setTxSuccessMessage(null);
      setTxError(formatError(error));
    }
  }

  // ── Withdraw ──────────────────────────────────────────────────────────────
  const {
    writeContractAsync: writeWithdrawAsync,
    data: withdrawTxHash,
    isPending: withdrawPending,
    error: withdrawWriteError,
  } = useWriteContract();
  const {
    isLoading: withdrawConfirming,
    isSuccess: withdrawSuccess,
    isError: withdrawReceiptError,
    error: withdrawReceiptErrorValue,
  } = useWaitForTransactionReceipt({ hash: withdrawTxHash });

  useEffect(() => {
    if (withdrawSuccess) {
      setTxError(null);
      setTxSuccessMessage(
        lastWithdrawAmount
          ? `Withdrawal confirmed: ${lastWithdrawAmount} HSK returned to your wallet.`
          : "Withdrawal confirmed.",
      );
      refetchWalletBalance();
      refetchBalance();
      fetchStats();
      fetchHistory(1, historyFilter, true);
    }
  }, [withdrawSuccess, lastWithdrawAmount]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (withdrawWriteError) {
      setTxSuccessMessage(null);
      setTxError(formatError(withdrawWriteError));
    }
  }, [withdrawWriteError]);

  useEffect(() => {
    if (withdrawReceiptError) {
      setTxSuccessMessage(null);
      setTxError(formatError(withdrawReceiptErrorValue));
    }
  }, [withdrawReceiptError, withdrawReceiptErrorValue]);

  async function withdraw(amountHsk: string) {
    try {
      setTxError(null);
      setTxSuccessMessage(null);
      setLastWithdrawAmount(amountHsk);

      if (!VAULT_ADDRESS) throw new Error("Vault contract is not configured.");
      if (isWrongNetwork) throw new Error("Switch your wallet to HashKey Testnet (chain 133).");

      await writeWithdrawAsync({
        chainId: hashkeyTestnet.id,
        address: VAULT_ADDRESS,
        abi: VAULT_ABI,
        functionName: "withdraw",
        args: [parseEther(amountHsk)],
      });
    } catch (error) {
      setTxSuccessMessage(null);
      setTxError(formatError(error));
    }
  }

  // ── HSK price ─────────────────────────────────────────────────────────────
  const [hskPrice, setHskPrice] = useState<number | null>(null);
  useEffect(() => {
    fetch(
      "https://api.coingecko.com/api/v3/simple/price?ids=hashkey-platform-token&vs_currencies=usd",
    )
      .then((r) => r.json())
      .then((d) => setHskPrice(d?.["hashkey-platform-token"]?.usd ?? null))
      .catch(() => setHskPrice(0.16));
  }, []);

  return {
    address,
    isConnected,
    chainId,
    isWrongNetwork,
    switchToHashKeyTestnet,
    switchChainPending,
    switchChainError: switchChainError ? formatError(switchChainError) : null,
    walletBalance: walletBalanceData
      ? formatEther(walletBalanceData.value)
      : null,
    walletBalanceLoading,
    walletBalanceError: walletBalanceError ? formatError(walletBalanceError) : null,
    stats,
    statsLoading: statsLoading || balanceLoading,
    statsError,
    balanceError: balanceError ? formatError(balanceError) : null,
    refetchStats: fetchStats,
    history,
    historyLoading,
    historyFilter,
    hasMoreHistory,
    changeFilter,
    loadMoreHistory,
    deposit,
    depositPending: depositPending || depositConfirming,
    depositSuccess,
    depositTxHash,
    withdraw,
    withdrawPending: withdrawPending || withdrawConfirming,
    withdrawSuccess,
    withdrawTxHash,
    txError,
    txSuccessMessage,
    hskPrice,
    VAULT_ADDRESS,
    STANDARD_FEE: "2",
    PRIORITY_FEE: "5",
  };
}
