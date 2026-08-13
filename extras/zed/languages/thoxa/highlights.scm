; Thoxa syntax highlighting (lexer-level + JSX island).
;
; The grammar is deliberately thin, so this colours the lexical layer:
; keywords, casing-based types-vs-values, literals, comments, operators.
; Tag trees reuse captures Zed themes already paint as markup — not a
; custom palette — so HTML reads as a different register from Thoxa code.
; The Thoxa LSP refines identifiers (function / parameter) via semantic
; tokens in Zed's `combined` highlighting mode.

(keyword) @keyword
((operator) @keyword (#eq? @keyword "-->"))
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

; ── JSX island: markup captures, not code captures ────────────────────────
;   tag names      → @tag            (HTML hue)
;   components     → @constructor    (type-ish, distinct from <div>)
;   < > /          → @tag.delimiter
;   attributes     → @attribute
;   raw text       → @markup.italic  (prose, not identifiers)
;   { } embeds     → @punctuation.special  (holes back into Thoxa)

(jsx_opening ["<" ">"] @tag.delimiter)
(jsx_closing ["<" "/" ">"] @tag.delimiter)
(jsx_self_closing ["<" "/" ">"] @tag.delimiter)

(jsx_opening (identifier) @tag)
(jsx_closing (identifier) @tag)
(jsx_self_closing (identifier) @tag)

(jsx_opening (type_identifier) @constructor)
(jsx_closing (type_identifier) @constructor)
(jsx_self_closing (type_identifier) @constructor)

(jsx_attribute (identifier) @attribute)
(jsx_attribute (keyword) @attribute)
(jsx_attribute "-" @tag.delimiter)
(jsx_attribute "=" @operator)

(jsx_text) @markup.italic

(jsx_embed ["{" "}"] @punctuation.special)
