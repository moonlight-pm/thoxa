//! Multi-file module loader (R8).
//!
//! Specifiers:
//! - `./…` relative to the importing file's directory (no `..` segments)
//! - `~/…` relative to the project root (nearest ancestor `main.thx`, else
//!   the entry file's directory)
//! - `std/…` product standard library (D97) — `THOXA_STD`, else `{THOXA_ROOT|workspace}/std`
//! - bare `ffi` / `ffi/…` — still deferred

use crate::ast::{self, Import, ImportKind, Item};
use crate::error::{Error, Result};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::span::Span;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Directory-package entry file (D68 / D138).
pub const ENTRY_FILE: &str = "main.thx";

/// A parsed source module with identity and path.
#[derive(Debug, Clone)]
pub struct SourceModule {
    /// Module id for C mangling (D35 / D138).
    ///
    /// Entry: file stem (symbols stay unprefixed). Library: path-qualified
    /// kebab-case path, sanitized (`std/http-server` → `std_http_server`).
    pub id: String,
    pub path: PathBuf,
    pub ast: ast::Module,
    pub is_entry: bool,
}

/// Fully loaded package: entry + all transitive imports (dependency order).
#[derive(Debug, Clone)]
pub struct Package {
    pub project_root: PathBuf,
    /// Modules in dependency order (callees before callers); entry last.
    pub modules: Vec<SourceModule>,
}

/// Directory that `~/…` resolves against for `entry_file`.
///
/// D31/D68: normally the entry file's parent. When the file sits under a
/// directory package (nearest ancestor `main.thx`), that directory is the root
/// so `thoxa check` / LSP on `app/pages.thx` still finds `~/framework/wire`.
/// No `main.thx` ancestor → file parent (lone-file / flat conformance).
/// Does **not** stop at `.git` (the repo root is not a Thoxa package).
pub fn package_root_for(entry_file: &Path) -> PathBuf {
    let start = entry_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut dir = start.clone();
    for _ in 0..24 {
        if dir.join(ENTRY_FILE).is_file() {
            return dir;
        }
        match dir.parent() {
            Some(p) if p != dir => dir = p.to_path_buf(),
            _ => break,
        }
    }
    start
}

/// Resolve a CLI path to the entry `.thx` file.
///
/// - A **file** path is returned as-is (after soft canonicalize).
/// - A **directory** path resolves to `<dir>/main.thx` (D68 / D138). The directory is
///   the project root for `~/…` once `load_package` runs on that file.
pub fn resolve_entry(path: &Path) -> Result<PathBuf> {
    let path = canonicalize_soft(path)?;
    if path.is_dir() {
        let main = path.join(ENTRY_FILE);
        if main.is_file() {
            return Ok(main);
        }
        if main.exists() {
            return Err(Error::new(format!(
                "{ENTRY_FILE} exists but is not a file in {}",
                path.display()
            )));
        }
        return Err(Error::new(format!(
            "no {ENTRY_FILE} in {} — a runnable Thoxa project needs a {ENTRY_FILE}",
            path.display()
        )));
    }
    if path.is_file() {
        return Ok(path);
    }
    Err(Error::new(format!("source not found: {}", path.display())))
}

/// Load the entry file and every `./` / `~/` import transitively.
///
/// `entry` may be a `.thx` file or a directory containing `main.thx`.
pub fn load_package(entry: &Path) -> Result<Package> {
    let entry = resolve_entry(entry)?;
    if !entry.is_file() {
        return Err(Error::new(format!(
            "source file not found: {}",
            entry.display()
        )));
    }
    let project_root = package_root_for(&entry);

    let mut by_path: HashMap<PathBuf, SourceModule> = HashMap::new();
    let mut order: Vec<PathBuf> = Vec::new();
    let mut visiting: HashSet<PathBuf> = HashSet::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();

    load_one(
        &entry,
        true,
        &project_root,
        &mut by_path,
        &mut order,
        &mut visiting,
        &mut visited,
    )?;

    let modules = order
        .into_iter()
        .map(|p| by_path.remove(&p).expect("loaded module"))
        .collect();

    let package = Package {
        project_root,
        modules,
    };
    check_unique_module_ids(&package)?;
    Ok(package)
}

