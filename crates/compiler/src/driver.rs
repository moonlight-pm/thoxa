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

/// Resolved once per process.
static WORKSPACE_ROOT: OnceLock<PathBuf> = OnceLock::new();

fn workspace_root() -> PathBuf {
    WORKSPACE_ROOT.get_or_init(discover_workspace_root).clone()
}

fn discover_workspace_root() -> PathBuf {
    if let Ok(root) = std::env::var("THOXA_ROOT") {
        return PathBuf::from(root);
    }
    // Walk up looking for workspace Cargo.toml that lists crates/compiler: first
    // from the working directory, then from the binary itself, so an installed
    // `thoxa` (symlink into target/rust/…) works outside the repo.
    let starts = [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf)),
    ];
    for start in starts.into_iter().flatten() {
        let mut dir = start;
        loop {
            if dir.join("crates/compiler/Cargo.toml").is_file() {
                return dir;
            }
            if !dir.pop() {
                break;
            }
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
    let package = loader::load_package(source)?;
    let mut program = mir::lower_package(&package)?;
    mir::retain_reachable(&mut program);
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

fn compile_c_object(src: &Path, obj: &Path, includes: &[&Path], extra_defs: &[&str]) -> Result<()> {
    let mut cmd = Command::new("cc");
    cmd.args(["-std=c11", "-O2", "-fwrapv", "-fPIC", "-c"]);
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

    // -fPIC: toolchains that link position-independent executables by default
    // need it to link the archive.
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

    let status = Command::new("ar")
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
        SanitizeMode::None => &["-std=c11", "-O2", "-fwrapv"],
        SanitizeMode::Leak => &[
            "-std=c11",
            "-O1",
            "-fwrapv",
            "-g",
            "-fno-omit-frame-pointer",
            "-fsanitize=leak",
        ],
        SanitizeMode::Address => &[
            "-std=c11",
            "-O1",
            "-fwrapv",
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

    let status = Command::new("cc")
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
    // A program killed by a signal reports 128 + signal, as a shell does.
    use std::os::unix::process::ExitStatusExt;
    Ok(status
        .code()
        .unwrap_or_else(|| status.signal().map_or(1, |signal| 128 + signal)))
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

    /// Whether `/proc/<pid>/status` lists SIGTERM (15) among caught signals.
    fn catches_sigterm(pid: &str) -> bool {
        let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) else {
            return false;
        };
        status
            .lines()
            .find_map(|line| line.strip_prefix("SigCgt:"))
            .and_then(|mask| u64::from_str_radix(mask.trim(), 16).ok())
            .is_some_and(|mask| mask & (1 << (15 - 1)) != 0)
    }

    /// D170: the kernel drops a default-action SIGTERM sent to a PID
    /// namespace's init, so without the runtime's handler this `pause()`
    /// would never return and `podman stop` would wait for SIGKILL.
    #[test]
    fn program_as_pid_one_ends_on_sigterm() {
        let namespace = ["--user", "--map-root-user", "--pid", "--fork"];
        let probe = Command::new("unshare").args(namespace).arg("true").status();
        if !matches!(probe, Ok(status) if status.success()) {
            eprintln!("skipped: unshare cannot make a PID namespace here");
            return;
        }
        let dir = temp_prog(
            "extern fn sysPause() -> Int32 from \"c\" \"pause\"\n\n\
             fn main() -> Int64 {\n    sysPause()\n    0\n}\n",
        );
        let name = format!("pid_one_{}", std::process::id());
        let built = build(&opts_for(&dir, &name)).expect("build");
        let bin = built.bin_path.expect("binary");
        let mut outer = Command::new("unshare")
            .args(namespace)
            .arg(&bin)
            .spawn()
            .expect("spawn unshare");

        // The program is unshare's only child, and it is PID 1 inside.
        let children = format!("/proc/{0}/task/{0}/children", outer.id());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut program = None;
        while std::time::Instant::now() < deadline {
            program = fs::read_to_string(&children)
                .ok()
                .and_then(|text| text.split_whitespace().next().map(str::to_string));
            if program.as_deref().is_some_and(catches_sigterm) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let program = program.expect("unshare never started the program");
        if !catches_sigterm(&program) {
            let _ = Command::new("kill").args(["-KILL", &program]).status();
            let _ = outer.wait();
            panic!("as PID 1 the program did not catch SIGTERM");
        }

        let sent = Command::new("kill").args(["-TERM", &program]).status();
        assert!(sent.is_ok_and(|status| status.success()), "kill -TERM failed");
        let status = outer.wait().expect("wait for unshare");
        assert_eq!(status.code(), Some(128 + 15), "exit status after SIGTERM");
    }

    /// D173: what `main` returns is the exit status. 0 to 255 pass through;
    /// D197: the C holds only what `main` reaches. std/string is in every
    /// package; a program that calls `contains` carries that method and not
    /// `toUpper`, and a function nothing calls is gone.
    #[test]
    fn unreachable_functions_are_not_emitted() {
        let dir = temp_prog(
            "fn unused() -> Int64 {\n    7\n}\n\nfn main() -> Int64 {\n    if \"latte\".contains(\"at\") {\n        0\n    } else {\n        1\n    }\n}\n",
        );
        let c = compile_to_c(&dir.join("main.thx")).expect("compile");
        assert!(!c.contains("thoxa_fn_unused("), "unused function emitted");
        assert!(c.contains("_extend_String_contains("), "contains missing");
        assert!(!c.contains("_extend_String_toUpper("), "toUpper emitted");
    }

    /// a value the kernel would cut to its low 8 bits (256 reads as 0)
    /// panics instead.
    #[test]
    fn main_return_is_the_exit_status() {
        use std::os::unix::process::ExitStatusExt;
        let cases = [(0, Some(0)), (255, Some(255)), (256, None), (-1, None)];
        for (index, (returned, expected)) in cases.into_iter().enumerate() {
            let dir = temp_prog(&format!("fn main() -> Int64 {{\n    {returned}\n}}\n"));
            let name = format!("exit_status_{}_{index}", std::process::id());
            let built = build(&opts_for(&dir, &name)).expect("build");
            let output = Command::new(built.bin_path.expect("binary"))
                .output()
                .expect("run the program");
            let stderr = String::from_utf8_lossy(&output.stderr);
            match expected {
                Some(code) => {
                    assert_eq!(output.status.code(), Some(code), "main returned {returned}");
                    assert!(stderr.is_empty(), "main returned {returned}: {stderr}");
                }
                None => {
                    assert_eq!(output.status.signal(), Some(6), "main returned {returned}");
                    let message =
                        format!("thoxa panic: main returned {returned}; an exit status is 0 to 255");
                    assert!(stderr.contains(&message), "main returned {returned}: {stderr}");
                }
            }
        }
    }
}
