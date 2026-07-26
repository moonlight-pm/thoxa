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
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Snapshot of PATH for host tools (`cc` / `ar`) before session `path()` replaces it.
/// Set once via [`capture_toolchain_path`]; consulted by [`host_tool`].
static TOOLCHAIN_PATH: OnceLock<String> = OnceLock::new();

/// Env var mirror of [`TOOLCHAIN_PATH`] so subprocesses / tests can inspect it.
pub const TOOLCHAIN_PATH_ENV: &str = "THOXA_TOOLCHAIN_PATH";

/// Capture the current process `PATH` once for later host-tool resolution.
/// Safe to call repeatedly; first non-empty PATH wins.
pub fn capture_toolchain_path() {
    if TOOLCHAIN_PATH.get().is_some() {
        return;
    }
    let path = std::env::var("PATH").unwrap_or_default();
    if path.is_empty() {
        // Fall back to env var if set (e.g. tests).
        if let Ok(p) = std::env::var(TOOLCHAIN_PATH_ENV) {
            let _ = TOOLCHAIN_PATH.set(p);
        }
        return;
    }
    let _ = TOOLCHAIN_PATH.set(path.clone());
    // Also publish for any external tooling; ignore if already set.
    if std::env::var_os(TOOLCHAIN_PATH_ENV).is_none() {
        std::env::set_var(TOOLCHAIN_PATH_ENV, &path);
    }
}

fn toolchain_search_path() -> String {
    if let Some(p) = TOOLCHAIN_PATH.get() {
        return p.clone();
    }
    if let Ok(p) = std::env::var(TOOLCHAIN_PATH_ENV) {
        return p;
    }
    std::env::var("PATH").unwrap_or_default()
}

/// Resolve `program` on the toolchain search path (absolute path if found).
fn resolve_host_tool(program: &str) -> PathBuf {
    let search = toolchain_search_path();
    for dir in search.split(':') {
        if dir.is_empty() {
            continue;
        }
        let cand = Path::new(dir).join(program);
        if cand.is_file() {
            return cand;
        }
    }
    PathBuf::from(program)
}

/// `Command` for a host build tool (`cc`, `ar`, …) that survives empty session PATH.
fn host_tool(program: &str) -> Command {
    Command::new(resolve_host_tool(program))
}

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

/// Cached once: host `cd` must not re-resolve the tree from the new cwd (REPL).
static WORKSPACE_ROOT: OnceLock<PathBuf> = OnceLock::new();

fn workspace_root() -> PathBuf {
    WORKSPACE_ROOT.get_or_init(discover_workspace_root).clone()
}

