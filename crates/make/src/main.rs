//! xtask: `cargo make <command> …`
//!
//! Project automation without shell scripts.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::time::SystemTime;

mod unicode;

fn main() {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        print_help();
        process::exit(2);
    }
    let cmd = args.remove(0);
    let code = match cmd.as_str() {
        "help" | "-h" | "--help" => {
            print_help();
            0
        }
        "runtime" => cmd_runtime(),
        "test" => cmd_test(),
        "test-asan" => cmd_test_asan(),
        "run" => cmd_run(&args),
        "build" => cmd_build(&args),
        "emit-c" => cmd_emit_c(&args),
        "clean" => cmd_clean(&args),
        "zed" => cmd_zed(&args),
        "unicode" => cmd_unicode(&args),
        other => {
            eprintln!("unknown make command `{other}` (try `cargo make help`)");
            2
        }
    };
    process::exit(code);
}

fn print_help() {
    eprintln!(
        "\
Thoxa xtask (`cargo make …`)

Build tree (everything under target/):
    target/rust/     Cargo (compiler, xtask)
    target/c/        C runtime (libthoxa_rt.a)
    target/thoxa/    Thoxa products (c/, bin/, bin-leak/, bin-address/)

Commands:
    cargo make runtime              Build libthoxa_rt.a + compile_commands.json
    cargo make test                 Run conformance suite (LeakSanitizer), site samples, unit tests
    cargo make test-asan            Conformance under ASan (non-fiber; not default)
    cargo make run <file.thx|dir>   Compile and run a program
    cargo make build <file.thx|dir> Compile to target/thoxa/bin/
    cargo make emit-c <file.thx|dir> Write generated C under target/thoxa/c/
    cargo make clean                Remove target/c and target/thoxa
    cargo make clean --all          Also remove target/rust (full wipe)
    cargo make zed                  Sync editor into local Zed (LSP + grammar + lang)
    cargo make zed --grammar        Only rebuild/install tree-sitter grammar WASM
    cargo make zed --all            Also rebuild extension.wasm (rare)
    cargo make unicode <ucd-dir>    Regenerate std/unicode.thx from the Unicode
                                    Character Database (case tables, D195)

A directory argument resolves to <dir>/main.thx (same as `thoxa run <dir>`).

After `cargo make zed`: restart the language server in Zed (grammar colors may
need a full Zed quit/restart). See extras/README.md.
"
    );
}

fn cmd_unicode(args: &[String]) -> i32 {
    let [ucd] = args else {
        eprintln!(
            "usage: cargo make unicode <ucd-dir>  (UnicodeData.txt, SpecialCasing.txt, \
             CaseFolding.txt, DerivedCoreProperties.txt, DerivedNormalizationProps.txt \
             from https://www.unicode.org/Public/<version>/ucd/)"
        );
        return 2;
    };
    let out = workspace_root().join("std/unicode.thx");
    if let Err(error) = unicode::generate(Path::new(ucd), &out).map(|summary| {
        eprintln!("wrote {} ({summary})", out.display());
    }) {
        eprintln!("cargo make unicode: {error}");
        return 1;
    }
    let out = workspace_root().join("std/normalize-data.thx");
    match unicode::generate_normalization(Path::new(ucd), &out) {
        Ok(summary) => {
            eprintln!("wrote {} ({summary})", out.display());
            0
        }
        Err(error) => {
            eprintln!("cargo make unicode: {error}");
            1
        }
    }
}

fn workspace_root() -> PathBuf {
    // crates/make → workspace root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Cargo puts binaries under `target/rust/` (see `.cargo/config.toml`).
fn thoxa_bin_path() -> PathBuf {
    workspace_root().join("target/rust/debug/thoxa")
}

fn ensure_thoxa_built() -> i32 {
    let status = Command::new("cargo")
        .args(["build", "-p", "thoxa"])
        .current_dir(workspace_root())
        .status();
    match status {
        Ok(s) if s.success() => 0,
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("failed to build thoxa: {e}");
            1
        }
    }
}

