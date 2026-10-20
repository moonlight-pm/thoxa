//! D150: desugar type-body route clauses (`GET "/…" { … }`) into ordinary
//! free-function handlers + a generated `route` method on the type.
//!
//! Handlers are module-level `fn __{Type}_route_N` (not type methods) because
//! Thoxa cannot call private methods via `Type().m()` from another method.
//!
//! `route(req: Request) -> Response`:
//! - clause returns `String` (or omits `->`) → `ok(Bytes.fromString(s), "text/plain")`
//! - clause returns `Response` → pass through
//! - miss → `notFound()`
//! - `Int64` capture parse fail → `badRequest("invalid {name}")`
//!
//! Optional formal `req: Request` is passed through. Path captures (`String` or
//! `Int64`) may appear in any segment (`/user/:userId/edit`).
//!
//! Generated code calls `std/http`'s `Request`, `Response`, `ok`, `notFound`,
//! and `badRequest` by the module's own name when it defines or imports one,
//! and by the mangled name (`std_http_ok`) otherwise, so a module imports only
//! what it writes. Segment split and int parse are generated helpers.
//!
//! Branch `experiment/router-clause-syntax`. No HTTP lang-items.

use crate::ast::*;
use crate::ast_build::{cast_reader_key, match_arm_block, match_expr};
use crate::error::{Error, Result};
use crate::loader::Package;
use crate::span::Span;
use std::collections::HashSet;

const SEGMENTS_FN: &str = "__routeSegments";
const PARSE_INT_FN: &str = "__routeParseInt64";
const PARSE_INT_TY: &str = "__RouteInt64";
/// `__routeBody_K(request) -> Trial<T, Array<Issue>>`: added empty here, given
/// its body by the codec pass, which knows whether `T` reads from a form
/// (D191 §5).
pub const BODY_READER_PREFIX: &str = "__routeBody_";

/// The names generated route code uses for `std/http` items: the module's
/// own when it defines or imports one (hover then shows `Response`), the
/// mangled one otherwise.
struct HttpNames {
    request: String,
    response: String,
    ok: String,
    not_found: String,
    bad_request: String,
}

impl HttpNames {
    fn of(module: &Module) -> Self {
        let mut local: HashSet<String> = HashSet::new();
        for item in &module.items {
            match item {
                Item::Function(f) => {
                    local.insert(f.name.clone());
                }
                Item::Type(td) => {
                    local.insert(td.name.clone());
                }
                Item::Import(import) => {
                    let names = match &import.kind {
                        ImportKind::Selective(names) => names.as_slice(),
                        ImportKind::Combined { names, .. } => names.as_slice(),
                        ImportKind::Namespace { .. } => &[],
                    };
                    for imported in names {
                        let bound = imported.alias.as_ref().map(|(alias, _)| alias).unwrap_or(&imported.name);
                        local.insert(bound.clone());
                    }
                }
                _ => {}
            }
        }
        let pick = |name: &str| {
            if local.contains(name) {
                name.to_string()
            } else {
                format!("std_http_{name}")
            }
        };
        HttpNames {
            request: pick("Request"),
            response: pick("Response"),
            ok: pick("ok"),
            not_found: pick("notFound"),
            bad_request: pick("badRequest"),
        }
    }
}

/// Rewrite every type that has `routes` into handler free functions plus
/// `route(req: Request) -> Response` on the type. Clears `routes`.
pub fn desugar_package(package: &mut Package) -> Result<()> {
    let json = package.modules.iter().any(|m| m.id == "std_json");
    for m in &mut package.modules {
        desugar_module(&mut m.ast).map_err(|error| {
            if error.path.is_none() {
                error.with_path(m.path.clone())
            } else {
                error
            }
        })?;
        let reads_body = m.ast.items.iter().any(|item| {
            matches!(item, Item::Function(f) if f.name.starts_with(BODY_READER_PREFIX))
        });
        if reads_body && !json {
            return Err(Error::new(
                "a typed route body parameter reads JSON and forms: import { Issue } from \"std/json\"",
            )
            .with_path(m.path.clone()));
        }
    }
    Ok(())
}

