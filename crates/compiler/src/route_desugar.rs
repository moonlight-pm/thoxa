//! Experiment: desugar type-body route clauses (`GET "/…" { … }`) into ordinary
//! free-function handlers + a generated `route` method on the type.
//!
//! Handlers are module-level `fn __{Type}_route_N` (not type methods) because
//! Thoxa cannot call private methods via `Type().m()` from another method.
//!
//! `route(req: Request) -> Response`:
//! - clause returns `String` (or omits `->`) → `ok(Bytes.fromString(s), "text/plain")`
//! - clause returns `Response` → pass through
//! - miss → `notFound()`
//!
//! Compiler only names userland `Request` (`.method` / `.path`), `Response`,
//! and the wrap/miss helpers `ok` / `notFound` (typically `std/Http`).
//!
//! Branch `experiment/router-clause-syntax`. No HTTP lang-items.

use crate::ast::*;
use crate::error::{Error, Result};
use crate::loader::Package;
use crate::span::Span;

/// Rewrite every type that has `routes` into handler free functions plus
/// `route(req: Request) -> Response` on the type. Clears `routes`.
pub fn desugar_package(package: &mut Package) -> Result<()> {
    for m in &mut package.modules {
        desugar_module(&mut m.ast)?;
    }
    Ok(())
}

/// Public for LSP resolve / tests (also invoked from package lower).
pub fn desugar_module(module: &mut Module) -> Result<()> {
    let mut extra_fns: Vec<Function> = Vec::new();
    for item in &mut module.items {
        if let Item::Type(td) = item {
            extra_fns.extend(desugar_type(td)?);
        }
    }
    for f in extra_fns {
        module.items.push(Item::Function(f));
    }
    Ok(())
}

/// Returns synthetic free functions for handlers (to append to the module).
fn desugar_type(td: &mut TypeDef) -> Result<Vec<Function>> {
    if td.routes.is_empty() {
        return Ok(Vec::new());
    }
    if td.union_members.is_some() {
        return Err(Error::at(
            "union types cannot have route clauses",
            td.name_span,
        ));
    }
    if !td.params.is_empty() || !td.fields.is_empty() {
        return Err(Error::at(
            "route clauses on types with constructor params or body fields \
             are not supported in this experiment (use a nullary type)",
            td.name_span,
        ));
    }
    if td.methods.iter().any(|m| m.name == "route") {
        return Err(Error::at(
            "type already has a `route` method; remove it or rename so route \
             clauses can generate `route`",
            td.name_span,
        ));
    }

    let routes = std::mem::take(&mut td.routes);
    let type_name = td.name.clone();
    let mut handlers: Vec<HandlerMeta> = Vec::new();
    let mut free_fns: Vec<Function> = Vec::new();

    for (i, r) in routes.into_iter().enumerate() {
        let handler_name = format!("__{type_name}_route_{i}");
        let pat = analyze_pattern(&r.pattern, r.pattern_span)?;
        validate_formals(&r, &pat)?;

        let ret_kind = classify_return(&r)?;
        let ret_ty = match ret_kind {
            HandlerRet::Text => ty_string(),
            HandlerRet::Response => ty_response(),
        };

        handlers.push(HandlerMeta {
            http_method: r.method.clone(),
            pattern: pat,
            handler_name: handler_name.clone(),
            def_span: r.method_span,
            ret: ret_kind,
        });

        free_fns.push(Function {
            exported: false,
            name: handler_name,
            name_span: r.method_span,
            params: r.params,
            return_type: Some(ret_ty),
            body: r.body,
        });
    }

    let route_span = handlers
        .first()
        .map(|h| h.def_span)
        .unwrap_or(td.name_span);
    td.methods.push(build_route_method(&handlers, route_span)?);
    Ok(free_fns)
}

struct HandlerMeta {
    http_method: String,
    pattern: PathPattern,
    handler_name: String,
    /// Source span of the HTTP method token (for go-to-def / outline).
    def_span: Span,
    ret: HandlerRet,
}

#[derive(Clone, Copy)]
enum HandlerRet {
    /// Omitted or `-> String` — wrap with `ok(…, "text/plain")`.
    Text,
    /// `-> Response` — return the handler value as-is.
    Response,
}

fn classify_return(r: &RouteClause) -> Result<HandlerRet> {
    match r.return_type.as_ref().map(|t| t.name.as_str()) {
        None | Some("String") => Ok(HandlerRet::Text),
        Some("Response") => Ok(HandlerRet::Response),
        Some(other) => Err(Error::at(
            format!(
                "route handler return type must be String or Response \
                 (got `{other}`)"
            ),
            r.method_span,
        )),
    }
}

