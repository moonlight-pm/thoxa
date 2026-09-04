//! Interactive session REPL (`thoxa` with no args) — plan PR4/PR5 + D93/D96/D105.
//!
//! Each executable chunk is compiled via
//! [`build_session_step`](crate::driver::build_session_step) and run **in-process**.
//! Backend (D105): vendored **libtcc** memory output → call `thoxa_session_step`
//! (no compiler spawn, no step `.so`). Declaration lines accumulate so later
//! chunks see prior `let` / `type` / `fn` / `import` / `extern`.
//!
//! **Shell status:** host-held status is threaded into each step and written back
//! (so pure-Thoxa status survives across chunks).
//!
//! **cwd / status / pipes:** pure session step path only — `Shell.cd` /
//! `showStatus` / `pipeline` (and command sugar) run via `thoxa_session_step`.
//! Host no longer special-cases those lines (D103).
//!
//! **rc / prompt / path (D96, D100):** interactive REPL loads
//! `~/.config/thoxa/shell.thx` once at start (decls accumulate; statements run
//! once). If the session defines `fn prompt() -> String`, the host rebuilds a
//! prompt step image when decls change and **calls** it every primary prompt
//! (stdout captured in-process). Host sets `THOXA_STATUS` so Thoxa can read last
//! status via `getenv`.
//!
//! If the session defines `fn path(parent: Array<String>) -> Array<String>`,
//! the host runs it (same step path) after rc load and whenever decls change:
//! `parent` is the current process `PATH` split on `:`; the returned components
//! **replace** process `PATH` (joined with `:`).

use crate::command::{desugar_session_source, for_each_code_byte};
use crate::driver::{self, SessionBuildCache, SessionStepLib};
use crate::error::{Error, Result};
use crate::line_edit::{ReadOutcome, SessionEditor};
use std::fs;
use std::io::{self, BufRead, Write};
use std::os::raw::{c_int, c_void};
use std::path::PathBuf;