fn run_thoxa(args: &[&str]) -> i32 {
    if ensure_thoxa_built() != 0 {
        return 1;
    }
    let bin = thoxa_bin_path();
    let status = Command::new(&bin)
        .args(args)
        .current_dir(workspace_root())
        .env("THOXA_ROOT", workspace_root())
        .status();
    match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("failed to run thoxa: {e}");
            1
        }
    }
}

fn cmd_runtime() -> i32 {
    run_thoxa(&["runtime"])
}

fn cmd_run(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: cargo make run <file.thx|dir>");
        return 2;
    }
    let mut thoxa_args = vec!["run".to_string()];
    thoxa_args.extend(args.iter().cloned());
    let refs: Vec<&str> = thoxa_args.iter().map(|s| s.as_str()).collect();
    run_thoxa(&refs)
}

fn cmd_build(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: cargo make build <file.thx|dir> [-o name]");
        return 2;
    }
    let mut thoxa_args = vec!["build".to_string()];
    thoxa_args.extend(args.iter().cloned());
    let refs: Vec<&str> = thoxa_args.iter().map(|s| s.as_str()).collect();
    run_thoxa(&refs)
}

fn cmd_emit_c(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: cargo make emit-c <file.thx|dir>");
        return 2;
    }
    let mut thoxa_args = vec!["emit-c".to_string()];
    thoxa_args.extend(args.iter().cloned());
    let refs: Vec<&str> = thoxa_args.iter().map(|s| s.as_str()).collect();
    run_thoxa(&refs)
}

fn cmd_clean(args: &[String]) -> i32 {
    let root = workspace_root();
    let all = args.iter().any(|a| a == "--all" || a == "-a");

    for sub in ["c", "thoxa"] {
        let path = root.join("target").join(sub);
        if path.is_dir() {
            let _ = fs::remove_dir_all(&path);
            eprintln!("removed {}", path.display());
        }
    }

    // Legacy path from earlier layout
    let legacy = root.join("targets");
    if legacy.is_dir() {
        let _ = fs::remove_dir_all(&legacy);
        eprintln!("removed {}", legacy.display());
    }

    if all {
        let rust_dir = root.join("target/rust");
        if rust_dir.is_dir() {
            let _ = fs::remove_dir_all(&rust_dir);
            eprintln!("removed {}", rust_dir.display());
        }
        // Drop empty target/ if present
        let target = root.join("target");
        if target.is_dir()
            && fs::read_dir(&target)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
        {
            let _ = fs::remove_dir(&target);
        }
    }

    0
}

enum ConformanceGate {
    /// Default `cargo make test` — LeakSanitizer. `lsan-skip` is a ratchet.
    Leak,
    /// Optional `cargo make test-asan` — AddressSanitizer on non-fiber
    /// programs. `asan-skip` excepts `-->` fibers (ucontext); not a ratchet.
    Address,
}

fn cmd_test() -> i32 {
    let conformance = run_conformance(ConformanceGate::Leak);
    let samples = run_site_samples();
    // The compiler's own unit tests are the only coverage for the LSP and
    // diagnostics: run them in the same gate.
    let unit = run_unit_tests();
    if conformance != 0 {
        conformance
    } else if samples != 0 {
        samples
    } else {
        unit
    }
}

