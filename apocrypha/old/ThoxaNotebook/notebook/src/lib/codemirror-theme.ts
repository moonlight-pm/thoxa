import { tags as t } from '@lezer/highlight'
import { createTheme } from '@uiw/codemirror-themes'

export const thoxaDarkTheme = createTheme({
  theme: 'dark',
  settings: {
    // Main editor background and text
    background: '#030712', // gray-950
    foreground: '#e5e7eb', // gray-200

    // Caret and selection
    caret: '#60a5fa', // blue-400
    selection: '#1f2937', // gray-800
    selectionMatch: '#374151', // gray-700

    // Editor UI elements
    lineHighlight: '#111827', // gray-900
    gutterBackground: '#030712', // gray-950
    gutterForeground: '#6b7280', // gray-500
    gutterBorder: '#1f2937', // gray-800
  },
  styles: [
    // Comments
    { tag: t.comment, color: '#6b7280' }, // gray-500

    // Strings
    { tag: t.string, color: '#86efac' }, // green-300
    { tag: t.special(t.string), color: '#86efac' },

    // Keywords
    { tag: t.keyword, color: '#c084fc' }, // purple-400
    { tag: t.controlKeyword, color: '#c084fc' },
    { tag: t.operatorKeyword, color: '#c084fc' },
    { tag: t.definitionKeyword, color: '#c084fc' },
    { tag: t.moduleKeyword, color: '#c084fc' },

    // Functions and methods
    { tag: t.function(t.variableName), color: '#93c5fd' }, // blue-300
    { tag: t.function(t.definition(t.variableName)), color: '#93c5fd' },
    { tag: t.definition(t.function(t.variableName)), color: '#93c5fd' },

    // Variables
    { tag: t.variableName, color: '#fbbf24' }, // amber-400
    { tag: t.definition(t.variableName), color: '#fbbf24' },
    { tag: t.local(t.variableName), color: '#fbbf24' },

    // Types
    { tag: t.typeName, color: '#67e8f9' }, // cyan-300
    { tag: t.definition(t.typeName), color: '#67e8f9' },
    { tag: t.standard(t.typeName), color: '#67e8f9' },

    // Properties
    { tag: t.propertyName, color: '#fdba74' }, // orange-300
    { tag: t.definition(t.propertyName), color: '#fdba74' },

    // Numbers
    { tag: t.number, color: '#fca5a5' }, // red-300
    { tag: t.integer, color: '#fca5a5' },
    { tag: t.float, color: '#fca5a5' },

    // Booleans and constants
    { tag: t.bool, color: '#a78bfa' }, // violet-400
    { tag: t.constant(t.name), color: '#a78bfa' },
    { tag: t.null, color: '#a78bfa' },

    // Operators
    { tag: t.operator, color: '#94a3b8' }, // slate-400
    { tag: t.arithmeticOperator, color: '#94a3b8' },
    { tag: t.logicOperator, color: '#94a3b8' },
    { tag: t.compareOperator, color: '#94a3b8' },

    // Brackets and punctuation
    { tag: t.bracket, color: '#9ca3af' }, // gray-400
    { tag: t.punctuation, color: '#9ca3af' },
    { tag: t.separator, color: '#9ca3af' },
    { tag: t.angleBracket, color: '#9ca3af' },
    { tag: t.squareBracket, color: '#9ca3af' },
    { tag: t.paren, color: '#9ca3af' },
    { tag: t.brace, color: '#9ca3af' },

    // Special
    { tag: t.meta, color: '#6b7280' },
    { tag: t.link, color: '#60a5fa', textDecoration: 'underline' },
    { tag: t.heading, color: '#e5e7eb', fontWeight: 'bold' },
    { tag: t.emphasis, color: '#e5e7eb', fontStyle: 'italic' },
    { tag: t.strong, color: '#e5e7eb', fontWeight: 'bold' },

    // Invalid
    { tag: t.invalid, color: '#ef4444' }, // red-500
  ],
})
