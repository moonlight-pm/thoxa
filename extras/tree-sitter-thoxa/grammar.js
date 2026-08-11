/**
 * Lexer-level tree-sitter grammar for native Thoxa.
 *
 * Deliberately thin: it tokenizes rather than fully parses. The *lexical* layer
 * (keywords, casing, comments, strings, numbers, operators) is far more stable
 * than full syntax — so tracking only that keeps the grammar cheap. Structural
 * refinements (function vs variable) come from `thoxa lsp` semantic tokens in
 * Zed's `combined` highlighting mode.
 *
 * Casing is meaningful (PascalCase => type, camelCase => value).
 */
module.exports = grammar({
  name: 'thoxa',

  // `word` enables keyword extraction: lex a word, then check the keyword set.
  word: $ => $.identifier,

  // Whitespace and comments may appear between any tokens.
  extras: $ => [/\s+/, $.comment],

  rules: {
    source_file: $ => repeat($._token),

    _token: $ => choice(
      $.keyword,
      $.boolean,
      $.type_identifier,
      $.identifier,
      $.number,
      $.string,
      $.char,
      $.operator,
      $.bracket,
      $.delimiter,
    ),

    keyword: _ => choice(
      'let', 'mut', 'fn', 'type', 'export', 'import', 'from', 'extern',
      'if', 'else', 'loop', 'while', 'for', 'in', 'break', 'continue',
      'return', 'match', 'and', 'or', 'not', 'as', 'can', 'is',
    ),

    boolean: _ => choice('true', 'false'),

    // camelCase value/function names (also the keyword-extraction word token).
    identifier: _ => /[a-z][A-Za-z0-9]*/,
    // PascalCase type names.
    type_identifier: _ => /[A-Z][A-Za-z0-9]*/,

    number: _ => token(choice(
      /0[xX][0-9a-fA-F_]+([iu](8|16|32|64))?/,                            // hex
      /0[bB][01_]+([iu](8|16|32|64))?/,                                   // binary
      /[0-9][0-9_]*(\.[0-9_]+)?([eE][+-]?[0-9_]+)?([iuf](8|16|32|64))?/,  // dec/float
    )),

    // Strings may span multiple lines and contain `{...}` interpolation; a
    // lexer-level grammar colours the whole literal (interpolation refinement is
    // deferred). `\\.` covers escapes; `{{`/`}}` are just literal characters.
    string: _ => token(seq('"', repeat(choice(/[^"\\]/, /\\./)), '"')),

    char: _ => token(seq("'", choice(/[^'\\]/, /\\./), "'")),

    operator: _ => choice(
      // Longer forms first so the lexer prefers them over prefixes.
      '==', '!=', '<=', '>=', '+=', '-=', '*=', '/=', '=>', '->', '..=', '...',
      '..', '<<', '>>',
      '+', '-', '*', '/', '%', '<', '>', '=', '?', '|', '&', '^', '~', '!', '.',
    ),

    bracket: _ => choice('(', ')', '[', ']', '{', '}'),
    delimiter: _ => choice(',', ':'),

    // `#` line comments and `##` doc comments.
    comment: _ => token(/#[^\n]*/),
  },
});