enum PathPattern {
    /// Full path equality (`"/home"`).
    Exact(String),
    /// Literal prefix + final `:name` capture (`"/user/:userId"` → prefix `/user/`).
    TrailingCapture { prefix: String, capture: String },
}

fn analyze_pattern(pattern: &str, span: Span) -> Result<PathPattern> {
    if pattern.is_empty() {
        return Err(Error::at("route pattern must not be empty", span));
    }
    if !pattern.starts_with('/') {
        return Err(Error::at(
            "route pattern must start with `/`",
            span,
        ));
    }
    let segs: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let mut capture_idxs: Vec<usize> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        if let Some(rest) = s.strip_prefix(':') {
            if rest.is_empty() {
                return Err(Error::at("empty path capture name in pattern", span));
            }
            if !rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(Error::at(
                    format!(
                        "path capture `:{rest}` must be alphanumeric (Thoxa idents \
                         cannot contain `_`)"
                    ),
                    span,
                ));
            }
            capture_idxs.push(i);
        } else if s.contains(':') {
            return Err(Error::at(
                "invalid `:` in path segment (use `:name` for a capture)",
                span,
            ));
        }
    }
    if capture_idxs.is_empty() {
        return Ok(PathPattern::Exact(pattern.to_string()));
    }
    if capture_idxs.len() != 1 || capture_idxs[0] != segs.len() - 1 {
        return Err(Error::at(
            "this experiment only supports exact paths or a single `:capture` \
             as the final segment (e.g. `/user/:userId`)",
            span,
        ));
    }
    let capture = segs[segs.len() - 1][1..].to_string();
    let prefix = if segs.len() == 1 {
        "/".to_string()
    } else {
        format!("/{}/", segs[..segs.len() - 1].join("/"))
    };
    Ok(PathPattern::TrailingCapture { prefix, capture })
}

fn validate_formals(r: &RouteClause, pat: &PathPattern) -> Result<()> {
    match pat {
        PathPattern::Exact(_) => {
            if !r.params.is_empty() {
                return Err(Error::at(
                    "exact path route cannot take path-capture parameters",
                    r.method_span,
                ));
            }
        }
        PathPattern::TrailingCapture { capture, .. } => {
            if r.params.len() != 1 {
                return Err(Error::at(
                    format!(
                        "trailing-capture route must have exactly one parameter \
                         named `{capture}`"
                    ),
                    r.method_span,
                ));
            }
            let p = &r.params[0];
            if p.name != *capture {
                return Err(Error::at(
                    format!(
                        "parameter `{}` must match path capture `:{capture}`",
                        p.name
                    ),
                    p.name_span,
                ));
            }
            if p.ty.name != "String" {
                return Err(Error::at(
                    "path capture parameter must be `String` in this experiment \
                     (no Int64 parse yet)",
                    p.name_span,
                ));
            }
        }
    }
    Ok(())
}

fn build_route_method(handlers: &[HandlerMeta], name_span: Span) -> Result<Method> {
    // Nested ifs with early return; tail is `notFound()`.
    let mut items: Vec<BlockItem> = Vec::new();
    for h in handlers {
        items.push(BlockItem::Expr(handler_try_if(h)?));
    }
    let body = Block {
        items,
        tail: Some(Box::new(call("notFound", vec![]))),
        span: name_span,
    };
    Ok(Method {
        exported: true,
        name: "route".into(),
        name_span,
        params: vec![param("req", ty_request())],
        return_type: Some(ty_response()),
        body,
    })
}

fn handler_try_if(h: &HandlerMeta) -> Result<Expr> {
    let method_ok = bin(
        BinaryOp::Eq,
        field(name("req"), "method"),
        str_lit(&h.http_method),
    );
    let inner = match &h.pattern {
        PathPattern::Exact(pat) => {
            let path_ok = bin(BinaryOp::Eq, field(name("req"), "path"), str_lit(pat));
            let call = return_handler_call(&h.handler_name, vec![], h.ret);
            if_expr(path_ok, block_stmts(vec![BlockItem::Expr(call)]), None)
        }
        PathPattern::TrailingCapture { prefix, capture } => {
            let _ = capture; // name already validated against formal
            trailing_capture_block(&h.handler_name, prefix, h.ret)?
        }
    };
    Ok(if_expr(
        method_ok,
        block_expr(inner),
        None,
    ))
}

