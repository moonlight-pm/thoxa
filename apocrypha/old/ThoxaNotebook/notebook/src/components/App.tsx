import type { ReactNode } from 'react'

export default function App({ children }: { children?: ReactNode }) {
  return (
    <html lang="en">
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>Thoxa Interactive Notebook</title>
        <meta
          name="description"
          content="Learn the Thoxa programming language through interactive examples"
        />
      </head>
      <body>
        <div id="app">{children}</div>
      </body>
    </html>
  )
}