/// The programs on thoxa.dev's home page (`workloads/landing/samples`, D189).
/// One with a sibling `.out` must exit 0 and print exactly that, under
/// LeakSanitizer like the conformance suite; one without (a server, a
/// terminal program) must typecheck.
fn run_site_samples() -> i32 {
    let dir = workspace_root().join("workloads/landing/samples");
    let Ok(read) = fs::read_dir(&dir) else {
        return 0;
    };
    let mut entries: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|x| x.to_str()) == Some("thx"))
        .collect();
    entries.sort();
    eprintln!("site samples (workloads/landing/samples)");

    let thoxa = thoxa_bin_path();
    let mut passed = 0;
    let mut failed = 0;
    for entry in &entries {
        let name = entry.file_stem().unwrap().to_string_lossy().into_owned();
        let expected = fs::read_to_string(dir.join(format!("{name}.out"))).ok();
        let mut command = Command::new(&thoxa);
        command
            .current_dir(workspace_root())
            .env("THOXA_ROOT", workspace_root());
        if expected.is_some() {
            command
                .args(["run", entry.to_str().unwrap()])
                .env("THOXA_SANITIZE", "leak");
            if env::var_os("LSAN_OPTIONS").is_none() {
                command.env("LSAN_OPTIONS", "detect_leaks=1:print_suppressions=0");
            }
        } else {
            command.args(["check", entry.to_str().unwrap()]);
        }
        let output = match command.output() {
            Ok(output) => output,
            Err(error) => {
                eprintln!("FAIL samples/{name}: {error}");
                failed += 1;
                continue;
            }
        };
        if !output.status.success() {
            eprintln!("FAIL samples/{name}: exit {}", output.status.code().unwrap_or(1));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            failed += 1;
            continue;
        }
        if let Some(expected) = expected {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout != expected {
                eprintln!("FAIL samples/{name}: stdout mismatch");
                eprintln!("  expected: {expected:?}");
                eprintln!("  got:      {stdout:?}");
                failed += 1;
                continue;
            }
        }
        eprintln!("ok   samples/{name}");
        passed += 1;
    }
    eprintln!("{passed} samples passed, {failed} failed");
    if failed > 0 {
        1
    } else {
        0
    }
}

fn run_unit_tests() -> i32 {
    eprintln!("unit tests (cargo test -p thoxa --lib)");
    let status = Command::new("cargo")
        .args(["test", "-q", "-p", "thoxa", "--lib"])
        .current_dir(workspace_root())
        .status();
    match status {
        Ok(s) if s.success() => 0,
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("failed to run unit tests: {e}");
            1
        }
    }
}

fn cmd_test_asan() -> i32 {
    run_conformance(ConformanceGate::Address)
}

