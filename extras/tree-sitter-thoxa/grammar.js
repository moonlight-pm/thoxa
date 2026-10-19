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
 * captures (`@tag`, `@attribute`, `@markup`). Not a full CST. As in the
 * compiler, a tag is `<name` / `<>` with no space after `<`; a `<` glued to the
 * name before it (`Array<Int64>`) opens type arguments, and `a < b` compares.
 *
 * Strings: `{…}` holes are Thoxa tokens, so a string inside a hole
 * (`"{greet("ada")}"`, D176) stays inside the literal.
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

  rules: {
    source_file: $ => repeat($._token),

    _token: $ => choice(
      $.jsx_element,
      $.sql_string,
      $.type_arguments_open,
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
      seq($.jsx_fragment_open, repeat($._jsx_child), $.jsx_fragment_close),
    )),

    jsx_fragment_open: _ => '<>',

    jsx_fragment_close: _ => seq('<', '/', '>'),

    jsx_self_closing: $ => seq(
      '<',
      $._jsx_name,
      repeat(choice($.jsx_attribute, $.jsx_embed)),
      '/',
      '>',
    ),

    jsx_opening: $ => seq(
      '<',
      $._jsx_name,
      repeat(choice($.jsx_attribute, $.jsx_embed)),
      '>',
    ),

    jsx_closing: $ => seq(
      '<',
      '/',
      choice($.identifier, $.type_identifier),
      '>',
    ),

    // Glued to the `<` before it: `< b` is a comparison.
    _jsx_name: $ => choice(
      alias(token.immediate(/[a-z][A-Za-z0-9]*/), $.identifier),
      alias(token.immediate(/[A-Z][A-Za-z0-9]*/), $.type_identifier),
    ),

    // `Array<` / `fn id<`: the `<` is glued to the name. The compiler rejects a
    // glued `a<b` in an expression, so this is never a comparison or a tag.
    type_arguments_open: $ => seq(
      choice($.identifier, $.type_identifier),
      alias(token.immediate(prec(1, '<')), $.operator),
    ),

    _jsx_child: $ => choice(
      $.jsx_element,
      $.jsx_embed,
      $.jsx_text,
    ),

    // Prose between tags / embeds. Token so spaces stay inside the node.
    jsx_text: _ => token(/[^<{]+/),

    // `disabled` alone is a boolean attribute. `{...attrs}` spreads are
    // `jsx_embed`s in the tag.
    jsx_attribute: $ => seq(
      $._jsx_attr_name,
      optional(seq('=', choice($.string, $.jsx_embed))),
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
      $.type_arguments_open,
      $.keyword,
      $.boolean,
      $.type_identifier,
      $.identifier,
      $.number,
      $.string,
      $.escaped_string,
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
      'return', 'match', 'and', 'or', 'not', 'as', 'where', 'extend',
      'trait', 'conform',
      // Experiment: type-body route verbs (also parsed as idents by the compiler).
      'GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS',
    ),

    boolean: _ => choice('true', 'false'),

    // camelCase value/function names (also the keyword-extraction word token).
    // A lone `_` is a name too (the match discard).
    identifier: _ => /[a-z][A-Za-z0-9]*|_/,
    // PascalCase type names.
    type_identifier: _ => /[A-Z][A-Za-z0-9]*/,

    number: _ => token(choice(
      /0[xX][0-9a-fA-F_]+([iu](8|16|32|64))?/,                            // hex
      /0[bB][01_]+([iu](8|16|32|64))?/,                                   // binary
      /[0-9][0-9_]*(\.[0-9_]+)?([eE][+-]?[0-9_]+)?([iuf](8|16|32|64))?/,  // dec/float
    )),

    // Strings may span multiple lines. A `{…}` hole holds Thoxa tokens, strings
    // included (D176); `{{` is literal text.
    string: $ => seq(
      '"',
      repeat(choice($.string_content, $.escape_sequence, $.interpolation)),
      token.immediate('"'),
    ),

    // `prec` so a `#` in the text is not read as a comment.
    string_content: _ => token.immediate(prec(1, /([^"\\{]|\{\{)+/)),

    escape_sequence: _ => token.immediate(prec(1, /\\(x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|.)/)),

    // `"{f(\"host\")}"`: a string in a hole may also escape its quotes.
    escaped_string: _ => token(seq('\\"', /([^"\\]|\\[^"])*/, '\\"')),

    interpolation: $ => seq(
      alias(token.immediate('{'), '{'),
      repeat($._embed_token),
      '}',
    ),

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
      // Longer forms first so the lexer prefers them over prefixes. `<=` / `<<`
      // match the glued `<` of `type_arguments_open` in precedence, so the
      // longer token still wins in `x<=y`.
      '-->',
      '==', '!=', token(prec(1, '<=')), '>=', '+=', '-=', '*=', '/=', '=>', '->',
      '..=', '...', '..', token(prec(1, '<<')), '>>',
      '+', '-', '*', '/', '%', '<', '>', '=', '?', '|', '&', '^', '~', '!', '.',
    ),

    bracket: _ => choice('(', ')', '[', ']', '{', '}'),
    delimiter: _ => choice(',', ':'),

    // `#` line comments and `##` doc comments.
    comment: _ => token(/#[^\n]*/),
  },
});
