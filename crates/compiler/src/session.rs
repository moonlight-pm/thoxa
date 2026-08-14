//! Session script preparation (plan PR1–PR2).
//!
//! When the **entry** module has no `fn main`, treat it as a session script:
//! - Top-level [`Item::SessionStmt`]s become the body of `Shell.step`.
//! - Default `Shell` + helpers come from product [`std/Shell.thx`](../../../../std/Shell.thx)
//!   when needed (D104); on-disk std is preferred, with a compile-time embed fallback.
//! - Synthetic `main` is `Shell(status: 0).step().status`.
//!
//! Command sugar ([`crate::command`]) rewrites lines **before** parse.

use crate::ast::*;
use crate::error::{Error, Result};
use crate::lexer::Lexer;
use crate::loader::{self, Package, SourceModule};
use crate::parser::Parser;
use crate::span::Span;
use std::fs;

/// Compile-time fallback when `std/Shell.thx` cannot be resolved at runtime.
/// Keep in sync with the product file under `std/Shell.thx` (D104).
const SESSION_PRELUDE_EMBED: &str = include_str!("../../../std/Shell.thx");

/// Compile-time fallback for `std/Fallible.thx` when the std tree is missing.
/// Keep in sync with the product file under `std/Fallible.thx`.
const FALLIBLE_EMBED: &str = include_str!("../../../std/Fallible.thx");

/// Rewrite the package entry for session mode when needed.
pub fn prepare_package(package: &mut Package) -> Result<()> {
    if package.modules.is_empty() {
        return Err(Error::new("package has no modules"));
    }
    // Prefer `is_entry`; fall back to last module (loader dependency order).
    // LSP may demote library buffers so `is_entry` is false on all modules.
    let entry_idx = package
        .modules
        .iter()
        .position(|m| m.is_entry)
        .unwrap_or(package.modules.len() - 1);
    let injected_shell = prepare_entry(&mut package.modules[entry_idx])?;
    // Shell.thx imports Fallible + Logging (and Logging pulls Env/Str/Path).
    // Inject happens after load_package, so pull those modules into the package.
    if injected_shell {
        ensure_shell_std_deps(package)?;
    }
    Ok(())
}

/// Load std modules required by the injected Shell prelude (D104 + D125).
fn ensure_shell_std_deps(package: &mut Package) -> Result<()> {
    // Prefer on-disk std tree so Logging/Env/Str/Path load transitively.
    if let Ok(root) = loader::std_lib_root() {
        let logging = root.join("Logging.thx");
        if logging.is_file() {
            loader::ensure_module_in_package(package, &logging)?;
            return Ok(());
        }
    }
    // Embed fallback path: Fallible only (legacy); Logging needs disk std.
    ensure_fallible_module(package)?;
    Ok(())
}