/// Load a package using an already-parsed entry AST (LSP buffer / soft-recover).
///
/// Dependencies are still loaded from disk. Use this when the entry parse had
/// recoverable errors so check can still lower the partial AST for multi-diag.
pub fn load_package_with_entry_ast(entry: &Path, entry_ast: ast::Module) -> Result<Package> {
    let entry = resolve_entry(entry)?;
    if !entry.is_file() {
        return Err(Error::new(format!(
            "source file not found: {}",
            entry.display()
        )));
    }
    let project_root = package_root_for(&entry);
    let entry = canonicalize_soft(&entry)?;

    let mut by_path: HashMap<PathBuf, SourceModule> = HashMap::new();
    let mut order: Vec<PathBuf> = Vec::new();
    let mut visiting: HashSet<PathBuf> = HashSet::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();

    // Load deps first (from the buffer AST's imports).
    let dir = entry
        .parent()
        .ok_or_else(|| Error::new("entry file has no parent directory"))?;
    let imports: Vec<&ast::Import> = entry_ast
        .items
        .iter()
        .filter_map(|i| match i {
            ast::Item::Import(imp) => Some(imp),
            _ => None,
        })
        .collect();
    for imp in &imports {
        let dep = resolve_specifier(&imp.path, dir, &project_root, imp.path_span)?;
        check_library_stem(&dep, &imp.path, imp.path_span)?;
        load_one(
            &dep,
            false,
            &project_root,
            &mut by_path,
            &mut order,
            &mut visiting,
            &mut visited,
        )?;
    }

    let id = module_id_for(&entry, true, &project_root);
    order.push(entry.clone());
    by_path.insert(
        entry.clone(),
        SourceModule {
            id,
            path: entry,
            ast: entry_ast,
            is_entry: true,
        },
    );

    let modules = order
        .into_iter()
        .map(|p| by_path.remove(&p).expect("loaded module"))
        .collect();

    let package = Package {
        project_root,
        modules,
    };
    check_unique_module_ids(&package)?;
    Ok(package)
}

/// Load `path` and its transitive imports into `package` if not already present.
///
/// Used when session inject splices imports (e.g. Shell → Logging) after the
/// initial package load. New modules are inserted before the entry module so
/// dependency order is preserved.
pub fn ensure_module_in_package(package: &mut Package, path: &Path) -> Result<()> {
    let path = canonicalize_soft(path)?;
    if package.modules.iter().any(|m| m.path == path) {
        return Ok(());
    }

    let mut by_path: HashMap<PathBuf, SourceModule> = HashMap::new();
    let mut order: Vec<PathBuf> = Vec::new();
    let mut visiting: HashSet<PathBuf> = HashSet::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();

    for m in &package.modules {
        let p = m.path.clone();
        visited.insert(p.clone());
        by_path.insert(p.clone(), m.clone());
        order.push(p);
    }

    load_one(
        &path,
        false,
        &package.project_root,
        &mut by_path,
        &mut order,
        &mut visiting,
        &mut visited,
    )?;

    // Rebuild modules: non-entry in dependency order, entry last.
    let mut entry: Option<SourceModule> = None;
    let mut modules = Vec::new();
    for p in order {
        let m = by_path.remove(&p).expect("loaded module");
        if m.is_entry {
            entry = Some(m);
        } else {
            modules.push(m);
        }
    }
    if let Some(e) = entry {
        modules.push(e);
    } else if let Some(e) = package.modules.iter().find(|m| m.is_entry).cloned() {
        modules.push(e);
    }
    package.modules = modules;
    check_unique_module_ids(package)?;
    Ok(())
}