fn trailing_capture_block(
    handler_name: &str,
    prefix: &str,
    ret: HandlerRet,
) -> Result<Expr> {
    let path_b_init = method_call(
        name("Bytes"),
        "fromString",
        vec![Arg::Pos(field(name("req"), "path"))],
    );
    let pre_b_init = method_call(name("Bytes"), "fromString", vec![Arg::Pos(str_lit(prefix))]);

    let mut items = vec![
        let_bind("pathB", false, path_b_init),
        let_bind("preB", false, pre_b_init),
    ];

    // if preB.count <= pathB.count { … }
    let len_ok = bin(
        BinaryOp::Le,
        field(name("preB"), "count"),
        field(name("pathB"), "count"),
    );

    let mut inner_items = vec![
        let_bind("ok", true, bool_lit(true)),
        let_bind("i", true, int_lit(0)),
    ];

    // loop { if i >= preB.count { break }; if pathB.get(i) != preB.get(i) { ok = false; break }; i = i + 1 }
    let loop_body = Block {
        items: vec![
            BlockItem::Expr(if_expr(
                bin(
                    BinaryOp::Ge,
                    name("i"),
                    field(name("preB"), "count"),
                ),
                block_stmts(vec![BlockItem::Expr(Expr {
                    kind: ExprKind::Break,
                    span: dummy(),
                })]),
                None,
            )),
            BlockItem::Expr(if_expr(
                bin(
                    BinaryOp::Ne,
                    method_call(name("pathB"), "get", vec![Arg::Pos(name("i"))]),
                    method_call(name("preB"), "get", vec![Arg::Pos(name("i"))]),
                ),
                block_stmts(vec![
                    BlockItem::Assign {
                        target: AssignTarget::Name {
                            name: "ok".into(),
                            name_span: dummy(),
                        },
                        value: bool_lit(false),
                    },
                    BlockItem::Expr(Expr {
                        kind: ExprKind::Break,
                        span: dummy(),
                    }),
                ]),
                None,
            )),
            BlockItem::Assign {
                target: AssignTarget::Name {
                    name: "i".into(),
                    name_span: dummy(),
                },
                value: bin(BinaryOp::Add, name("i"), int_lit(1)),
            },
        ],
        tail: None,
        span: dummy(),
    };
    inner_items.push(BlockItem::Expr(Expr {
        kind: ExprKind::Loop {
            body: loop_body,
        },
        span: dummy(),
    }));

    // if ok { let rest = pathB.slice(preB.count, pathB.count).toString(); return Type().handler(rest) }
    let slice = method_call(
        name("pathB"),
        "slice",
        vec![
            Arg::Pos(field(name("preB"), "count")),
            Arg::Pos(field(name("pathB"), "count")),
        ],
    );
    let rest_init = method_call(slice, "toString", vec![]);
    let call = return_handler_call(handler_name, vec![Arg::Pos(name("rest"))], ret);
    inner_items.push(BlockItem::Expr(if_expr(
        name("ok"),
        Block {
            items: vec![let_bind("rest", false, rest_init)],
            tail: Some(Box::new(call)),
            span: dummy(),
        },
        None,
    )));

    items.push(BlockItem::Expr(if_expr(
        len_ok,
        Block {
            items: inner_items,
            tail: None,
            span: dummy(),
        },
        None,
    )));

    Ok(Expr {
        kind: ExprKind::Block(Block {
            items,
            tail: None,
            span: dummy(),
        }),
        span: dummy(),
    })
}

fn return_handler_call(handler: &str, args: Vec<Arg>, ret: HandlerRet) -> Expr {
    let inner = call(handler, args);
    let value = match ret {
        HandlerRet::Response => inner,
        HandlerRet::Text => {
            // ok(Bytes.fromString(handler(…)), "text/plain")
            let bytes = method_call(
                name("Bytes"),
                "fromString",
                vec![Arg::Pos(inner)],
            );
            call(
                "ok",
                vec![Arg::Pos(bytes), Arg::Pos(str_lit("text/plain"))],
            )
        }
    };
    Expr {
        kind: ExprKind::Return {
            value: Some(Box::new(value)),
        },
        span: dummy(),
    }
}

// ── AST builders ────────────────────────────────────────────────────────────

fn dummy() -> Span {
    Span::dummy()
}

fn ty_string() -> TypeExpr {
    TypeExpr {
        name: "String".into(),
        args: vec![],
        span: dummy(),
        lit: None,
        mark: None,
        union_members: None,
    }
}