/// Returns `true` when product Shell items were injected into the entry.
fn prepare_entry(entry: &mut SourceModule) -> Result<bool> {
    let has_main = entry
        .ast
        .items
        .iter()
        .any(|item| matches!(item, Item::Function(f) if f.name == "main"));
    let session_stmts: Vec<BlockItem> = entry
        .ast
        .items
        .iter()
        .filter_map(|item| match item {
            Item::SessionStmt(s) => Some(s.clone()),
            _ => None,
        })
        .collect();

    if has_main {
        if !session_stmts.is_empty() {
            // Prefer one diagnostic per statement with a real span (parser also
            // catches this; this path covers AST built without re-parse).
            let errors: Vec<Error> = session_stmts
                .iter()
                .map(|bi| {
                    Error::coded_at(
                        crate::diagnostic::CODE_TOP_LEVEL_WITH_MAIN,
                        crate::parser::TOP_LEVEL_STMT_WITH_MAIN_MSG,
                        session_block_item_span(bi),
                    )
                    .with_help(
                        "move the statement into `fn main` or remove `fn main` for a session script",
                    )
                })
                .collect();
            return Err(Error::multi(errors));
        }
        return Ok(false);
    }

    entry
        .ast
        .items
        .retain(|item| !matches!(item, Item::SessionStmt(_)));

    let shell_idx = entry
        .ast
        .items
        .iter()
        .position(|item| matches!(item, Item::Type(t) if t.name == "Shell"));

    let has_step = shell_idx.is_some_and(|i| {
        if let Item::Type(t) = &entry.ast.items[i] {
            t.methods.iter().any(|m| m.name == "step")
        } else {
            false
        }
    });

    if session_stmts.is_empty() && !has_step {
        // Library module — leave unchanged.
        return Ok(false);
    }

    if !session_stmts.is_empty() && has_step {
        return Err(Error::new(
            "session script has both top-level statements and `Shell.step` — use one or the other",
        ));
    }

    let mut injected_shell = false;
    if !session_stmts.is_empty() {
        let needs_prelude = shell_idx.is_none();
        if needs_prelude {
            inject_prelude(entry)?;
            injected_shell = true;
        } else if let Some(i) = shell_idx {
            if let Item::Type(t) = &entry.ast.items[i] {
                ensure_shell_status_param(t)?;
            }
        }
        let step = make_step_method(session_stmts);
        // Re-find Shell after possible prelude inject.
        let shell_idx = entry
            .ast
            .items
            .iter()
            .position(|item| matches!(item, Item::Type(t) if t.name == "Shell"))
            .ok_or_else(|| Error::new("internal: Shell missing after session prepare"))?;
        if let Item::Type(t) = &mut entry.ast.items[shell_idx] {
            t.methods.push(step);
        }
    } else if let Some(i) = shell_idx {
        if let Item::Type(t) = &entry.ast.items[i] {
            ensure_shell_status_param(t)?;
        }
    } else {
        return Err(Error::new(
            "session script defines `Shell.step` path without `type Shell`",
        ));
    }

    entry.ast.items.push(Item::Function(make_session_main()));
    Ok(injected_shell)
}

fn inject_prelude(entry: &mut SourceModule) -> Result<()> {
    let src = session_shell_source()?;
    let tokens = Lexer::new(&src)
        .tokenize()
        .map_err(|e| Error::new(format!("session Shell prelude lex: {e}")))?;
    let prelude = Parser::new(tokens)
        .parse_module()
        .map_err(|e| Error::new(format!("session Shell prelude parse: {e}")))?;
    // Insert prelude items before user items (externs/helpers/Shell first).
    let mut merged = prelude.items;
    merged.append(&mut entry.ast.items);
    entry.ast.items = merged;
    Ok(())
}

/// Load product `std/Shell.thx` from the std tree when available; else embedded copy.
fn session_shell_source() -> Result<String> {
    if let Ok(root) = loader::std_lib_root() {
        let path = root.join("Shell.thx");
        if path.is_file() {
            return fs::read_to_string(&path).map_err(|e| {
                Error::new(format!(
                    "read session Shell prelude {}: {e}",
                    path.display()
                ))
            });
        }
    }
    Ok(SESSION_PRELUDE_EMBED.to_string())
}

/// Ensure `std/Fallible` is a package module (Shell inject imports it after load).
fn ensure_fallible_module(package: &mut Package) -> Result<()> {
    if package.modules.iter().any(|m| m.id == "Fallible") {
        return Ok(());
    }
    let (path, src) = fallible_source()?;
    let tokens = Lexer::new(&src)
        .tokenize()
        .map_err(|e| Error::new(format!("std/Fallible lex: {e}")))?;
    let ast = Parser::new(tokens)
        .parse_module()
        .map_err(|e| Error::new(format!("std/Fallible parse: {e}")))?;
    // Dependency order: Fallible before modules that import it (entry is last).
    package.modules.insert(
        0,
        SourceModule {
            id: "Fallible".into(),
            path,
            ast,
            is_entry: false,
        },
    );
    Ok(())
}