fn load_one(
    path: &Path,
    is_entry: bool,
    project_root: &Path,
    by_path: &mut HashMap<PathBuf, SourceModule>,
    order: &mut Vec<PathBuf>,
    visiting: &mut HashSet<PathBuf>,
    visited: &mut HashSet<PathBuf>,
) -> Result<()> {
    let path = canonicalize_soft(path)?;
    if visited.contains(&path) {
        return Ok(());
    }
    if !visiting.insert(path.clone()) {
        // Value-init cycles are checked later; mutual fn/type recursion is fine.
        // Still allow re-entry for register phase — mark as visited only after deps.
        return Ok(());
    }

    let src = fs::read_to_string(&path)
        .map_err(|e| Error::new(format!("cannot read {}: {e}", path.display())))?;
    // Entry session scripts: command-skin desugar before parse (PR2).
    let src = if is_entry {
        crate::command::desugar_session_source(&src)?
    } else {
        src
    };
    let tokens = Lexer::new(&src).tokenize()?;
    let ast = Parser::new(tokens).with_source(src).parse_module()?;
    let id = module_id_for(&path, is_entry, project_root);

    // Collect imports and load dependencies first (post-order).
    let imports: Vec<&Import> = ast
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Import(imp) => Some(imp),
            _ => None,
        })
        .collect();

    let dir = path
        .parent()
        .ok_or_else(|| Error::new("module has no parent directory"))?;

    for imp in &imports {
        let dep = resolve_specifier(&imp.path, dir, project_root, imp.path_span)?;
        check_library_stem(&dep, &imp.path, imp.path_span)?;
        load_one(&dep, false, project_root, by_path, order, visiting, visited)?;
    }

    visiting.remove(&path);
    visited.insert(path.clone());
    order.push(path.clone());
    by_path.insert(
        path.clone(),
        SourceModule {
            id,
            path,
            ast,
            is_entry,
        },
    );
    Ok(())
}

/// Directory containing product `std/*.thx` modules (D97).
///
/// Order: `THOXA_STD` → `{THOXA_ROOT}/std` → workspace walk (`…/std` next to
/// `crates/compiler`) → `../std` / `../../std` relative to the running `thoxa`
/// binary (dev builds under `target/rust/debug`).
pub fn std_lib_root() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("THOXA_STD") {
        let p = PathBuf::from(p);
        if p.is_dir() {
            return Ok(p);
        }
        return Err(Error::new(format!(
            "THOXA_STD is not a directory: {}",
            p.display()
        )));
    }
    if let Ok(root) = std::env::var("THOXA_ROOT") {
        let p = PathBuf::from(root).join("std");
        if p.is_dir() {
            return Ok(p);
        }
    }
    // Walk up from cwd for a Thoxa workspace.
    let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        let marker = dir.join("crates/compiler/Cargo.toml");
        let std_dir = dir.join("std");
        if marker.is_file() && std_dir.is_dir() {
            return Ok(std_dir);
        }
        if !dir.pop() {
            break;
        }
    }
    // Dev binary: target/rust/debug/thoxa → ../../../std
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for up in ["../../../std", "../../std", "../std", "std"] {
                let candidate = dir.join(up);
                if candidate.is_dir() {
                    return canonicalize_soft(&candidate);
                }
            }
        }
    }
    Err(Error::new(
        "cannot find Thoxa std library (set THOXA_STD or THOXA_ROOT, or run from a workspace)",
    ))
}

/// Map an import specifier to a filesystem path **without** requiring the file
/// to exist. Used by the LSP for open-buffer overlay (target may only be open).
///
/// Returns `None` for unsupported specs (`ffi`, `..`, empty) or when `std/`
/// cannot be resolved.
pub fn resolve_import_path(
    spec: &str,
    importer_dir: &Path,
    project_root: &Path,
) -> Option<PathBuf> {
    if spec.contains("..") {
        return None;
    }
    let path = if let Some(rest) = spec.strip_prefix("./") {
        if rest.is_empty() {
            return None;
        }
        importer_dir.join(format_thx(rest))
    } else if let Some(rest) = spec.strip_prefix("~/") {
        if rest.is_empty() {
            return None;
        }
        project_root.join(format_thx(rest))
    } else if let Some(rest) = spec.strip_prefix("std/") {
        if rest.is_empty() {
            return None;
        }
        let root = std_lib_root().ok()?;
        root.join(format_thx(rest))
    } else {
        // `ffi` still deferred.
        return None;
    };
    Some(path)
}

fn resolve_specifier(
    spec: &str,
    importer_dir: &Path,
    project_root: &Path,
    span: crate::span::Span,
) -> Result<PathBuf> {
    if spec.contains("..") {
        return Err(Error::at(
            "upward imports aren't allowed — address it from the project root with `~/…`",
            span,
        ));
    }

    let path = if let Some(rest) = spec.strip_prefix("./") {
        if rest.is_empty() {
            return Err(Error::at("empty local import specifier", span));
        }
        importer_dir.join(format_thx(rest))
    } else if let Some(rest) = spec.strip_prefix("~/") {
        if rest.is_empty() {
            return Err(Error::at("empty project import specifier", span));
        }
        project_root.join(format_thx(rest))
    } else if let Some(rest) = spec.strip_prefix("std/") {
        if rest.is_empty() {
            return Err(Error::at("empty std import specifier", span));
        }
        let root = std_lib_root().map_err(|e| Error::at(e.to_string(), span))?;
        root.join(format_thx(rest))
    } else if spec == "ffi" || spec.starts_with("ffi/") {
        return Err(Error::at(
            format!("named root `{spec}` is not supported yet (use std/…, ./…, or ~/…)"),
            span,
        ));
    } else {
        return Err(Error::at(
            format!("unknown import origin in `{spec}` — use `./…`, `~/…`, or `std/…`"),
            span,
        ));
    };

    if !path.is_file() {
        return Err(Error::at(
            format!("module not found: {} (from \"{spec}\")", path.display()),
            span,
        ));
    }
    canonicalize_soft(&path)
}