extern "C" {
    fn pipe(fds: *mut c_int) -> c_int;
    fn dup(fd: c_int) -> c_int;
    fn dup2(oldfd: c_int, newfd: c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn fflush(stream: *mut c_void) -> c_int;
}

const STDOUT_FILENO: c_int = 1;

/// One-shot session chunk (`thoxa -c`). Same compiler path as a REPL line.
pub fn run_one_shot(src: &str) -> Result<i32> {
    driver::pin_workspace_root();
    driver::capture_toolchain_path();
    let mut decls: Vec<String> = Vec::new();
    let mut cache = SessionBuildCache::default();
    let mut loaded: Option<SessionStepLib> = None;
    match run_chunk(src, &mut decls, 0, &mut cache, &mut loaded) {
        Ok(o) => Ok(o.status),
        Err(e) => Err(e),
    }
}

/// Run the interactive (or piped) session REPL. Returns final status.
pub fn run_repl() -> Result<i32> {
    let interactive = is_tty_stdin();
    let mut decls: Vec<String> = Vec::new();
    let mut status: i32 = 0;
    let mut prompt_state = PromptState::default();
    let mut step_cache = SessionBuildCache::default();
    // Keep last step image until the next load (libtcc state owns symbols).
    let mut loaded: Option<SessionStepLib> = None;

    // Pin paths before any host `cd` so runtime/std resolve stays stable.
    driver::pin_workspace_root();
    driver::capture_toolchain_path();

    if interactive {
        match load_shell_rc(&mut decls, &mut step_cache, &mut loaded) {
            Ok(st) => status = st,
            Err(e) => {
                eprintln!("error: shell rc: {e}");
                status = 1;
            }
        }
        prompt_state.on_decls_changed(&decls);
        if let Err(e) = apply_path_hook(&decls, &mut step_cache) {
            eprintln!("error: path: {e}");
        }
    }

    if interactive {
        match SessionEditor::new() {
            Ok(mut editor) => {
                editor.set_decls(&decls);
                loop {
                    let prompt = primary_prompt(&decls, &mut prompt_state, status, &mut step_cache);
                    let line = match editor.read(&prompt)? {
                        ReadOutcome::Line(l) => l,
                        ReadOutcome::Interrupt => continue,
                        ReadOutcome::Eof => break,
                    };
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if trimmed == "exit" || trimmed == "quit" {
                        break;
                    }

                    let mut chunk = line;
                    let mut aborted = false;
                    while needs_more(&chunk) {
                        match editor.read("> ")? {
                            ReadOutcome::Line(l) => {
                                chunk.push('\n');
                                chunk.push_str(&l);
                            }
                            ReadOutcome::Interrupt => {
                                aborted = true;
                                break;
                            }
                            ReadOutcome::Eof => break,
                        }
                    }
                    if aborted {
                        continue;
                    }
                    apply_chunk(
                        &chunk,
                        &mut decls,
                        &mut status,
                        &mut prompt_state,
                        &mut step_cache,
                        &mut loaded,
                    );
                    editor.set_decls(&decls);
                }
                drop(loaded);
                return Ok(status);
            }
            Err(e) => {
                eprintln!("warning: line editor unavailable ({e}); using plain stdin");
            }
        }
    }

    // Piped / non-TTY, or rustyline failed to start: cooked lines.
    let mut stdout = io::stdout();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        if interactive {
            let prompt = primary_prompt(&decls, &mut prompt_state, status, &mut step_cache);
            write!(stdout, "{prompt}").ok();
            stdout.flush().ok();
        }
        let line = match lines.next() {
            None => break, // EOF
            Some(Ok(l)) => l,
            Some(Err(e)) => return Err(Error::new(format!("stdin: {e}"))),
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "exit" || trimmed == "quit" {
            break;
        }

        let mut chunk = line;
        while needs_more(&chunk) {
            if interactive {
                write!(stdout, "> ").ok();
                stdout.flush().ok();
            }
            match lines.next() {
                None => break,
                Some(Ok(l)) => {
                    chunk.push('\n');
                    chunk.push_str(&l);
                }
                Some(Err(e)) => return Err(Error::new(format!("stdin: {e}"))),
            }
        }

        apply_chunk(
            &chunk,
            &mut decls,
            &mut status,
            &mut prompt_state,
            &mut step_cache,
            &mut loaded,
        );
    }

    drop(loaded);
    if interactive {
        println!();
    }
    Ok(status)
}

fn apply_chunk(
    chunk: &str,
    decls: &mut Vec<String>,
    status: &mut i32,
    prompt_state: &mut PromptState,
    step_cache: &mut SessionBuildCache,
    loaded: &mut Option<SessionStepLib>,
) {
    match run_chunk(chunk, decls, *status, step_cache, loaded) {
        Ok(ChunkOutcome {
            status: st,
            decls_changed,
        }) => {
            *status = st;
            if decls_changed {
                prompt_state.on_decls_changed(decls);
                if let Err(e) = apply_path_hook(decls, step_cache) {
                    eprintln!("error: path: {e}");
                }
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            *status = 1;
        }
    }
}

/// Run `f` with stdout redirected to a pipe; return `(f's result, captured bytes)`.
fn with_captured_stdout<T, F>(f: F) -> Result<(T, String)>
where
    F: FnOnce() -> Result<T>,
{
    // Flush Rust and C stdout before swapping FDs.
    let _ = io::stdout().flush();
    unsafe {
        fflush(std::ptr::null_mut()); // fflush(NULL) flushes all C streams
    }

    let mut fds: [c_int; 2] = [0, 0];
    if unsafe { pipe(fds.as_mut_ptr()) } != 0 {
        return Err(Error::new("pipe failed for stdout capture"));
    }
    let (read_fd, write_fd) = (fds[0], fds[1]);

    let saved = unsafe { dup(STDOUT_FILENO) };
    if saved < 0 {
        unsafe {
            close(read_fd);
            close(write_fd);
        }
        return Err(Error::new("dup(stdout) failed"));
    }
    if unsafe { dup2(write_fd, STDOUT_FILENO) } < 0 {
        unsafe {
            close(read_fd);
            close(write_fd);
            close(saved);
        }
        return Err(Error::new("dup2(pipe→stdout) failed"));
    }
    unsafe {
        close(write_fd);
    }

    let run_result = f();

    // Restore stdout before reading (writer side closed → EOF on read).
    let _ = io::stdout().flush();
    unsafe {
        fflush(std::ptr::null_mut());
        dup2(saved, STDOUT_FILENO);
        close(saved);
    }

    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = unsafe { read(read_fd, tmp.as_mut_ptr() as *mut c_void, tmp.len()) };
        if n <= 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n as usize]);
    }
    unsafe {
        close(read_fd);
    }

    let captured = String::from_utf8_lossy(&buf).into_owned();
    match run_result {
        Ok(v) => Ok((v, captured)),
        Err(e) => Err(e),
    }
}

fn is_tty_stdin() -> bool {
    use std::io::IsTerminal;
    io::stdin().is_terminal()
}

