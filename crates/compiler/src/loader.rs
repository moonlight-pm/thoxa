//! Multi-file module loader (R8).
//!
//! Specifiers:
//! - `./…` relative to the importing file's directory (no `..` segments)
//! - `~/…` relative to the project root (entry file's directory)
//! - bare roots (`std/…`, `ffi`) — deferred beyond R8

use crate::ast::{self, Import, ImportKind, Item};
use crate::error::{Error, Result};
use crate::lexer::Lexer;
use crate::parser::Parser;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A parsed source module with identity and path.
#[derive(Debug, Clone)]
pub struct SourceModule {
    /// Module id = file stem (PascalCase for library modules).
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

/// Resolve a CLI path to the entry `.thx` file.
///
/// - A **file** path is returned as-is (after soft canonicalize).
/// - A **directory** path resolves to `<dir>/Main.thx` (D68). The directory is
///   the project root for `~/…` once `load_package` runs on that file.
pub fn resolve_entry(path: &Path) -> Result<PathBuf> {
    let path = canonicalize_soft(path)?;
    if path.is_dir() {
        let main = path.join("Main.thx");
        if main.is_file() {
            return Ok(main);
        }
        if main.exists() {
            return Err(Error::new(format!(
                "Main.thx exists but is not a file in {}",
                path.display()
            )));
        }
        return Err(Error::new(format!(
            "no Main.thx in {} — a runnable Thoxa project needs a Main.thx",
            path.display()
        )));
    }
    if path.is_file() {
        return Ok(path);
    }
    Err(Error::new(format!(
        "source not found: {}",
        path.display()
    )))
}

/// Load the entry file and every `./` / `~/` import transitively.
///
/// `entry` may be a `.thx` file or a directory containing `Main.thx`.
pub fn load_package(entry: &Path) -> Result<Package> {
    let entry = resolve_entry(entry)?;
    if !entry.is_file() {
        return Err(Error::new(format!(
            "source file not found: {}",
            entry.display()
        )));
    }
    let project_root = entry
        .parent()
        .ok_or_else(|| Error::new("entry file has no parent directory"))?
        .to_path_buf();

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

    Ok(Package {
        project_root,
        modules,
    })
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

    let src = fs::read_to_string(&path).map_err(|e| {
        Error::new(format!("cannot read {}: {e}", path.display()))
    })?;
    let tokens = Lexer::new(&src).tokenize()?;
    let ast = Parser::new(tokens).parse_module()?;
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module")
        .to_string();

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
        // Non-entry modules must be PascalCase file names.
        let stem = dep
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if !is_pascal_case(stem) {
            return Err(Error::at(
                format!(
                    "module file name `{stem}` must be PascalCase (got `{}`)",
                    imp.path
                ),
                imp.path_span,
            ));
        }
        load_one(
            &dep,
            false,
            project_root,
            by_path,
            order,
            visiting,
            visited,
        )?;
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
    } else if spec.starts_with("std/") || spec == "ffi" || spec.starts_with("ffi/") {
        return Err(Error::at(
            format!("named root `{spec}` is not supported yet (R8: only ./ and ~/ local imports)"),
            span,
        ));
    } else {
        return Err(Error::at(
            format!(
                "unknown import origin in `{spec}` — use `./…`, `~/…`, or a named root"
            ),
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

fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => true,
        _ => false,
    }
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
    /// Namespace import: `import mu from "./MathUtils"` — members via `mu.x`.
    Namespace { from_module: String },
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
            let dep_path = resolve_specifier(&imp.path, dir, &package.project_root, imp.path_span)?;
            let from_module = path_to_id
                .get(&dep_path)
                .cloned()
                .ok_or_else(|| {
                    Error::at(
                        format!("internal: module {} not loaded", dep_path.display()),
                        imp.path_span,
                    )
                })?;
            let export_set = exports.get(&from_module).cloned().unwrap_or_default();

            match &imp.kind {
                ImportKind::Selective(names) => {
                    for n in names {
                        let local = n
                            .alias
                            .as_ref()
                            .map(|(a, _)| a.clone())
                            .unwrap_or_else(|| n.name.clone());
                        if !export_set.contains(&n.name) {
                            return Err(Error::at(
                                format!(
                                    "`{}` is not exported from module `{from_module}`",
                                    n.name
                                ),
                                n.name_span,
                            ));
                        }
                        if map.contains_key(&local) {
                            return Err(Error::at(
                                format!("duplicate import binding `{local}`"),
                                n.name_span,
                            ));
                        }
                        map.insert(
                            local,
                            BoundImport::Name {
                                from_module: from_module.clone(),
                                original: n.name.clone(),
                            },
                        );
                    }
                }
                ImportKind::Namespace {
                    binding,
                    binding_span,
                } => {
                    if map.contains_key(binding) {
                        return Err(Error::at(
                            format!("duplicate import binding `{binding}`"),
                            *binding_span,
                        ));
                    }
                    map.insert(
                        binding.clone(),
                        BoundImport::Namespace {
                            from_module: from_module.clone(),
                        },
                    );
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
        let main = dir.join("Main.thx");
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
            err.message.contains("no Main.thx"),
            "unexpected message: {}",
            err.message
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_entry_main_not_a_file() {
        let dir = unique_temp_dir("notfile");
        // `Main.thx` as a subdirectory, not a regular file.
        fs::create_dir_all(dir.join("Main.thx")).unwrap();
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
}