fn format_thx(rest: &str) -> String {
    if rest.ends_with(".thx") {
        rest.to_string()
    } else {
        format!("{rest}.thx")
    }
}

/// Non-entry library stems are kebab-case (D138): `http`, `http-server`.
fn is_kebab_case(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    let mut prev_hyphen = false;
    for c in chars {
        if c == '-' {
            if prev_hyphen {
                return false;
            }
            prev_hyphen = true;
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            prev_hyphen = false;
        } else {
            return false;
        }
    }
    !prev_hyphen
}

fn check_library_stem(path: &Path, spec: &str, span: Span) -> Result<()> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if is_kebab_case(stem) {
        return Ok(());
    }
    Err(Error::at(
        format!("module file name `{stem}` must be kebab-case (got `{spec}`)"),
        span,
    ))
}

/// C-mangling id (D35 / D138). Entry keeps the stem; libraries are
/// path-qualified (`std/http-server` → `std_http_server`).
pub fn module_id_for(path: &Path, is_entry: bool, project_root: &Path) -> String {
    if is_entry {
        return path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Main")
            .to_string();
    }
    sanitize_module_id(&logical_module_path(path, project_root))
}

fn logical_module_path(path: &Path, project_root: &Path) -> String {
    let strip_thx = |p: &Path| p.with_extension("");
    if let Ok(std_root) = std_lib_root() {
        if let Ok(rel) = path.strip_prefix(&std_root) {
            return format!("std/{}", path_to_slash(&strip_thx(rel)));
        }
    }
    let display = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = display.strip_prefix("std/") {
        return format!("std/{}", rest.trim_end_matches(".thx"));
    }
    if let Some(idx) = display.find("/std/") {
        let rest = &display[idx + 5..];
        return format!("std/{}", rest.trim_end_matches(".thx"));
    }
    if let Ok(rel) = path.strip_prefix(project_root) {
        return path_to_slash(&strip_thx(rel));
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string()
}

fn path_to_slash(path: &Path) -> String {
    path.iter()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn sanitize_module_id(logical: &str) -> String {
    // `/` and `-` both become `_`. Do **not** use `__` — MIR monomorphs already
    // split type args on `__` (`Fallible_Trial__Void__String`).
    logical.replace('\\', "/").replace('/', "_").replace('-', "_")
}

fn check_unique_module_ids(package: &Package) -> Result<()> {
    let mut seen: HashMap<&str, &Path> = HashMap::new();
    for m in &package.modules {
        if let Some(prev) = seen.insert(m.id.as_str(), m.path.as_path()) {
            return Err(Error::new(format!(
                "module id `{}` is used by both {} and {} — rename one file so sanitized paths differ",
                m.id,
                prev.display(),
                m.path.display()
            )));
        }
    }
    Ok(())
}

fn canonicalize_soft(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).or_else(|_| {
        // Fall back when path doesn't exist yet (error reported by caller) or on platforms
        // without full canonicalize for relative paths.
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            let cwd = std::env::current_dir().map_err(|e| Error::new(e.to_string()))?;
            Ok(cwd.join(path))
        }
    })
}

/// Local name bound by an import, mapped to a defining module + original name.
#[derive(Debug, Clone)]
pub enum BoundImport {
    /// Selective import of a fn/type/let/extern.
    Name {
        from_module: String,
        original: String,
    },
    /// Namespace import: `import mu from "./math-utils"` — members via `mu.x`.
    Namespace { from_module: String },
}

