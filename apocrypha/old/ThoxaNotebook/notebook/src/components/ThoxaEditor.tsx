'use client'

import { EditorView } from '@codemirror/view'
import CodeMirror from '@uiw/react-codemirror'
import { useCallback, useEffect, useRef, useState } from 'react'
import { thoxaDarkTheme } from '../lib/codemirror-theme'
import { thoxaLanguage } from '../lib/thoxa-lang'
import { type ExecutionResult, thoxaRunner } from '../lib/thoxa-runner'

interface Tab {
  id: string
  code: string
  output: string
  error: string
  isPending: boolean
}

interface ThoxaEditorProps {
  initialCode?: string | string[]
  showAST?: boolean
  editable?: boolean
  onRun?: (result: ExecutionResult) => void
  sectionId?: string
  onSave?: (code: string, tabIndex?: number) => Promise<void>
}

export default function ThoxaEditor({
  initialCode = '',
  showAST = false,
  editable = true,
  onRun,
  sectionId,
  onSave,
}: ThoxaEditorProps) {
  // Initialize tabs with stored data or default
  const initializeTabs = () => {
    if (sectionId && typeof window !== 'undefined') {
      const stored = sessionStorage.getItem(`thoxa-tabs-${sectionId}`)
      if (stored) {
        try {
          const parsed = JSON.parse(stored)
          if (parsed.tabs && Array.isArray(parsed.tabs)) {
            return parsed.tabs
          }
        } catch (e) {
          console.error('Failed to parse stored tabs:', e)
        }
      }
    }

    // Parse initial code - handle both array and single string formats
    let tabsFromInitial: Tab[] = []

    if (initialCode) {
      // Check if initialCode is already an array (from server-side parsing)
      if (Array.isArray(initialCode)) {
        tabsFromInitial = initialCode.map((codeItem, index) => {
          // Handle different types of code items
          let code = ''
          if (typeof codeItem === 'string') {
            code = codeItem
          } else if (
            codeItem &&
            typeof codeItem === 'object' &&
            'code' in codeItem
          ) {
            // Handle snippet objects from database
            code = String((codeItem as { code?: unknown }).code || '')
          }
          return {
            id: String(index + 1),
            code,
            output: '',
            error: '',
            isPending: false,
          }
        })
      } else if (typeof initialCode === 'string') {
        // Single code block
        tabsFromInitial = [
          {
            id: '1',
            code: initialCode,
            output: '',
            error: '',
            isPending: false,
          },
        ]
      }
    }

    return tabsFromInitial.length > 0
      ? tabsFromInitial
      : [
          {
            id: '1',
            code: '',
            output: '',
            error: '',
            isPending: false,
          },
        ]
  }

  const [tabs, setTabs] = useState<Tab[]>(initializeTabs)
  const [activeTabId, setActiveTabId] = useState(() => {
    if (sectionId && typeof window !== 'undefined') {
      const stored = sessionStorage.getItem(`thoxa-active-tab-${sectionId}`)
      if (stored && tabs.find((t) => t.id === stored)) {
        return stored
      }
    }
    return tabs[0]?.id || '1'
  })

  const [ast, setAst] = useState<object | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [isInitialized, setIsInitialized] = useState(false)
  const [isSaving, setIsSaving] = useState(false)
  const [saveMessage, setSaveMessage] = useState('')
  const [copyStatus, setCopyStatus] = useState<'idle' | 'copied'>('idle')
  const debounceTimer = useRef<NodeJS.Timeout | null>(null)

  const activeTab = tabs.find((t) => t.id === activeTabId) ||
    tabs[0] || {
      id: '1',
      code: '',
      output: '',
      error: '',
      isPending: false,
    }

  // Save tabs to sessionStorage whenever they change
  useEffect(() => {
    if (sectionId && typeof window !== 'undefined') {
      sessionStorage.setItem(
        `thoxa-tabs-${sectionId}`,
        JSON.stringify({ tabs }),
      )
    }
  }, [tabs, sectionId])

  // Save active tab to sessionStorage
  useEffect(() => {
    if (sectionId && typeof window !== 'undefined') {
      sessionStorage.setItem(`thoxa-active-tab-${sectionId}`, activeTabId)
    }
  }, [activeTabId, sectionId])

  // Update first tab when initialCode prop changes
  useEffect(() => {
    setTabs((prevTabs) => {
      const newTabs = [...prevTabs]
      if (newTabs[0]) {
        newTabs[0].code = typeof initialCode === 'string' ? initialCode : ''
        newTabs[0].output = ''
        newTabs[0].error = ''
      }
      return newTabs
    })
  }, [initialCode])

  const updateActiveTab = useCallback(
    (updates: Partial<Tab>) => {
      setTabs((prevTabs) =>
        prevTabs.map((tab) =>
          tab.id === activeTabId ? { ...tab, ...updates } : tab,
        ),
      )
    },
    [activeTabId],
  )

  // Initialize Pyodide on mount
  useEffect(() => {
    const init = async () => {
      setIsLoading(true)
      try {
        await thoxaRunner.initialize()
        setIsInitialized(true)
      } catch (err) {
        updateActiveTab({ error: `Failed to initialize: ${err}` })
      } finally {
        setIsLoading(false)
      }
    }

    if (!thoxaRunner.isReady()) {
      init()
    } else {
      setIsInitialized(true)
    }
  }, [updateActiveTab])

  const runCode = useCallback(
    async (codeToRun?: string) => {
      const actualCode = codeToRun ?? activeTab.code

      if (!isInitialized || !actualCode.trim()) {
        return
      }

      setIsLoading(true)
      updateActiveTab({ isPending: false, error: '', output: '' })
      setAst(null)

      try {
        const result = await thoxaRunner.execute(actualCode, showAST)

        if (result.error) {
          updateActiveTab({ error: result.error })
        } else {
          updateActiveTab({ output: result.output })
          if (result.ast) {
            setAst(result.ast)
          }
        }

        onRun?.(result)
      } catch (err) {
        updateActiveTab({ error: `Execution error: ${err}` })
      } finally {
        setIsLoading(false)
      }
    },
    [activeTab.code, showAST, isInitialized, onRun, updateActiveTab],
  )

  // Debounced code execution on change
  const handleCodeChange = useCallback(
    (newCode: string) => {
      updateActiveTab({ code: newCode })
      setSaveMessage('') // Clear save message on code change

      // Clear existing timer
      if (debounceTimer.current) {
        clearTimeout(debounceTimer.current)
      }

      // Show pending status
      updateActiveTab({ isPending: true })

      // Set new timer for auto-run
      debounceTimer.current = setTimeout(() => {
        runCode(newCode)
      }, 800) // 800ms debounce
    },
    [runCode, updateActiveTab],
  )

  // Add new tab
  const addTab = useCallback(() => {
    const newId = String(Math.max(...tabs.map((t) => parseInt(t.id, 10))) + 1)
    const newTab: Tab = {
      id: newId,
      code: '',
      output: '',
      error: '',
      isPending: false,
    }
    setTabs([...tabs, newTab])
    setActiveTabId(newId)
  }, [tabs])

  // Delete tab
  const deleteTab = useCallback(
    (tabId: string) => {
      if (tabs.length <= 1) return // Can't delete last tab

      const newTabs = tabs.filter((t) => t.id !== tabId)
      setTabs(newTabs)

      // If deleting active tab, switch to another
      if (tabId === activeTabId) {
        const deletedIndex = tabs.findIndex((t) => t.id === tabId)
        const newActiveIndex = Math.min(deletedIndex, newTabs.length - 1)
        const nextTab = newTabs[newActiveIndex]
        if (nextTab) {
          setActiveTabId(nextTab.id)
        }
      }
    },
    [tabs, activeTabId],
  )

  // Handle save - saves the active tab with its index
  const handleSave = useCallback(async () => {
    if (!onSave || !editable) return

    setIsSaving(true)
    setSaveMessage('')

    try {
      // Find the index of the active tab
      const tabIndex = tabs.findIndex((t) => t.id === activeTabId)

      // Save the active tab's code with its index
      await onSave(activeTab.code, tabIndex)
      setSaveMessage('Saved!')
      setTimeout(() => setSaveMessage(''), 3000)
    } catch {
      setSaveMessage('Save failed')
      setTimeout(() => setSaveMessage(''), 3000)
    } finally {
      setIsSaving(false)
    }
  }, [activeTab, activeTabId, tabs, onSave, editable])

  // Handle copy
  const handleCopy = useCallback(() => {
    const copyText = [
      `# Section: ${sectionId || 'unknown'}`,
      `# saved-code-samples.yaml key: ${sectionId}`,
      `# Tab: ${activeTabId}`,
      '',
      '## Code:',
      '```thoxa',
      activeTab.code,
      '```',
      '',
      '## Output:',
      activeTab.error
        ? `Error: ${activeTab.error}`
        : activeTab.output || '(no output)',
    ].join('\n')

    navigator.clipboard.writeText(copyText).then(() => {
      setCopyStatus('copied')
      setTimeout(() => setCopyStatus('idle'), 2000)
    })
  }, [activeTab, sectionId, activeTabId])

  // Run code on initial load when Pyodide is ready
  useEffect(() => {
    if (isInitialized && activeTab.code && activeTab.code.trim()) {
      runCode(activeTab.code)
    }
  }, [isInitialized, activeTab.code, runCode])

  // Auto-sizing extensions for CodeMirror
  const extensions = [
    thoxaLanguage,
    EditorView.theme({
      '&': {
        fontSize: '14px',
      },
      '.cm-content': {
        padding: '12px',
        minHeight: '60px',
      },
      '.cm-editor': {
        minHeight: '60px',
      },
      '.cm-scroller': {
        minHeight: '60px',
        paddingBottom: '12px',
      },
    }),
  ]

  return (
    <div className="border border-gray-700 rounded-lg overflow-hidden bg-gray-950">
      {/* Editor Header */}
      <div className="bg-gray-800 px-4 py-2 border-b border-gray-700 flex items-center justify-between">
        <span className="text-sm font-medium text-gray-300">Thoxa Code</span>
        <div className="flex items-center gap-3">
          {/* Copy button */}
          <button
            type="button"
            onClick={handleCopy}
            className="px-3 py-1 text-xs rounded transition-colors bg-gray-700 text-gray-300 hover:bg-gray-600"
            title="Copy section ID, code, and output"
          >
            {copyStatus === 'copied' ? 'Copied!' : 'Copy All'}
          </button>

          {editable && onSave && (
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={handleSave}
                disabled={isSaving || !tabs.some((t) => t.code?.trim())}
                className={`px-3 py-1 text-xs rounded transition-colors ${
                  isSaving || !tabs.some((t) => t.code?.trim())
                    ? 'bg-gray-700 text-gray-500 cursor-not-allowed'
                    : 'bg-blue-600 text-white hover:bg-blue-500'
                }`}
              >
                {isSaving ? 'Saving...' : 'Save All'}
              </button>
              {saveMessage && (
                <span
                  className={`text-xs ${
                    saveMessage.includes('saved')
                      ? 'text-green-400'
                      : 'text-red-400'
                  }`}
                >
                  {saveMessage}
                </span>
              )}
            </div>
          )}
          {isLoading && (
            <span className="text-xs text-gray-400">Running...</span>
          )}
          {!isInitialized && (
            <span className="text-xs text-gray-400">Initializing...</span>
          )}
          {isInitialized && !isLoading && !activeTab.isPending && (
            <span className="text-xs text-gray-500">Auto-run enabled</span>
          )}
        </div>
      </div>

      {/* Tab Bar */}
      <div className="bg-gray-900 px-4 py-1 border-b border-gray-700 flex items-center gap-1 overflow-x-auto">
        {tabs.map((tab, index) => (
          <button
            key={tab.id}
            type="button"
            onClick={() => setActiveTabId(tab.id)}
            className={`flex items-center gap-1 px-3 py-1.5 rounded cursor-pointer transition-colors ${
              tab.id === activeTabId
                ? 'bg-gray-800 text-white'
                : 'bg-gray-850 text-gray-400 hover:bg-gray-800 hover:text-gray-300'
            }`}
          >
            <span className="text-sm font-medium">{index + 1}</span>
            {tabs.length > 1 && index !== 0 && (
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation()
                  deleteTab(tab.id)
                }}
                className="ml-1 text-xs text-gray-500 hover:text-red-400"
                title="Delete tab"
              >
                ×
              </button>
            )}
          </button>
        ))}
        <button
          type="button"
          onClick={addTab}
          className="px-2 py-1 text-sm text-gray-400 hover:text-white transition-colors"
          title="Add new tab"
        >
          +
        </button>
      </div>

      {/* Code Editor */}
      <CodeMirror
        key={activeTabId} // Force remount when tab changes
        value={activeTab.code}
        editable={editable}
        extensions={extensions}
        onChange={(value) => handleCodeChange(value)}
        theme={thoxaDarkTheme}
        basicSetup={{
          lineNumbers: true,
          foldGutter: false,
          dropCursor: false,
          allowMultipleSelections: false,
          indentOnInput: true,
          bracketMatching: true,
          closeBrackets: true,
          autocompletion: true,
          rectangularSelection: false,
          highlightSelectionMatches: false,
          searchKeymap: false,
        }}
      />

      {/* Output Section */}
      {(activeTab.output ||
        activeTab.error ||
        activeTab.isPending ||
        isLoading) && (
        <div className="border-t border-gray-700">
          <div className="bg-gray-800 px-4 py-2 border-b border-gray-700 flex items-center justify-between">
            <span className="text-sm font-medium text-gray-300">Output</span>
            {activeTab.isPending && !isLoading && (
              <span className="text-xs text-yellow-400">Pending...</span>
            )}
            {isLoading && (
              <span className="text-xs text-blue-400">Running...</span>
            )}
          </div>
          <div className="p-4 bg-gray-900">
            {activeTab.error ? (
              <pre className="text-red-400 text-sm font-mono whitespace-pre-wrap">
                {activeTab.error}
              </pre>
            ) : activeTab.output ? (
              <pre className="text-gray-300 text-sm font-mono whitespace-pre-wrap">
                {activeTab.output}
              </pre>
            ) : (
              <pre className="text-gray-500 text-sm font-mono">
                {activeTab.isPending ? 'Changes pending...' : 'Running...'}
              </pre>
            )}
          </div>
        </div>
      )}

      {/* AST Section */}
      {showAST && ast && (
        <div className="border-t border-gray-700">
          <div className="bg-gray-800 px-4 py-2 border-b border-gray-700">
            <span className="text-sm font-medium text-gray-300">AST</span>
          </div>
          <div className="p-4 overflow-auto max-h-64 bg-gray-900">
            <pre className="text-xs font-mono text-gray-400">
              {JSON.stringify(ast, null, 2)}
            </pre>
          </div>
        </div>
      )}
    </div>
  )
}
