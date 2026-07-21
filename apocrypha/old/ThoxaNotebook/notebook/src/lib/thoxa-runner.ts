import type { PyodideInterface } from 'pyodide'

declare global {
  interface Window {
    loadPyodide: (config: {
      indexURL?: string
      stdin?: () => string
      stdout?: (text: string) => void
      stderr?: (text: string) => void
    }) => Promise<PyodideInterface>
  }
}

export interface ExecutionResult {
  output: string
  error?: string
  ast?: object
}

export class ThoxaRunner {
  private pyodide: PyodideInterface | null = null
  private isInitializing = false
  private initPromise: Promise<void> | null = null

  async initialize(): Promise<void> {
    if (this.pyodide) return
    if (this.isInitializing && this.initPromise) {
      return this.initPromise
    }

    this.isInitializing = true
    this.initPromise = this._doInitialize()

    try {
      await this.initPromise
    } finally {
      this.isInitializing = false
    }
  }

  private async _doInitialize(): Promise<void> {
    // Load Pyodide script
    if (!window.loadPyodide) {
      const script = document.createElement('script')
      script.src = 'https://cdn.jsdelivr.net/pyodide/v0.26.4/full/pyodide.js'
      document.head.appendChild(script)

      await new Promise((resolve) => {
        script.onload = resolve
      })
    }

    // Initialize Pyodide
    this.pyodide = await window.loadPyodide({
      indexURL: 'https://cdn.jsdelivr.net/pyodide/v0.26.4/full/',
    })

    // Install micropip
    await this.pyodide.loadPackage(['micropip'])

    // Install lark (dependency)
    await this.pyodide.runPythonAsync(`
      import micropip
      await micropip.install('lark')
    `)

    // Install the Thoxa wheel with cache bust
    const cacheBust = Date.now()
    await this.pyodide.runPythonAsync(`
      import micropip
      await micropip.install('/thoxa-wheel/thoxa-0.1.0-py3-none-any.whl?v=${cacheBust}')
    `)
  }

  async execute(
    code: string,
    showAST: boolean = false,
  ): Promise<ExecutionResult> {
    if (!this.pyodide) {
      await this.initialize()
    }

    if (!this.pyodide) {
      return { output: '', error: 'Failed to initialize Pyodide' }
    }

    // Generate a unique ID for this execution to avoid race conditions
    const executionId = `exec_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`

    try {
      await this.pyodide.runPythonAsync(`
        from thoxa.parser import parse
        from thoxa.interpreter import Interpreter
        import json
        import js
        import io
        import sys
        
        # Create a fresh output capture for this execution
        class LocalOutputCapture:
            def __init__(self):
                self.buffer = io.StringIO()
                self.old_stdout = None
            
            def __enter__(self):
                self.old_stdout = sys.stdout
                sys.stdout = self.buffer
                return self
            
            def __exit__(self, exc_type, exc_val, exc_tb):
                sys.stdout = self.old_stdout
            
            def get_output(self):
                return self.buffer.getvalue()
        
        output_capture = LocalOutputCapture()
        result_dict = {}
        
        try:
            # Parse the code
            ast = parse("""${code.replace(/"/g, '\\"').replace(/\n/g, '\\n')}""")
            
            # Optionally get AST representation
            ast_json = None
            if ${showAST ? 'True' : 'False'}:
                # Simple AST representation
                def ast_to_dict(node):
                    if hasattr(node, '__dict__'):
                        result = {'type': node.__class__.__name__}
                        for key, value in node.__dict__.items():
                            if not key.startswith('_'):
                                if isinstance(value, list):
                                    result[key] = [ast_to_dict(item) for item in value]
                                elif hasattr(value, '__dict__'):
                                    result[key] = ast_to_dict(value)
                                else:
                                    result[key] = str(value)
                        return result
                    return str(node)
                
                ast_json = json.dumps(ast_to_dict(ast))
            
            # Interpret the code with fresh interpreter
            with output_capture:
                interpreter = Interpreter()
                interpreter.interpret(ast)
            
            captured_output = output_capture.get_output()
            
            result_dict = {'output': captured_output, 'ast': ast_json, 'error': None}
            
        except Exception as e:
            result_dict = {'output': output_capture.get_output() if 'output_capture' in locals() else '', 
             'ast': None, 
             'error': str(e)}
        
        # Store results with unique execution ID to avoid race conditions
        setattr(js, '${executionId}_output', result_dict.get('output', ''))
        setattr(js, '${executionId}_error', result_dict.get('error', None))
        setattr(js, '${executionId}_ast', result_dict.get('ast', None))
      `)

      // Get the results using the unique execution ID
      const output =
        ((globalThis as Record<string, unknown>)[
          `${executionId}_output`
        ] as string) || ''
      const error =
        ((globalThis as Record<string, unknown>)[
          `${executionId}_error`
        ] as string) || undefined
      const ast = (globalThis as Record<string, unknown>)[
        `${executionId}_ast`
      ] as object

      // Clean up
      delete (globalThis as Record<string, unknown>)[`${executionId}_output`]
      delete (globalThis as Record<string, unknown>)[`${executionId}_error`]
      delete (globalThis as Record<string, unknown>)[`${executionId}_ast`]

      return {
        output,
        error,
        ast: ast ? JSON.parse(String(ast)) : undefined,
      }
    } catch (error) {
      return {
        output: '',
        error: error instanceof Error ? error.message : String(error),
      }
    }
  }

  isReady(): boolean {
    return this.pyodide !== null
  }
}

// Singleton instance
export const thoxaRunner = new ThoxaRunner()
