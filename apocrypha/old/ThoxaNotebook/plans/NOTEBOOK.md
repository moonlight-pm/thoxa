# Thoxa Interactive Notebook - Server-Side Execution Plan

## Overview

An interactive programming notebook for the Thoxa language using server-side execution via React Server Components (RSC) and Server Actions, replacing the current client-side WebAssembly/Pyodide approach.

## Technology Stack

- **Waku**: React Server Components framework with server actions support
- **Native Python**: Direct execution of Thoxa interpreter on server
- **CodeMirror 6**: Code editor for the client interface
- **SQLite**: Persistent storage for code snippets and user progress
- **Tailwind CSS**: Styling framework
- **Bun**: JavaScript runtime and package manager

## Migration Architecture

### Current State (Client-Side)
- **Pyodide/WebAssembly**: Loads Python interpreter in browser
- **thoxa-runner.ts**: Manages Pyodide instance and executes code
- **ThoxaEditor.tsx**: Client component that calls runner directly
- **Execution Context**: Maintained in browser memory

### Target State (Server-Side)
- **Server Actions**: Execute Thoxa code on server via Python subprocess
- **Streaming Response**: Real-time output streaming to client
- **Session Management**: Server-side execution contexts per user session
- **Security**: Sandboxed execution with resource limits

## Implementation Plan

### Phase 1: Create Server Actions Infrastructure
1. **Create execution server action** (`src/actions/thoxa.ts`)
   - Accept code string and optional session ID
   - Execute Python subprocess with timeout
   - Capture stdout, stderr, and exit code
   - Return structured result

2. **Session management** (`src/lib/execution-session.ts`)
   - Generate unique session IDs
   - Store execution contexts in memory or Redis
   - Clean up stale sessions after timeout
   - Support isolated vs. continued execution modes

3. **Security sandboxing**
   - Resource limits (CPU time, memory)
   - Restricted file system access
   - No network access from executed code
   - Input sanitization

### Phase 2: Refactor Client Components
1. **Update ThoxaEditor.tsx**
   - Remove Pyodide dependencies
   - Call server action instead of local runner
   - Handle loading states during execution
   - Stream output updates if possible

2. **Remove client-side execution code**
   - Delete `thoxa-runner.ts`
   - Remove Pyodide script loading
   - Clean up WebAssembly references
   - Remove `/public/thoxa-wheel` directory

### Phase 3: Server Action Implementation

```typescript
// src/actions/thoxa.ts
'use server'

import { spawn } from 'child_process'
import { v4 as uuidv4 } from 'uuid'

export interface ExecutionResult {
  output: string
  error?: string
  ast?: object
  executionTime: number
  sessionId: string
}

const sessions = new Map<string, ExecutionContext>()

export async function executeThoxa(
  code: string,
  options?: {
    sessionId?: string
    showAST?: boolean
    timeout?: number
  }
): Promise<ExecutionResult> {
  const sessionId = options?.sessionId || uuidv4()
  const timeout = options?.timeout || 5000

  return new Promise((resolve) => {
    const startTime = Date.now()

    const python = spawn('python', ['-m', 'thoxa'], {
      timeout,
      env: { ...process.env, PYTHONPATH: '/path/to/thoxa' }
    })

    let output = ''
    let error = ''

    python.stdout.on('data', (data) => {
      output += data.toString()
    })

    python.stderr.on('data', (data) => {
      error += data.toString()
    })

    python.on('close', (code) => {
      resolve({
        output,
        error: code !== 0 ? error : undefined,
        executionTime: Date.now() - startTime,
        sessionId
      })
    })

    python.stdin.write(code)
    python.stdin.end()
  })
}
```

### Phase 4: Enhanced Features
1. **Output streaming**
   - Use Server-Sent Events or WebSockets
   - Stream output line by line
   - Show execution progress

2. **Execution queue**
   - Queue management for concurrent requests
   - Rate limiting per user/session
   - Priority execution for premium users

3. **Caching**
   - Cache execution results for identical code
   - Invalidate on interpreter updates
   - Store in Redis or SQLite

## Migration Steps

### Step 1: Server Action Setup
- [ ] Create `src/actions/thoxa.ts` with `executeThoxa` function
- [ ] Implement Python subprocess execution
- [ ] Add timeout and resource limits
- [ ] Test basic code execution

### Step 2: Session Management
- [ ] Create session ID generation
- [ ] Implement context persistence between executions
- [ ] Add session cleanup mechanism
- [ ] Test continued vs isolated execution

### Step 3: Client Refactoring
- [ ] Update ThoxaEditor to use server action
- [ ] Add loading states and error handling
- [ ] Remove Pyodide imports and initialization
- [ ] Update playground and learn pages

### Step 4: Cleanup
- [ ] Delete `thoxa-runner.ts`
- [ ] Remove Pyodide CDN scripts
- [ ] Delete `/public/thoxa-wheel` directory
- [ ] Update package.json dependencies

### Step 5: Testing & Optimization
- [ ] Test all code examples
- [ ] Verify session isolation
- [ ] Performance benchmarking
- [ ] Security audit

## Security Considerations

1. **Input Validation**
   - Sanitize code input
   - Limit code size (e.g., 10KB max)
   - Block system calls and file I/O

2. **Resource Limits**
   - CPU timeout (5 seconds default)
   - Memory limit (50MB)
   - Process count limit

3. **Isolation**
   - Run in separate process
   - No network access
   - Restricted file system

## Performance Targets

- **Execution latency**: < 100ms for simple code
- **Concurrent executions**: Support 100+ simultaneous users
- **Session cleanup**: Auto-cleanup after 30 minutes idle
- **Cache hit rate**: > 80% for example code

## Development Commands

```bash
# Install dependencies
cd notebook
bun install

# Run development server
bun run dev

# Test server actions
bun test src/actions/thoxa.test.ts

# Build for production
bun run build
```