fn ty_request() -> TypeExpr {
    TypeExpr {
        name: "Request".into(),
        args: vec![],
        span: dummy(),
        lit: None,
        mark: None,
        union_members: None,
    }
}

fn ty_response() -> TypeExpr {
    TypeExpr {
        name: "Response".into(),
        args: vec![],
        span: dummy(),
        lit: None,
        mark: None,
        union_members: None,
    }
}

fn call(callee: &str, args: Vec<Arg>) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: callee.into(),
            args,
        },
        span: dummy(),
    }
}

fn param(name: &str, ty: TypeExpr) -> Param {
    Param {
        name: name.into(),
        name_span: dummy(),
        positional: false,
        ty,
        default: None,
    }
}

fn str_lit(s: &str) -> Expr {
    Expr {
        kind: ExprKind::String(s.into()),
        span: dummy(),
    }
}

fn int_lit(n: i64) -> Expr {
    Expr {
        kind: ExprKind::Int(n),
        span: dummy(),
    }
}

fn bool_lit(b: bool) -> Expr {
    Expr {
        kind: ExprKind::Bool(b),
        span: dummy(),
    }
}

fn name(s: &str) -> Expr {
    Expr {
        kind: ExprKind::Name(s.into()),
        span: dummy(),
    }
}

fn field(base: Expr, fname: &str) -> Expr {
    Expr {
        kind: ExprKind::Field {
            base: Box::new(base),
            name: fname.into(),
            name_span: dummy(),
        },
        span: dummy(),
    }
}

fn bin(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        span: dummy(),
    }
}

fn method_call(receiver: Expr, method: &str, args: Vec<Arg>) -> Expr {
    Expr {
        kind: ExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: method.into(),
            method_span: dummy(),
            args,
        },
        span: dummy(),
    }
}

fn let_bind(name: &str, mutable: bool, init: Expr) -> BlockItem {
    BlockItem::Let {
        name: name.into(),
        name_span: dummy(),
        mutable,
        ty: None,
        init,
    }
}

fn if_expr(cond: Expr, then_branch: Block, else_branch: Option<Block>) -> Expr {
    Expr {
        kind: ExprKind::If {
            cond: Box::new(cond),
            then_branch,
            else_branch,
        },
        span: dummy(),
    }
}

fn block_stmts(items: Vec<BlockItem>) -> Block {
    Block {
        items,
        tail: None,
        span: dummy(),
    }
}

fn block_expr(e: Expr) -> Block {
    Block {
        items: vec![],
        tail: Some(Box::new(e)),
        span: dummy(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse_desugar(src: &str) -> Module {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let mut module = Parser::new(tokens).parse_module().unwrap();
        desugar_module(&mut module).unwrap();
        module
    }

    #[test]
    fn desugar_exact_and_trailing_capture() {
        let module = parse_desugar(
            r#"
            type MyRouter {
                GET "/home" {
                    "Home"
                }
                PUT "/user/:userId" (userId: String) -> String {
                    "user {userId}"
                }
            }
            fn main() -> Int64 { 0 }
        "#,
        );
        match &module.items[0] {
            Item::Type(td) => {
                assert!(td.routes.is_empty());
                assert_eq!(td.methods.len(), 1);
                assert_eq!(td.methods[0].name, "route");
                assert_eq!(td.methods[0].params.len(), 1);
                assert_eq!(td.methods[0].params[0].name, "req");
                assert_eq!(td.methods[0].params[0].ty.name, "Request");
                let ret = td.methods[0].return_type.as_ref().expect("route ret");
                assert_eq!(ret.name, "Response");
            }
            _ => panic!("expected type"),
        }
        let free: Vec<_> = module
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Function(f) if f.name.starts_with("__MyRouter_route_") => Some(f.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(free, vec!["__MyRouter_route_0", "__MyRouter_route_1"]);
    }

    #[test]
    fn desugar_response_passthrough() {
        let module = parse_desugar(
            r#"
            type MyRouter {
                GET "/home" { "Home" }
                GET "/health" -> Response { notFound() }
            }
            fn main() -> Int64 { 0 }
        "#,
        );
        match &module.items[0] {
            Item::Type(td) => {
                let ret = td.methods[0].return_type.as_ref().expect("route ret");
                assert_eq!(ret.name, "Response");
            }
            _ => panic!("expected type"),
        }
        let handlers: Vec<_> = module
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Function(f) if f.name.starts_with("__MyRouter_route_") => {
                    Some(f.return_type.as_ref().map(|t| t.name.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(handlers, vec![Some("String"), Some("Response")]);
    }
}
