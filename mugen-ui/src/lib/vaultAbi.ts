// mugen-ui/src/lib/vaultAbi.ts

export const VAULT_ABI = [
  // ── Constructor & Receive ────────────────────────────────────────────────
  {
    type: "constructor",
    stateMutability: "nonpayable",
    inputs: [
      { name: "_gateway", type: "address" },
      { name: "_initialOwner", type: "address" },
    ],
  },
  {
    type: "receive",
    stateMutability: "payable",
  },

  // ── Read ─────────────────────────────────────────────────────────────────
  {
    type: "function",
    name: "PRIORITY_FEE",
    stateMutability: "view",
    inputs: [],
    outputs: [{ type: "uint256" }],
  },
  {
    type: "function",
    name: "STANDARD_FEE",
    stateMutability: "view",
    inputs: [],
    outputs: [{ type: "uint256" }],
  },
  {
    type: "function",
    name: "balanceOf",
    stateMutability: "view",
    inputs: [{ name: "user", type: "address" }],
    outputs: [{ type: "uint256" }],
  },
  {
    type: "function",
    name: "balances",
    stateMutability: "view",
    inputs: [{ type: "address" }],
    outputs: [{ type: "uint256" }],
  },
  {
    type: "function",
    name: "canProve",
    stateMutability: "view",
    inputs: [
      { name: "user", type: "address" },
      { name: "tier", type: "uint8" },
    ],
    outputs: [{ type: "bool" }],
  },
  {
    type: "function",
    name: "gateway",
    stateMutability: "view",
    inputs: [],
    outputs: [{ type: "address" }],
  },
  {
    type: "function",
    name: "owner",
    stateMutability: "view",
    inputs: [],
    outputs: [{ type: "address" }],
  },
  {
    type: "function",
    name: "pendingOwner",
    stateMutability: "view",
    inputs: [],
    outputs: [{ type: "address" }],
  },
  {
    type: "function",
    name: "paused",
    stateMutability: "view",
    inputs: [],
    outputs: [{ type: "bool" }],
  },
  {
    type: "function",
    name: "totalFeesCollected",
    stateMutability: "view",
    inputs: [],
    outputs: [{ type: "uint256" }],
  },
  {
    type: "function",
    name: "totalProofsPaid",
    stateMutability: "view",
    inputs: [],
    outputs: [{ type: "uint256" }],
  },

  // ── Write ────────────────────────────────────────────────────────────────
  {
    type: "function",
    name: "deposit",
    stateMutability: "payable",
    inputs: [],
    outputs: [],
  },
  {
    type: "function",
    name: "withdraw",
    stateMutability: "nonpayable",
    inputs: [{ name: "amount", type: "uint256" }],
    outputs: [],
  },
  {
    type: "function",
    name: "deductFee",
    stateMutability: "nonpayable",
    inputs: [
      { name: "user", type: "address" },
      { name: "tier", type: "uint8" },
      { name: "jobId", type: "string" },
    ],
    outputs: [],
  },
  {
    type: "function",
    name: "withdrawFees",
    stateMutability: "nonpayable",
    inputs: [{ name: "to", type: "address" }],
    outputs: [],
  },
  {
    type: "function",
    name: "setGateway",
    stateMutability: "nonpayable",
    inputs: [{ name: "newGateway", type: "address" }],
    outputs: [],
  },

  // Ownership
  {
    type: "function",
    name: "transferOwnership",
    stateMutability: "nonpayable",
    inputs: [{ name: "newOwner", type: "address" }],
    outputs: [],
  },
  {
    type: "function",
    name: "acceptOwnership",
    stateMutability: "nonpayable",
    inputs: [],
    outputs: [],
  },
  {
    type: "function",
    name: "renounceOwnership",
    stateMutability: "nonpayable",
    inputs: [],
    outputs: [],
  },

  // Pause
  {
    type: "function",
    name: "pause",
    stateMutability: "nonpayable",
    inputs: [],
    outputs: [],
  },
  {
    type: "function",
    name: "unpause",
    stateMutability: "nonpayable",
    inputs: [],
    outputs: [],
  },

  // ── Events ───────────────────────────────────────────────────────────────
  {
    type: "event",
    name: "Deposited",
    inputs: [
      { name: "user", type: "address", indexed: true },
      { name: "amount", type: "uint256" },
      { name: "newBalance", type: "uint256" },
    ],
  },
  {
    type: "event",
    name: "Withdrawn",
    inputs: [
      { name: "user", type: "address", indexed: true },
      { name: "amount", type: "uint256" },
      { name: "remaining", type: "uint256" },
    ],
  },
  {
    type: "event",
    name: "FeeDeducted",
    inputs: [
      { name: "user", type: "address", indexed: true },
      { name: "fee", type: "uint256" },
      { name: "tier", type: "uint8" },
      { name: "remaining", type: "uint256" },
      { name: "jobId", type: "string" },
    ],
  },
  {
    type: "event",
    name: "FeesWithdrawn",
    inputs: [
      { name: "to", type: "address", indexed: true },
      { name: "amount", type: "uint256" },
    ],
  },
  {
    type: "event",
    name: "GatewayUpdated",
    inputs: [
      { name: "oldGateway", type: "address", indexed: true },
      { name: "newGateway", type: "address", indexed: true },
    ],
  },
  {
    type: "event",
    name: "OwnershipTransferStarted",
    inputs: [
      { name: "previousOwner", type: "address", indexed: true },
      { name: "newOwner", type: "address", indexed: true },
    ],
  },
  {
    type: "event",
    name: "OwnershipTransferred",
    inputs: [
      { name: "previousOwner", type: "address", indexed: true },
      { name: "newOwner", type: "address", indexed: true },
    ],
  },
  {
    type: "event",
    name: "Paused",
    inputs: [{ name: "account", type: "address" }],
  },
  {
    type: "event",
    name: "Unpaused",
    inputs: [{ name: "account", type: "address" }],
  },

  // ── Errors ───────────────────────────────────────────────────────────────
  {
    type: "error",
    name: "InsufficientBalance",
    inputs: [
      { name: "user", type: "address" },
      { name: "required", type: "uint256" },
      { name: "available", type: "uint256" },
    ],
  },
  {
    type: "error",
    name: "NotGateway",
    inputs: [{ name: "caller", type: "address" }],
  },
  { type: "error", name: "ZeroDeposit", inputs: [] },
  { type: "error", name: "ZeroWithdraw", inputs: [] },
  { type: "error", name: "ZeroAddress", inputs: [] },
  { type: "error", name: "WithdrawFailed", inputs: [] },
  { type: "error", name: "NoFeesToWithdraw", inputs: [] },
  { type: "error", name: "FeeWithdrawFailed", inputs: [] },
  { type: "error", name: "EnforcedPause", inputs: [] },
  { type: "error", name: "ExpectedPause", inputs: [] },
  {
    type: "error",
    name: "OwnableUnauthorizedAccount",
    inputs: [{ name: "account", type: "address" }],
  },
  {
    type: "error",
    name: "OwnableInvalidOwner",
    inputs: [{ name: "owner", type: "address" }],
  },
  { type: "error", name: "ReentrancyGuardReentrantCall", inputs: [] },
] as const;