fn discover_workspace_root() -> PathBuf {
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

/// Force-resolve and cache the workspace root early (e.g. REPL start before `cd`).
pub fn pin_workspace_root() {
    let _ = workspace_root();
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

    // -fPIC so the archive can be linked into session step shared objects (D93).
    let status = host_tool("cc")
        .args(["-std=c11", "-O2", "-fPIC", "-c"])
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
    let status = host_tool("ar")
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

    let status = host_tool("cc")
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

// ── Session step shared libraries (D93 v2) ────────────────────────────────

use std::sync::atomic::{AtomicU64, Ordering};

static SESSION_CACHE_ID: AtomicU64 = AtomicU64::new(1);

/// Cache for session REPL builds: preamble object reused while decls are stable.
/// Each instance gets a unique id so parallel unit tests do not clobber objects.
#[derive(Debug)]
pub struct SessionBuildCache {
    /// Stable per-cache-instance id (file name prefix).
    id: u64,
    decls_key: u64,
    base_obj: Option<PathBuf>,
    /// Monotonic counter so we never overwrite a still-mapped .so.
    gen: u64,
}

impl Default for SessionBuildCache {
    fn default() -> Self {
        Self {
            id: SESSION_CACHE_ID.fetch_add(1, Ordering::Relaxed),
            decls_key: 0,
            base_obj: None,
            gen: 0,
        }
    }
}

/// Result of building a session step shared object.
#[derive(Debug)]
pub struct SessionStepLib {
    pub so_path: PathBuf,
}

fn hash_decls(decls: &[String]) -> u64 {
    let mut h = DefaultHasher::new();
    for d in decls {
        d.hash(&mut h);
    }
    h.finish()
}

/// Compile `decls + chunk` as a session script into a shared library exporting
/// `thoxa_session_step`. When `decls` are unchanged, reuses a cached preamble
/// object and only recompiles the `Shell.step` body.
pub fn build_session_step(
    decls: &[String],
    chunk: &str,
    cache: &mut SessionBuildCache,
) -> Result<SessionStepLib> {
    ensure_runtime()?;

    let dir = thoxa_target_dir().join("repl");
    fs::create_dir_all(&dir).map_err(|e| Error::new(e.to_string()))?;

    // Full source for MIR (session prepare needs stmts + decls together).
    let mut src = String::new();
    for d in decls {
        src.push_str(d);
        if !d.ends_with('\n') {
            src.push('\n');
        }
    }
    src.push_str(chunk);
    if !chunk.ends_with('\n') {
        src.push('\n');
    }

    let thx_path = dir.join(format!("chunk_{}.thx", cache.id));
    fs::write(&thx_path, &src).map_err(|e| Error::new(e.to_string()))?;

    let mut package = loader::load_package(&thx_path)?;
    crate::session::prepare_package(&mut package)?;
    let program = mir::lower_package(&package)?;

    if !program.functions.iter().any(|f| f.name == "Shell__step") {
        return Err(Error::new(
            "session step: lowered program has no Shell.step (internal)",
        ));
    }

    let decls_key = hash_decls(decls);
    let base_obj = dir.join(format!("session_base_{}.o", cache.id));
    let need_base = cache.base_obj.as_ref() != Some(&base_obj)
        || cache.decls_key != decls_key
        || !base_obj.is_file();

    if need_base {
        let base_c = cgen::emit_session_base(&program);
        let base_c_path = dir.join(format!("session_base_{}.c", cache.id));
        fs::write(&base_c_path, base_c).map_err(|e| Error::new(e.to_string()))?;
        compile_c_object(&base_c_path, &base_obj)?;
        cache.decls_key = decls_key;
        cache.base_obj = Some(base_obj.clone());
    }

    let step_c = cgen::emit_session_step(&program);
    let step_c_path = dir.join(format!("session_step_{}.c", cache.id));
    fs::write(&step_c_path, step_c).map_err(|e| Error::new(e.to_string()))?;
    let step_obj = dir.join(format!("session_step_{}.o", cache.id));
    compile_c_object(&step_c_path, &step_obj)?;

    cache.gen = cache.gen.wrapping_add(1);
    let so_path = dir.join(format!("session_step_{}_{}.so", cache.id, cache.gen));
    link_shared(&so_path, &[&step_obj, &base_obj])?;

    Ok(SessionStepLib { so_path })
}

fn compile_c_object(c_path: &Path, obj_path: &Path) -> Result<()> {
    let status = host_tool("cc")
        .args(["-std=c11", "-O0", "-fPIC", "-c"])
        .arg(c_path)
        .arg("-I")
        .arg(runtime_include_dir())
        .arg("-o")
        .arg(obj_path)
        .status()
        .map_err(|e| Error::new(format!("failed to spawn cc: {e}")))?;
    if !status.success() {
        return Err(Error::new(format!(
            "cc failed compiling {}",
            c_path.display()
        )));
    }
    Ok(())
}

fn link_shared(so_path: &Path, objs: &[&Path]) -> Result<()> {
    let mut cmd = host_tool("cc");
    cmd.args(["-std=c11", "-O0", "-shared", "-fPIC"]);
    for o in objs {
        cmd.arg(o);
    }
    cmd.arg(runtime_lib_path());
    cmd.arg("-o").arg(so_path);
    let status = cmd
        .status()
        .map_err(|e| Error::new(format!("failed to spawn cc (shared): {e}")))?;
    if !status.success() {
        return Err(Error::new(format!(
            "cc failed linking shared {}",
            so_path.display()
        )));
    }
    Ok(())
}
