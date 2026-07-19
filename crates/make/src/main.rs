//! xtask: `cargo make <command> …`
//!
//! Project automation without shell scripts.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{self, Command};

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
        "run" => cmd_run(&args),
        "build" => cmd_build(&args),
        "emit-c" => cmd_emit_c(&args),
        "clean" => cmd_clean(),
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
ThoxaNative xtask (`cargo make …`)

Commands:
    cargo make runtime              Build libthoxa_rt.a + compile_commands.json
    cargo make test                 Run conformance suite
    cargo make run <file.thx>       Compile and run a program
    cargo make build <file.thx>     Compile to targets/bin/
    cargo make emit-c <file.thx>    Write generated C under targets/c/
    cargo make clean                Remove targets/ contents (keeps .gitkeep)
"
    );
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
    let bin = workspace_root().join("target/debug/thoxa");
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
        eprintln!("usage: cargo make run <file.thx>");
        return 2;
    }
    let mut thoxa_args = vec!["run".to_string()];
    thoxa_args.extend(args.iter().cloned());
    let refs: Vec<&str> = thoxa_args.iter().map(|s| s.as_str()).collect();
    run_thoxa(&refs)
}

fn cmd_build(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: cargo make build <file.thx> [-o name]");
        return 2;
    }
    let mut thoxa_args = vec!["build".to_string()];
    thoxa_args.extend(args.iter().cloned());
    let refs: Vec<&str> = thoxa_args.iter().map(|s| s.as_str()).collect();
    run_thoxa(&refs)
}

fn cmd_emit_c(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: cargo make emit-c <file.thx>");
        return 2;
    }
    let mut thoxa_args = vec!["emit-c".to_string()];
    thoxa_args.extend(args.iter().cloned());
    let refs: Vec<&str> = thoxa_args.iter().map(|s| s.as_str()).collect();
    run_thoxa(&refs)
}

fn cmd_clean() -> i32 {
    let targets = workspace_root().join("targets");
    if targets.is_dir() {
        for entry in fs::read_dir(&targets).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str()) == Some(".gitkeep") {
                continue;
            }
            if path.is_dir() {
                let _ = fs::remove_dir_all(&path);
            } else {
                let _ = fs::remove_file(&path);
            }
        }
    }
    // Also clean cargo target? leave that to `cargo clean`
    eprintln!("cleaned targets/");
    0
}

fn cmd_test() -> i32 {
    if ensure_thoxa_built() != 0 {
        return 1;
    }
    // Build runtime once
    if run_thoxa(&["runtime"]) != 0 {
        return 1;
    }

    let conf = workspace_root().join("tests/conformance");
    if !conf.is_dir() {
        eprintln!("no tests/conformance directory");
        return 1;
    }

    let mut entries: Vec<PathBuf> = fs::read_dir(&conf)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("thx"))
        .collect();
    entries.sort();

    if entries.is_empty() {
        eprintln!("no .thx files in tests/conformance");
        return 1;
    }

    let thoxa = workspace_root().join("target/debug/thoxa");
    let mut passed = 0;
    let mut failed = 0;

    for thx in &entries {
        let name = thx.file_stem().unwrap().to_string_lossy();
        let out_path = thx.with_extension("out");
        let expected = if out_path.is_file() {
            fs::read_to_string(&out_path).unwrap_or_default()
        } else {
            String::new()
        };

        let output = Command::new(&thoxa)
            .args(["run", thx.to_str().unwrap()])
            .current_dir(workspace_root())
            .env("THOXA_ROOT", workspace_root())
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let status = out.status.code().unwrap_or(1);
                if status != 0 {
                    eprintln!("FAIL {name}: exit {status}");
                    eprintln!("{}", String::from_utf8_lossy(&out.stderr));
                    failed += 1;
                    continue;
                }
                if stdout != expected {
                    eprintln!("FAIL {name}: stdout mismatch");
                    eprintln!("  expected: {expected:?}");
                    eprintln!("  got:      {stdout:?}");
                    failed += 1;
                } else {
                    eprintln!("ok   {name}");
                    passed += 1;
                }
            }
            Err(e) => {
                eprintln!("FAIL {name}: {e}");
                failed += 1;
            }
        }
    }

    eprintln!("{passed} passed, {failed} failed");
    if failed > 0 {
        1
    } else {
        0
    }
}