fn needs_more(chunk: &str) -> bool {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut quote = b'\0';
    let mut esc = false;
    for &b in chunk.as_bytes() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == quote {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => {
                in_str = true;
                quote = b;
            }
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
    }
    in_str || depth > 0
}

// ── shell.thx rc ──────────────────────────────────────────────────────────

/// Resolve the session rc path. `None` means "do not load".
///
/// Order: `THOXA_NORC` (skip) → `THOXA_SHELL_RC` (override, empty = skip) →
/// `$HOME/.config/thoxa/shell.thx`.
pub fn shell_rc_path() -> Option<PathBuf> {
    if env_truthy("THOXA_NORC") {
        return None;
    }
    if let Ok(p) = std::env::var("THOXA_SHELL_RC") {
        if p.is_empty() {
            return None;
        }
        return Some(PathBuf::from(p));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/thoxa/shell.thx"))
}

fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim();
            !(v.is_empty()
                || v == "0"
                || v.eq_ignore_ascii_case("false")
                || v.eq_ignore_ascii_case("no"))
        }
        Err(_) => false,
    }
}

/// Load `shell.thx`: accumulate declarations; run top-level statements once.
/// Missing file is a silent no-op. Returns status from the one-shot stmt run (or 0).
fn load_shell_rc(
    decls: &mut Vec<String>,
    cache: &mut SessionBuildCache,
    loaded: &mut Option<SessionStepLib>,
) -> Result<i32> {
    let Some(path) = shell_rc_path() else {
        return Ok(0);
    };
    if !path.is_file() {
        return Ok(0);
    }
    let src =
        fs::read_to_string(&path).map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
    apply_rc_source(&src, decls, cache, loaded)
}

/// Split desugared rc into decl forms vs statement forms; accumulate decls;
/// execute statements once with those decls in scope.
fn apply_rc_source(
    src: &str,
    decls: &mut Vec<String>,
    cache: &mut SessionBuildCache,
    loaded: &mut Option<SessionStepLib>,
) -> Result<i32> {
    let desugared = desugar_session_source(src)?;
    let forms = split_top_level_forms(&desugared);
    let mut stmts: Vec<String> = Vec::new();
    for form in forms {
        if is_effectively_empty_form(&form) {
            continue;
        }
        if is_decl_chunk(form.trim()) {
            decls.push(form);
        } else {
            stmts.push(form);
        }
    }
    if stmts.is_empty() {
        return Ok(0);
    }
    let chunk = stmts.join("\n");
    let out = run_chunk(&chunk, decls, 0, cache, loaded)?;
    Ok(out.status)
}