fn insert_selective_import_names(
    map: &mut HashMap<String, BoundImport>,
    names: &[ast::ImportName],
    from_module: &str,
    export_set: &HashSet<String>,
    importer_path: &Path,
) -> Result<()> {
    for n in names {
        let local = n
            .alias
            .as_ref()
            .map(|(a, _)| a.clone())
            .unwrap_or_else(|| n.name.clone());
        if !export_set.contains(&n.name) {
            return Err(Error::at(
                format!("`{}` is not exported from module `{from_module}`", n.name),
                n.name_span,
            )
            .with_path(importer_path));
        }
        if map.contains_key(&local) {
            return Err(
                Error::at(format!("duplicate import binding `{local}`"), n.name_span)
                    .with_path(importer_path),
            );
        }
        map.insert(
            local,
            BoundImport::Name {
                from_module: from_module.to_string(),
                original: n.name.clone(),
            },
        );
    }
    Ok(())
}

fn insert_namespace_import(
    map: &mut HashMap<String, BoundImport>,
    binding: &str,
    binding_span: Span,
    from_module: &str,
    importer_path: &Path,
) -> Result<()> {
    if map.contains_key(binding) {
        return Err(Error::at(
            format!("duplicate import binding `{binding}`"),
            binding_span,
        )
        .with_path(importer_path));
    }
    map.insert(
        binding.to_string(),
        BoundImport::Namespace {
            from_module: from_module.to_string(),
        },
    );
    Ok(())
}

