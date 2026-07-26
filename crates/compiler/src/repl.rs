//! Interactive session REPL (`thoxa` with no args) — plan PR4/PR5 + D93/D96.
//!
//! Each executable chunk is compiled to a small shared object exporting
//! [`thoxa_session_step`](crate::driver::build_session_step) and run **in-process**
//! via `dlopen` (D93 v2). Declaration lines accumulate and are prepended so later
//! chunks see prior `let` / `type` / `fn` / `import` / `extern`. The preamble
//! object is cached while decls are stable; only `Shell.step` is rebuilt per line.
//!
//! **Shell status:** host-held `ThoxaShellState.status` is threaded into each step
//! and written back (so pure-Thoxa status survives across chunks).
//!
//! **cwd:** in-process `Shell.cd` / `chdir` affects the host. A host fast-path for
//! pure `cd` still skips compile for common use.
//!
//! **rc / prompt / path (D96, D100):** interactive REPL loads
//! `~/.config/thoxa/shell.thx` once at start (decls accumulate; statements run
//! once). If the session defines `fn prompt() -> String`, the host rebuilds a
//! prompt **step .so** when decls change and **calls** it every primary prompt
//! (stdout captured in-process). Host sets `THOXA_STATUS` so Thoxa can read last
//! status via `getenv`.
//!
//! If the session defines `fn path(parent: Array<String>) -> Array<String>`,
//! the host runs it (same dlopen step path) after rc load and whenever decls
//! change: `parent` is the current process `PATH` split on `:`; the returned
//! components **replace** process `PATH` (joined with `:`).

use crate::command::{desugar_command_line, desugar_session_source, split_words};
use crate::driver::{self, SessionBuildCache};
use crate::error::{Error, Result};
use std::ffi::{CStr, CString};
use std::fs;
use std::io::{self, BufRead, Write};
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// C ABI mirror of `ThoxaShellState` in `thoxa_rt.h`.
#[repr(C)]
struct ThoxaShellState {
    status: i64,
}

type SessionStepFn = unsafe extern "C" fn(*mut ThoxaShellState) -> i64;

#[link(name = "dl")]
extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

extern "C" {
    fn pipe(fds: *mut c_int) -> c_int;
    fn dup(fd: c_int) -> c_int;
    fn dup2(oldfd: c_int, newfd: c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn fflush(stream: *mut c_void) -> c_int;
}

// RTLD_NOW
const RTLD_NOW: c_int = 2;
const STDOUT_FILENO: c_int = 1;

/// Run the interactive (or piped) session REPL. Returns final status.
pub fn run_repl() -> Result<i32> {
    let interactive = is_tty_stdin();
    let mut decls: Vec<String> = Vec::new();
    let mut status: i32 = 0;
    let mut prompt_state = PromptState::default();
    let mut step_cache = SessionBuildCache::default();
    // Keep last .so mapped until the next load so we can dlclose safely.
    let mut loaded: Option<LoadedStep> = None;

    // Pin paths before any host `cd` so runtime/std resolve stays stable.
    driver::pin_workspace_root();
    driver::capture_toolchain_path();

    if interactive {
        println!("thoxa session — Thoxa is the language; commands desugar to Shell");
        println!("type help, or exit / Ctrl-D to quit");
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

        match run_chunk(
            &chunk,
            &mut decls,
            status,
            &mut step_cache,
            &mut loaded,
        ) {
            Ok(ChunkOutcome {
                status: st,
                decls_changed,
            }) => {
                status = st;
                if decls_changed {
                    prompt_state.on_decls_changed(&decls);
                    if let Err(e) = apply_path_hook(&decls, &mut step_cache) {
                        eprintln!("error: path: {e}");
                    }
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                status = 1;
            }
        }
    }

    drop(loaded);

    if interactive {
        println!();
    }
    Ok(status)
}

struct LoadedStep {
    handle: *mut c_void,
    step: SessionStepFn,
    path: PathBuf,
}

// dlopen handles are process-global; not Send/Sync in the type system, but the
// REPL is single-threaded.
unsafe impl Send for LoadedStep {}

impl Drop for LoadedStep {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_null() {
                dlclose(self.handle);
            }
        }
        // Best-effort cleanup of unique .so files.
        let _ = fs::remove_file(&self.path);
    }
}

fn load_and_call_step(
    so_path: &Path,
    prev_status: i32,
    loaded: &mut Option<LoadedStep>,
) -> Result<i32> {
    let lib = open_step_lib(so_path)?;
    let status = call_step_fn(lib.step, prev_status);
    *loaded = Some(lib);
    Ok(status)
}

fn open_step_lib(so_path: &Path) -> Result<LoadedStep> {
    let c_path = CString::new(so_path.to_string_lossy().as_bytes())
        .map_err(|_| Error::new("session step: path contains NUL"))?;
    unsafe {
        dlerror();
        let handle = dlopen(c_path.as_ptr(), RTLD_NOW);
        if handle.is_null() {
            let err = dlerror_string();
            return Err(Error::new(format!(
                "dlopen {}: {err}",
                so_path.display()
            )));
        }
        let sym = CString::new("thoxa_session_step").unwrap();
        dlerror();
        let ptr = dlsym(handle, sym.as_ptr());
        let err = dlerror();
        if !err.is_null() || ptr.is_null() {
            dlclose(handle);
            let msg = if err.is_null() {
                "symbol thoxa_session_step not found".into()
            } else {
                CStr::from_ptr(err).to_string_lossy().into_owned()
            };
            return Err(Error::new(format!("dlsym: {msg}")));
        }
        let step: SessionStepFn = std::mem::transmute(ptr);
        Ok(LoadedStep {
            handle,
            step,
            path: so_path.to_path_buf(),
        })
    }
}

