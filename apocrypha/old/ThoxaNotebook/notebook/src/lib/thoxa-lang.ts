import { StreamLanguage } from '@codemirror/language'
import { tags as t } from '@lezer/highlight'

// Define Thoxa language for syntax highlighting
export const thoxaLanguage = StreamLanguage.define({
  name: 'thoxa',

  startState: () => ({
    inComment: false,
    inString: false,
    stringDelimiter: null,
  }),

  token(stream, state) {
    // Handle multi-line strings (if in the middle of one)
    if (state.inString) {
      if (state.stringDelimiter && stream.match(state.stringDelimiter)) {
        state.inString = false
        state.stringDelimiter = null
        return 'string'
      }
      if (stream.match(/\\./)) {
        return 'string'
      }
      stream.next()
      return 'string'
    }

    // Skip whitespace
    if (stream.eatSpace()) {
      return null
    }

    // Comments
    if (stream.match(/^#.*/)) {
      return 'comment'
    }

    // String literals
    if (stream.match('"')) {
      let escaped = false
      let next
      while ((next = stream.next()) != null) {
        if (next === '"' && !escaped) {
          break
        }
        escaped = !escaped && next === '\\'
      }
      return 'string'
    }

    // Numbers
    if (stream.match(/^\d+(\.\d+)?([eE][+-]?\d+)?/)) {
      return 'number'
    }

    // Keywords
    if (
      stream.match(
        /^(fn|let|mut|type|if|else|while|for|in|loop|match|return|break|continue|true|false|not|and|or|export)\b/,
      )
    ) {
      return 'keyword'
    }

    // Built-in functions
    if (stream.match(/^(println|print|len)\b/)) {
      return 'function'
    }

    // Type names (capitalized identifiers)
    if (stream.match(/^[A-Z][a-zA-Z0-9_]*/)) {
      return 'typeName'
    }

    // Identifiers
    if (stream.match(/^[a-zA-Z_][a-zA-Z0-9_]*/)) {
      return 'variableName'
    }

    // Operators
    if (stream.match(/^(==|!=|<=|>=|->|\.\.)/)) {
      return 'operator'
    }

    if (stream.match(/^[+\-*/%<>=!&|]/)) {
      return 'operator'
    }

    // Punctuation
    if (stream.match(/^[()[\]{},.:;]/)) {
      return 'punctuation'
    }

    // If nothing matches, consume one character
    stream.next()
    return null
  },

  languageData: {
    commentTokens: { line: '#' },
    closeBrackets: { brackets: ['(', '[', '{', '"'] },
  },

  tokenTable: {
    keyword: t.keyword,
    comment: t.comment,
    string: t.string,
    number: t.number,
    function: t.function(t.variableName),
    typeName: t.typeName,
    variableName: t.variableName,
    operator: t.operator,
    punctuation: t.punctuation,
  },
})
