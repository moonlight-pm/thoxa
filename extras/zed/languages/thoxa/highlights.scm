; Thoxa syntax highlighting (lexer-level).
;
; Kept in sync with extras/tree-sitter-thoxa/queries/highlights.scm.
; LSP semantic tokens refine function vs variable in combined mode.

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