/// Build per-module import maps (local binding → source).
pub fn build_import_maps(
    package: &Package,
) -> Result<HashMap<String, HashMap<String, BoundImport>>> {
    // module_id → exported names (for validation)
    let mut exports: HashMap<String, HashSet<String>> = HashMap::new();
    for m in &package.modules {
        let mut set = HashSet::new();
        for item in &m.ast.items {
            match item {
                Item::Function(f) if f.exported => {
                    set.insert(f.name.clone());
                }
                Item::Type(t) if t.exported => {
                    set.insert(t.name.clone());
                }
                Item::Extern(e) if e.exported => {
                    set.insert(e.name.clone());
                }
                Item::Global(g) if g.exported => {
                    set.insert(g.name.clone());
                }
                _ => {}
            }
        }
        exports.insert(m.id.clone(), set);
    }

    // path → module id
    let path_to_id: HashMap<PathBuf, String> = package
        .modules
        .iter()
        .map(|m| (m.path.clone(), m.id.clone()))
        .collect();

    let mut maps: HashMap<String, HashMap<String, BoundImport>> = HashMap::new();

    for m in &package.modules {
        let dir = m.path.parent().unwrap();
        let mut map = HashMap::new();
        for item in &m.ast.items {
            let Item::Import(imp) = item else { continue };
            let dep_path = resolve_specifier(&imp.path, dir, &package.project_root, imp.path_span)
                .map_err(|e| e.with_path(&m.path))?;
            let from_module = path_to_id.get(&dep_path).cloned().ok_or_else(|| {
                Error::at(
                    format!("internal: module {} not loaded", dep_path.display()),
                    imp.path_span,
                )
                .with_path(&m.path)
            })?;
            let export_set = exports.get(&from_module).cloned().unwrap_or_default();

            match &imp.kind {
                ImportKind::Selective(names) => {
                    insert_selective_import_names(
                        &mut map,
                        names,
                        &from_module,
                        &export_set,
                        &m.path,
                    )?;
                }
                ImportKind::Namespace {
                    binding,
                    binding_span,
                } => {
                    insert_namespace_import(
                        &mut map,
                        binding,
                        *binding_span,
                        &from_module,
                        &m.path,
                    )?;
                }
                ImportKind::Combined {
                    binding,
                    binding_span,
                    names,
                } => {
                    insert_namespace_import(
                        &mut map,
                        binding,
                        *binding_span,
                        &from_module,
                        &m.path,
                    )?;
                    insert_selective_import_names(
                        &mut map,
                        names,
                        &from_module,
                        &export_set,
                        &m.path,
                    )?;
                }
            }
        }
        maps.insert(m.id.clone(), map);
    }

    Ok(maps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_resolve_entry_{label}_{}_{}",
            std::process::id(),
            ns
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_entry_dir_with_main() {
        let dir = unique_temp_dir("ok");
        let main = dir.join("main.thx");
        fs::write(&main, "fn main() -> Int64 { 0 }\n").unwrap();
        let got = resolve_entry(&dir).expect("resolve");
        assert_eq!(got, fs::canonicalize(&main).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_entry_missing_main() {
        let dir = unique_temp_dir("missing");
        let err = resolve_entry(&dir).unwrap_err();
        assert!(
            err.message.contains("no main.thx"),
            "unexpected message: {}",
            err.message
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_entry_main_not_a_file() {
        let dir = unique_temp_dir("notfile");
        // `main.thx` as a subdirectory, not a regular file.
        fs::create_dir_all(dir.join("main.thx")).unwrap();
        let err = resolve_entry(&dir).unwrap_err();
        assert!(
            err.message.contains("exists but is not a file"),
            "unexpected message: {}",
            err.message
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_entry_file_unchanged() {
        let dir = unique_temp_dir("file");
        let file = dir.join("hello.thx");
        fs::write(&file, "fn main() -> Int64 { 0 }\n").unwrap();
        let got = resolve_entry(&file).expect("resolve file");
        assert_eq!(got, fs::canonicalize(&file).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_root_walks_to_nearest_main() {
        let dir = unique_temp_dir("pkgroot");
        fs::create_dir_all(dir.join("app")).unwrap();
        fs::write(dir.join("main.thx"), "fn main() -> Int64 { 0 }\n").unwrap();
        let nested = dir.join("app").join("pages.thx");
        fs::write(&nested, "export fn go() -> Int64 { 0 }\n").unwrap();
        assert_eq!(package_root_for(&nested), dir);
        assert_eq!(package_root_for(&dir.join("main.thx")), dir);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_root_falls_back_to_file_dir_without_main() {
        let dir = unique_temp_dir("nomaing");
        fs::create_dir_all(dir.join("app")).unwrap();
        let nested = dir.join("app").join("pages.thx");
        fs::write(&nested, "export fn go() -> Int64 { 0 }\n").unwrap();
        assert_eq!(package_root_for(&nested), dir.join("app"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_package_nested_file_resolves_tilde_from_main() {
        let dir = unique_temp_dir("tilde");
        fs::create_dir_all(dir.join("app")).unwrap();
        fs::create_dir_all(dir.join("framework")).unwrap();
        fs::write(dir.join("main.thx"), "fn main() -> Int64 { 0 }\n").unwrap();
        fs::write(
            dir.join("framework").join("wire.thx"),
            "export fn ping() -> Int64 { 1 }\n",
        )
        .unwrap();
        let pages = dir.join("app").join("pages.thx");
        fs::write(
            &pages,
            "import wire from \"~/framework/wire\"\nexport fn go() -> Int64 { wire.ping() }\n",
        )
        .unwrap();
        let pkg = load_package(&pages).expect("load nested");
        assert_eq!(pkg.project_root, dir);
        assert!(
            pkg.modules.iter().any(|m| m.id == "framework_wire"),
            "expected framework_wire loaded: {:?}",
            pkg.modules.iter().map(|m| &m.id).collect::<Vec<_>>()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn kebab_case_stems() {
        assert!(is_kebab_case("http"));
        assert!(is_kebab_case("http-server"));
        assert!(is_kebab_case("file-io"));
        assert!(is_kebab_case("a1"));
        assert!(!is_kebab_case("Http"));
        assert!(!is_kebab_case("httpServer"));
        assert!(!is_kebab_case("http_server"));
        assert!(!is_kebab_case("-http"));
        assert!(!is_kebab_case("http-"));
        assert!(!is_kebab_case("http--server"));
        assert!(!is_kebab_case(""));
    }

    #[test]
    fn module_id_sanitizes_hyphens_and_slashes() {
        assert_eq!(sanitize_module_id("std/http-server"), "std_http_server");
        assert_eq!(sanitize_module_id("app/routes"), "app_routes");
        assert_eq!(sanitize_module_id("math-utils"), "math_utils");
    }

    #[test]
    fn load_rejects_pascal_case_library() {
        let dir = unique_temp_dir("pascal");
        fs::write(
            dir.join("main.thx"),
            "import h from \"./Helper\"\nfn main() -> Int64 { 0 }\n",
        )
        .unwrap();
        fs::write(dir.join("Helper.thx"), "export fn go() -> Int64 { 1 }\n").unwrap();
        let err = load_package(&dir).expect_err("pascal library");
        assert!(
            err.message.contains("kebab-case"),
            "expected kebab-case error: {}",
            err.message
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