fn fallible_source() -> Result<(std::path::PathBuf, String)> {
    if let Ok(root) = loader::std_lib_root() {
        let path = root.join("Fallible.thx");
        if path.is_file() {
            let src = fs::read_to_string(&path)
                .map_err(|e| Error::new(format!("read std/Fallible {}: {e}", path.display())))?;
            // Match loader::resolve_specifier path key (canonical when possible).
            let path = fs::canonicalize(&path).unwrap_or(path);
            return Ok((path, src));
        }
    }
    Ok((
        std::path::PathBuf::from("std/Fallible.thx"),
        FALLIBLE_EMBED.to_string(),
    ))
}

fn session_block_item_span(bi: &BlockItem) -> Span {
    match bi {
        BlockItem::Let {
            name_span, init, ..
        } => Span::new(name_span.start, init.span.end),
        BlockItem::Assign { target, value } => {
            let start = match target {
                AssignTarget::Name { name_span, .. } => name_span.start,
                AssignTarget::Field { base_span, .. } => base_span.start,
                AssignTarget::Index { base_span, .. } => base_span.start,
                AssignTarget::DynIndex { name_span, .. } => name_span.start,
            };
            Span::new(start, value.span.end)
        }
        BlockItem::Expr(e) => e.span,
    }
}

fn ensure_shell_status_param(t: &TypeDef) -> Result<()> {
    if t.params.is_empty() && !t.fields.is_empty() {
        return Err(Error::new(
            "session `Shell` must use constructor params including `status: Int64` \
             (body-field Shell is not supported for synthetic main yet)",
        ));
    }
    let has_status = t.params.iter().any(|p| p.name == "status");
    if !has_status {
        return Err(Error::new(
            "session `type Shell` must have a constructor parameter `status: Int64`",
        ));
    }
    Ok(())
}

fn shell_with_status(status_expr: Expr) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: "Shell".into(),
            args: vec![Arg::Named {
                name: "status".into(),
                name_span: Span::dummy(),
                value: status_expr,
            }],
        },
        span: Span::dummy(),
    }
}

fn shell_status_zero() -> Expr {
    shell_with_status(Expr {
        kind: ExprKind::Int(0),
        span: Span::dummy(),
    })
}

/// Receiver field `status` (for method bodies).
fn self_status() -> Expr {
    Expr {
        kind: ExprKind::Name("status".into()),
        span: Span::dummy(),
    }
}

fn make_step_method(stmts: Vec<BlockItem>) -> Method {
    // Seed from the receiver so REPL/host can pass prior status through.
    // mut shell = Shell(status: status)
    // <user stmts — may assign shell = shell.echo(…)>
    // shell
    let mut items = vec![BlockItem::Let {
        name: "shell".into(),
        name_span: Span::dummy(),
        mutable: true,
        ty: None,
        init: shell_with_status(self_status()),
    }];
    items.extend(stmts);
    Method {
        exported: true,
        name: "step".into(),
        name_span: Span::dummy(),
        params: vec![],
        return_type: Some(TypeExpr {
            name: "Shell".into(),
            args: vec![],
            span: Span::dummy(),
            lit: None,
            mark: None,
            union_members: None,
            fn_type: None,
        }),
        body: Block {
            items,
            tail: Some(Box::new(Expr {
                kind: ExprKind::Name("shell".into()),
                span: Span::dummy(),
            })),
            span: Span::dummy(),
        },
    }
}

