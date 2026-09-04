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
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

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
    /// Args after the source path, forwarded to the program (`fn main(arguments)`).
    pub program_args: Vec<String>,
}

pub struct BuildArtifacts {
    pub c_path: PathBuf,
    pub bin_path: Option<PathBuf>,
    /// True when `build` reused an existing binary (inputs unchanged).
    pub cached: bool,
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

fn runtime_inputs(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("runtime/src/thoxa_rt.c"),
        root.join("runtime/src/thoxa_sql.c"),
        root.join("runtime/include/thoxa_rt.h"),
        root.join("vendor/sqlite/sqlite3.c"),
        root.join("vendor/sqlite/sqlite3.h"),
    ]
}

fn runtime_is_stale(lib: &Path, inputs: &[PathBuf]) -> bool {
    let Ok(lib_m) = lib.metadata() else {
        return true;
    };
    let Some(lib_t) = lib_m.modified().ok() else {
        return true;
    };
    inputs
        .iter()
        .any(|p| match p.metadata().and_then(|m| m.modified()) {
            Ok(t) => lib_t < t,
            Err(_) => true,
        })
}

pub fn ensure_runtime() -> Result<()> {
    // Rebuild when the library is missing or older than runtime / sqlite sources.
    // Packed installs ship `libthoxa_rt.a` without C sources / `cc` — keep the archive.
    let lib = runtime_lib_path();
    let root = workspace_root();
    let inputs = runtime_inputs(&root);
    let can_rebuild = inputs.iter().all(|p| p.is_file());
    if can_rebuild && runtime_is_stale(&lib, &inputs) {
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

/// Writable dir for REPL chunk `.thx` (store `THOXA_ROOT` is root-owned on Oath).
fn session_scratch_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("thoxa/repl");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".cache/thoxa/repl");
        }
    }
    std::env::temp_dir().join(format!("thoxa-repl-{}", std::process::id()))
}

fn compile_c_object(src: &Path, obj: &Path, includes: &[&Path], extra_defs: &[&str]) -> Result<()> {
    let mut cmd = host_tool("cc");
    cmd.args(["-std=c11", "-O2", "-fPIC", "-c"]);
    for def in extra_defs {
        cmd.arg(def);
    }
    cmd.arg(src);
    for inc in includes {
        cmd.arg("-I").arg(inc);
    }
    cmd.arg("-o").arg(obj);
    let status = cmd
        .status()
        .map_err(|e| Error::new(format!("failed to spawn cc: {e}")))?;
    if !status.success() {
        return Err(Error::new(format!("cc failed building {}", src.display())));
    }
    Ok(())
}

pub fn build_runtime() -> Result<()> {
    let root = workspace_root();
    let out_dir = c_target_dir();
    fs::create_dir_all(&out_dir).map_err(|e| Error::new(e.to_string()))?;

    let include = root.join("runtime/include");
    let sqlite_inc = root.join("vendor/sqlite");
    let rt_src = root.join("runtime/src/thoxa_rt.c");
    let sql_src = root.join("runtime/src/thoxa_sql.c");
    let sqlite_src = root.join("vendor/sqlite/sqlite3.c");
    let rt_obj = out_dir.join("thoxa_rt.o");
    let sql_obj = out_dir.join("thoxa_sql.o");
    let sqlite_obj = out_dir.join("sqlite3.o");
    let lib = out_dir.join("libthoxa_rt.a");

    // -fPIC so the archive can be linked into session step images (D93/D105).
    compile_c_object(&rt_src, &rt_obj, &[&include], &[])?;
    compile_c_object(&sql_src, &sql_obj, &[&include, &sqlite_inc], &[])?;
    compile_c_object(
        &sqlite_src,
        &sqlite_obj,
        &[&sqlite_inc],
        &[
            "-DSQLITE_THREADSAFE=0",
            "-DSQLITE_OMIT_LOAD_EXTENSION=1",
            "-DSQLITE_DQS=0",
            "-DSQLITE_DEFAULT_MEMSTATUS=0",
        ],
    )?;

    let status = host_tool("ar")
        .args(["rcs"])
        .arg(&lib)
        .arg(&rt_obj)
        .arg(&sql_obj)
        .arg(&sqlite_obj)
        .status()
        .map_err(|e| Error::new(format!("failed to spawn ar: {e}")))?;
    if !status.success() {
        return Err(Error::new("ar failed creating libthoxa_rt.a"));
    }

    write_compile_commands(&root, &rt_src, &include)?;
    Ok(())
}

