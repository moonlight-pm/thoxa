//! `thoxa` CLI.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use thoxa::diagnostic;
use thoxa::driver::{self, BuildOptions};
use thoxa::error::Error;
use thoxa::lsp;

/// Parsing and lowering recurse over the AST. Batch commands run on a thread
/// with a large stack (address space; pages are committed only as used) so a
/// deep but legal program does not overflow the 8 MiB main-thread stack.
const CLI_STACK_BYTES: usize = 256 * 1024 * 1024;

fn main() -> ExitCode {
    // Session steps (REPL, `-c`) run libtcc-compiled code in this process, and
    // that code addresses thread-local storage as the main thread's: run them
    // on the main thread or they corrupt the worker's TLS.
    let session = matches!(
        env::args().nth(1).as_deref(),
        None | Some("-l" | "--login" | "-" | "-c" | "repl")
    );
    if session {
        return run_cli();
    }
    let worker = std::thread::Builder::new()
        .name("thoxa".into())
        .stack_size(CLI_STACK_BYTES)
        .spawn(run_cli);
    match worker {
        Ok(handle) => match handle.join() {
            Ok(code) => code,
            Err(panic) => std::panic::resume_unwind(panic),
        },
        // Could not reserve the large stack: run on the main thread instead.
        Err(_) => run_cli(),
    }
}

fn run_cli() -> ExitCode {
    match try_main() {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            // Prefer structured pretty when we have a single primary error.
            let diags = e.into_structured(None);
            if diags.is_empty() {
                eprintln!("error: unknown failure");
            } else if diags.len() == 1 && diags[0].span.is_none() && diags[0].code.is_none() {
                eprintln!("error: {}", diags[0].message);
            } else {
                for (i, d) in diags.iter().enumerate() {
                    if i > 0 {
                        eprintln!();
                    }
                    // Spans are offsets into the diagnostic's file: read it for line:col + snippet.
                    let source = d.path.as_ref().and_then(|p| fs::read_to_string(p).ok());
                    eprint!("{}", diagnostic::format_pretty(d, source.as_deref()));
                }
            }
            ExitCode::from(1)
        }
    }
}

fn try_main() -> Result<i32, Error> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    // Bare `thoxa` → session REPL (plan D88). Login shells pass `-l` / `--login`.
    if args.is_empty()
        || matches!(
            args.first().map(String::as_str),
            Some("-l" | "--login" | "-")
        )
    {
        return thoxa::repl::run_repl();
    }
    let cmd = args.remove(0);
    match cmd.as_str() {
        "-c" => {
            let src = args.first().cloned().ok_or_else(|| {
                Error::new("thoxa -c requires a session chunk (try `thoxa -c 'echo hi'`)")
            })?;
            run_dash_c(&src)
        }
        "build" => {
            let opts = parse_build_args(&args)?;
            let art = driver::build(&opts)?;
            if let Some(bin) = art.bin_path {
                if art.cached {
                    eprintln!("up to date {}", bin.display());
                } else {
                    eprintln!("built {}", bin.display());
                }
            }
            Ok(0)
        }
        "run" => {
            let opts = parse_build_args(&args)?;
            driver::run(&opts)
        }
        "emit-c" => {
            let opts = parse_build_args(&args)?;
            let mut opts = opts;
            opts.emit_c_only = true;
            let path = driver::emit_c(&opts)?;
            println!("{}", path.display());
            Ok(0)
        }
        "runtime" => {
            driver::build_runtime()?;
            eprintln!("runtime → {}", driver::runtime_lib_path().display());
            Ok(0)
        }
        "lsp" => {
            // Language server: stdio JSON-RPC. Identity on stderr; protocol on stdout.
            Ok(lsp::run())
        }
        "check" => {
            let (path, json) = parse_check_args(&args)?;
            run_check(path, json)
        }
        "version" | "--version" | "-V" => {
            println!("{}", thoxa::version::identity_line());
            Ok(0)
        }
        "repl" => thoxa::repl::run_repl(),
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(0)
        }
        // `thoxa path.thx` / `thoxa dir/` — run as program or session script.
        other if looks_like_source_path(other) => {
            let opts = BuildOptions {
                source: PathBuf::from(other),
                output_name: None,
                emit_c_only: false,
                program_args: args,
            };
            driver::run(&opts)
        }
        other => Err(Error::new(format!(
            "unknown command `{other}` (try `thoxa help`)"
        ))),
    }
}

/// POSIX `sh -c` equivalent: one session chunk (command sugar or Thoxa).
fn run_dash_c(src: &str) -> Result<i32, Error> {
    thoxa::repl::run_one_shot(src)
}