fn run_conformance(gate: ConformanceGate) -> i32 {
    if ensure_thoxa_built() != 0 {
        return 1;
    }
    // Runtime objects stay unsanitized; sanitizers are link-only on the program.
    if run_thoxa(&["runtime"]) != 0 {
        return 1;
    }

    match gate {
        ConformanceGate::Leak => {
            eprintln!("conformance (LeakSanitizer; THOXA_SANITIZE=leak)");
        }
        ConformanceGate::Address => {
            eprintln!("conformance (AddressSanitizer; THOXA_SANITIZE=address; non-fiber)");
        }
    }

    let conf = workspace_root().join("tests/conformance");
    if !conf.is_dir() {
        eprintln!("no tests/conformance directory");
        return 1;
    }

    // Top-level `*.thx` files, plus package directories that contain `main.thx`.
    let mut entries: Vec<PathBuf> = fs::read_dir(&conf)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            if p.extension().and_then(|x| x.to_str()) == Some("thx") {
                return true;
            }
            p.is_dir() && p.join("main.thx").is_file()
        })
        .collect();
    entries.sort();

    if entries.is_empty() {
        eprintln!("no .thx files or main.thx packages in tests/conformance");
        return 1;
    }

    let skip = match gate {
        ConformanceGate::Leak => load_skip_names(&conf.join("lsan-skip")),
        ConformanceGate::Address => load_skip_names(&conf.join("asan-skip")),
    };
    let thoxa = thoxa_bin_path();
    let mut passed = 0;
    let mut skipped = 0;
    let mut failed = 0;

    for entry in &entries {
        let name = if entry.is_dir() {
            entry.file_name().unwrap().to_string_lossy().into_owned()
        } else {
            entry.file_stem().unwrap().to_string_lossy().into_owned()
        };
        let known_skip = skip.contains(&name);
        // ASan: do not run excepted fiber programs (ucontext is not a ratchet).
        if known_skip && matches!(gate, ConformanceGate::Address) {
            eprintln!("skip {name} (asan: ucontext fibers)");
            skipped += 1;
            continue;
        }
        // Sibling `.out`: `hello.thx` → `hello.out`; package dir `pkgmain/` → `pkgmain.out`.
        let out_path = conf.join(format!("{name}.out"));
        let expected = if out_path.is_file() {
            fs::read_to_string(&out_path).unwrap_or_default()
        } else {
            String::new()
        };

        let mut cmd = Command::new(&thoxa);
        cmd.args(["run", entry.to_str().unwrap()])
            .current_dir(workspace_root())
            .env("THOXA_ROOT", workspace_root());
        match gate {
            ConformanceGate::Leak => {
                cmd.env("THOXA_SANITIZE", "leak");
                if env::var_os("LSAN_OPTIONS").is_none() {
                    cmd.env("LSAN_OPTIONS", "detect_leaks=1:print_suppressions=0");
                }
            }
            ConformanceGate::Address => {
                cmd.env("THOXA_SANITIZE", "address");
                if env::var_os("ASAN_OPTIONS").is_none() {
                    cmd.env(
                        "ASAN_OPTIONS",
                        "detect_leaks=0:halt_on_error=1:abort_on_error=0:print_suppressions=0",
                    );
                }
            }
        }
        let output = cmd.output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let status = out.status.code().unwrap_or(1);
                if stdout != expected {
                    eprintln!("FAIL {name}: stdout mismatch");
                    eprintln!("  expected: {expected:?}");
                    eprintln!("  got:      {stdout:?}");
                    if status != 0 {
                        eprintln!("{}", String::from_utf8_lossy(&out.stderr));
                    }
                    failed += 1;
                    continue;
                }
                if status == 0 {
                    if known_skip {
                        eprintln!(
                            "FAIL {name}: LeakSanitizer-clean — remove from tests/conformance/lsan-skip"
                        );
                        failed += 1;
                    } else {
                        eprintln!("ok   {name}");
                        passed += 1;
                    }
                } else if known_skip {
                    eprintln!("skip {name} (lsan)");
                    skipped += 1;
                } else {
                    eprintln!("FAIL {name}: exit {status}");
                    eprintln!("{}", String::from_utf8_lossy(&out.stderr));
                    failed += 1;
                }
            }
            Err(e) => {
                eprintln!("FAIL {name}: {e}");
                failed += 1;
            }
        }
    }

    match gate {
        ConformanceGate::Leak => {
            eprintln!("{passed} passed, {skipped} lsan-skip, {failed} failed");
        }
        ConformanceGate::Address => {
            eprintln!("{passed} passed, {skipped} asan-skip, {failed} failed");
        }
    }
    // Rejections and panics have no sanitizer story; run them once, in the
    // default gate.
    let rejected = match gate {
        ConformanceGate::Leak => run_expected_failures(&conf.join("fail")),
        ConformanceGate::Address => 0,
    };
    if failed > 0 || rejected != 0 {
        1
    } else {
        0
    }
}