/// Public for LSP resolve / tests (also invoked from package lower).
pub fn desugar_module(module: &mut Module) -> Result<()> {
    let mut extra_fns: Vec<Function> = Vec::new();
    let mut extra_types: Vec<TypeDef> = Vec::new();
    let mut need_segments = false;
    let mut need_parse_int = false;
    let mut bodies: Vec<TypeExpr> = Vec::new();
    let names = HttpNames::of(module);
    for item in &mut module.items {
        if let Item::Type(td) = item {
            let (fns, flags) = desugar_type(td, &names)?;
            extra_fns.extend(fns);
            need_segments |= flags.need_segments;
            need_parse_int |= flags.need_parse_int;
            for target in flags.bodies {
                if !bodies.iter().any(|seen| cast_reader_key(seen) == cast_reader_key(&target)) {
                    bodies.push(target);
                }
            }
        }
    }
    for target in &bodies {
        extra_fns.push(body_reader_stub(target, &names));
    }
    if need_parse_int {
        extra_types.push(parse_int_type());
        extra_fns.push(parse_int_fn());
    }
    if need_segments {
        extra_fns.push(segments_fn());
    }
    for t in extra_types {
        module.items.push(Item::Type(t));
    }
    for f in extra_fns {
        module.items.push(Item::Function(f));
    }
    Ok(())
}

struct DesugarFlags {
    need_segments: bool,
    need_parse_int: bool,
    /// Types read by typed body parameters (one reader each per module).
    bodies: Vec<TypeExpr>,
}

