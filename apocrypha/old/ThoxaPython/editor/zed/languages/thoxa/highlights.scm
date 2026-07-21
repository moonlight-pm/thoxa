; Thoxa syntax highlighting (lexer-level).
;
; The grammar is deliberately thin, so this colours the lexical layer only:
; keywords, casing-based types-vs-values, literals, comments, operators. The
; Thoxa LSP refines identifiers (function / parameter) via semantic tokens in
; Zed's `combined` highlighting mode — do not duplicate that logic here.
;
; Kept in sync with editor/tree-sitter-thoxa/queries/highlights.scm.

(keyword) @keyword
(boolean) @constant.builtin
(type_identifier) @type
(identifier) @variable
(number) @number
(string) @string
(char) @string
(comment) @comment
(operator) @operator
(bracket) @punctuation.bracket
(delimiter) @punctuation.delimiter