fn call_step_fn(step: SessionStepFn, prev_status: i32) -> i32 {
    let mut state = ThoxaShellState {
        status: prev_status as i64,
    };
    unsafe { step(&mut state) as i32 }
}

fn dlerror_string() -> String {
    unsafe {
        let err = dlerror();
        if err.is_null() {
            "unknown dlerror".into()
        } else {
            CStr::from_ptr(err).to_string_lossy().into_owned()
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
            !(v.is_empty() || v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("no"))
        }
        Err(_) => false,
    }
}

/// Load `shell.thx`: accumulate declarations; run top-level statements once.
/// Missing file is a silent no-op. Returns status from the one-shot stmt run (or 0).
fn load_shell_rc(
    decls: &mut Vec<String>,
    cache: &mut SessionBuildCache,
    loaded: &mut Option<LoadedStep>,
) -> Result<i32> {
    let Some(path) = shell_rc_path() else {
        return Ok(0);
    };
    if !path.is_file() {
        return Ok(0);
    }
    let src = fs::read_to_string(&path)
        .map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
    apply_rc_source(&src, decls, cache, loaded)
}

/// Split desugared rc into decl forms vs statement forms; accumulate decls;
/// execute statements once with those decls in scope.
fn apply_rc_source(
    src: &str,
    decls: &mut Vec<String>,
    cache: &mut SessionBuildCache,
    loaded: &mut Option<LoadedStep>,
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
    /// Rebuild prompt step .so before next use.
    dirty: bool,
    /// Last successful prompt library (kept mapped; re-called each line).
    lib: Option<LoadedStep>,
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
    after.starts_with('(')
        || after
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace())
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

/// Session step .so whose body is `println(prompt())`.
fn build_prompt_lib(decls: &[String], cache: &mut SessionBuildCache) -> Result<LoadedStep> {
    driver::capture_toolchain_path();
    let art = driver::build_session_step(decls, "println(prompt())\n", cache)?;
    open_step_lib(&art.so_path)
}

fn run_prompt_lib(state: &mut PromptState, status: i32) -> Result<String> {
    let step = state
        .lib
        .as_ref()
        .ok_or_else(|| Error::new("prompt: library not loaded"))?
        .step;
    std::env::set_var("THOXA_STATUS", status.to_string());
    let (_, out) = with_captured_stdout(|| {
        let _st = call_step_fn(step, status);
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
    after.starts_with('(')
        || after
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace())
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
    let art = driver::build_session_step(decls, &chunk, cache)?;
    let lib = open_step_lib(&art.so_path)?;
    let (_, out) = with_captured_stdout(|| {
        let st = call_step_fn(lib.step, 0);
        if st != 0 {
            return Err(Error::new(format!("path step status {st}")));
        }
        Ok(())
    })?;
    // Drop lib after capture (unmaps .so).
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
/// Constraints (session top-level):
/// - Module `let` has no type annotation — empty parent uses `sessionEmptyStrings()`.
/// - Multi-line `for { … }` is desugared as a command (the `{` breaks lexing); keep the loop one line.
/// - Avoid `let result = path(parent)` as a second global: Array-typed globals used as
///   call args from another global init currently type as Void — call inside the `for`.
fn path_hook_chunk(parent: &[String]) -> String {
    let mut src = String::new();
    if parent.is_empty() {
        // Prelude helper — typed empty Array<String> without module-level type ann.
        src.push_str("let parent = sessionEmptyStrings()\n");
    } else {
        src.push_str("let parent = ");
        src.push_str(&thoxa_string_array_lit(parent));
        src.push('\n');
    }
    src.push_str("for p in path(parent) { println(p) }\n");
    src
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
    loaded: &mut Option<LoadedStep>,
) -> Result<ChunkOutcome> {
    let trimmed = chunk.trim();

    // Host-side `status` — print host-held shell status without compiling.
    if trimmed == "status" {
        println!("{prev_status}");
        return Ok(ChunkOutcome {
            status: prev_status,
            decls_changed: false,
        });
    }

    // Host-side cd (fast path; in-process Thoxa `cd` also works via dlopen step).
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

    // D93 v2: compile step shared lib (cached preamble) → dlopen → call.
    driver::capture_toolchain_path();
    let lib = driver::build_session_step(decls, chunk, cache)?;
    let status = load_and_call_step(&lib.so_path, prev_status, loaded)?;
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
        let decls = vec![
            r#"
fn path(parent: Array<String>) -> Array<String> {
    ["/opt/custom", "/run/system"]
}
"#
            .trim()
            .to_string(),
        ];
        assert!(decls_define_path(&decls));
        let parent = vec!["/old/a".into(), "/old/b".into()];
        let mut cache = SessionBuildCache::default();
        let out = run_path_hook(&decls, &parent, &mut cache).expect("path hook");
        assert_eq!(out, vec!["/opt/custom", "/run/system"]);
    }

    #[test]
    fn path_hook_can_merge_parent() {
        let decls = vec![
            r#"
fn path(parent: Array<String>) -> Array<String> {
    mut out: Array<String> = ["/first"]
    for p in parent {
        out = out.append(p)
    }
    out
}
"#
            .trim()
            .to_string(),
        ];
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
        let decls = vec![
            r#"
fn path(parent: Array<String>) -> Array<String> {
    []
}
"#
            .trim()
            .to_string(),
        ];
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
        let decls = vec![
            r#"
fn path(parent: Array<String>) -> Array<String> {
    mut out: Array<String> = []
    out
}
"#
            .trim()
            .to_string(),
        ];
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
        let base = std::env::temp_dir().join(format!(
            "{prefix}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    /// Guard for tests that mutate process PATH (parallel test safety).
    fn env_path_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }
}