/// Returns synthetic free functions for handlers (to append to the module).
fn desugar_type(td: &mut TypeDef, names: &HttpNames) -> Result<(Vec<Function>, DesugarFlags)> {
    let empty = DesugarFlags {
        need_segments: false,
        need_parse_int: false,
        bodies: Vec::new(),
    };
    if td.routes.is_empty() {
        return Ok((Vec::new(), empty));
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
             are not supported (use a nullary type)",
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
    let mut flags = empty;

    for (i, r) in routes.into_iter().enumerate() {
        let handler_name = format!("__{type_name}_route_{i}");
        let pat = analyze_pattern(&r.pattern, r.pattern_span)?;
        let slots = validate_formals(&r, &pat)?;
        if matches!(pat, PathPattern::Segments(_)) {
            flags.need_segments = true;
        }
        if slots.iter().any(|s| {
            matches!(
                s,
                CallSlot::Cap {
                    ty: CapTy::Int64,
                    ..
                }
            )
        }) {
            flags.need_parse_int = true;
        }
        for slot in &slots {
            if let CallSlot::Body { target, .. } = slot {
                flags.bodies.push(target.clone());
            }
        }

        let ret_kind = classify_return(&r)?;
        let ret_ty = match ret_kind {
            HandlerRet::Text => ty_string(),
            HandlerRet::Response => ty_named(&names.response),
        };

        handlers.push(HandlerMeta {
            http_method: r.method.clone(),
            pattern: pat,
            handler_name: handler_name.clone(),
            def_span: r.method_span,
            ret: ret_kind,
            slots,
        });

        free_fns.push(Function {
            exported: false,
            name: handler_name,
            name_span: r.method_span,
            // Only the generated `route` calls a handler, by position (D200).
            params: r
                .params
                .into_iter()
                .map(|p| Param {
                    positional: true,
                    ..p
                })
                .collect(),
            return_type: Some(ret_ty),
            body: r.body,
            type_params: Vec::new(),
            generic: Vec::new(),
        });
    }

    let route_span = handlers.first().map(|h| h.def_span).unwrap_or(td.name_span);
    td.methods.push(build_route_method(&handlers, route_span, names)?);
    Ok((free_fns, flags))
}

struct HandlerMeta {
    http_method: String,
    pattern: PathPattern,
    handler_name: String,
    def_span: Span,
    ret: HandlerRet,
    slots: Vec<CallSlot>,
}

#[derive(Clone, Copy)]
enum HandlerRet {
    /// Omitted or `-> String` — wrap with `ok(…, "text/plain")`.
    Text,
    /// `-> Response` — return the handler value as-is.
    Response,
}

#[derive(Clone)]
enum CallSlot {
    Req,
    Cap { name: String, ty: CapTy, seg: usize },
    /// A parameter that reads the request body (or a GET's query) into
    /// `target` (D191 §5). With `handles_failure` (the parameter is
    /// `Trial<T, Array<Issue>>`) the clause gets the read's result; otherwise a
    /// failed read is answered with a 400 before the clause runs.
    Body { target: TypeExpr, handles_failure: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CapTy {
    String,
    Int64,
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

#[derive(Clone)]
enum PathPattern {
    Exact(String),
    Segments(Vec<Seg>),
}

#[derive(Clone)]
enum Seg {
    Lit(String),
    Capture(String),
}

impl PathPattern {
    fn captures(&self) -> Vec<(usize, String)> {
        match self {
            PathPattern::Exact(_) => Vec::new(),
            PathPattern::Segments(segs) => segs
                .iter()
                .enumerate()
                .filter_map(|(i, s)| match s {
                    Seg::Capture(n) => Some((i, n.clone())),
                    Seg::Lit(_) => None,
                })
                .collect(),
        }
    }
}

fn analyze_pattern(pattern: &str, span: Span) -> Result<PathPattern> {
    if pattern.is_empty() {
        return Err(Error::at("route pattern must not be empty", span));
    }
    if !pattern.starts_with('/') {
        return Err(Error::at("route pattern must start with `/`", span));
    }
    let raw: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let mut segs: Vec<Seg> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut any_cap = false;
    for s in &raw {
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
            if !seen.insert(rest.to_string()) {
                return Err(Error::at(format!("duplicate path capture `:{rest}`"), span));
            }
            segs.push(Seg::Capture(rest.to_string()));
            any_cap = true;
        } else if s.contains(':') {
            return Err(Error::at(
                "invalid `:` in path segment (use `:name` for a capture)",
                span,
            ));
        } else {
            segs.push(Seg::Lit((*s).to_string()));
        }
    }
    if any_cap {
        Ok(PathPattern::Segments(segs))
    } else {
        Ok(PathPattern::Exact(pattern.to_string()))
    }
}

fn validate_formals(r: &RouteClause, pat: &PathPattern) -> Result<Vec<CallSlot>> {
    let caps = pat.captures();
    let cap_names: HashSet<&str> = caps.iter().map(|(_, n)| n.as_str()).collect();
    let mut used_caps: HashSet<&str> = HashSet::new();
    let mut have_req = false;
    let mut slots: Vec<CallSlot> = Vec::new();

    let mut have_body = false;
    for p in &r.params {
        if let Some((target, handles_failure)) = body_target(p, &cap_names) {
            if have_body {
                return Err(Error::at(
                    "a route clause reads the request body into one parameter at most",
                    p.name_span,
                ));
            }
            have_body = true;
            slots.push(CallSlot::Body {
                target,
                handles_failure,
            });
            continue;
        }
        if p.ty.name == "Request" && !cap_names.contains(p.name.as_str()) {
            if have_req {
                return Err(Error::at(
                    "route clause can take at most one `Request` formal",
                    p.name_span,
                ));
            }
            have_req = true;
            slots.push(CallSlot::Req);
            continue;
        }
        let Some(&(seg, _)) = caps.iter().find(|(_, n)| n == &p.name) else {
            return Err(Error::at(
                format!(
                    "parameter `{}` is not a path capture and is not `Request`",
                    p.name
                ),
                p.name_span,
            ));
        };
        if !used_caps.insert(p.name.as_str()) {
            return Err(Error::at(
                format!("duplicate capture parameter `{}`", p.name),
                p.name_span,
            ));
        }
        let ty = match p.ty.name.as_str() {
            "String" => CapTy::String,
            "Int64" => CapTy::Int64,
            other => {
                return Err(Error::at(
                    format!(
                        "path capture `{0}` must be String or Int64 (got `{other}`)",
                        p.name
                    ),
                    p.name_span,
                ));
            }
        };
        slots.push(CallSlot::Cap {
            name: p.name.clone(),
            ty,
            seg,
        });
    }

    for (_, n) in &caps {
        if !used_caps.contains(n.as_str()) {
            return Err(Error::at(
                format!("path capture `:{n}` needs a matching parameter `{n}`"),
                r.method_span,
            ));
        }
    }
    Ok(slots)
}

/// A parameter that is neither a path capture nor the `Request` reads the
/// body: `T` (a named type or an `Array<T>`, D218) or `Trial<T, Array<Issue>>`.
fn body_target(p: &Param, cap_names: &HashSet<&str>) -> Option<(TypeExpr, bool)> {
    if cap_names.contains(p.name.as_str()) || p.ty.name == "Request" {
        return None;
    }
    if p.ty.name == "Trial" && p.ty.args.len() == 2 {
        if let TypeArg::Type(inner) = &p.ty.args[0] {
            return Some((inner.clone(), true));
        }
    }
    let plain = p.ty.union_members.is_none() && p.ty.lit.is_none();
    let array = p.ty.name == "Array" && p.ty.args.len() == 1;
    (plain && (p.ty.args.is_empty() || array)).then(|| (p.ty.clone(), false))
}

/// `fn __routeBody_K(request: Request) -> Trial<T, Array<Issue>> {}`
fn body_reader_stub(target: &TypeExpr, names: &HttpNames) -> Function {
    let issues = ty_array(ty_named("std_json_Issue"));
    let trial = TypeExpr {
        name: "Trial".into(),
        args: vec![TypeArg::Type(target.clone()), TypeArg::Type(issues)],
        span: dummy(),
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    };
    Function {
        exported: false,
        name: format!("{BODY_READER_PREFIX}{}", cast_reader_key(target)),
        name_span: dummy(),
        params: vec![param("request", ty_named(&names.request))],
        return_type: Some(trial),
        body: Block {
            items: vec![],
            tail: None,
            span: dummy(),
        },
        type_params: Vec::new(),
        generic: Vec::new(),
    }
}

/// The handler call, after reading a typed body parameter when there is one:
/// 415 for a body it does not read, then the read, then a 400 for a failed
/// read (unless the clause takes the `Trial`).
fn handler_call(h: &HandlerMeta, names: &HttpNames) -> Expr {
    let returned = return_handler_call(&h.handler_name, call_args(&h.slots), h.ret, names);
    let Some((target, handles_failure)) = h.slots.iter().find_map(|slot| match slot {
        CallSlot::Body {
            target,
            handles_failure,
        } => Some((target, *handles_failure)),
        _ => None,
    }) else {
        return returned;
    };
    let unsupported = if_expr(
        bin(
            BinaryOp::Eq,
            call("std_http_bodyKind", vec![Arg::Pos(name("req"))]),
            str_lit("other"),
        ),
        block_stmts(vec![BlockItem::Expr(return_value(call(
            "std_http_unsupportedMediaType",
            vec![],
        )))]),
        None,
    );
    let read = let_bind(
        "__body",
        false,
        call(
            &format!("{BODY_READER_PREFIX}{}", cast_reader_key(target)),
            vec![Arg::Pos(name("req"))],
        ),
    );
    let rest = if handles_failure {
        returned
    } else {
        match_expr(
            name("__body"),
            vec![
                match_arm_block(
                    "Fail",
                    vec![Some("__issues".into())],
                    vec![BlockItem::Expr(return_value(call(
                        "std_http_issuesResponse",
                        vec![Arg::Pos(name("req")), Arg::Pos(name("__issues"))],
                    )))],
                    None,
                ),
                match_arm_block(
                    "Pass",
                    vec![Some("__value".into())],
                    vec![BlockItem::Expr(returned)],
                    None,
                ),
            ],
        )
    };
    Expr {
        kind: ExprKind::Block(block_stmts(vec![
            BlockItem::Expr(unsupported),
            read,
            BlockItem::Expr(rest),
        ])),
        span: dummy(),
    }
}

fn return_value(value: Expr) -> Expr {
    Expr {
        kind: ExprKind::Return {
            value: Some(Box::new(value)),
        },
        span: dummy(),
    }
}

fn build_route_method(handlers: &[HandlerMeta], name_span: Span, names: &HttpNames) -> Result<Method> {
    let mut items: Vec<BlockItem> = Vec::new();
    for h in handlers {
        items.push(BlockItem::Expr(handler_try_if(h, names)?));
    }
    let body = Block {
        items,
        tail: Some(Box::new(call(&names.not_found, vec![]))),
        span: name_span,
    };
    Ok(Method {
        exported: true,
        name: "route".into(),
        name_span,
        params: vec![param("req", ty_named(&names.request))],
        return_type: Some(ty_named(&names.response)),
        body,
        type_params: Vec::new(),
        provided: None,
        requires: Vec::new(),
        generic: Vec::new(),
    })
}

fn handler_try_if(h: &HandlerMeta, names: &HttpNames) -> Result<Expr> {
    let method_ok = bin(
        BinaryOp::Eq,
        field(name("req"), "method"),
        str_lit(&h.http_method),
    );
    let inner = match &h.pattern {
        PathPattern::Exact(pat) => {
            let path_ok = bin(BinaryOp::Eq, field(name("req"), "path"), str_lit(pat));
            let call = handler_call(h, names);
            if_expr(path_ok, block_stmts(vec![BlockItem::Expr(call)]), None)
        }
        PathPattern::Segments(segs) => segments_block(h, segs, names)?,
    };
    // Statement `if` (not a tail): then contains `return`, else is absent.
    Ok(if_expr(
        method_ok,
        block_stmts(vec![BlockItem::Expr(inner)]),
        None,
    ))
}

fn call_args(slots: &[CallSlot]) -> Vec<Arg> {
    slots
        .iter()
        .map(|s| match s {
            CallSlot::Req => Arg::Pos(name("req")),
            CallSlot::Cap {
                name: n,
                ty: CapTy::Int64,
                ..
            } => Arg::Pos(name(n)),
            CallSlot::Cap { seg, .. } => Arg::Pos(index(name("segs"), int_lit(*seg as i64))),
            CallSlot::Body {
                handles_failure: true,
                ..
            } => Arg::Pos(name("__body")),
            CallSlot::Body { .. } => Arg::Pos(name("__value")),
        })
        .collect()
}

fn segments_block(h: &HandlerMeta, segs: &[Seg], names: &HttpNames) -> Result<Expr> {
    let n = segs.len() as i64;
    // All `if`s here are statements (no tail) so a missing else is Void, not
    // a Response-vs-Void lub. Hits `return` from the then branch.
    let mut core = handler_call(h, names);

    for slot in h.slots.iter().rev() {
        let CallSlot::Cap {
            name: cap,
            ty: CapTy::Int64,
            seg,
        } = slot
        else {
            continue;
        };
        let parsed = call(
            PARSE_INT_FN,
            vec![Arg::Pos(index(name("segs"), int_lit(*seg as i64)))],
        );
        let then = Block {
            items: vec![
                let_bind(cap, false, field(name("parsed"), "value")),
                BlockItem::Expr(core),
            ],
            tail: None,
            span: dummy(),
        };
        let else_b = block_stmts(vec![BlockItem::Expr(Expr {
            kind: ExprKind::Return {
                value: Some(Box::new(call(
                    &names.bad_request,
                    vec![Arg::Pos(str_lit(&format!("invalid {cap}")))],
                ))),
            },
            span: dummy(),
        })]);
        core = Expr {
            kind: ExprKind::Block(Block {
                items: vec![
                    let_bind("parsed", false, parsed),
                    BlockItem::Expr(if_expr(field(name("parsed"), "ok"), then, Some(else_b))),
                ],
                tail: None,
                span: dummy(),
            }),
            span: dummy(),
        };
    }

    for (i, seg) in segs.iter().enumerate().rev() {
        let Seg::Lit(lit) = seg else { continue };
        let ok = bin(
            BinaryOp::Eq,
            index(name("segs"), int_lit(i as i64)),
            str_lit(lit),
        );
        core = if_expr(ok, as_stmt_block(core), None);
    }

    let count_ok = bin(BinaryOp::Eq, field(name("segs"), "count"), int_lit(n));
    let after_count = if_expr(count_ok, as_stmt_block(core), None);

    Ok(Expr {
        kind: ExprKind::Block(Block {
            items: vec![
                let_bind(
                    "segs",
                    false,
                    call(SEGMENTS_FN, vec![Arg::Pos(field(name("req"), "path"))]),
                ),
                BlockItem::Expr(after_count),
            ],
            tail: None,
            span: dummy(),
        }),
        span: dummy(),
    })
}

fn as_stmt_block(e: Expr) -> Block {
    match e.kind {
        ExprKind::Block(b) => b,
        _ => block_stmts(vec![BlockItem::Expr(e)]),
    }
}

fn return_handler_call(handler: &str, args: Vec<Arg>, ret: HandlerRet, names: &HttpNames) -> Expr {
    let inner = call(handler, args);
    let value = match ret {
        HandlerRet::Response => inner,
        HandlerRet::Text => {
            let bytes = method_call(name("Bytes"), "fromString", vec![Arg::Pos(inner)]);
            call(&names.ok, vec![Arg::Pos(bytes), Arg::Pos(str_lit("text/plain"))])
        }
    };
    Expr {
        kind: ExprKind::Return {
            value: Some(Box::new(value)),
        },
        span: dummy(),
    }
}

// ── Generated helpers ───────────────────────────────────────────────────────

fn parse_int_type() -> TypeDef {
    TypeDef {
        exported: false,
        name: PARSE_INT_TY.into(),
        name_span: dummy(),
        type_params: vec![],
        params: vec![labelled(param("ok", ty_bool())), labelled(param("value", ty_int64()))],
        fields: vec![],
        methods: vec![],
        routes: vec![],
        conversions: vec![],
        union_members: None,
        traits: vec![],
        conditional: vec![],
        derived: vec![],
        param_bounds: Vec::new(),
        bounds: Vec::new(),
        assoc_bounds: Vec::new(),
        mixed: None,
        assoc: Vec::new(),
        checking: Vec::new(),
    }
}

fn parse_int_ctor(ok: Expr, value: Expr) -> Expr {
    call(
        PARSE_INT_TY,
        vec![
            Arg::Named {
                name: "ok".into(),
                name_span: dummy(),
                value: ok,
            },
            Arg::Named {
                name: "value".into(),
                name_span: dummy(),
                value,
            },
        ],
    )
}

fn parse_int_fn() -> Function {
    // Digit walk; empty / non-digits → ok: false. Optional leading '-'.
    let fail = parse_int_ctor(bool_lit(false), int_lit(0));
    let ok_val = parse_int_ctor(bool_lit(true), bin(BinaryOp::Mul, name("n"), name("sign")));

    let loop_body = Block {
        items: vec![
            BlockItem::Expr(if_expr(
                un(UnaryOp::Not, name("good")),
                block_stmts(vec![BlockItem::Expr(brk())]),
                None,
            )),
            BlockItem::Expr(if_expr(
                bin(BinaryOp::Ge, name("i"), field(name("b"), "count")),
                block_stmts(vec![BlockItem::Expr(brk())]),
                None,
            )),
            let_bind(
                "c",
                false,
                method_call(name("b"), "get", vec![Arg::Pos(name("i"))]),
            ),
            BlockItem::Expr(if_expr(
                bin(BinaryOp::Lt, name("c"), int_lit(48)),
                block_stmts(vec![assign("good", bool_lit(false))]),
                Some(block_stmts(vec![BlockItem::Expr(if_expr(
                    bin(BinaryOp::Gt, name("c"), int_lit(57)),
                    block_stmts(vec![assign("good", bool_lit(false))]),
                    Some(block_stmts(vec![
                        assign(
                            "n",
                            bin(
                                BinaryOp::Add,
                                bin(BinaryOp::Mul, name("n"), int_lit(10)),
                                bin(BinaryOp::Sub, name("c"), int_lit(48)),
                            ),
                        ),
                        assign("i", bin(BinaryOp::Add, name("i"), int_lit(1))),
                    ])),
                ))])),
            )),
        ],
        tail: None,
        span: dummy(),
    };

    let after_sign = Block {
        items: vec![BlockItem::Expr(Expr {
            kind: ExprKind::Loop { body: loop_body },
            span: dummy(),
        })],
        tail: Some(Box::new(if_expr(
            name("good"),
            block_expr(ok_val),
            Some(block_expr(fail.clone())),
        ))),
        span: dummy(),
    };

    let minus_body = Block {
        items: vec![assign("sign", int_lit(-1)), assign("i", int_lit(1))],
        tail: Some(Box::new(if_expr(
            bin(BinaryOp::Eq, field(name("b"), "count"), int_lit(1)),
            block_stmts(vec![assign("good", bool_lit(false))]),
            None,
        ))),
        span: dummy(),
    };

    let nonempty = Block {
        items: vec![
            let_bind("i", true, int_lit(0)),
            let_bind("sign", true, int_lit(1)),
            let_bind("n", true, int_lit(0)),
            let_bind("good", true, bool_lit(true)),
            BlockItem::Expr(if_expr(
                bin(
                    BinaryOp::Eq,
                    method_call(name("b"), "get", vec![Arg::Pos(int_lit(0))]),
                    int_lit(45),
                ),
                minus_body,
                None,
            )),
        ],
        tail: Some(Box::new(Expr {
            kind: ExprKind::Block(after_sign),
            span: dummy(),
        })),
        span: dummy(),
    };

    let body = Block {
        items: vec![let_bind(
            "b",
            false,
            method_call(name("Bytes"), "fromString", vec![Arg::Pos(name("s"))]),
        )],
        tail: Some(Box::new(if_expr(
            bin(BinaryOp::Eq, field(name("b"), "count"), int_lit(0)),
            block_expr(fail),
            Some(nonempty),
        ))),
        span: dummy(),
    };

    Function {
        exported: false,
        name: PARSE_INT_FN.into(),
        name_span: dummy(),
        params: vec![param("s", ty_string())],
        return_type: Some(ty_named(PARSE_INT_TY)),
        body,
        type_params: Vec::new(),
        generic: Vec::new(),
    }
}

fn segments_fn() -> Function {
    let slash = int_lit(47);
    let loop_body = Block {
        items: vec![
            BlockItem::Expr(if_expr(
                bin(BinaryOp::Ge, name("i"), name("n")),
                block_stmts(vec![BlockItem::Expr(brk())]),
                None,
            )),
            BlockItem::Expr(if_expr(
                bin(
                    BinaryOp::Eq,
                    method_call(name("data"), "get", vec![Arg::Pos(name("i"))]),
                    slash,
                ),
                block_stmts(vec![
                    BlockItem::Expr(if_expr(
                        bin(BinaryOp::Gt, name("i"), name("start")),
                        block_stmts(vec![BlockItem::Expr(method_call(
                            name("segs"),
                            "append",
                            vec![Arg::Pos(method_call(
                                method_call(
                                    name("data"),
                                    "slice",
                                    vec![Arg::Pos(name("start")), Arg::Pos(name("i"))],
                                ),
                                "toString",
                                vec![],
                            ))],
                        ))]),
                        None,
                    )),
                    assign("start", bin(BinaryOp::Add, name("i"), int_lit(1))),
                ]),
                None,
            )),
            assign("i", bin(BinaryOp::Add, name("i"), int_lit(1))),
        ],
        tail: None,
        span: dummy(),
    };

    let body = Block {
        items: vec![
            let_bind(
                "data",
                false,
                method_call(name("Bytes"), "fromString", vec![Arg::Pos(name("path"))]),
            ),
            let_bind_ty(
                "segs",
                true,
                ty_array(ty_string()),
                Expr {
                    kind: ExprKind::ArrayLit(vec![]),
                    span: dummy(),
                },
            ),
            let_bind("start", true, int_lit(0)),
            let_bind("i", true, int_lit(0)),
            let_bind("n", false, field(name("data"), "count")),
            BlockItem::Expr(Expr {
                kind: ExprKind::Loop { body: loop_body },
                span: dummy(),
            }),
            BlockItem::Expr(if_expr(
                bin(BinaryOp::Lt, name("start"), name("n")),
                block_stmts(vec![BlockItem::Expr(method_call(
                    name("segs"),
                    "append",
                    vec![Arg::Pos(method_call(
                        method_call(
                            name("data"),
                            "slice",
                            vec![Arg::Pos(name("start")), Arg::Pos(name("n"))],
                        ),
                        "toString",
                        vec![],
                    ))],
                ))]),
                None,
            )),
        ],
        tail: Some(Box::new(name("segs"))),
        span: dummy(),
    };

    Function {
        exported: false,
        name: SEGMENTS_FN.into(),
        name_span: dummy(),
        params: vec![param("path", ty_string())],
        return_type: Some(ty_array(ty_string())),
        body,
        type_params: Vec::new(),
        generic: Vec::new(),
    }
}

// ── AST builders ────────────────────────────────────────────────────────────

fn dummy() -> Span {
    Span::dummy()
}

fn ty_string() -> TypeExpr {
    ty_named("String")
}


fn ty_bool() -> TypeExpr {
    ty_named("Bool")
}

fn ty_int64() -> TypeExpr {
    ty_named("Int64")
}

fn ty_named(name: &str) -> TypeExpr {
    TypeExpr {
        name: name.into(),
        args: vec![],
        span: dummy(),
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    }
}

fn ty_array(elem: TypeExpr) -> TypeExpr {
    TypeExpr {
        name: "Array".into(),
        args: vec![TypeArg::Type(elem)],
        span: dummy(),
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    }
}

fn call(callee: &str, args: Vec<Arg>) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: callee.into(),
            args,
            type_args: Vec::new(),
        },
        span: dummy(),
    }
}

