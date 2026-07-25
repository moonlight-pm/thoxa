//! Compile pipeline: source → C → binary under `target/`.
//!
//! Layout:
//! - `target/rust/`   — Cargo (`CARGO_TARGET_DIR` / `.cargo/config.toml`)
//! - `target/c/`      — C runtime objects and `libthoxa_rt.a`
//! - `target/thoxa/`  — Thoxa compiler products (`c/`, `bin/`)

use crate::cgen;
use crate::error::{Error, Result};
use crate::loader;
use crate::mir;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Workspace-relative paths resolved from cwd or `THOXA_ROOT`.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub source: PathBuf,
    /// Base name for outputs under `target/thoxa/` (default: source stem).
    pub output_name: Option<String>,
    /// If set, only write generated C under `target/thoxa/c/`.
    pub emit_c_only: bool,
}

pub struct BuildArtifacts {
    pub c_path: PathBuf,
    pub bin_path: Option<PathBuf>,
}

fn workspace_root() -> PathBuf {
    if let Ok(root) = std::env::var("THOXA_ROOT") {
        return PathBuf::from(root);
    }
    // Walk up looking for workspace Cargo.toml that lists crates/compiler
    let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        let marker = dir.join("crates/compiler/Cargo.toml");
        if marker.is_file() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Root of all build products: `target/`.
pub fn target_dir() -> PathBuf {
    workspace_root().join("target")
}

/// Cargo output: `target/rust/`.
pub fn rust_target_dir() -> PathBuf {
    target_dir().join("rust")
}

/// C runtime build products: `target/c/`.
pub fn c_target_dir() -> PathBuf {
    target_dir().join("c")
}

/// Thoxa compiler products: `target/thoxa/`.
pub fn thoxa_target_dir() -> PathBuf {
    target_dir().join("thoxa")
}

pub fn runtime_include_dir() -> PathBuf {
    workspace_root().join("runtime/include")
}

pub fn runtime_lib_path() -> PathBuf {
    c_target_dir().join("libthoxa_rt.a")
}

pub fn compile_to_c(source: &Path) -> Result<String> {
    let mut package = loader::load_package(source)?;
    crate::session::prepare_package(&mut package)?;
    let program = mir::lower_package(&package)?;
    Ok(cgen::emit_program(&program))
}

pub fn ensure_runtime() -> Result<()> {
    // Rebuild when the library is missing or older than runtime sources.
    let lib = runtime_lib_path();
    let src = workspace_root().join("runtime/src/thoxa_rt.c");
    let hdr = workspace_root().join("runtime/include/thoxa_rt.h");
    let needs = match (lib.metadata(), src.metadata(), hdr.metadata()) {
        (Ok(lib_m), Ok(src_m), Ok(hdr_m)) => {
            let lib_t = lib_m.modified().ok();
            let src_t = src_m.modified().ok();
            let hdr_t = hdr_m.modified().ok();
            match (lib_t, src_t, hdr_t) {
                (Some(lt), Some(st), Some(ht)) => lt < st || lt < ht,
                _ => true,
            }
        }
        _ => true,
    };
    if needs {
        build_runtime()?;
    }
    if !lib.is_file() {
        return Err(Error::new(format!(
            "runtime library missing at {} — run `cargo make runtime`",
            lib.display()
        )));
    }
    Ok(())
}

pub fn build_runtime() -> Result<()> {
    let root = workspace_root();
    let out_dir = c_target_dir();
    fs::create_dir_all(&out_dir).map_err(|e| Error::new(e.to_string()))?;

    let src = root.join("runtime/src/thoxa_rt.c");
    let obj = out_dir.join("thoxa_rt.o");
    let lib = out_dir.join("libthoxa_rt.a");
    let include = root.join("runtime/include");

    let status = Command::new("cc")
        .args(["-std=c11", "-O2", "-c"])
        .arg(&src)
        .arg("-I")
        .arg(&include)
        .arg("-o")
        .arg(&obj)
        .status()
        .map_err(|e| Error::new(format!("failed to spawn cc: {e}")))?;
    if !status.success() {
        return Err(Error::new("cc failed building runtime object"));
    }

    // ar rcs lib.a obj
    let status = Command::new("ar")
        .args(["rcs"])
        .arg(&lib)
        .arg(&obj)
        .status()
        .map_err(|e| Error::new(format!("failed to spawn ar: {e}")))?;
    if !status.success() {
        return Err(Error::new("ar failed creating libthoxa_rt.a"));
    }

    write_compile_commands(&root, &src, &include)?;
    Ok(())
}

/// Write a clangd-friendly `compile_commands.json` at the workspace root so
/// editors resolve `thoxa_rt.h` and system headers the same way `cc` does.
///
/// On NixOS, clangd does not see gcc's builtin include paths unless we copy
/// them into the compile command (or configure `--query-driver` in the editor).
fn write_compile_commands(root: &Path, src: &Path, include: &Path) -> Result<()> {
    let directory = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf());
    let file = src
        .canonicalize()
        .unwrap_or_else(|_| src.to_path_buf());
    let include = include
        .canonicalize()
        .unwrap_or_else(|_| include.to_path_buf());

    let mut args: Vec<String> = vec![
        "cc".into(),
        "-std=c11".into(),
        "-Wall".into(),
        "-Wextra".into(),
        "-I".into(),
        include.to_string_lossy().into_owned(),
    ];

    for sys in cc_system_include_dirs() {
        args.push("-isystem".into());
        args.push(sys);
    }

    args.push("-c".into());
    args.push(file.to_string_lossy().into_owned());

    let mut args_json = String::from("[\n");
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            args_json.push_str(",\n");
        }
        args_json.push_str(&format!("      \"{}\"", json_escape(a)));
    }
    args_json.push_str("\n    ]");

    let body = format!(
        r#"[
  {{
    "directory": "{}",
    "file": "{}",
    "arguments": {}
  }}
]
"#,
        json_escape(&directory.to_string_lossy()),
        json_escape(&file.to_string_lossy()),
        args_json
    );

    let path = root.join("compile_commands.json");
    fs::write(&path, body).map_err(|e| Error::new(e.to_string()))?;
    Ok(())
}