fn looks_like_source_path(s: &str) -> bool {
    let p = PathBuf::from(s);
    p.exists() || s.ends_with(".thx") || s.contains('/') || s == "." || s == ".."
}

fn parse_check_args(args: &[String]) -> Result<(PathBuf, bool), Error> {
    let mut path: Option<PathBuf> = None;
    let mut json = false;
    for a in args {
        match a.as_str() {
            "--json" => json = true,
            "-h" | "--help" => {
                // fall through to usage via missing path if only flags
            }
            s if s.starts_with('-') => {
                return Err(Error::new(format!("unknown check flag `{s}`")));
            }
            s => {
                if path.is_none() {
                    path = Some(PathBuf::from(s));
                } else {
                    return Err(Error::new("check accepts a single path"));
                }
            }
        }
    }
    let path = path.ok_or_else(|| {
        Error::new("check requires a path (file.thx or directory); optional --json")
    })?;
    Ok((path, json))
}

fn run_check(path: PathBuf, json: bool) -> Result<i32, Error> {
    let entry = thoxa::loader::resolve_entry(&path)?;
    let text = fs::read_to_string(&entry)
        .map_err(|e| Error::new(format!("cannot read {}: {e}", entry.display())))?;
    let filename = entry.to_string_lossy();
    let diags = lsp::check_source(&filename, &text);
    if json {
        let structured: Vec<_> = diags.iter().map(|d| d.to_diagnostic()).collect();
        let report = diagnostic::format_json_report(&structured, Some(&text));
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_else(|e| {
                format!(r#"{{"ok":false,"diagnostics":[],"error":"{e}"}}"#)
            })
        );
    } else {
        for (i, d) in diags.iter().enumerate() {
            if i > 0 {
                eprintln!();
            }
            let structured = d.to_diagnostic();
            eprint!("{}", diagnostic::format_pretty(&structured, Some(&text)));
        }
    }
    // Errors fail the check; warnings alone still exit 0 (D124).
    if diags
        .iter()
        .any(|d| d.severity == diagnostic::Severity::Error)
    {
        Ok(1)
    } else {
        Ok(0)
    }
}

fn parse_build_args(args: &[String]) -> Result<BuildOptions, Error> {
    let mut source: Option<PathBuf> = None;
    let mut output_name: Option<String> = None;
    let mut program_args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                let name = args
                    .get(i)
                    .ok_or_else(|| Error::new("-o requires a name"))?;
                output_name = Some(name.clone());
            }
            s if s.starts_with('-') && source.is_none() => {
                return Err(Error::new(format!("unknown flag `{s}`")));
            }
            s => {
                if source.is_none() {
                    source = Some(PathBuf::from(s));
                } else {
                    // Everything after the source is forwarded to the program.
                    program_args.push(s.to_string());
                }
            }
        }
        i += 1;
    }
    let source = source.ok_or_else(|| Error::new("missing source path (file.thx or directory)"))?;
    Ok(BuildOptions {
        source,
        output_name,
        emit_c_only: false,
        program_args,
    })
}

fn print_usage() {
    eprintln!(
        "\
thoxa — native Thoxa compiler + session REPL (compile-to-C)

USAGE:
    thoxa                        Session REPL (commands desugar to Shell)
    thoxa -c '<chunk>'           One-shot session (POSIX login-shell -c)
    thoxa repl                   Same as bare thoxa
    thoxa <file.thx|dir>         Run program (main) or session script (no main)
    thoxa run    <file.thx|dir>
    thoxa build  <file.thx|dir> [-o name]
    thoxa emit-c <file.thx|dir>
    thoxa check  <file.thx|dir> [--json]
    thoxa runtime
    thoxa lsp
    thoxa version                Build identity (package + git hash + path)

A directory argument resolves to <dir>/main.thx (project root for ~/ is that dir).
Build products land under target/ (rust/, c/, thoxa/).

Session scripts omit `fn main`; top-level statements become Shell.step.
Interactive REPL loads ~/.config/thoxa/shell.thx (THOXA_SHELL_RC / THOXA_NORC).
Optional fn prompt() -> String customizes the primary prompt (THOXA_STATUS env).
Optional fn path(parent: Array<String>) -> Array<String> replaces process PATH.
import from \"std/…\" (Str Path Ansi Env Git Fallible); THOXA_STD overrides the tree.
`thoxa lsp` speaks Language Server Protocol over stdio (for editors).
`thoxa check` type-checks without codegen; pretty diagnostics by default (codes,
caret, help). Pass `--json` for a machine-readable report (D124).
`thoxa version` prints package+git hash and the binary path (compare to LSP logs)."
    );
}