/// A generated constructor field, passed by label (D200).
fn labelled(param: Param) -> Param {
    Param {
        positional: false,
        ..param
    }
}

fn param(name: &str, ty: TypeExpr) -> Param {
    Param {
        name: name.into(),
        name_span: dummy(),
        positional: true,
        mutable: false,
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

fn index(base: Expr, i: Expr) -> Expr {
    Expr {
        kind: ExprKind::Index {
            base: Box::new(base),
            index: Box::new(i),
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

fn un(op: UnaryOp, expr: Expr) -> Expr {
    Expr {
        kind: ExprKind::Unary {
            op,
            expr: Box::new(expr),
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
            type_args: Vec::new(),
        },
        span: dummy(),
    }
}

fn let_bind_ty(name: &str, mutable: bool, ty: TypeExpr, init: Expr) -> BlockItem {
    BlockItem::Let {
        name: name.into(),
        name_span: dummy(),
        mutable,
        ty: Some(ty),
        init,
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

fn assign(name: &str, value: Expr) -> BlockItem {
    BlockItem::Assign {
        target: AssignTarget::Name {
            name: name.into(),
            name_span: dummy(),
        },
        value,
    }
}

fn brk() -> Expr {
    Expr {
        kind: ExprKind::Break,
        span: dummy(),
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

    fn parse_desugar_err(src: &str) -> String {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let mut module = Parser::new(tokens).parse_module().unwrap();
        desugar_module(&mut module).unwrap_err().to_string()
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
                // Nothing imported: generated code names std/http itself.
                assert_eq!(td.methods[0].params[0].ty.name, "std_http_Request");
                let ret = td.methods[0].return_type.as_ref().expect("route ret");
                assert_eq!(ret.name, "std_http_Response");
            }
            _ => panic!("expected type"),
        }
        let free: Vec<_> = module
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Function(f) if f.name.starts_with("__MyRouter_route_") => {
                    Some(f.name.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(free, vec!["__MyRouter_route_0", "__MyRouter_route_1"]);
        assert!(
            module
                .items
                .iter()
                .any(|i| matches!(i, Item::Function(f) if f.name == SEGMENTS_FN)),
            "expected {SEGMENTS_FN}"
        );
    }

    #[test]
    fn desugar_response_passthrough() {
        let module = parse_desugar(
            r#"
            import { Request, Response, notFound } from "std/http"
            type MyRouter {
                GET "/home" { "Home" }
                GET "/health" -> Response { notFound() }
            }
            fn main() -> Int64 { 0 }
        "#,
        );
        // `Response` is imported: generated code uses the module's own name.
        match &module.items[1] {
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

    #[test]
    fn desugar_int64_and_mid_path_and_req() {
        let module = parse_desugar(
            r#"
            type MyRouter {
                GET "/home" (req: Request) { "Home" }
                GET "/user/:userId" (userId: Int64) -> String { "u" }
                GET "/user/:userId/edit" (req: Request, userId: Int64) -> String { "e" }
            }
            fn main() -> Int64 { 0 }
        "#,
        );
        assert!(
            module
                .items
                .iter()
                .any(|i| matches!(i, Item::Type(t) if t.name == PARSE_INT_TY)),
            "expected {PARSE_INT_TY}"
        );
        assert!(
            module
                .items
                .iter()
                .any(|i| matches!(i, Item::Function(f) if f.name == PARSE_INT_FN)),
            "expected {PARSE_INT_FN}"
        );
        let edit = module
            .items
            .iter()
            .find_map(|i| match i {
                Item::Function(f) if f.name == "__MyRouter_route_2" => Some(f),
                _ => None,
            })
            .expect("edit handler");
        assert_eq!(edit.params.len(), 2);
        assert_eq!(edit.params[0].name, "req");
        assert_eq!(edit.params[0].ty.name, "Request");
        assert_eq!(edit.params[1].name, "userId");
        assert_eq!(edit.params[1].ty.name, "Int64");
    }

    #[test]
    fn desugar_rejects_unknown_capture_type() {
        let err = parse_desugar_err(
            r#"
            type MyRouter {
                GET "/user/:userId" (userId: Bool) -> String { "x" }
            }
            fn main() -> Int64 { 0 }
        "#,
        );
        assert!(err.contains("String or Int64"), "{err}");
    }

    #[test]
    fn desugar_rejects_missing_capture_param() {
        let err = parse_desugar_err(
            r#"
            type MyRouter {
                GET "/user/:userId" { "x" }
            }
            fn main() -> Int64 { 0 }
        "#,
        );
        assert!(err.contains("userId"), "{err}");
    }
}
