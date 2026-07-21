import type { ReactNode } from 'react'
import '../styles.css'

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <div className="h-screen flex flex-col">
      <nav className="bg-gray-950 text-gray-100 px-8 py-4 flex items-center justify-between border-b border-gray-800 flex-shrink-0">
        <div className="flex items-center gap-8">
          <a href="/" className="text-gray-100 no-underline font-bold text-xl">
            Thoxa
          </a>
          <div className="flex gap-6">
            <a
              href="/learn"
              className="text-gray-400 no-underline hover:text-gray-100 transition-colors"
            >
              Learn
            </a>
            <a
              href="/reference"
              className="text-gray-400 no-underline hover:text-gray-100 transition-colors"
            >
              Reference
            </a>
            <a
              href="/playground"
              className="text-gray-400 no-underline hover:text-gray-100 transition-colors"
            >
              Playground
            </a>
          </div>
        </div>
      </nav>
      <main className="flex-1 overflow-hidden">{children}</main>
    </div>
  )
}

export const getConfig = async () => {
  return {
    render: 'static',
  } as const
}