/// `tests/conformance/fail/<name>.thx` must not succeed: `thoxa run` exits
/// non-zero (a compile error or a runtime panic) and stderr contains every
/// line of `<name>.err`. A sibling `<name>.out` must match stdout, so a panic
/// test can show how far the program got.
fn run_expected_failures(dir: &Path) -> i32 {
    let Ok(read) = fs::read_dir(dir) else {
        return 0;
    };
    let mut entries: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|x| x.to_str()) == Some("thx"))
        .collect();
    entries.sort();
    eprintln!("expected failures (tests/conformance/fail)");

    let thoxa = thoxa_bin_path();
    let mut passed = 0;
    let mut failed = 0;
    for entry in &entries {
        let name = entry.file_stem().unwrap().to_string_lossy().into_owned();
        let Ok(expected_err) = fs::read_to_string(dir.join(format!("{name}.err"))) else {
            eprintln!("FAIL fail/{name}: missing {name}.err");
            failed += 1;
            continue;
        };
        let needles: Vec<&str> = expected_err
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty())
            .collect();
        if needles.is_empty() {
            eprintln!("FAIL fail/{name}: {name}.err is empty");
            failed += 1;
            continue;
        }
        let expected_out = fs::read_to_string(dir.join(format!("{name}.out"))).ok();

        let output = Command::new(&thoxa)
            .args(["run", entry.to_str().unwrap()])
            .current_dir(workspace_root())
            .env("THOXA_ROOT", workspace_root())
            .env_remove("THOXA_SANITIZE")
            .output();
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                eprintln!("FAIL fail/{name}: {error}");
                failed += 1;
                continue;
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.success() {
            eprintln!("FAIL fail/{name}: exited 0, expected a failure");
            failed += 1;
            continue;
        }
        let missing: Vec<&&str> = needles
            .iter()
            .filter(|needle| !stderr.contains(**needle))
            .collect();
        if !missing.is_empty() {
            eprintln!("FAIL fail/{name}: stderr is missing {missing:?}");
            eprintln!("{stderr}");
            failed += 1;
            continue;
        }
        if let Some(expected_out) = expected_out {
            if stdout != expected_out {
                eprintln!("FAIL fail/{name}: stdout mismatch");
                eprintln!("  expected: {expected_out:?}");
                eprintln!("  got:      {stdout:?}");
                failed += 1;
                continue;
            }
        }
        eprintln!("ok   fail/{name}");
        passed += 1;
    }
    eprintln!("{passed} rejected as expected, {failed} failed");
    if failed > 0 {
        1
    } else {
        0
    }
}

fn load_skip_names(path: &Path) -> HashSet<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return HashSet::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// Zed / extras DX sync
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum ZedMode {
    /// Language files + grammar WASM + build thoxa + PATH link.
    Default,
    /// Grammar WASM only.
    Grammar,
    /// Default plus extension.wasm rebuild.
    All,
}

fn cmd_zed(args: &[String]) -> i32 {
    let mut mode = ZedMode::Default;
    for a in args {
        match a.as_str() {
            "--grammar" | "grammar" => mode = ZedMode::Grammar,
            "--all" | "all" | "-a" => mode = ZedMode::All,
            "-h" | "--help" | "help" => {
                eprintln!(
                    "\
usage: cargo make zed [--grammar | --all]

  (default)   languages/thoxa + grammar.wasm + cargo build -p thoxa + ~/.local/bin/thoxa
  --grammar   only rebuild/install tree-sitter grammar WASM
  --all       also rebuild extras/zed → extension.wasm

Install target: ~/.local/share/zed/extensions/installed/thoxa/
"
                );
                return 0;
            }
            other => {
                eprintln!("unknown zed flag `{other}` (try `cargo make zed --help`)");
                return 2;
            }
        }
    }

    let root = workspace_root();
    let extras = root.join("extras");
    let zed_ext = zed_install_dir();
    eprintln!("==> Zed install dir: {}", zed_ext.display());

    if let Err(e) = fs::create_dir_all(zed_ext.join("grammars")) {
        eprintln!("cannot create {}: {e}", zed_ext.join("grammars").display());
        return 1;
    }
    if let Err(e) = fs::create_dir_all(zed_ext.join("languages/thoxa")) {
        eprintln!(
            "cannot create {}: {e}",
            zed_ext.join("languages/thoxa").display()
        );
        return 1;
    }

    // Language config + highlights (cheap; skip for --grammar only).
    if mode != ZedMode::Grammar {
        eprintln!("==> languages/thoxa (config + highlights + injections)");
        let lang_src = extras.join("zed/languages/thoxa");
        let lang_dst = zed_ext.join("languages/thoxa");
        for name in ["config.toml", "highlights.scm", "injections.scm"] {
            let from = lang_src.join(name);
            let to = lang_dst.join(name);
            if let Err(e) = fs::copy(&from, &to) {
                eprintln!("copy {} → {}: {e}", from.display(), to.display());
                return 1;
            }
        }
        ensure_installed_extension_toml(&zed_ext, &root);
    }

    // Grammar WASM.
    if mode == ZedMode::Default || mode == ZedMode::Grammar || mode == ZedMode::All {
        eprintln!("==> tree-sitter grammar → thoxa.wasm");
        if let Err(e) = maybe_regenerate_parser(&extras.join("tree-sitter-thoxa")) {
            eprintln!("{e}");
            // Non-fatal if tree-sitter CLI missing and parser.c already exists.
        }
        let out = zed_ext.join("grammars/thoxa.wasm");
        if let Err(e) = build_grammar_wasm(&extras.join("tree-sitter-thoxa"), &out) {
            eprintln!("grammar WASM build failed: {e}");
            return 1;
        }
        eprintln!("    wrote {}", out.display());
    }

    // Extension WASM (shim) — rare.
    if mode == ZedMode::All {
        eprintln!("==> extension.wasm (zed_thoxa)");
        if let Err(e) = build_extension_wasm(&root, &zed_ext) {
            eprintln!("extension WASM build failed: {e}");
            return 1;
        }
    }

    // Native thoxa binary + PATH.
    if mode != ZedMode::Grammar {
        eprintln!("==> cargo build -p thoxa (LSP binary)");
        if ensure_thoxa_built() != 0 {
            return 1;
        }
        if let Err(e) = link_thoxa_on_path(&root) {
            eprintln!("PATH link warning: {e}");
        }
    }

    eprintln!();
    eprintln!("Done. In Zed:");
    eprintln!("  • LSP / diagnostics:  command palette → editor: restart language server");
    eprintln!("  • Grammar colors:     full quit + restart Zed if colors look stale");
    0
}

