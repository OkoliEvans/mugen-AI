
import React from 'react'
import ReactDOM from 'react-dom/client'
import { BrowserRouter, Routes, Route } from 'react-router-dom'
import { WagmiProvider } from 'wagmi'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { wagmiConfig }   from './lib/wagmiConfig'
import App               from './App'
import ProofExplorer     from './pages/ProofExplorer'
import VaultDashboard    from './pages/VaultDashboard'
import './index.css'

const queryClient = new QueryClient()

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <WagmiProvider config={wagmiConfig}>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <Routes>
            <Route path="/"                   element={<App />} />
            <Route path="/explorer"           element={<ProofExplorer />} />
            <Route path="/explorer/:hashOrId" element={<ProofExplorer />} />
            <Route path="/account"            element={<VaultDashboard />} />
          </Routes>
        </BrowserRouter>
      </QueryClientProvider>
    </WagmiProvider>
  </React.StrictMode>
)