; Inject Zed's SQL grammar into `sql"…"` bodies.
; `{…}` holes are `sql_embed` nodes, so they stay Thoxa (not SQL).

((sql_text) @injection.content
 (#set! injection.language "sql"))
