import { StrictMode } from 'react'
import { createRoot, hydrateRoot } from 'react-dom/client'
import { Router } from 'waku/router/client'

const rootElement = (
  <StrictMode>
    <Router />
  </StrictMode>
)

declare global {
  var __WAKU_HYDRATE__: boolean | undefined
}

const rootEl = document.getElementById('root')
if (rootEl) {
  if (globalThis.__WAKU_HYDRATE__) {
    hydrateRoot(rootEl, rootElement)
  } else {
    createRoot(rootEl).render(rootElement)
  }
}