/// Write a clangd-friendly `compile_commands.json` at the workspace root so
/// editors resolve `thoxa_rt.h` and system headers the same way `cc` does.
///
/// On NixOS, clangd does not see gcc's builtin include paths unless we copy
/// them into the compile command (or configure `--query-driver` in the editor).
fn write_compile_commands(root: &Path, src: &Path, include: &Path) -> Result<()> {
    let directory = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let file = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
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

/// Set to `1` / `true` / `yes` to ignore the program-binary stamp.
pub const FORCE_BUILD_ENV: &str = "THOXA_FORCE_BUILD";

/// Batch `cc` sanitizer: `leak` (LeakSanitizer) or `address` (ASan, includes LSan).
/// Session/libtcc steps ignore this (in-process tcc cannot link liblsan).
pub const SANITIZE_ENV: &str = "THOXA_SANITIZE";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SanitizeMode {
    None,
    Leak,
    Address,
}

fn sanitize_mode() -> Result<SanitizeMode> {
    match std::env::var(SANITIZE_ENV) {
        Err(_) => Ok(SanitizeMode::None),
        Ok(raw) => {
            let value = raw.trim();
            if value.is_empty() || value == "0" || value.eq_ignore_ascii_case("none") {
                Ok(SanitizeMode::None)
            } else if value.eq_ignore_ascii_case("leak") || value.eq_ignore_ascii_case("lsan") {
                Ok(SanitizeMode::Leak)
            } else if value.eq_ignore_ascii_case("address") || value.eq_ignore_ascii_case("asan") {
                Ok(SanitizeMode::Address)
            } else {
                Err(Error::new(format!(
                    "unknown {SANITIZE_ENV}={raw:?} (use leak, address, or unset)"
                )))
            }
        }
    }
}

fn sanitize_bin_dir_name(mode: SanitizeMode) -> &'static str {
    match mode {
        SanitizeMode::None => "bin",
        SanitizeMode::Leak => "bin-leak",
        SanitizeMode::Address => "bin-address",
    }
}

fn sanitize_cc_args(mode: SanitizeMode) -> &'static [&'static str] {
    match mode {
        SanitizeMode::None => &["-std=c11", "-O2"],
        SanitizeMode::Leak => &[
            "-std=c11",
            "-O1",
            "-g",
            "-fno-omit-frame-pointer",
            "-fsanitize=leak",
        ],
        SanitizeMode::Address => &[
            "-std=c11",
            "-O1",
            "-g",
            "-fno-omit-frame-pointer",
            "-fsanitize=address",
        ],
    }
}

