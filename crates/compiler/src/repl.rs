//! Interactive session REPL (`thoxa` with no args) — plan PR4/PR5.
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

use crate::command::{desugar_command_line, split_words};
use crate::driver::{self, BuildOptions};
use crate::error::{Error, Result};
use std::fs;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

/// Run the interactive (or piped) session REPL. Returns final status.
pub fn run_repl() -> Result<i32> {
    let interactive = is_tty_stdin();
    if interactive {
        println!("thoxa session — Thoxa is the language; commands desugar to Shell");
        println!("type help, or exit / Ctrl-D to quit");
    }

    let mut decls: Vec<String> = Vec::new();
    let mut status: i32 = 0;
    let mut stdout = io::stdout();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        if interactive {
            write!(stdout, "$ ").ok();
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
            Ok(st) => status = st,
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
    // libc isatty(0) without extra deps: try `std` heuristic via stderr of `test -t 0`
    // Prefer nix-free: check if we can use libc via std::io::IsTerminal (Rust 1.70+).
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

fn run_chunk(chunk: &str, decls: &mut Vec<String>, _interactive: bool) -> Result<i32> {
    let trimmed = chunk.trim();

    // Host-side cd so directory persists across compile/exec children.
    if let Some(st) = try_host_cd(trimmed)? {
        return Ok(st);
    }

    // Host-side OS pipes: `cmd1 | cmd2 | …` (PR6). Compile path later.
    if let Some(st) = try_host_pipeline(trimmed)? {
        return Ok(st);
    }

    // Declaration-only chunk: accumulate, do not exec.
    if is_decl_chunk(trimmed) {
        decls.push(chunk.to_string());
        return Ok(0);
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
    Ok(out.code().unwrap_or(1))
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
    let parts: Vec<&str> = trimmed.split('|').map(str::trim).filter(|s| !s.is_empty()).collect();
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
