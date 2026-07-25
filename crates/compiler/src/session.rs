//! Session script preparation (plan PR1+).
//!
//! When the **entry** module has no `fn main`, treat it as a session script:
//! - Top-level [`Item::SessionStmt`]s become the body of `Shell.step`.
//! - If the user already defined `Shell.step` and there are no session stmts,
//!   only a synthetic `main` is injected.
//! - Synthetic `main` is `Shell(status: 0).step().status`.
//!
//! Programs with `fn main` are unchanged. Session stmts + `main` is an error.

use crate::ast::*;
use crate::error::{Error, Result};
use crate::loader::{Package, SourceModule};
use crate::span::Span;

/// Rewrite the package entry for session mode when needed.
pub fn prepare_package(package: &mut Package) -> Result<()> {
    let entry_idx = package
        .modules
        .iter()
        .position(|m| m.is_entry)
        .ok_or_else(|| Error::new("package has no entry module"))?;
    prepare_entry(&mut package.modules[entry_idx])
}

fn prepare_entry(entry: &mut SourceModule) -> Result<()> {
    let has_main = entry.ast.items.iter().any(|item| {
        matches!(item, Item::Function(f) if f.name == "main")
    });
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
            return Err(Error::new(
                "session statements cannot appear in a program that defines `fn main` \
                 (use only declarations + main, or omit main for a session script)",
            ));
        }
        return Ok(());
    }

    // Drop SessionStmt items from the module; they move into step.
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
        // Library module (Io.thx, helpers, …) — leave unchanged.
        // `thoxa run` still fails later with "program requires fn main".
        return Ok(());
    }

    if !session_stmts.is_empty() && has_step {
        return Err(Error::new(
            "session script has both top-level statements and `Shell.step` — use one or the other",
        ));
    }

    if !session_stmts.is_empty() {
        let step = make_step_method(session_stmts);
        match shell_idx {
            Some(i) => {
                if let Item::Type(t) = &mut entry.ast.items[i] {
                    ensure_shell_status_param(t)?;
                    t.methods.push(step);
                }
            }
            None => {
                entry.ast.items.push(Item::Type(make_shell_type(step)));
            }
        }
    } else if let Some(i) = shell_idx {
        // step already present — still require status ctor field for synthetic main
        if let Item::Type(t) = &entry.ast.items[i] {
            ensure_shell_status_param(t)?;
        }
    } else {
        return Err(Error::new(
            "session script defines `Shell.step` path without `type Shell`",
        ));
    }

    entry.ast.items.push(Item::Function(make_session_main()));
    Ok(())
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

fn make_shell_type(step: Method) -> TypeDef {
    TypeDef {
        exported: false,
        name: "Shell".into(),
        name_span: Span::dummy(),
        params: vec![Param {
            name: "status".into(),
            name_span: Span::dummy(),
            positional: false,
            ty: TypeExpr {
                name: "Int64".into(),
                args: vec![],
                span: Span::dummy(),
            },
        }],
        fields: vec![],
        methods: vec![step],
    }
}

fn shell_status_zero() -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: "Shell".into(),
            args: vec![Arg::Named {
                name: "status".into(),
                name_span: Span::dummy(),
                value: Expr {
                    kind: ExprKind::Int(0),
                    span: Span::dummy(),
                },
            }],
        },
        span: Span::dummy(),
    }
}

fn make_step_method(stmts: Vec<BlockItem>) -> Method {
    // Body: user statements, then `Shell(status: 0)` so step returns Shell.
    Method {
        exported: true,
        name: "step".into(),
        name_span: Span::dummy(),
        params: vec![],
        return_type: Some(TypeExpr {
            name: "Shell".into(),
            args: vec![],
            span: Span::dummy(),
        }),
        body: Block {
            items: stmts,
            tail: Some(Box::new(shell_status_zero())),
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
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse(src: &str) -> Module {
        let tokens = Lexer::new(src).tokenize().expect("lex");
        Parser::new(tokens).parse_module().expect("parse")
    }

    fn prep(src: &str) -> Module {
        let mut entry = SourceModule {
            id: "Main".into(),
            path: std::path::PathBuf::from("Main.thx"),
            ast: parse(src),
            is_entry: true,
        };
        prepare_entry(&mut entry).expect("prepare");
        entry.ast
    }

    #[test]
    fn session_stmt_injects_shell_step_and_main() {
        let m = prep(r#"println("hi")"#);
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
        assert!(!m
            .items
            .iter()
            .any(|i| matches!(i, Item::SessionStmt(_))));
    }

    #[test]
    fn program_with_main_unchanged() {
        let src = r#"fn main() -> Int64 { println("x") 0 }"#;
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
println("nope")
fn main() -> Int64 { 0 }
"#;
        let mut entry = SourceModule {
            id: "Main".into(),
            path: std::path::PathBuf::from("Main.thx"),
            ast: parse(src),
            is_entry: true,
        };
        assert!(prepare_entry(&mut entry).is_err());
    }
}