/// Parse `cc -E -Wp,-v -x c -` for the `#include <...>` search list.
fn cc_system_include_dirs() -> Vec<String> {
    let output = Command::new("cc")
        .args(["-E", "-Wp,-v", "-x", "c", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut dirs = Vec::new();
    let mut in_list = false;
    for line in stderr.lines() {
        if line.starts_with("#include <...> search starts here:") {
            in_list = true;
            continue;
        }
        if in_list {
            if line.starts_with("End of search list") {
                break;
            }
            let path = line.trim();
            if !path.is_empty() && Path::new(path).is_dir() {
                dirs.push(path.to_string());
            }
        }
    }
    dirs
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn emit_c(opts: &BuildOptions) -> Result<PathBuf> {
    let c_source = compile_to_c(&opts.source)?;
    let name = output_stem(opts);
    let c_dir = thoxa_target_dir().join("c");
    fs::create_dir_all(&c_dir).map_err(|e| Error::new(e.to_string()))?;
    let c_path = c_dir.join(format!("{name}.c"));
    fs::write(&c_path, c_source).map_err(|e| Error::new(e.to_string()))?;
    Ok(c_path)
}

pub fn build(opts: &BuildOptions) -> Result<BuildArtifacts> {
    let c_path = emit_c(opts)?;
    if opts.emit_c_only {
        return Ok(BuildArtifacts {
            c_path,
            bin_path: None,
        });
    }

    ensure_runtime()?;

    let name = output_stem(opts);
    let bin_dir = thoxa_target_dir().join("bin");
    fs::create_dir_all(&bin_dir).map_err(|e| Error::new(e.to_string()))?;
    let bin_path = bin_dir.join(&name);

    let status = Command::new("cc")
        .args(["-std=c11", "-O2"])
        .arg(&c_path)
        .arg("-I")
        .arg(runtime_include_dir())
        .arg(runtime_lib_path())
        .arg("-o")
        .arg(&bin_path)
        .status()
        .map_err(|e| Error::new(format!("failed to spawn cc: {e}")))?;
    if !status.success() {
        return Err(Error::new(format!(
            "cc failed linking {}",
            bin_path.display()
        )));
    }

    Ok(BuildArtifacts {
        c_path,
        bin_path: Some(bin_path),
    })
}

pub fn run(opts: &BuildOptions) -> Result<i32> {
    let artifacts = build(opts)?;
    let bin = artifacts
        .bin_path
        .ok_or_else(|| Error::new("no binary produced"))?;
    let status = Command::new(&bin)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| Error::new(format!("failed to execute {}: {e}", bin.display())))?;
    Ok(status.code().unwrap_or(1))
}

fn output_stem(opts: &BuildOptions) -> String {
    opts.output_name.clone().unwrap_or_else(|| {
        // Directory packages (`thoxa run pkg/`) → use the directory name so
        // multiple Main.thx packages do not collide on binary name "Main".
        // Canonicalize first so `.` / `..` yield a real basename (not `"out"`).
        if opts.source.is_dir() {
            let dir = fs::canonicalize(&opts.source).unwrap_or_else(|_| opts.source.clone());
            dir.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("out")
                .to_string()
        } else {
            opts.source
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("out")
                .to_string()
        }
    })
}