fn zed_install_dir() -> PathBuf {
    if let Ok(p) = env::var("ZED_EXT") {
        return PathBuf::from(p);
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/zed/extensions/installed/thoxa")
}

fn ensure_installed_extension_toml(zed_ext: &Path, repo: &Path) {
    let path = zed_ext.join("extension.toml");
    if path.is_file() {
        return;
    }
    let repo_uri = format!("file://{}", repo.display());
    let body = format!(
        r#"id = "thoxa"
name = "Thoxa"
version = "0.1.0"
schema_version = 1
description = "Thoxa language support: thin tree-sitter highlighting plus thoxa lsp."
repository = "{repo_uri}"
authors = ["Joshua <joshua@moonlight.pm>"]
themes = []
icon_themes = []
languages = ["languages/thoxa"]
capabilities = []

[lib]
kind = "Rust"
version = "0.7.0"

[grammars.thoxa]
repository = "{repo_uri}"
rev = "HEAD"
path = "extras/tree-sitter-thoxa"

[language_servers.thoxa]
name = "Thoxa LSP"
languages = ["Thoxa"]
"#
    );
    let _ = fs::write(path, body);
}

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn maybe_regenerate_parser(grammar_dir: &Path) -> Result<(), String> {
    let grammar_js = grammar_dir.join("grammar.js");
    let parser_c = grammar_dir.join("src/parser.c");
    let newer = match (mtime(&grammar_js), mtime(&parser_c)) {
        (Some(js), Some(c)) => js > c,
        (Some(_), None) => true,
        _ => false,
    };
    if !newer {
        return Ok(());
    }
    // Prefer tree-sitter on PATH.
    let status = Command::new("tree-sitter")
        .arg("generate")
        .current_dir(grammar_dir)
        .status();
    match status {
        Ok(s) if s.success() => {
            eprintln!("    regenerated src/parser.c (grammar.js is newer)");
            Ok(())
        }
        Ok(s) => Err(format!(
            "tree-sitter generate failed (exit {})",
            s.code().unwrap_or(1)
        )),
        Err(_) => {
            eprintln!("    warning: grammar.js newer than parser.c but `tree-sitter` not on PATH;");
            eprintln!("    using existing parser.c. Install tree-sitter or run:");
            eprintln!(
                "      nix-shell -p tree-sitter clang --run 'export CC=clang; cd extras/tree-sitter-thoxa && tree-sitter generate'"
            );
            Ok(())
        }
    }
}

/// Build tree-sitter SIDE_MODULE wasm via emcc (dylink.0 required by Zed).
fn build_grammar_wasm(grammar_dir: &Path, out: &Path) -> Result<(), String> {
    let emcc = find_emcc().ok_or_else(|| {
        "emcc not found (install emscripten, or set EMCC=/path/to/emcc)".to_string()
    })?;
    let src = grammar_dir.join("src");
    let tmp_out = env::temp_dir().join(format!("thoxa-grammar-{}.wasm", std::process::id()));
    let status = Command::new(&emcc)
        .current_dir(&src)
        .args([
            "-o",
            tmp_out.to_str().unwrap(),
            "-Os",
            "-s",
            "WASM=1",
            "-s",
            "SIDE_MODULE=2",
            "-s",
            "TOTAL_MEMORY=33554432",
            "-s",
            "NODEJS_CATCH_EXIT=0",
            "-s",
            "EXPORTED_FUNCTIONS=[\"_tree_sitter_thoxa\"]",
            "-fno-exceptions",
            "-fvisibility=hidden",
            "-I",
            ".",
            "parser.c",
        ])
        .status()
        .map_err(|e| format!("failed to spawn emcc ({}): {e}", emcc.display()))?;
    if !status.success() {
        return Err(format!("emcc failed (exit {})", status.code().unwrap_or(1)));
    }
    fs::copy(&tmp_out, out).map_err(|e| format!("copy wasm: {e}"))?;
    let _ = fs::remove_file(&tmp_out);
    Ok(())
}

fn find_emcc() -> Option<PathBuf> {
    if let Ok(p) = env::var("EMCC") {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    if let Ok(path) = env::var("PATH") {
        for dir in env::split_paths(&path) {
            let cand = dir.join("emcc");
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    // Common nix store layout (best-effort; optional).
    if let Ok(rd) = fs::read_dir("/nix/store") {
        let mut hits: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.contains("emscripten-") && !n.contains("emscripten-bin"))
                    .unwrap_or(false)
            })
            .map(|p| p.join("bin/emcc"))
            .filter(|p| p.is_file())
            .collect();
        hits.sort();
        if let Some(last) = hits.pop() {
            return Some(last);
        }
    }
    None
}

fn build_extension_wasm(root: &Path, zed_ext: &Path) -> Result<(), String> {
    // Ensure wasm target (ignore failure if rustup missing / already installed).
    let _ = Command::new("rustup")
        .args(["target", "add", "wasm32-wasip2"])
        .status();

    let zed_crate = root.join("extras/zed");
    let status = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&zed_crate)
        .status()
        .map_err(|e| format!("cargo build extension: {e}"))?;
    if !status.success() {
        return Err(format!(
            "cargo build zed_thoxa failed (exit {})",
            status.code().unwrap_or(1)
        ));
    }

    // Workspace target-dir is target/rust (see .cargo/config.toml).
    let candidates = [
        root.join("target/rust/wasm32-wasip2/release/zed_thoxa.wasm"),
        zed_crate.join("target/wasm32-wasip2/release/zed_thoxa.wasm"),
    ];
    let src = candidates
        .iter()
        .find(|p| p.is_file())
        .ok_or_else(|| "zed_thoxa.wasm not found after build".to_string())?;
    let dst = zed_ext.join("extension.wasm");
    fs::copy(src, &dst).map_err(|e| format!("copy extension.wasm: {e}"))?;
    eprintln!("    wrote {}", dst.display());
    Ok(())
}

fn link_thoxa_on_path(root: &Path) -> Result<(), String> {
    let home = env::var_os("HOME").ok_or_else(|| "HOME not set".to_string())?;
    let bin_dir = PathBuf::from(home).join(".local/bin");
    fs::create_dir_all(&bin_dir).map_err(|e| format!("mkdir {}: {e}", bin_dir.display()))?;
    let link = bin_dir.join("thoxa");
    let target = root.join("target/rust/debug/thoxa");
    // Replace existing symlink/file.
    let _ = fs::remove_file(&link);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(&target, &link).map_err(|e| format!("symlink: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        fs::copy(&target, &link).map_err(|e| format!("copy thoxa to PATH: {e}"))?;
    }
    eprintln!("    {} → {}", link.display(), target.display());
    Ok(())
}
