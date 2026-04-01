import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  resolve: {
    preserveSymlinks: true,
    conditions: ['import', 'module', 'browser', 'default'],
  },
  optimizeDeps: {
    include: [
      '@mugen-ai/sdk',
      'axios',
      'react',
      'react-dom',
      'scheduler',
    ],
  },
  build: {
    commonjsOptions: {
      include: [/@mugen-ai\/sdk/, /axios/, /node_modules/],
    },
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