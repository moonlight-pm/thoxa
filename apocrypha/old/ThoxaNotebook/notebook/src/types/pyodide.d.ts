declare module 'pyodide' {
  export interface PyodideInterface {
    runPython: (code: string) => unknown
    runPythonAsync: (code: string) => Promise<unknown>
    loadPackage: (packages: string | string[]) => Promise<void>
    globals: Record<string, unknown>
  }

  export function loadPyodide(options?: {
    indexURL?: string
    stdin?: () => string
    stdout?: (text: string) => void
    stderr?: (text: string) => void
  }): Promise<PyodideInterface>
}
