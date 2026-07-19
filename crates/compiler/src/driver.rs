//! Compile pipeline: source → C → binary under `targets/`.

use crate::cgen;
use crate::error::{Error, Result};
use crate::lexer::Lexer;
use crate::mir;
use crate::parser::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Workspace-relative paths resolved from cwd or `THOXA_ROOT`.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub source: PathBuf,
    /// Base name for outputs under targets/ (default: source stem).
    pub output_name: Option<String>,
    /// If set, also write the generated C here (in addition to targets/).
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

pub fn targets_dir() -> PathBuf {
    workspace_root().join("targets")
}

pub fn runtime_include_dir() -> PathBuf {
    workspace_root().join("runtime/include")
}

pub fn runtime_lib_path() -> PathBuf {
    targets_dir().join("runtime/libthoxa_rt.a")
}

pub fn compile_to_c(source: &Path) -> Result<String> {
    let src = fs::read_to_string(source).map_err(|e| {
        Error::new(format!("cannot read {}: {e}", source.display()))
    })?;
    let tokens = Lexer::new(&src).tokenize()?;
    let module = Parser::new(tokens).parse_module()?;
    let program = mir::lower_module(&module)?;
    Ok(cgen::emit_program(&program))
}

pub fn ensure_runtime() -> Result<()> {
    let lib = runtime_lib_path();
    if lib.is_file() {
        return Ok(());
    }
    build_runtime()?;
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
    let out_dir = targets_dir().join("runtime");
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
    Ok(())
}

pub fn emit_c(opts: &BuildOptions) -> Result<PathBuf> {
    let c_source = compile_to_c(&opts.source)?;
    let name = output_stem(opts);
    let c_dir = targets_dir().join("c");
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
    let bin_dir = targets_dir().join("bin");
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
        opts.source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("out")
            .to_string()
    })
}
