//! Zed extension shim for native Thoxa.
//!
//! Compiles to WASM and only returns the launch command for `thoxa lsp`.
//! The language server lives in the native compiler binary (not Python).

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
        // Discovery order (native binary — never Python / .venv):
        //   1. `thoxa` on PATH (`worktree.which`)
        //   2. {worktree}/target/rust/debug/thoxa   (cargo default for this repo)
        //   3. {worktree}/target/rust/release/thoxa
        //
        // Do NOT probe paths via `worktree.read_text_file` for gitignored
        // directories (oracle lesson: `.venv` is ignored, so probes fail).
        // Build the dev paths directly; Zed surfaces a spawn error if missing.
        let command = worktree.which("thoxa").unwrap_or_else(|| {
            let root = worktree.root_path();
            let debug = format!("{root}/target/rust/debug/thoxa");
            let release = format!("{root}/target/rust/release/thoxa");
            // Prefer debug for day-to-day; release if debug is absent on disk
            // is not knowable from WASM without read — always try debug first
            // and let spawn fail clearly. Document both paths in extras/README.
            let _ = release;
            debug
        });

        Ok(Command {
            command,
            args: vec!["lsp".to_string()],
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(ThoxaExtension);
