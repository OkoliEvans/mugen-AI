// mugen-ui/src/lib/wagmiConfig.ts
// Run: pnpm --filter mugen-ui add wagmi viem @tanstack/react-query

import { createConfig, http } from "wagmi";
import { injected, walletConnect } from "wagmi/connectors";
import { defineChain } from "viem";

// HashKey testnet — not in viem's built-in chains list
export const hashkeyTestnet = defineChain({
  id: 133,
  name: "HashKey Testnet",
  nativeCurrency: {
    name: "HashKey Token",
    symbol: "HSK",
    decimals: 18,
  },
  rpcUrls: {
    default: { http: ["https://testnet.hsk.xyz"] },
  },
  blockExplorers: {
    default: {
      name: "HashKey Explorer",
      url: "https://testnet-explorer.hsk.xyz",
    },
  },
  testnet: true,
});

const WC_PROJECT_ID = import.meta.env.VITE_WC_PROJECT_ID ?? "";

export const wagmiConfig = createConfig({
  chains: [hashkeyTestnet],
  connectors: [
    injected(),
    ...(WC_PROJECT_ID ? [walletConnect({ projectId: WC_PROJECT_ID })] : []),
  ],
  transports: {
    [hashkeyTestnet.id]: http("https://testnet.hsk.xyz"),
  },
});
