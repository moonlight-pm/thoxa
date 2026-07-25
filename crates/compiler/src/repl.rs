//! Interactive session REPL (`thoxa` with no args) — plan PR4/PR5 + D96.
//!
//! Each chunk is compiled as a session script (same path as `thoxa run` on a
//! no-main file). Declaration lines accumulate and are prepended so later
//! chunks see prior `let` / `type` / `fn` / `import` / `extern`.
//!
//! **cwd:** child processes cannot keep `cd` for the parent. The host applies
//! `cd` itself when the chunk is a pure `cd` command (PR4 pragmatism until
//! in-process dlopen steps land).
//!
//! **status:** last process exit code (session `shell.status`).
//!
//! **rc / prompt (D96):** interactive REPL loads `~/.config/thoxa/shell.thx`
//! once at start (decls accumulate; statements run once). If the session
//! defines `fn prompt() -> String`, the host rebuilds a small prompt binary
//! when decls change and **runs** it every primary prompt (live time/cwd/…).
//! Host sets `THOXA_STATUS` so Thoxa can read last status via `getenv`.

use crate::command::{desugar_command_line, desugar_session_source, split_words};
use crate::driver::{self, BuildOptions};
use crate::error::{Error, Result};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Run the interactive (or piped) session REPL. Returns final status.
pub fn run_repl() -> Result<i32> {
    let interactive = is_tty_stdin();
    let mut decls: Vec<String> = Vec::new();
    let mut status: i32 = 0;
    let mut prompt_state = PromptState::default();

    if interactive {
        println!("thoxa session — Thoxa is the language; commands desugar to Shell");
        println!("type help, or exit / Ctrl-D to quit");
        match load_shell_rc(&mut decls) {
            Ok(st) => status = st,
            Err(e) => {
                eprintln!("error: shell rc: {e}");
                status = 1;
            }
        }
        prompt_state.on_decls_changed(&decls);
    }

    let mut stdout = io::stdout();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        if interactive {
            let prompt = primary_prompt(&decls, &mut prompt_state, status);
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

        // Multiline continuation: incomplete braces / quotes → read more.
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

        match run_chunk(&chunk, &mut decls, status) {
            Ok(ChunkOutcome {
                status: st,
                decls_changed,
            }) => {
                status = st;
                if decls_changed {
                    prompt_state.on_decls_changed(&decls);
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                status = 1;
            }
        }
    }

    if interactive {
        println!();
    }
    Ok(status)
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
            !(v.is_empty() || v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("no"))
        }
        Err(_) => false,
    }
}

/// Load `shell.thx`: accumulate declarations; run top-level statements once.
/// Missing file is a silent no-op. Returns status from the one-shot stmt run (or 0).
fn load_shell_rc(decls: &mut Vec<String>) -> Result<i32> {
    let Some(path) = shell_rc_path() else {
        return Ok(0);
    };
    if !path.is_file() {
        return Ok(0);
    }
    let src = fs::read_to_string(&path)
        .map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
    apply_rc_source(&src, decls)
}

/// Split desugared rc into decl forms vs statement forms; accumulate decls;
/// execute statements once with those decls in scope.
fn apply_rc_source(src: &str, decls: &mut Vec<String>) -> Result<i32> {
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
    let out = run_chunk(&chunk, decls, 0)?;
    Ok(out.status)
}

