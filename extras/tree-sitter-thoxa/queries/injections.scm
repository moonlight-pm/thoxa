; Inject SQL into `sql"…"` bodies. `{…}` holes stay Thoxa (`sql_embed`).

((sql_text) @injection.content
 (#set! injection.language "sql"))
