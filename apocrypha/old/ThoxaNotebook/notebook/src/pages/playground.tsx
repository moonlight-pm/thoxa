'use client'

import { lazy, Suspense } from 'react'

// Lazily import ThoxaEditor to avoid SSR issues with CodeMirror
const ThoxaEditor = lazy(() => import('../components/ThoxaEditor'))

const defaultCode = `# Welcome to Thoxa Playground!
# Try running this example:

fn main() {
    println("Hello, Thoxa!");
    
    let x = 5;
    mut y = 10;
    y = y + x;
    
    println("x =", x);
    println("y =", y);
}

# No need to call main() - it's called automatically`

export default function PlaygroundPage() {
  return (
    <div className="container mx-auto p-8">
      <h1 className="text-3xl font-bold mb-6">Thoxa Playground</h1>
      <p className="text-gray-400 mb-8">
        Write and run Thoxa code directly in your browser. The interpreter runs
        entirely client-side using WebAssembly.
      </p>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <div>
          <Suspense
            fallback={
              <div className="flex items-center justify-center h-64">
                Loading editor...
              </div>
            }
          >
            <ThoxaEditor
              initialCode={defaultCode}
              showAST={false}
              editable={true}
            />
          </Suspense>
        </div>

        <div>
          <div className="border border-gray-700 rounded-lg p-4 bg-gray-800">
            <h3 className="font-semibold mb-3 text-gray-200">
              Quick Reference
            </h3>
            <div className="space-y-2 text-sm">
              <div>
                <strong>Variables:</strong> <code>let x = 5;</code> (immutable)
              </div>
              <div>
                <strong>Mutable:</strong> <code>mut y = 10;</code>
              </div>
              <div>
                <strong>Functions:</strong> <code>fn name() {'{ }'}</code>
              </div>
              <div>
                <strong>Types:</strong> <code>type Name {'{ }'}</code>
              </div>
              <div>
                <strong>Arrays:</strong> <code>[1, 2, 3]</code>
              </div>
              <div>
                <strong>Control Flow:</strong> <code>if</code>,{' '}
                <code>while</code>, <code>for x in array</code>
              </div>
            </div>
          </div>

          <div className="mt-4 border border-gray-700 rounded-lg p-4 bg-gray-800">
            <h3 className="font-semibold mb-2 text-gray-200">
              Try These Examples:
            </h3>
            <ul className="list-disc list-inside space-y-1 text-sm text-gray-400">
              <li>Create a counter type with methods</li>
              <li>Write a function that calculates factorial</li>
              <li>Use arrays and loops</li>
              <li>Create custom types with constructors</li>
            </ul>
          </div>
        </div>
      </div>
    </div>
  )
}
