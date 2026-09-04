//! In-process libtcc bindings for session step compile (D105).
//!
//! Happy path: `tcc_compile_string` → `TCC_OUTPUT_MEMORY` → relocate →
//! `tcc_get_symbol`. The owning [`State`] must outlive any function pointers
//! taken from it (never call after `tcc_delete`).

use crate::error::{Error, Result};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Mutex;

const TCC_OUTPUT_MEMORY: c_int = 1;

#[repr(C)]
struct TCCState {
    _private: [u8; 0],
}

#[link(name = "tcc", kind = "static")]
extern "C" {
    fn tcc_new() -> *mut TCCState;
    fn tcc_delete(s: *mut TCCState);
    fn tcc_set_lib_path(s: *mut TCCState, path: *const c_char);
    fn tcc_set_error_func(
        s: *mut TCCState,
        error_opaque: *mut c_void,
        error_func: Option<extern "C" fn(*mut c_void, *const c_char)>,
    );
    fn tcc_add_include_path(s: *mut TCCState, pathname: *const c_char) -> c_int;
    fn tcc_add_library_path(s: *mut TCCState, pathname: *const c_char) -> c_int;
    fn tcc_set_output_type(s: *mut TCCState, output_type: c_int) -> c_int;
    fn tcc_compile_string(s: *mut TCCState, buf: *const c_char) -> c_int;
    fn tcc_add_file(s: *mut TCCState, filename: *const c_char) -> c_int;
    fn tcc_relocate(s: *mut TCCState) -> c_int;
    fn tcc_get_symbol(s: *mut TCCState, name: *const c_char) -> *mut c_void;
}

/// Directory containing `libtcc1.a` (from `build.rs`).
///
/// `THOXA_TCCDIR` overrides the compile-time prefix so a relocated install
/// (Oath store, prefix-dir) does not need the build machine's path.
pub fn tccdir() -> PathBuf {
    if let Ok(p) = std::env::var("THOXA_TCCDIR") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from(env!("THOXA_TCCDIR"))
}

/// Extra `-I` dirs from `THOXA_C_INCLUDE` (colon-separated).
pub fn extra_include_dirs() -> Vec<PathBuf> {
    colon_dirs("THOXA_C_INCLUDE")
}

/// Extra `-L` dirs from `THOXA_TCC_LIBPATHS` (colon-separated).
pub fn extra_lib_dirs() -> Vec<PathBuf> {
    colon_dirs("THOXA_TCC_LIBPATHS")
}

fn colon_dirs(var: &str) -> Vec<PathBuf> {
    let Ok(raw) = std::env::var(var) else {
        return Vec::new();
    };
    raw.split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Directory of the libc this process is already using (`/proc/self/maps`).
fn libc_dir_from_maps() -> Option<PathBuf> {
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    for line in maps.lines() {
        let Some(path) = line.split_whitespace().nth(5) else {
            continue;
        };
        if path.ends_with("/libc.so.6") || path.ends_with("/libc.so") {
            return Path::new(path).parent().map(Path::to_path_buf);
        }
    }
    None
}

fn host_lib_dirs() -> Vec<PathBuf> {
    let mut dirs = extra_lib_dirs();
    let push = |dirs: &mut Vec<PathBuf>, d: PathBuf| {
        if d.is_dir() && !dirs.iter().any(|x| x == &d) {
            dirs.push(d);
        }
    };
    if let Some(d) = libc_dir_from_maps() {
        push(&mut dirs, d);
    }
    push(&mut dirs, PathBuf::from("/oath/store/pkg/glibc/lib"));
    push(&mut dirs, tccdir());
    dirs
}

struct ErrorBuf {
    messages: Mutex<Vec<String>>,
}

extern "C" fn collect_error(opaque: *mut c_void, msg: *const c_char) {
    if opaque.is_null() || msg.is_null() {
        return;
    }
    let buf = unsafe { &*(opaque as *const ErrorBuf) };
    let s = unsafe { CStr::from_ptr(msg) }
        .to_string_lossy()
        .into_owned();
    if let Ok(mut v) = buf.messages.lock() {
        v.push(s);
    }
}

/// Owned libtcc compilation context. Drop invalidates all symbols from it.
pub struct State {
    ptr: *mut TCCState,
    /// Keep error buffer alive for the lifetime of the state.
    _errors: Box<ErrorBuf>,
}

// Single-threaded REPL; TCCState is not Sync.
unsafe impl Send for State {}

impl Drop for State {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { tcc_delete(self.ptr) };
            self.ptr = ptr::null_mut();
        }
    }
}

impl State {
    fn new() -> Result<Self> {
        let errors = Box::new(ErrorBuf {
            messages: Mutex::new(Vec::new()),
        });
        let ptr = unsafe { tcc_new() };
        if ptr.is_null() {
            return Err(Error::new("tcc_new failed"));
        }
        let opaque = (&*errors as *const ErrorBuf) as *mut c_void;
        unsafe {
            tcc_set_error_func(ptr, opaque, Some(collect_error));
            let lib = path_c(&tccdir())?;
            tcc_set_lib_path(ptr, lib.as_ptr());
            if tcc_set_output_type(ptr, TCC_OUTPUT_MEMORY) < 0 {
                tcc_delete(ptr);
                return Err(Error::new("tcc_set_output_type(MEMORY) failed"));
            }
        }
        Ok(Self {
            ptr,
            _errors: errors,
        })
    }