/// Split source into top-level forms (brace/string-aware, line-oriented).
/// `#` / `##` comments are not strings: an apostrophe in `## not Thoxa's`
/// must not glue the following `import` / `fn` into one form.
fn split_top_level_forms(src: &str) -> Vec<String> {
    let mut forms = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut quote = b'\0';
    let mut esc = false;

    for (i, line) in src.lines().enumerate() {
        if i > 0 || !cur.is_empty() {
            if !cur.is_empty() {
                cur.push('\n');
            }
        }
        cur.push_str(line);

        for_each_code_byte(line, &mut in_str, &mut quote, &mut esc, |b| match b {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        });

        if !in_str && depth <= 0 {
            depth = 0;
            if !cur.trim().is_empty() {
                forms.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if !cur.trim().is_empty() {
        forms.push(cur);
    }
    forms
}

fn is_effectively_empty_form(form: &str) -> bool {
    form.lines().all(|l| {
        let t = l.trim();
        t.is_empty() || t.starts_with('#')
    })
}

// ── prompt() ──────────────────────────────────────────────────────────────

const DEFAULT_PROMPT: &str = "§ ";

#[derive(Default)]
struct PromptState {
    /// Whether decls currently define `fn prompt`.
    has_prompt: bool,
    /// Rebuild prompt step image before next use.
    dirty: bool,
    /// Last successful prompt image (kept live; re-called each line).
    lib: Option<SessionStepLib>,
    /// Avoid spamming the same rebuild error every line.
    last_error: Option<String>,
}

impl PromptState {
    fn on_decls_changed(&mut self, decls: &[String]) {
        self.has_prompt = decls_define_prompt(decls);
        self.dirty = self.has_prompt;
        if !self.has_prompt {
            self.lib = None;
            self.last_error = None;
        } else {
            // Drop old image so we rebuild against new decls.
            self.lib = None;
        }
    }
}

/// True when accumulated decl sources define `fn prompt` (not `promptFoo`).
pub fn decls_define_prompt(decls: &[String]) -> bool {
    for d in decls {
        for line in d.lines() {
            if line_defines_fn_prompt(line) {
                return true;
            }
        }
    }
    false
}

fn line_defines_fn_prompt(line: &str) -> bool {
    let t = line.trim();
    let t = t.strip_prefix("export ").unwrap_or(t).trim_start();
    let Some(rest) = t.strip_prefix("fn ") else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(after) = rest.strip_prefix("prompt") else {
        return false;
    };
    // `prompt()` / `prompt ()` / `prompt\t(` — not `promptHelper`.
    after.starts_with('(') || after.chars().next().is_some_and(|c| c.is_whitespace())
}

fn primary_prompt(
    decls: &[String],
    state: &mut PromptState,
    status: i32,
    cache: &mut SessionBuildCache,
) -> String {
    if !state.has_prompt {
        return DEFAULT_PROMPT.to_string();
    }
    match ensure_prompt_lib(decls, state, cache) {
        Ok(()) => match run_prompt_lib(state, status) {
            Ok(s) => {
                state.last_error = None;
                if s.is_empty() {
                    DEFAULT_PROMPT.to_string()
                } else {
                    s
                }
            }
            Err(e) => {
                let msg = e.to_string();
                if state.last_error.as_deref() != Some(&msg) {
                    eprintln!("error: prompt: {msg}");
                    state.last_error = Some(msg);
                }
                DEFAULT_PROMPT.to_string()
            }
        },
        Err(e) => {
            let msg = e.to_string();
            if state.last_error.as_deref() != Some(&msg) {
                eprintln!("error: prompt: {msg}");
                state.last_error = Some(msg);
            }
            DEFAULT_PROMPT.to_string()
        }
    }
}

fn ensure_prompt_lib(
    decls: &[String],
    state: &mut PromptState,
    cache: &mut SessionBuildCache,
) -> Result<()> {
    if !state.dirty {
        if state.lib.is_some() {
            return Ok(());
        }
    }
    let lib = build_prompt_lib(decls, cache)?;
    state.lib = Some(lib);
    state.dirty = false;
    Ok(())
}

/// Session step image whose body prints `prompt()` via std/logging.
fn build_prompt_lib(decls: &[String], cache: &mut SessionBuildCache) -> Result<SessionStepLib> {
    driver::capture_toolchain_path();
    driver::build_session_step(
        decls,
        "logging.toStdout()\nlogging.setFormat(logging.formatPlain)\nlogging.info(prompt())\n",
        cache,
    )
}

fn run_prompt_lib(state: &mut PromptState, status: i32) -> Result<String> {
    let lib = state
        .lib
        .as_ref()
        .ok_or_else(|| Error::new("prompt: library not loaded"))?;
    std::env::set_var("THOXA_STATUS", status.to_string());
    let (_, out) = with_captured_stdout(|| {
        let _st = lib.call(status);
        Ok(())
    })?;
    Ok(strip_one_trailing_newline(&out).to_string())
}

fn strip_one_trailing_newline(s: &str) -> &str {
    s.strip_suffix("\r\n")
        .or_else(|| s.strip_suffix('\n'))
        .unwrap_or(s)
}

// ── path() ────────────────────────────────────────────────────────────────

/// True when accumulated decl sources define `fn path` (not `pathFoo`).
pub fn decls_define_path(decls: &[String]) -> bool {
    for d in decls {
        for line in d.lines() {
            if line_defines_fn_path(line) {
                return true;
            }
        }
    }
    false
}

fn line_defines_fn_path(line: &str) -> bool {
    let t = line.trim();
    let t = t.strip_prefix("export ").unwrap_or(t).trim_start();
    let Some(rest) = t.strip_prefix("fn ") else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(after) = rest.strip_prefix("path") else {
        return false;
    };
    after.starts_with('(') || after.chars().next().is_some_and(|c| c.is_whitespace())
}

/// If `fn path` is defined, run it with parent PATH components and replace
/// process `PATH` with the joined return value. No-op when undefined.
///
/// Captures the pre-hook PATH once for host tools (`cc` / `ar`) so an empty or
/// minimal session PATH does not break compile-per-chunk.
fn apply_path_hook(decls: &[String], cache: &mut SessionBuildCache) -> Result<()> {
    if !decls_define_path(decls) {
        return Ok(());
    }
    // Snapshot toolchain PATH before we overwrite session PATH.
    driver::capture_toolchain_path();
    let parent = current_path_components();
    let components = run_path_hook(decls, &parent, cache)?;
    let joined = components.join(":");
    // Empty return → empty PATH (commands only via absolute paths).
    std::env::set_var("PATH", &joined);
    Ok(())
}

fn current_path_components() -> Vec<String> {
    match std::env::var("PATH") {
        Ok(p) if !p.is_empty() => p.split(':').map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Build + run path via session step; return components (one per stdout line).
fn run_path_hook(
    decls: &[String],
    parent: &[String],
    cache: &mut SessionBuildCache,
) -> Result<Vec<String>> {
    driver::capture_toolchain_path();
    let chunk = path_hook_chunk(parent);
    let lib = driver::build_session_step(decls, &chunk, cache)?;
    let (_, out) = with_captured_stdout(|| {
        let st = lib.call(0);
        if st != 0 {
            return Err(Error::new(format!("path step status {st}")));
        }
        Ok(())
    })?;
    // Drop image after capture (frees libtcc state / unmaps .so).
    drop(lib);
    Ok(parse_path_hook_stdout(&out))
}

/// One component per non-empty line (trailing newline stripped per line).
fn parse_path_hook_stdout(s: &str) -> Vec<String> {
    s.lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Session statements: call `path(…)` and println each component.
///
/// Keep this chunk **statements only** (no top-level `let`) so a decl-stable
/// session cache can splice it as `Shell.step`. Empty parent uses
/// `sessionEmptyStrings()` (typed empty Array<String>). The `for` stays one
/// line so `{` is not command-desugared.
fn path_hook_chunk(parent: &[String]) -> String {
    let call = if parent.is_empty() {
        "path(sessionEmptyStrings())".to_string()
    } else {
        format!("path({})", thoxa_string_array_lit(parent))
    };
    format!(
        "logging.toStdout()\nlogging.setFormat(logging.formatPlain)\nfor p in {call} {{ logging.info(p) }}\n"
    )
}

fn thoxa_string_array_lit(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    let mut out = String::from("[");
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&thoxa_string_lit(s));
    }
    out.push(']');
    out
}

fn thoxa_string_lit(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── chunks ────────────────────────────────────────────────────────────────

struct ChunkOutcome {
    status: i32,
    decls_changed: bool,
}

fn run_chunk(
    chunk: &str,
    decls: &mut Vec<String>,
    prev_status: i32,
    cache: &mut SessionBuildCache,
    loaded: &mut Option<SessionStepLib>,
) -> Result<ChunkOutcome> {
    let trimmed = chunk.trim();

    // Declaration-only chunk: accumulate, do not exec.
    if is_decl_chunk(trimmed) {
        decls.push(chunk.to_string());
        return Ok(ChunkOutcome {
            status: 0,
            decls_changed: true,
        });
    }

    // D93/D103/D105: all executable lines go through the session step image —
    // no host command special-cases.
    driver::capture_toolchain_path();
    let lib = driver::build_session_step(decls, chunk, cache)?;
    let status = lib.call(prev_status);
    *loaded = Some(lib);
    Ok(ChunkOutcome {
        status,
        decls_changed: false,
    })
}

fn is_decl_chunk(trimmed: &str) -> bool {
    let first = trimmed.lines().next().unwrap_or("").trim();
    first.starts_with("fn ")
        || first.starts_with("export ")
        || first.starts_with("type ")
        || first.starts_with("extern ")
        || first.starts_with("import ")
        || first.starts_with("let ")
        || first.starts_with("mut ")
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prompt_is_section_sign() {
        assert_eq!(DEFAULT_PROMPT, "§ ");
    }

    #[test]
    fn split_forms_function_and_stmt() {
        let src = r#"
fn prompt() -> String {
    "x$ "
}
echo hi
"#;
        let forms = split_top_level_forms(src.trim());
        assert_eq!(forms.len(), 2, "{forms:?}");
        assert!(is_decl_chunk(forms[0].trim()));
        assert!(!is_decl_chunk(forms[1].trim()));
    }

    fn nonempty_rc_forms(src: &str) -> (Vec<String>, Vec<String>) {
        let mut decls = Vec::new();
        let mut stmts = Vec::new();
        for form in split_top_level_forms(src.trim()) {
            if is_effectively_empty_form(&form) {
                continue;
            }
            if is_decl_chunk(form.trim()) {
                decls.push(form);
            } else {
                stmts.push(form);
            }
        }
        (decls, stmts)
    }

    #[test]
    fn split_forms_hash_comment_apostrophe_does_not_glue_decls() {
        // Dogfood: possessive in a ## line must not open a string, or import/fn
        // braces are ignored and the file becomes one non-decl form.
        let src = r##"
## Canto session rc. Signature is metal-canary brass.
## not Thoxa's default cyan — see path() and `prompt`
import { defaultPrompt } from "std/prompt"

fn prompt() -> String {
    defaultPrompt(signature: "#E2B84A")
}
"##;
        let (decls, stmts) = nonempty_rc_forms(src);
        assert_eq!(decls.len(), 2, "decls={decls:?}");
        assert!(
            decls[0].trim().starts_with("import "),
            "decl0={:?}",
            decls[0]
        );
        assert!(
            decls[1].trim().starts_with("fn prompt"),
            "decl1={:?}",
            decls[1]
        );
        assert!(stmts.is_empty(), "stmt forms={stmts:?}");
    }

    #[test]
    fn split_forms_apostrophe_in_string_still_tracks_braces() {
        let src = r#"
fn prompt() -> String {
    "not Thoxa's default {still a string}"
}
echo hi
"#;
        let (decls, stmts) = nonempty_rc_forms(src);
        assert_eq!(decls.len(), 1, "decls={decls:?}");
        assert!(decls[0].trim().starts_with("fn prompt"));
        assert_eq!(stmts.len(), 1, "stmts={stmts:?}");
        assert!(stmts[0].trim().starts_with("echo "));
    }

    #[test]
    fn apply_rc_comment_apostrophe_decl_only_does_not_require_main() {
        let src = r##"
## not Thoxa's default cyan
import { defaultPrompt } from "std/prompt"

fn prompt() -> String {
    defaultPrompt(signature: "#E2B84A")
}
"##;
        let mut decls = Vec::new();
        let mut cache = SessionBuildCache::default();
        let mut loaded = None;
        let status = apply_rc_source(src, &mut decls, &mut cache, &mut loaded)
            .expect("decl-only rc must not compile as a program");
        assert_eq!(status, 0);
        assert!(decls_define_prompt(&decls), "decls={decls:?}");
    }

    #[test]
    fn apply_rc_stmt_after_comment_apostrophe_still_runs() {
        let src = r#"
## not Thoxa's default
logging.toStdout()
logging.setFormat(logging.formatPlain)
logging.info("from-rc-apostrophe")
"#;
        let (decls, stmts) = nonempty_rc_forms(src);
        assert!(decls.is_empty(), "decls={decls:?}");
        assert!(!stmts.is_empty(), "stmts={stmts:?}");
        assert!(
            !stmts[0].trim().starts_with('#'),
            "stmt form must not start with the comment: {:?}",
            stmts[0]
        );

        let mut acc = Vec::new();
        let mut cache = SessionBuildCache::default();
        let mut loaded = None;
        let (status, out) =
            with_captured_stdout(|| apply_rc_source(src, &mut acc, &mut cache, &mut loaded))
                .expect("capture");
        assert_eq!(status, 0);
        assert!(
            acc.is_empty(),
            "stmt must not accumulate as a decl: {acc:?}"
        );
        assert!(
            out.contains("from-rc-apostrophe"),
            "stmt must run, stdout={out:?}"
        );
    }

    #[test]
    fn prompt_name_detection() {
        assert!(decls_define_prompt(&[
            "fn prompt() -> String {\n    \"$ \"\n}".into()
        ]));
        assert!(decls_define_prompt(&[
            "export fn prompt() -> String { \"a\" }".into()
        ]));
        assert!(!decls_define_prompt(&[
            "fn promptHelper() -> String { \"a\" }".into()
        ]));
        assert!(!decls_define_prompt(&["fn other() -> Int64 { 0 }".into()]));
    }

    #[test]
    fn strip_newline() {
        assert_eq!(strip_one_trailing_newline("hi\n"), "hi");
        assert_eq!(strip_one_trailing_newline("hi\r\n"), "hi");
        assert_eq!(strip_one_trailing_newline("hi"), "hi");
        assert_eq!(strip_one_trailing_newline("a\nb\n"), "a\nb");
    }

    #[test]
    fn pure_step_status_and_pipeline() {
        // D103: status / pipes are compiled steps, not host fast paths.
        let mut decls = Vec::new();
        let mut cache = SessionBuildCache::default();
        let mut loaded = None;
        driver::capture_toolchain_path();

        // Absolute path — bare `false` is Thoxa Bool and stays non-command sugar.
        let false_bin = host_bin("false");
        let cat_bin = host_bin("cat");
        let out = run_chunk(&false_bin, &mut decls, 0, &mut cache, &mut loaded)
            .unwrap_or_else(|e| panic!("{false_bin}: {e}"));
        assert_eq!(out.status, 1);

        // showStatus keeps host-threaded status (compile path, not host println).
        let status_out =
            run_chunk("status", &mut decls, out.status, &mut cache, &mut loaded).expect("status");
        assert_eq!(status_out.status, 1);

        // Absolute paths: path_hook unit tests may clear process PATH in parallel.
        let pipe = run_chunk(
            &format!("echo pure-step-pipe | {cat_bin}"),
            &mut decls,
            status_out.status,
            &mut cache,
            &mut loaded,
        )
        .expect("pipe");
        assert_eq!(pipe.status, 0);
    }

    #[test]
    fn session_step_cache_reuses_prelude_across_echoes() {
        let mut decls = Vec::new();
        let mut cache = SessionBuildCache::default();
        let mut loaded = None;
        driver::capture_toolchain_path();
        for i in 0..5 {
            let line = format!("echo cache-{i}");
            let out = run_chunk(&line, &mut decls, 0, &mut cache, &mut loaded)
                .unwrap_or_else(|e| panic!("echo {i}: {e}"));
            assert_eq!(out.status, 0, "echo {i}");
            assert!(!out.decls_changed);
        }
        let decl = run_chunk("let cacheMarker = 1", &mut decls, 0, &mut cache, &mut loaded)
            .expect("decl");
        assert!(decl.decls_changed);
        let after = run_chunk("echo after-decl", &mut decls, 0, &mut cache, &mut loaded)
            .expect("echo after decl");
        assert_eq!(after.status, 0);
    }

    #[test]
    fn parse_echo_chunk_is_session_stmts() {
        let stmts = crate::session::parse_session_chunk_stmts("echo hello")
            .expect("parse echo");
        assert!(!stmts.is_empty());
    }

    #[test]
    fn pure_step_cd_changes_process_cwd() {
        let dir = tempfile_dir("thoxa_cd_step");
        let mut decls = Vec::new();
        let mut cache = SessionBuildCache::default();
        let mut loaded = None;
        driver::capture_toolchain_path();

        let prev = std::env::current_dir().expect("cwd");
        let path = dir.to_string_lossy().into_owned();
        let out = run_chunk(
            &format!("cd {path}"),
            &mut decls,
            0,
            &mut cache,
            &mut loaded,
        )
        .expect("cd");
        assert_eq!(out.status, 0);
        let now = std::env::current_dir().expect("cwd after cd");
        assert_eq!(now, dir);
        std::env::set_current_dir(&prev).expect("restore cwd");
    }

    #[test]
    fn apply_rc_accumulates_prompt_and_runs_stmt() {
        let dir = tempfile_dir("thoxa_rc_test");
        let rc = dir.join("shell.thx");
        fs::write(
            &rc,
            r#"
fn prompt() -> String {
    "CUSTOM$ "
}
## logging import comes from injected Shell prelude (std/logging).
logging.toStdout()
logging.setFormat(logging.formatPlain)
logging.info("from-rc")
"#,
        )
        .unwrap();

        let mut decls = Vec::new();
        let mut cache = SessionBuildCache::default();
        let mut loaded = None;
        // Point THOXA_SHELL_RC via apply_rc_source directly (no env races).
        let src = fs::read_to_string(&rc).unwrap();
        let st = apply_rc_source(&src, &mut decls, &mut cache, &mut loaded).expect("rc");
        assert_eq!(st, 0);
        assert!(decls_define_prompt(&decls), "decls={decls:?}");

        let mut pstate = PromptState::default();
        pstate.on_decls_changed(&decls);
        ensure_prompt_lib(&decls, &mut pstate, &mut cache).expect("build prompt");
        let out = run_prompt_lib(&mut pstate, 42).expect("run prompt");
        assert_eq!(out, "CUSTOM$ ");
    }

    #[test]
    fn prompt_can_read_thoxa_status_env() {
        // Prelude (injected for session stmts) already exports sysGetenv.
        let decls = vec![r#"
fn prompt() -> String {
    let st = sysGetenv("THOXA_STATUS")
    "s={st}$ "
}
"#
        .trim()
        .to_string()];
        assert!(decls_define_prompt(&decls));
        let mut cache = SessionBuildCache::default();
        let mut pstate = PromptState::default();
        pstate.on_decls_changed(&decls);
        ensure_prompt_lib(&decls, &mut pstate, &mut cache).expect("build");
        let out = run_prompt_lib(&mut pstate, 7).expect("run");
        assert_eq!(out, "s=7$ ");
    }

    #[test]
    fn path_name_detection() {
        assert!(decls_define_path(&[
            "fn path(parent: Array<String>) -> Array<String> {\n    parent\n}".into()
        ]));
        assert!(decls_define_path(&[
            "export fn path(parent: Array<String>) -> Array<String> { parent }".into()
        ]));
        assert!(!decls_define_path(&[
            "fn pathHelper(parent: Array<String>) -> Array<String> { parent }".into()
        ]));
        assert!(!decls_define_path(&["fn other() -> Int64 { 0 }".into()]));
    }

    #[test]
    fn path_hook_replaces_with_returned_components() {
        let decls = vec![r#"
fn path(parent: Array<String>) -> Array<String> {
    ["/opt/custom", "/run/system"]
}
"#
        .trim()
        .to_string()];
        assert!(decls_define_path(&decls));
        let parent = vec!["/old/a".into(), "/old/b".into()];
        let mut cache = SessionBuildCache::default();
        let out = run_path_hook(&decls, &parent, &mut cache).expect("path hook");
        assert_eq!(out, vec!["/opt/custom", "/run/system"]);
    }

    #[test]
    fn path_hook_can_merge_parent() {
        let decls = vec![r#"
fn path(parent: Array<String>) -> Array<String> {
    mut out: Array<String> = ["/first"]
    for p in parent {
        out.append(p)
    }
    out
}
"#
        .trim()
        .to_string()];
        let parent = vec!["/a".into(), "/b".into()];
        let mut cache = SessionBuildCache::default();
        let out = run_path_hook(&decls, &parent, &mut cache).expect("path merge");
        assert_eq!(out, vec!["/first", "/a", "/b"]);
    }

    #[test]
    fn path_hook_empty_array_return_clears_path() {
        // Serialize PATH mutations so parallel tests still find `cc`.
        let _lock = env_path_lock();
        driver::capture_toolchain_path();

        // Bare `[]` with `-> Array<String>` (typed empty return).
        let decls = vec![r#"
fn path(parent: Array<String>) -> Array<String> {
    []
}
"#
        .trim()
        .to_string()];
        let parent = vec!["/a".into(), "/b".into()];
        let mut cache = SessionBuildCache::default();
        let out = run_path_hook(&decls, &parent, &mut cache).expect("empty path");
        assert!(out.is_empty(), "expected no components, got {out:?}");

        // apply_path_hook must set process PATH to "" (not leave parent).
        // Keep a real toolchain PATH captured first so the hook build can spawn cc.
        let prev = std::env::var("PATH").ok();
        apply_path_hook(&decls, &mut cache).expect("apply empty path");
        let after = std::env::var("PATH").unwrap_or_else(|_| "<unset>".into());
        assert_eq!(after, "", "PATH should be cleared, got {after:?}");
        // Host tools still resolve via THOXA_TOOLCHAIN_PATH / capture.
        let still = run_path_hook(&decls, &parent, &mut cache).expect("rebuild after empty PATH");
        assert!(still.is_empty());
        match prev {
            Some(p) => std::env::set_var("PATH", p),
            None => std::env::remove_var("PATH"),
        }
    }

    #[test]
    fn path_hook_empty_via_annotated_local() {
        let decls = vec![r#"
fn path(parent: Array<String>) -> Array<String> {
    mut out: Array<String> = []
    out
}
"#
        .trim()
        .to_string()];
        let mut cache = SessionBuildCache::default();
        let out = run_path_hook(&decls, &["/x".into()], &mut cache).expect("empty local");
        assert!(out.is_empty());
    }

    #[test]
    fn parse_path_stdout_skips_empty_lines() {
        assert_eq!(
            parse_path_hook_stdout("/a\n\n/b\n"),
            vec!["/a".to_string(), "/b".to_string()]
        );
        assert!(parse_path_hook_stdout("").is_empty());
        assert!(parse_path_hook_stdout("\n\n").is_empty());
    }

    fn tempfile_dir(prefix: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("{prefix}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    /// Guard for tests that mutate process PATH (parallel test safety).
    fn env_path_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// NixOS/Oath may not have `/bin/false`; still need an absolute path so
    /// bare `false` is not parsed as a Thoxa Bool.
    fn host_bin(name: &str) -> String {
        for dir in [
            "/bin",
            "/usr/bin",
            "/run/current-system/sw/bin",
            "/oath/store/pkg/coreutils/bin",
        ] {
            let path = std::path::Path::new(dir).join(name);
            if path.is_file() {
                return path.to_string_lossy().into_owned();
            }
        }
        panic!("test helper: {name} not found on host");
    }
}
