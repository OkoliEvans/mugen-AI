import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    port: 3000,
    proxy: {
      '/api': {
        // Updated to your new gateway URL
        target: 'https://afraid-ava-mist-labs-near-intents-730e15de.koyeb.app',
        rewrite: (path) => path.replace(/^\/api/, ''),
        changeOrigin: true,
        secure: true, // Set to true for HTTPS targets
      },
    },
  },
  resolve: {
    // Helps Vite follow the symlink created by PNPM workspace
    preserveSymlinks: true,
  },
  optimizeDeps: {
    // Prevents Vite from trying to pre-bundle the SDK as a CommonJS module
    exclude: ['@mugen-ai/sdk'],
  },
  build: {
    commonjsOptions: {
      // Ensures the SDK is included in the CommonJS transformation process
      include: [/@mugen-ai\/sdk/, /node_modules/],
    },
  },
})