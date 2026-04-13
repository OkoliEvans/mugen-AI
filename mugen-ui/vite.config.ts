// mugen-ui/vite.config.ts
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],

  resolve: {
    preserveSymlinks: true,
  },

  optimizeDeps: {
    include: [
      'wagmi',
      'viem',
      '@wagmi/core',
      '@tanstack/react-query',
      '@noble/hashes/sha256',
      '@noble/hashes/sha3',
      '@noble/hashes/ripemd160',
      '@noble/hashes/hmac',
      '@noble/curves/secp256k1',
      '@noble/curves/abstract/utils',
      'abitype',
      'isows',
      'use-sync-external-store/shim/with-selector.js',
    ],
  },

  server: {
    port: 3000,
    proxy: {
      '/api': {
        target:       'https://afraid-ava-mist-labs-near-intents-730e15de.koyeb.app',
        rewrite:      path => path.replace(/^\/api/, ''),
        changeOrigin: true,
        secure:       true,
      },
    },
  },
})