//! Zed extension shim for Thoxa.
//!
//! The only Rust in the stack: it tells Zed how to launch the Thoxa language
//! server. The server itself is Python (`thoxa lsp`, reusing the compiler
//! frontend); this compiles to WASM and merely returns the launch command.

use zed_extension_api::{self as zed, Command, LanguageServerId, Result, Worktree};

struct ThoxaExtension;

impl zed::Extension for ThoxaExtension {
    fn new() -> Self {
        ThoxaExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        // Do NOT probe for `.venv/bin/thoxa` via `worktree.read_text_file`: the
        // worktree honours `.gitignore`, and `.venv/` is ignored, so the probe
        // fails even though the file exists on disk (this is what broke the
        // first cut and left the server unlaunched). Instead build the path
        // directly and let Zed surface a spawn error if it is genuinely absent.
        // A `thoxa` already on PATH (e.g. a global/tool install) wins.
        let command = worktree
            .which("thoxa")
            .unwrap_or_else(|| format!("{}/.venv/bin/thoxa", worktree.root_path()));

        Ok(Command {
            command,
            args: vec!["lsp".to_string()],
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(ThoxaExtension);