/// `fn main() -> Int64 { Shell(status: 0).step().status }`
fn make_session_main() -> Function {
    let step_call = Expr {
        kind: ExprKind::MethodCall {
            receiver: Box::new(shell_status_zero()),
            method: "step".into(),
            method_span: Span::dummy(),
            args: vec![],
        },
        span: Span::dummy(),
    };
    let status_field = Expr {
        kind: ExprKind::Field {
            base: Box::new(step_call),
            name: "status".into(),
            name_span: Span::dummy(),
        },
        span: Span::dummy(),
    };
    Function {
        exported: false,
        name: "main".into(),
        name_span: Span::dummy(),
        params: vec![],
        return_type: Some(TypeExpr {
            name: "Int64".into(),
            args: vec![],
            span: Span::dummy(),
            lit: None,
            mark: None,
            union_members: None,
            fn_type: None,
        }),
        body: Block {
            items: vec![],
            tail: Some(Box::new(status_field)),
            span: Span::dummy(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::desugar_session_source;

    fn parse(src: &str) -> Module {
        let tokens = Lexer::new(src).tokenize().expect("lex");
        Parser::new(tokens).parse_module().expect("parse")
    }

    fn prep(src: &str) -> Module {
        let desugared = desugar_session_source(src).expect("desugar");
        let mut entry = SourceModule {
            id: "Main".into(),
            path: std::path::PathBuf::from("Main.thx"),
            ast: parse(&desugared),
            is_entry: true,
        };
        prepare_entry(&mut entry).expect("prepare");
        entry.ast
    }

    #[test]
    fn session_stmt_injects_shell_step_and_main() {
        let m = prep(r#"logging.info("hi")"#);
        assert!(m
            .items
            .iter()
            .any(|i| matches!(i, Item::Function(f) if f.name == "main")));
        let shell = m
            .items
            .iter()
            .find_map(|i| match i {
                Item::Type(t) if t.name == "Shell" => Some(t),
                _ => None,
            })
            .expect("Shell");
        assert!(shell.methods.iter().any(|m| m.name == "step"));
        assert!(shell.methods.iter().any(|m| m.name == "echo"));
        assert!(shell.methods.iter().any(|m| m.name == "runIo"));
        assert!(shell.methods.iter().any(|m| m.name == "pipeline"));
    }

    #[test]
    fn session_shell_source_loads_product_std() {
        let src = session_shell_source().expect("shell source");
        assert!(
            src.contains("type Shell(status: Int64)"),
            "expected Shell type in prelude"
        );
        // Prefer on-disk std when the workspace is discoverable.
        if let Ok(root) = loader::std_lib_root() {
            let path = root.join("Shell.thx");
            assert!(path.is_file(), "missing {}", path.display());
            let disk = fs::read_to_string(&path).expect("read Shell.thx");
            assert_eq!(src, disk, "inject should prefer on-disk std/Shell.thx");
        }
    }

    #[test]
    fn program_with_main_unchanged() {
        let src = r#"fn main() -> Int64 { 0 }"#;
        let mut entry = SourceModule {
            id: "Main".into(),
            path: std::path::PathBuf::from("Main.thx"),
            ast: parse(src),
            is_entry: true,
        };
        prepare_entry(&mut entry).unwrap();
        assert_eq!(entry.ast.items.len(), 1);
    }

    #[test]
    fn main_plus_session_stmt_errors() {
        let src = r#"
logging.info("nope")
fn main() -> Int64 { 0 }
"#;
        // Localized at parse time (and still guarded in prepare for synthetic ASTs).
        let tokens = Lexer::new(src).tokenize().expect("lex");
        let err = Parser::new(tokens)
            .parse_module()
            .expect_err("expected session+main error");
        assert!(
            err.message.contains("unexpected top-level statement"),
            "{err}"
        );
        assert!(err.span.is_some(), "expected span on session stmt");
        assert!(err.recovered_module.is_some(), "expected recovered AST");
    }

    #[test]
    fn prepare_rejects_session_stmt_with_main_on_synthetic_ast() {
        // Bypass parse post-check: build AST that still has SessionStmt + main.
        let tokens = Lexer::new("fn main() -> Int64 { 0 }")
            .tokenize()
            .expect("lex");
        let mut ast = Parser::new(tokens).parse_module().expect("parse");
        ast.items.insert(
            0,
            Item::SessionStmt(BlockItem::Expr(Expr {
                kind: ExprKind::Name("FOOBAR".into()),
                span: Span::new(0, 6),
            })),
        );
        let mut entry = SourceModule {
            id: "Main".into(),
            path: std::path::PathBuf::from("Main.thx"),
            ast,
            is_entry: true,
        };
        let err = prepare_entry(&mut entry).expect_err("prepare should reject");
        assert!(
            err.message.contains("unexpected top-level statement"),
            "{err}"
        );
        assert_eq!(err.span, Some(Span::new(0, 6)));
    }
}