    fn take_errors(&self) -> String {
        self._errors
            .messages
            .lock()
            .map(|mut v| {
                let s = v.join("; ");
                v.clear();
                s
            })
            .unwrap_or_default()
    }

    fn err_ctx(&self, what: &str) -> Error {
        let detail = self.take_errors();
        if detail.is_empty() {
            Error::new(what)
        } else {
            Error::new(format!("{what}: {detail}"))
        }
    }

    /// Add a C include search path (`-I`).
    pub fn add_include_path(&self, path: &Path) -> Result<()> {
        let c = path_c(path)?;
        let rc = unsafe { tcc_add_include_path(self.ptr, c.as_ptr()) };
        if rc < 0 {
            return Err(self.err_ctx(&format!("tcc_add_include_path {}", path.display())));
        }
        Ok(())
    }

    /// Add a library search path (`-L`).
    pub fn add_library_path(&self, path: &Path) -> Result<()> {
        let c = path_c(path)?;
        let rc = unsafe { tcc_add_library_path(self.ptr, c.as_ptr()) };
        if rc < 0 {
            return Err(self.err_ctx(&format!("tcc_add_library_path {}", path.display())));
        }
        Ok(())
    }

    /// Compile a C source string. Call before [`add_file`](Self::add_file) of archives.
    pub fn compile_string(&self, src: &str) -> Result<()> {
        let c = CString::new(src).map_err(|_| Error::new("C source contains NUL"))?;
        let rc = unsafe { tcc_compile_string(self.ptr, c.as_ptr()) };
        if rc < 0 {
            return Err(self.err_ctx("tcc_compile_string failed"));
        }
        Ok(())
    }

    /// Add an object or archive (e.g. `libthoxa_rt.a`). Prefer **after** compile strings.
    pub fn add_file(&self, path: &Path) -> Result<()> {
        let c = path_c(path)?;
        let rc = unsafe { tcc_add_file(self.ptr, c.as_ptr()) };
        if rc < 0 {
            return Err(self.err_ctx(&format!("tcc_add_file {}", path.display())));
        }
        Ok(())
    }

    /// Relocate for in-memory execution. Required before [`symbol`](Self::symbol).
    pub fn relocate(&self) -> Result<()> {
        let rc = unsafe { tcc_relocate(self.ptr) };
        if rc < 0 {
            return Err(self.err_ctx("tcc_relocate failed"));
        }
        Ok(())
    }

    /// Look up a global symbol. Pointer is valid while `self` lives.
    pub fn symbol(&self, name: &str) -> Result<*mut c_void> {
        let c = CString::new(name).map_err(|_| Error::new("symbol name contains NUL"))?;
        let p = unsafe { tcc_get_symbol(self.ptr, c.as_ptr()) };
        if p.is_null() {
            return Err(Error::new(format!("tcc symbol `{name}` not found")));
        }
        Ok(p)
    }
}

fn path_c(path: &Path) -> Result<CString> {
    CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| Error::new(format!("path contains NUL: {}", path.display())))
}

/// Options for compiling session (or smoke) C into a memory image.
#[derive(Debug, Clone)]
pub struct CompileOptions<'a> {
    pub include_paths: Vec<&'a Path>,
    /// Archives/objects added **after** source strings (e.g. `libthoxa_rt.a`).
    pub link_files: Vec<&'a Path>,
    pub sources: Vec<&'a str>,
}

/// Compile C sources to an in-memory image. Keeps [`State`] so symbols stay valid.
pub fn compile_memory(opts: CompileOptions<'_>) -> Result<State> {
    let state = State::new()?;
    for inc in &opts.include_paths {
        state.add_include_path(inc)?;
    }
    let extra = extra_include_dirs();
    for inc in &extra {
        if inc.is_dir() {
            state.add_include_path(inc)?;
        }
    }
    for dir in host_lib_dirs() {
        if dir.is_dir() {
            state.add_library_path(&dir)?;
        }
    }
    for src in &opts.sources {
        state.compile_string(src)?;
    }
    for f in &opts.link_files {
        state.add_file(f)?;
    }
    state.relocate()?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_compile_returns_42() {
        let state = compile_memory(CompileOptions {
            include_paths: vec![],
            link_files: vec![],
            sources: vec!["int thoxa_tcc_smoke_f(void){return 42;}\n"],
        })
        .expect("compile");
        let p = state.symbol("thoxa_tcc_smoke_f").expect("symbol");
        let f: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(p) };
        let v = unsafe { f() };
        assert_eq!(v, 42);
    }

    #[test]
    fn memory_compile_links_libc() {
        let state = compile_memory(CompileOptions {
            include_paths: vec![],
            link_files: vec![],
            sources: vec!["extern int abs(int); int thoxa_tcc_libc_f(void){return abs(-2);}\n"],
        })
        .expect("compile+link libc");
        let p = state.symbol("thoxa_tcc_libc_f").expect("symbol");
        let f: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(p) };
        let v = unsafe { f() };
        assert_eq!(v, 2);
    }

    #[test]
    fn memory_compile_stdint() {
        let state = compile_memory(CompileOptions {
            include_paths: vec![],
            link_files: vec![],
            sources: vec![
                "#include <stdint.h>\nint thoxa_tcc_stdint_f(void){return (int)sizeof(int32_t);}\n",
            ],
        })
        .expect("compile stdint.h");
        let p = state.symbol("thoxa_tcc_stdint_f").expect("symbol");
        let f: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(p) };
        let v = unsafe { f() };
        assert_eq!(v, 4);
    }
}