fn force_build() -> bool {
    matches!(
        std::env::var(FORCE_BUILD_ENV).as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn stamp_path(bin: &Path) -> PathBuf {
    let mut p = bin.as_os_str().to_os_string();
    p.push(".stamp");
    PathBuf::from(p)
}

fn collect_thx(root: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 24 {
        return;
    }
    if root.is_file() {
        if root.extension().and_then(|e| e.to_str()) == Some("thx") {
            out.push(root.to_path_buf());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for ent in entries.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        let path = ent.path();
        if path.is_dir() {
            collect_thx(&path, out, depth + 1);
        } else if path.extension().and_then(|e| e.to_str()) == Some("thx") {
            out.push(path);
        }
    }
}

fn package_watch_root(opts: &BuildOptions) -> PathBuf {
    if opts.source.is_dir() {
        return opts.source.clone();
    }
    match loader::resolve_entry(&opts.source) {
        Ok(entry) => loader::package_root_for(&entry),
        Err(_) => opts
            .source
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| opts.source.clone()),
    }
}

/// Identity of everything that can change the linked program: package `.thx`,
/// product `std/`, `libthoxa_rt.a`, and the running `thoxa` binary.
fn input_fingerprint(opts: &BuildOptions) -> String {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_thx(&package_watch_root(opts), &mut files, 0);
    let std_root = loader::std_lib_root().ok();
    if let Some(std) = &std_root {
        collect_thx(std, &mut files, 0);
    }
    files.push(runtime_lib_path());
    if let Ok(exe) = std::env::current_exe() {
        files.push(exe);
    }
    files.sort();
    files.dedup();

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    "v2".hash(&mut hasher);
    match sanitize_mode() {
        Ok(SanitizeMode::None) => "sanitize:none".hash(&mut hasher),
        Ok(SanitizeMode::Leak) => "sanitize:leak".hash(&mut hasher),
        Ok(SanitizeMode::Address) => "sanitize:address".hash(&mut hasher),
        Err(_) => "sanitize:invalid".hash(&mut hasher),
    }
    opts.source.hash(&mut hasher);
    std_root.hash(&mut hasher);
    for f in files {
        f.hash(&mut hasher);
        match fs::metadata(&f) {
            Ok(m) => {
                m.len().hash(&mut hasher);
                let ns = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                ns.hash(&mut hasher);
            }
            Err(_) => {
                "missing".hash(&mut hasher);
            }
        }
    }
    format!("{:016x}", hasher.finish())
}

fn is_fresh(opts: &BuildOptions, bin: &Path) -> bool {
    if force_build() || !bin.is_file() {
        return false;
    }
    let Ok(prev) = fs::read_to_string(stamp_path(bin)) else {
        return false;
    };
    prev.trim() == input_fingerprint(opts)
}

fn write_stamp(opts: &BuildOptions, bin: &Path) {
    let _ = fs::write(stamp_path(bin), input_fingerprint(opts));
}

pub fn build(opts: &BuildOptions) -> Result<BuildArtifacts> {
    let sanitize = sanitize_mode()?;
    if !opts.emit_c_only {
        ensure_runtime()?;
        let name = output_stem(opts);
        let bin_dir = thoxa_target_dir().join(sanitize_bin_dir_name(sanitize));
        let bin_path = bin_dir.join(&name);
        let c_path = thoxa_target_dir().join("c").join(format!("{name}.c"));
        if is_fresh(opts, &bin_path) {
            return Ok(BuildArtifacts {
                c_path,
                bin_path: Some(bin_path),
                cached: true,
            });
        }
    }

    let c_path = emit_c(opts)?;
    if opts.emit_c_only {
        return Ok(BuildArtifacts {
            c_path,
            bin_path: None,
            cached: false,
        });
    }

    ensure_runtime()?;

    let name = output_stem(opts);
    let bin_dir = thoxa_target_dir().join(sanitize_bin_dir_name(sanitize));
    fs::create_dir_all(&bin_dir).map_err(|e| Error::new(e.to_string()))?;
    let bin_path = bin_dir.join(&name);

    let status = host_tool("cc")
        .args(sanitize_cc_args(sanitize))
        .arg(&c_path)
        .arg("-I")
        .arg(runtime_include_dir())
        .arg(runtime_lib_path())
        .arg("-lm")
        .arg("-o")
        .arg(&bin_path)
        .status()
        .map_err(|e| Error::new(format!("failed to spawn cc: {e}")))?;
    if !status.success() {
        let hint = match sanitize {
            SanitizeMode::None => String::new(),
            SanitizeMode::Leak => {
                format!(" ({SANITIZE_ENV}=leak needs GCC/Clang liblsan)")
            }
            SanitizeMode::Address => {
                format!(" ({SANITIZE_ENV}=address needs GCC/Clang libasan)")
            }
        };
        return Err(Error::new(format!(
            "cc failed linking {}{hint}",
            bin_path.display()
        )));
    }

    write_stamp(opts, &bin_path);

    Ok(BuildArtifacts {
        c_path,
        bin_path: Some(bin_path),
        cached: false,
    })
}

pub fn run(opts: &BuildOptions) -> Result<i32> {
    let artifacts = build(opts)?;
    let bin = artifacts
        .bin_path
        .ok_or_else(|| Error::new("no binary produced"))?;
    let status = Command::new(&bin)
        .args(&opts.program_args)
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
        // multiple main.thx packages do not collide on binary name "Main".
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

// ── Session step (D105: libtcc only) ─────────────────────────────────────

use std::sync::atomic::{AtomicU64, Ordering};

static SESSION_CACHE_ID: AtomicU64 = AtomicU64::new(1);

/// Env: write generated session C under `target/thoxa/repl/` for inspection.
pub const REPL_KEEP_C_ENV: &str = "THOXA_REPL_KEEP_C";

/// Cache for session REPL builds. Instance id keeps parallel tests from
/// clobbering debug C dumps / base objects; `gen` names keep-C outputs.
///
/// When decls are unchanged, the prepared package and libtcc **base object**
/// are reused; only `Shell.step` is re-lowered and recompiled (D105 §3.5).
#[derive(Debug)]
pub struct SessionBuildCache {
    /// Stable per-cache-instance id (file name prefix).
    id: u64,
    /// Monotonic counter for optional keep-C dumps.
    gen: u64,
    /// Joined decl sources last used to build [`prepared`](Self::prepared).
    decls_key: Option<String>,
    /// Package after session prepare (Shell injected). Step body is replaced
    /// per chunk; do not mutate this in place.
    prepared: Option<loader::Package>,
    /// MIR + maps from the last decls lower (re-lower `Shell.step` only).
    mir: Option<mir::SessionMirCache>,
    /// Relocatable object of `emit_session_base` for the current decls.
    base_object: Option<PathBuf>,
}

impl Default for SessionBuildCache {
    fn default() -> Self {
        Self {
            id: SESSION_CACHE_ID.fetch_add(1, Ordering::Relaxed),
            gen: 0,
            decls_key: None,
            prepared: None,
            mir: None,
            base_object: None,
        }
    }
}

impl Drop for SessionBuildCache {
    fn drop(&mut self) {
        if let Some(path) = self.base_object.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn decls_cache_key(decls: &[String]) -> String {
    decls.join("\n\u{1e}\n")
}

fn invalidate_session_base(cache: &mut SessionBuildCache) {
    cache.decls_key = None;
    cache.prepared = None;
    cache.mir = None;
    if let Some(path) = cache.base_object.take() {
        let _ = fs::remove_file(path);
    }
}

/// C ABI mirror of `ThoxaShellState` (also in `repl`).
#[repr(C)]
struct ThoxaShellState {
    status: i64,
}

type SessionStepFn = unsafe extern "C" fn(*mut ThoxaShellState) -> i64;

/// Built session step ready to call. Owns the libtcc image (D105).
pub struct SessionStepLib {
    /// Must outlive `step`.
    #[cfg(thoxa_libtcc)]
    _tcc: crate::tcc::State,
    step: SessionStepFn,
}

// REPL is single-threaded; state is process-local.
unsafe impl Send for SessionStepLib {}

impl SessionStepLib {
    /// Invoke `thoxa_session_step` with host-held status; returns new status.
    pub fn call(&self, prev_status: i32) -> i32 {
        let mut state = ThoxaShellState {
            status: prev_status as i64,
        };
        unsafe { (self.step)(&mut state) as i32 }
    }
}

/// Compile `decls + chunk` as a session script exporting `thoxa_session_step`.
///
/// D105: libtcc in-memory — no compiler spawn, no step `.so`. Requires the
/// vendored TinyCC build (`thoxa_libtcc`, Linux x86_64).
///
/// When `decls` match the last successful compile, the prepared package and
/// base object are reused; only this chunk’s `Shell.step` is re-lowered.
pub fn build_session_step(
    decls: &[String],
    chunk: &str,
    cache: &mut SessionBuildCache,
) -> Result<SessionStepLib> {
    ensure_runtime()?;

    let profile = std::env::var_os("THOXA_REPL_PROFILE").is_some();
    let started = std::time::Instant::now();
    let key = decls_cache_key(decls);
    let program = if cache.decls_key.as_deref() == Some(key.as_str()) {
        if let Some(prepared) = cache.prepared.clone() {
            match crate::session::parse_session_chunk_stmts(chunk) {
                Ok(stmts) => {
                    let mut package = prepared;
                    crate::session::replace_step_body(&mut package, stmts)?;
                    if let Some(mir_cache) = &cache.mir {
                        match mir::relower_session_step(mir_cache, &package) {
                            Ok(program) => program,
                            Err(_) => {
                                if let Some(path) = cache.base_object.take() {
                                    let _ = fs::remove_file(path);
                                }
                                lower_session_package_fresh(decls, chunk, cache)?
                            }
                        }
                    } else {
                        lower_session_package_fresh(decls, chunk, cache)?
                    }
                }
                Err(_) => {
                    // Chunk is not statement-only (path-hook leftovers, mixed
                    // lets, …). Rebuild the package; keep going.
                    invalidate_session_base(cache);
                    lower_session_package_fresh(decls, chunk, cache)?
                }
            }
        } else {
            lower_session_package_fresh(decls, chunk, cache)?
        }
    } else {
        invalidate_session_base(cache);
        lower_session_package_fresh(decls, chunk, cache)?
    };

    if !program.functions.iter().any(|f| f.name == "Shell__step") {
        return Err(Error::new(
            "session step: lowered program has no Shell.step (internal)",
        ));
    }
    if profile {
        eprintln!("thoxa profile: frontend {:?}", started.elapsed());
    }
    let lib = build_session_step_libtcc(&program, cache)?;
    if profile {
        eprintln!("thoxa profile: total {:?}", started.elapsed());
    }
    Ok(lib)
}

fn concat_session_source(decls: &[String], chunk: &str) -> String {
    let mut src = String::new();
    for decl in decls {
        src.push_str(decl);
        if !decl.ends_with('\n') {
            src.push('\n');
        }
    }
    src.push_str(chunk);
    if !chunk.ends_with('\n') {
        src.push('\n');
    }
    src
}

fn lower_session_package_fresh(
    decls: &[String],
    chunk: &str,
    cache: &mut SessionBuildCache,
) -> Result<mir::Program> {
    let src = concat_session_source(decls, chunk);
    let ast = crate::session::parse_session_source(&src)?;
    let entry = crate::session::virtual_repl_entry();
    let mut package = loader::load_package_virtual(&entry, ast)?;
    crate::session::prepare_package(&mut package)?;
    let mir_cache = mir::lower_session_repl(&package)?;
    let program = mir_cache.program.clone();
    cache.decls_key = Some(decls_cache_key(decls));
    cache.prepared = Some(package);
    cache.mir = Some(mir_cache);
    Ok(program)
}

#[cfg(thoxa_libtcc)]
fn build_session_step_libtcc(
    program: &crate::mir::Program,
    cache: &mut SessionBuildCache,
) -> Result<SessionStepLib> {
    let step_c = cgen::emit_session_step(program);
    cache.gen = cache.gen.wrapping_add(1);

    let dir = session_scratch_dir();
    if keep_session_c() {
        fs::create_dir_all(&dir).map_err(|e| Error::new(e.to_string()))?;
        let step_path = dir.join(format!("session_step_{}_{}.c", cache.id, cache.gen));
        fs::write(&step_path, &step_c).map_err(|e| Error::new(e.to_string()))?;
    }

    let include = runtime_include_dir();
    let rt = runtime_lib_path();
    let base_obj = ensure_session_base_object(program, cache, &include)?;
    let tcc_started = std::time::Instant::now();
    let tcc = crate::tcc::compile_memory(crate::tcc::CompileOptions {
        include_paths: vec![include.as_path()],
        link_files: vec![base_obj.as_path(), rt.as_path()],
        sources: vec![&step_c],
    })?;
    if std::env::var_os("THOXA_REPL_PROFILE").is_some() {
        eprintln!(
            "thoxa profile: step-c {}B tcc {:?}",
            step_c.len(),
            tcc_started.elapsed()
        );
    }
    let sym = tcc.symbol("thoxa_session_step")?;
    let step: SessionStepFn = unsafe { std::mem::transmute(sym) };
    Ok(SessionStepLib { _tcc: tcc, step })
}

#[cfg(thoxa_libtcc)]
fn ensure_session_base_object(
    program: &crate::mir::Program,
    cache: &mut SessionBuildCache,
    include: &Path,
) -> Result<PathBuf> {
    if let Some(path) = &cache.base_object {
        if path.is_file() {
            return Ok(path.clone());
        }
    }
    let base_c = cgen::emit_session_base(program);
    let dir = session_scratch_dir();
    fs::create_dir_all(&dir).map_err(|e| Error::new(e.to_string()))?;
    if keep_session_c() {
        let base_path = dir.join(format!("session_base_{}_{}.c", cache.id, cache.gen));
        fs::write(&base_path, &base_c).map_err(|e| Error::new(e.to_string()))?;
    }
    let obj = dir.join(format!("session_base_{}.o", cache.id));
    crate::tcc::compile_object_file(
        crate::tcc::CompileOptions {
            include_paths: vec![include],
            link_files: vec![],
            sources: vec![&base_c],
        },
        &obj,
    )?;
    cache.base_object = Some(obj.clone());
    Ok(obj)
}

#[cfg(not(thoxa_libtcc))]
fn build_session_step_libtcc(
    _program: &crate::mir::Program,
    _cache: &mut SessionBuildCache,
) -> Result<SessionStepLib> {
    Err(Error::new(
        "session steps require vendored libtcc (Linux x86_64 product track; D105)",
    ))
}

fn keep_session_c() -> bool {
    matches!(
        std::env::var(REPL_KEEP_C_ENV).as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    static FRESH_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_prog(body: &str) -> PathBuf {
        let id = FRESH_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("thoxa_fresh_{}_{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.thx"), body).unwrap();
        dir
    }

    fn opts_for(dir: &Path, name: &str) -> BuildOptions {
        BuildOptions {
            source: dir.to_path_buf(),
            output_name: Some(name.into()),
            emit_c_only: false,
            program_args: vec![],
        }
    }

    #[test]
    fn build_reuses_binary_when_inputs_unchanged() {
        let dir = temp_prog("fn main() -> Int64 { 0 }\n");
        let name = format!("fresh_{}", std::process::id());
        let opts = opts_for(&dir, &name);
        let first = build(&opts).expect("first build");
        assert!(!first.cached, "first build must compile");
        let bin = first.bin_path.as_ref().unwrap().clone();
        let m1 = fs::metadata(&bin).unwrap().modified().unwrap();

        let second = build(&opts).expect("second build");
        assert!(second.cached, "unchanged inputs should reuse the binary");
        let m2 = fs::metadata(second.bin_path.as_ref().unwrap())
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(m1, m2, "cached build must not rewrite the binary");
    }

    #[test]
    fn build_rebuilds_when_source_changes() {
        let dir = temp_prog("fn main() -> Int64 { 0 }\n");
        let name = format!("fresh_chg_{}", std::process::id());
        let opts = opts_for(&dir, &name);
        let first = build(&opts).expect("first build");
        assert!(!first.cached);

        std::thread::sleep(Duration::from_millis(20));
        fs::write(dir.join("main.thx"), "fn main() -> Int64 { 1 }\n").unwrap();

        let second = build(&opts).expect("rebuild after edit");
        assert!(!second.cached, "edited source must invalidate the stamp");
    }
}