/// Split source into top-level forms (brace/string-aware, line-oriented).
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

        for &b in line.as_bytes() {
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

const DEFAULT_PROMPT: &str = "$ ";

#[derive(Default)]
struct PromptState {
    /// Whether decls currently define `fn prompt`.
    has_prompt: bool,
    /// Rebuild prompt binary before next use.
    dirty: bool,
    /// Last successful prompt binary.
    bin: Option<PathBuf>,
    /// Avoid spamming the same rebuild error every line.
    last_error: Option<String>,
}

impl PromptState {
    fn on_decls_changed(&mut self, decls: &[String]) {
        self.has_prompt = decls_define_prompt(decls);
        self.dirty = self.has_prompt;
        if !self.has_prompt {
            self.bin = None;
            self.last_error = None;
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
    after.starts_with('(')
        || after
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace())
}

fn primary_prompt(decls: &[String], state: &mut PromptState, status: i32) -> String {
    if !state.has_prompt {
        return DEFAULT_PROMPT.to_string();
    }
    match ensure_prompt_bin(decls, state) {
        Ok(bin) => match run_prompt_bin(&bin, status) {
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

fn ensure_prompt_bin(decls: &[String], state: &mut PromptState) -> Result<PathBuf> {
    if !state.dirty {
        if let Some(ref bin) = state.bin {
            if bin.is_file() {
                return Ok(bin.clone());
            }
        }
    }
    let bin = build_prompt_binary(decls)?;
    state.bin = Some(bin.clone());
    state.dirty = false;
    Ok(bin)
}

/// Build a session binary whose body is `println(prompt())` over `decls`.
fn build_prompt_binary(decls: &[String]) -> Result<PathBuf> {
    let mut src = String::new();
    for d in decls {
        src.push_str(d);
        if !d.ends_with('\n') {
            src.push('\n');
        }
    }
    src.push_str("println(prompt())\n");

    let dir = driver::thoxa_target_dir().join("repl");
    fs::create_dir_all(&dir).map_err(|e| Error::new(e.to_string()))?;
    let path = dir.join("prompt.thx");
    fs::write(&path, &src).map_err(|e| Error::new(e.to_string()))?;

    let opts = BuildOptions {
        source: path,
        output_name: Some("repl_prompt".into()),
        emit_c_only: false,
    };
    let art = driver::build(&opts)?;
    art.bin_path
        .ok_or_else(|| Error::new("prompt: no binary"))
}

fn run_prompt_bin(bin: &Path, status: i32) -> Result<String> {
    let out = Command::new(bin)
        .env("THOXA_STATUS", status.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| Error::new(format!("prompt exec: {e}")))?;
    if !out.status.success() {
        return Err(Error::new(format!(
            "prompt exited {}",
            out.status.code().unwrap_or(1)
        )));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(strip_one_trailing_newline(&s).to_string())
}

fn strip_one_trailing_newline(s: &str) -> &str {
    s.strip_suffix("\r\n")
        .or_else(|| s.strip_suffix('\n'))
        .unwrap_or(s)
}

// ── chunks ────────────────────────────────────────────────────────────────

struct ChunkOutcome {
    status: i32,
    decls_changed: bool,
}

fn run_chunk(chunk: &str, decls: &mut Vec<String>, prev_status: i32) -> Result<ChunkOutcome> {
    let trimmed = chunk.trim();

    // Host-side `status` — session chunks always start Shell at 0; host tracks real status.
    if trimmed == "status" {
        println!("{prev_status}");
        return Ok(ChunkOutcome {
            status: prev_status,
            decls_changed: false,
        });
    }

    // Host-side cd so directory persists across compile/exec children.
    if let Some(st) = try_host_cd(trimmed)? {
        return Ok(ChunkOutcome {
            status: st,
            decls_changed: false,
        });
    }

    // Host-side OS pipes: `cmd1 | cmd2 | …` (PR6). Compile path later.
    if let Some(st) = try_host_pipeline(trimmed)? {
        return Ok(ChunkOutcome {
            status: st,
            decls_changed: false,
        });
    }

    // Declaration-only chunk: accumulate, do not exec.
    if is_decl_chunk(trimmed) {
        decls.push(chunk.to_string());
        return Ok(ChunkOutcome {
            status: 0,
            decls_changed: true,
        });
    }

    // Build session source: decls + chunk
    let mut src = String::new();
    for d in decls.iter() {
        src.push_str(d);
        src.push('\n');
    }
    src.push_str(chunk);
    src.push('\n');

    let dir = driver::thoxa_target_dir().join("repl");
    fs::create_dir_all(&dir).map_err(|e| Error::new(e.to_string()))?;
    let path = dir.join("chunk.thx");
    fs::write(&path, &src).map_err(|e| Error::new(e.to_string()))?;

    let opts = BuildOptions {
        source: path,
        output_name: Some("repl_chunk".into()),
        emit_c_only: false,
    };
    // Build + run capturing nothing special — inherit stdout/stderr for prints.
    // Use a subprocess so a panic in user code does not kill the REPL.
    let art = driver::build(&opts)?;
    let bin = art
        .bin_path
        .ok_or_else(|| Error::new("repl: no binary"))?;
    let out = Command::new(&bin)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| Error::new(format!("repl exec: {e}")))?;
    Ok(ChunkOutcome {
        status: out.code().unwrap_or(1),
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

fn try_host_cd(trimmed: &str) -> Result<Option<i32>> {
    // Pure command-skin `cd` only (host chdir so cwd survives child execs).
    if trimmed.contains('\n') {
        return Ok(None);
    }
    let words = match split_words(trimmed) {
        Ok(w) => w,
        Err(_) => return Ok(None),
    };
    if words.first().map(|s| s.as_str()) != Some("cd") {
        return Ok(None);
    }
    let path = if words.len() == 1 {
        env_home()?
    } else if words.len() == 2 {
        words[1].clone()
    } else {
        eprintln!("cd: too many arguments");
        return Ok(Some(1));
    };
    match std::env::set_current_dir(&path) {
        Ok(()) => Ok(Some(0)),
        Err(e) => {
            eprintln!("cd: {path}: {e}");
            Ok(Some(1))
        }
    }
}

fn env_home() -> Result<String> {
    std::env::var("HOME").map_err(|_| Error::new("cd: HOME not set"))
}

/// `a | b | c` — OS pipeline in the host (same process group, inherit stdio ends).
fn try_host_pipeline(trimmed: &str) -> Result<Option<i32>> {
    if trimmed.contains('\n') || !trimmed.contains('|') {
        return Ok(None);
    }
    // Do not steal Thoxa bitwise-or expressions at top level (rare in REPL).
    if trimmed.contains("||") {
        return Ok(None);
    }
    let parts: Vec<&str> = trimmed
        .split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() < 2 {
        return Ok(None);
    }
    // Each stage must look like a command (word split succeeds).
    let mut stages: Vec<Vec<String>> = Vec::new();
    for p in &parts {
        let words = match split_words(p) {
            Ok(w) if !w.is_empty() => w,
            _ => return Ok(None),
        };
        stages.push(words);
    }

    // Resolve first tokens via PATH for Command::new.
    let mut children = Vec::new();
    let mut prev_stdout: Option<std::process::ChildStdout> = None;
    for (i, argv) in stages.iter().enumerate() {
        let mut cmd = Command::new(&argv[0]);
        if argv.len() > 1 {
            cmd.args(&argv[1..]);
        }
        if let Some(out) = prev_stdout.take() {
            cmd.stdin(Stdio::from(out));
        } else {
            cmd.stdin(Stdio::null());
        }
        if i + 1 < stages.len() {
            cmd.stdout(Stdio::piped());
        } else {
            cmd.stdout(Stdio::inherit());
        }
        cmd.stderr(Stdio::inherit());
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::new(format!("pipeline: {}: {e}", argv[0])))?;
        prev_stdout = child.stdout.take();
        children.push(child);
    }
    let mut status = 0;
    for mut c in children {
        let st = c
            .wait()
            .map_err(|e| Error::new(format!("pipeline wait: {e}")))?;
        status = st.code().unwrap_or(1);
    }
    Ok(Some(status))
}

#[allow(dead_code)]
fn _touch_desugar(line: &str) -> Result<Option<String>> {
    desugar_command_line(line)
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn prompt_name_detection() {
        assert!(decls_define_prompt(&[
            "fn prompt() -> String {\n    \"$ \"\n}".into()
        ]));
        assert!(decls_define_prompt(&["export fn prompt() -> String { \"a\" }".into()]));
        assert!(!decls_define_prompt(&["fn promptHelper() -> String { \"a\" }".into()]));
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
    fn apply_rc_accumulates_prompt_and_runs_stmt() {
        let dir = tempfile_dir("thoxa_rc_test");
        let rc = dir.join("shell.thx");
        fs::write(
            &rc,
            r#"
fn prompt() -> String {
    "CUSTOM$ "
}
println("from-rc")
"#,
        )
        .unwrap();

        let mut decls = Vec::new();
        // Point THOXA_SHELL_RC via apply_rc_source directly (no env races).
        let src = fs::read_to_string(&rc).unwrap();
        let st = apply_rc_source(&src, &mut decls).expect("rc");
        assert_eq!(st, 0);
        assert!(decls_define_prompt(&decls), "decls={decls:?}");

        let bin = build_prompt_binary(&decls).expect("build prompt");
        let out = run_prompt_bin(&bin, 42).expect("run prompt");
        assert_eq!(out, "CUSTOM$ ");
    }

    #[test]
    fn prompt_can_read_thoxa_status_env() {
        // Prelude (injected for session stmts) already exports sysGetenv.
        let decls = vec![
            r#"
fn prompt() -> String {
    let st = sysGetenv("THOXA_STATUS")
    "s={st}$ "
}
"#
            .trim()
            .to_string(),
        ];
        assert!(decls_define_prompt(&decls));
        let bin = build_prompt_binary(&decls).expect("build");
        let out = run_prompt_bin(&bin, 7).expect("run");
        assert_eq!(out, "s=7$ ");
    }

    fn tempfile_dir(prefix: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "{prefix}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }
}
