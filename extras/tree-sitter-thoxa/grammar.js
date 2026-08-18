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
 *
 * JSX island: tag trees are parsed just enough to paint them with markup
 * captures (`@tag`, `@attribute`, `@markup`). Not a full CST. `Array<Int64>`
 * stays punctuation — a tag must self-close or have a matching close.
 *
 * SQL island: glued `sql"…"` (no space, same as the compiler) is a tagged
 * string. Inner `{…}` holes stay Thoxa; the rest is injected as SQL in Zed.
 */
module.exports = grammar({
  name: 'thoxa',

  // `word` enables keyword extraction: lex a word, then check the keyword set.
  word: $ => $.identifier,

  // Whitespace and comments may appear between any tokens.
  extras: $ => [/\s+/, $.comment],

  // `<` is both comparison/generic punctuation and a tag start. GLR keeps
  // both; `prec.dynamic` on a completed element prefers the tag.
  conflicts: $ => [
    [$.jsx_opening, $.jsx_self_closing, $.operator],
  ],

  rules: {
    source_file: $ => repeat($._token),

    _token: $ => choice(
      $.jsx_element,
      $.sql_string,
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

    // ── JSX island (highlighting only) ───────────────────────────────────

    jsx_element: $ => prec.dynamic(10, choice(
      $.jsx_self_closing,
      seq($.jsx_opening, repeat($._jsx_child), $.jsx_closing),
    )),

    jsx_self_closing: $ => seq(
      '<',
      $._jsx_name,
      repeat($.jsx_attribute),
      '/',
      '>',
    ),

    jsx_opening: $ => seq(
      '<',
      $._jsx_name,
      repeat($.jsx_attribute),
      '>',
    ),

    jsx_closing: $ => seq(
      '<',
      '/',
      $._jsx_name,
      '>',
    ),

    _jsx_name: $ => choice($.identifier, $.type_identifier),

    _jsx_child: $ => choice(
      $.jsx_element,
      $.jsx_embed,
      $.jsx_text,
    ),

    // Prose between tags / embeds. Token so spaces stay inside the node.
    jsx_text: _ => token(/[^<{]+/),

    jsx_attribute: $ => seq(
      $._jsx_attr_name,
      '=',
      choice($.string, $.jsx_embed),
    ),

    // `type` / `for` are keywords; `data-foo` is ident + '-' + ident.
    _jsx_attr_name: $ => seq(
      choice($.identifier, $.keyword),
      repeat(seq('-', $.identifier)),
    ),

    // `{…}` holes back into Thoxa. Nested `{` is another embed (if-blocks).
    jsx_embed: $ => seq(
      '{',
      repeat($._embed_token),
      '}',
    ),

    _embed_token: $ => choice(
      $.jsx_element,
      $.jsx_embed,
      $.keyword,
      $.boolean,
      $.type_identifier,
      $.identifier,
      $.number,
      $.string,
      $.char,
      $.operator,
      alias('(', $.bracket),
      alias(')', $.bracket),
      alias('[', $.bracket),
      alias(']', $.bracket),
      $.delimiter,
    ),

    keyword: _ => choice(
      'let', 'mut', 'fn', 'type', 'export', 'import', 'from', 'extern',
      'if', 'else', 'loop', 'while', 'for', 'in', 'break', 'continue',
      'return', 'match', 'and', 'or', 'not', 'as', 'can', 'is',
      // Experiment: type-body route verbs (also parsed as idents by the compiler).
      'GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS',
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

    // `sql"…"` — tag glued to the quote (a space is an ordinary ident + string).
    sql_string: $ => prec.dynamic(15, seq(
      alias(token('sql"'), $.sql_open),
      repeat(choice($.sql_text, $.sql_embed)),
      alias(token.immediate('"'), $.sql_close),
    )),

    sql_text: _ => token.immediate(/([^"\\{]|\\.)+/),

    sql_embed: $ => seq(
      alias(token.immediate('{'), '{'),
      repeat($._embed_token),
      '}',
    ),

    char: _ => token(seq("'", choice(/[^'\\]/, /\\./), "'")),

    operator: _ => choice(
      // Longer forms first so the lexer prefers them over prefixes.
      '-->',
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
