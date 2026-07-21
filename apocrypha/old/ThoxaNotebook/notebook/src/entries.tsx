import { lazy } from 'react'
// @ts-expect-error
import { defineEntries } from 'waku/server'

const App = lazy(() => import('./components/App'))

export default defineEntries(
  // renderEntries
  async (_input: unknown) => {
    return {
      App: <App />,
    }
  },
)
