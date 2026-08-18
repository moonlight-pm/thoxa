//! Experiment: generate `Type.read` / `Type.fromJson` helpers for constructor-param
//! products (builtin validation, slice 1 — no `where` yet).
//!
//! For each eligible product type in a non-`std/` module, emit:
//! - `__fromJson_T(v: Json, path: String = "") -> Trial<T, Issue>`
//! - `__read_T(s: String) -> Trial<T, Issue>`
//!
//! Call sites `T.read(s)` / `T.fromJson(v)` are rewritten in MIR to these fns.
//! The defining module must be able to see `std/json` + `std/fallible` names;
//! missing names are merged into existing imports (or a new selective import).

use crate::ast::*;
use crate::error::{Error, Result};
use crate::loader::{self, BoundImport, Package};
use crate::span::Span;
use std::collections::{HashMap, HashSet};

const JSON_PATH: &str = "std/json";
const FALLIBLE_PATH: &str = "std/fallible";

const JSON_CORE: &[&str] = &["Json", "Issue", "readJson", "asObj"];

const FALLIBLE_NAMES: &[&str] = &["Pass", "Fail", "Trial"];

pub fn desugar_package(package: &mut Package) -> Result<()> {
    let import_maps = loader::build_import_maps(package)?;
    let mut world: HashMap<String, HashMap<String, TypeDef>> = HashMap::new();
    for m in &package.modules {
        let mut tys = HashMap::new();
        for item in &m.ast.items {
            if let Item::Type(td) = item {
                tys.insert(td.name.clone(), td.clone());
            }
        }
        world.insert(m.id.clone(), tys);
    }
    let mut resolve: HashMap<String, HashMap<String, (String, String)>> = HashMap::new();
    for m in &package.modules {
        let mut map = HashMap::new();
        for item in &m.ast.items {
            if let Item::Type(td) = item {
                map.insert(td.name.clone(), (m.id.clone(), td.name.clone()));
            }
        }
        if let Some(imports) = import_maps.get(&m.id) {
            for (local, b) in imports {
                if let BoundImport::Name {
                    from_module,
                    original,
                } = b
                {
                    if world
                        .get(from_module)
                        .is_some_and(|t| t.contains_key(original))
                    {
                        map.insert(local.clone(), (from_module.clone(), original.clone()));
                    }
                }
            }
        }
        resolve.insert(m.id.clone(), map);
    }

    for m in &mut package.modules {
        if is_std_module(&m.id) {
            continue;
        }
        desugar_module_with(&mut m.ast, &m.id, &world, &resolve)?;
    }
    Ok(())
}

/// Single-file / LSP path: generate readers for local eligible products.
pub fn desugar_module(module: &mut Module) -> Result<()> {
    let mut world = HashMap::new();
    let mut local = HashMap::new();
    let mut resolve_local = HashMap::new();
    for item in &module.items {
        if let Item::Type(td) = item {
            local.insert(td.name.clone(), td.clone());
            resolve_local.insert(td.name.clone(), ("local".into(), td.name.clone()));
        }
    }
    world.insert("local".into(), local);
    let mut resolve = HashMap::new();
    resolve.insert("local".into(), resolve_local);
    desugar_module_with(module, "local", &world, &resolve)
}

fn is_std_module(id: &str) -> bool {
    id == "std" || id.starts_with("std_")
}

fn desugar_module_with(
    module: &mut Module,
    module_id: &str,
    world: &HashMap<String, HashMap<String, TypeDef>>,
    resolve: &HashMap<String, HashMap<String, (String, String)>>,
) -> Result<()> {
    let mut extra: Vec<Function> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut json_need: HashSet<String> = JSON_CORE.iter().map(|s| (*s).to_string()).collect();
    for item in &module.items {
        if let Item::Type(td) = item {
            names.push(td.name.clone());
        }
    }
    let mut any = false;
    for name in names {
        let td = {
            let Item::Type(td) = module
                .items
                .iter()
                .find(|it| matches!(it, Item::Type(t) if t.name == name))
                .unwrap()
            else {
                continue;
            };
            td.clone()
        };
        if let Some((base, clause)) = refined_alias_of(&td, module_id, world, resolve) {
            extra.push(gen_from_refined(&td, base, &clause));
            json_need.insert("Issue".into());
            json_need.insert("issueMismatch".into());
            any = true;
            continue;
        }
        if !is_eligible(&td, module_id, world, resolve, &mut HashSet::new()) {
            continue;
        }
        if td.methods.iter().any(|m| m.name == "read" || m.name == "fromJson") {
            return Err(Error::at(
                format!(
                    "type `{}` already has `read` / `fromJson`; rename so builtin validation can attach them",
                    td.name
                ),
                td.name_span,
            ));
        }
        for p in &td.params {
            if let Some(sh) = shape_of(
                &p.ty,
                module_id,
                world,
                resolve,
                &mut HashSet::new(),
            ) {
                need_json_names(&sh, &mut json_need);
            }
        }
        extra.push(gen_from_json(&td, module_id, world, resolve)?);
        extra.push(gen_read(&td));
        any = true;
    }
    if !any {
        return Ok(());
    }
    if !module_imports_path(module, JSON_PATH) {
        // Loader already ran: do not inject a new `std/json` specifier.
        return Ok(());
    }
    let json_names: Vec<&str> = json_need.iter().map(|s| s.as_str()).collect();
    ensure_import(module, JSON_PATH, &json_names);
    if module_imports_path(module, FALLIBLE_PATH) {
        ensure_import(module, FALLIBLE_PATH, FALLIBLE_NAMES);
    }
    for f in extra {
        module.items.push(Item::Function(f));
    }
    Ok(())
}

fn module_imports_path(module: &Module, path: &str) -> bool {
    module.items.iter().any(|it| match it {
        Item::Import(imp) => imp.path == path,
        _ => false,
    })
}

fn is_eligible(
    td: &TypeDef,
    module_id: &str,
    world: &HashMap<String, HashMap<String, TypeDef>>,
    resolve: &HashMap<String, HashMap<String, (String, String)>>,
    visiting: &mut HashSet<String>,
) -> bool {
    if td.union_members.is_some() {
        return false;
    }
    if !td.type_params.is_empty() {
        return false;
    }
    if !td.fields.is_empty() {
        return false;
    }
    if td.params.is_empty() {
        return false;
    }
    let key = format!("{module_id}::{}", td.name);
    if !visiting.insert(key.clone()) {
        return true;
    }
    let ok = td.params.iter().all(|p| {
        shape_of(&p.ty, module_id, world, resolve, visiting).is_some()
    });
    visiting.remove(&key);
    ok
}

#[derive(Clone)]
enum Shape {
    Str,
    Int,
    Bool,
    Float,
    Array(Box<Shape>),
    Product(String),
    Refined { name: String, base: String },
}

fn need_json_names(shape: &Shape, need: &mut HashSet<String>) {
    match shape {
        Shape::Str => {
            need.insert("strField".into());
        }
        Shape::Int => {
            need.insert("intField".into());
        }
        Shape::Bool => {
            need.insert("boolField".into());
        }
        Shape::Float => {
            need.insert("floatField".into());
        }
        Shape::Product(_) => {
            need.insert("field".into());
            need.insert("joinPath".into());
        }
        Shape::Array(elem) => {
            need.insert("field".into());
            need.insert("asArr".into());
            need.insert("joinPath".into());
            need_json_names(elem, need);
            match elem.as_ref() {
                Shape::Str => {
                    need.insert("asStr".into());
                }
                Shape::Int => {
                    need.insert("asInt".into());
                }
                Shape::Bool => {
                    need.insert("asBool".into());
                }
                Shape::Float => {
                    need.insert("asFloat".into());
                }
                _ => {}
            }
        }
        Shape::Refined { base, .. } => {
            need.insert("joinPath".into());
            match base.as_str() {
                "String" => {
                    need.insert("strField".into());
                }
                "Float64" => {
                    need.insert("floatField".into());
                }
                "Bool" => {
                    need.insert("boolField".into());
                }
                _ => {
                    need.insert("intField".into());
                }
            }
        }
    }
}

fn shape_of(
    ty: &TypeExpr,
    module_id: &str,
    world: &HashMap<String, HashMap<String, TypeDef>>,
    resolve: &HashMap<String, HashMap<String, (String, String)>>,
    visiting: &mut HashSet<String>,
) -> Option<Shape> {
    if ty.fn_type.is_some() || ty.union_members.is_some() || ty.lit.is_some() {
        return None;
    }
    match ty.name.as_str() {
        "String" if ty.args.is_empty() => Some(Shape::Str),
        "Int64" if ty.args.is_empty() => Some(Shape::Int),
        "Bool" if ty.args.is_empty() => Some(Shape::Bool),
        "Float64" if ty.args.is_empty() => Some(Shape::Float),
        "Array" if ty.args.len() == 1 => match &ty.args[0] {
            TypeArg::Type(elem) => {
                let inner = shape_of(elem, module_id, world, resolve, visiting)?;
                Some(Shape::Array(Box::new(inner)))
            }
            TypeArg::Int { .. } => None,
        },
        // May fields: deferred (RC/expect around generated Has/Not). v0 is required fields.
        other => {
            let (def_mod, def_name) = resolve.get(module_id)?.get(other)?.clone();
            let td = world.get(&def_mod)?.get(&def_name)?;
            if let Some((base, _)) = refined_alias_of(td, &def_mod, world, resolve) {
                Some(Shape::Refined {
                    name: other.to_string(),
                    base: base.to_string(),
                })
            } else if is_eligible(td, &def_mod, world, resolve, visiting) {
                Some(Shape::Product(other.to_string()))
            } else {
                None
            }
        }
    }
}

fn gen_read(td: &TypeDef) -> Function {
    let from = format!("__fromJson_{}", td.name);
    Function {
        exported: false,
        name: format!("__read_{}", td.name),
        name_span: dummy(),
        params: vec![param("s", ty_named("String"))],
        return_type: Some(ty_trial(ty_named(&td.name))),
        body: Block {
            items: vec![let_bind(
                "v",
                false,
                try_expr(call("readJson", vec![Arg::Pos(name("s"))])),
            )],
            tail: Some(Box::new(call(
                &from,
                vec![Arg::Pos(name("v")), Arg::Pos(str_lit(""))],
            ))),
            span: dummy(),
        },
    }
}

fn gen_from_json(
    td: &TypeDef,
    module_id: &str,
    world: &HashMap<String, HashMap<String, TypeDef>>,
    resolve: &HashMap<String, HashMap<String, (String, String)>>,
) -> Result<Function> {
    let mut visiting = HashSet::new();
    let mut items = vec![let_bind(
        "m",
        false,
        try_expr(call(
            "asObj",
            vec![Arg::Pos(name("v")), Arg::Pos(name("path"))],
        )),
    )];
    let mut ctor_args = Vec::new();
    for p in &td.params {
        let shape = shape_of(&p.ty, module_id, world, resolve, &mut visiting).ok_or_else(|| {
            Error::at(
                format!("internal: field `{}` not readable", p.name),
                p.name_span,
            )
        })?;
        items.push(let_bind_ty(
            &p.name,
            false,
            p.ty.clone(),
            field_value(&p.name, &shape, &p.ty),
        ));
        ctor_args.push(Arg::Named {
            name: p.name.clone(),
            name_span: dummy(),
            value: name(&p.name),
        });
    }
    Ok(Function {
        exported: false,
        name: format!("__fromJson_{}", td.name),
        name_span: dummy(),
        params: vec![
            param("v", ty_named("Json")),
            param_default("path", ty_named("String"), str_lit("")),
        ],
        return_type: Some(ty_trial(ty_named(&td.name))),
        body: Block {
            items,
            tail: Some(Box::new(call(
                "Pass",
                vec![Arg::Pos(call(&td.name, ctor_args))],
            ))),
            span: dummy(),
        },
    })
}

fn field_value(field: &str, shape: &Shape, ty: &TypeExpr) -> Expr {
    match shape {
        Shape::Str => try_expr(call(
            "strField",
            vec![
                Arg::Pos(name("m")),
                Arg::Pos(str_lit(field)),
                Arg::Pos(name("path")),
            ],
        )),
        Shape::Int => try_expr(call(
            "intField",
            vec![
                Arg::Pos(name("m")),
                Arg::Pos(str_lit(field)),
                Arg::Pos(name("path")),
            ],
        )),
        Shape::Bool => try_expr(call(
            "boolField",
            vec![
                Arg::Pos(name("m")),
                Arg::Pos(str_lit(field)),
                Arg::Pos(name("path")),
            ],
        )),
        Shape::Float => try_expr(call(
            "floatField",
            vec![
                Arg::Pos(name("m")),
                Arg::Pos(str_lit(field)),
                Arg::Pos(name("path")),
            ],
        )),
        Shape::Refined { name: tn, base } => {
            let helper = match base.as_str() {
                "String" => "strField",
                "Float64" => "floatField",
                "Bool" => "boolField",
                _ => "intField",
            };
            let raw = try_expr(call(
                helper,
                vec![
                    Arg::Pos(name("m")),
                    Arg::Pos(str_lit(field)),
                    Arg::Pos(name("path")),
                ],
            ));
            try_expr(method_call(
                name(tn),
                "from",
                vec![
                    Arg::Pos(raw),
                    Arg::Pos(call(
                        "joinPath",
                        vec![Arg::Pos(name("path")), Arg::Pos(str_lit(field))],
                    )),
                ],
            ))
        }
        Shape::Product(tn) => {
            let raw = try_expr(call(
                "field",
                vec![
                    Arg::Pos(name("m")),
                    Arg::Pos(str_lit(field)),
                    Arg::Pos(name("path")),
                ],
            ));
            try_expr(method_call(
                name(tn),
                "fromJson",
                vec![
                    Arg::Pos(raw),
                    Arg::Pos(call(
                        "joinPath",
                        vec![Arg::Pos(name("path")), Arg::Pos(str_lit(field))],
                    )),
                ],
            ))
        }
        Shape::Array(elem) => {
            let raw = try_expr(call(
                "field",
                vec![
                    Arg::Pos(name("m")),
                    Arg::Pos(str_lit(field)),
                    Arg::Pos(name("path")),
                ],
            ));
            let arr_path = call(
                "joinPath",
                vec![Arg::Pos(name("path")), Arg::Pos(str_lit(field))],
            );
            convert_array(raw, elem, arr_path, ty)
        }
    }
}

fn convert_array(raw: Expr, elem: &Shape, path: Expr, ty: &TypeExpr) -> Expr {
    let out_ty = ty.clone();
    Expr {
        kind: ExprKind::Block(Block {
            items: vec![
                let_bind(
                    "xs",
                    false,
                    try_expr(call("asArr", vec![Arg::Pos(raw), Arg::Pos(path.clone())])),
                ),
                let_bind_ty(
                    "out",
                    true,
                    out_ty,
                    Expr {
                        kind: ExprKind::ArrayLit(vec![]),
                        span: dummy(),
                    },
                ),
                let_bind("i", true, int_lit(0)),
                BlockItem::Expr(Expr {
                    kind: ExprKind::For {
                        name: "x".into(),
                        name_span: dummy(),
                        value: None,
                        start: Box::new(name("xs")),
                        end: None,
                        inclusive: false,
                        body: Block {
                            items: vec![
                                let_bind(
                                    "one",
                                    false,
                                    try_expr(convert_value(
                                        name("x"),
                                        elem,
                                        call(
                                            "joinPath",
                                            vec![Arg::Pos(path), Arg::Pos(interp_int("i"))],
                                        ),
                                    )),
                                ),
                                BlockItem::Expr(method_call(
                                    name("out"),
                                    "append",
                                    vec![Arg::Pos(name("one"))],
                                )),
                                assign("i", bin(BinaryOp::Add, name("i"), int_lit(1))),
                            ],
                            tail: None,
                            span: dummy(),
                        },
                    },
                    span: dummy(),
                }),
            ],
            tail: Some(Box::new(name("out"))),
            span: dummy(),
        }),
        span: dummy(),
    }
}

fn convert_value(val: Expr, shape: &Shape, path: Expr) -> Expr {
    match shape {
        Shape::Str => call("asStr", vec![Arg::Pos(val), Arg::Pos(path)]),
        Shape::Int => call("asInt", vec![Arg::Pos(val), Arg::Pos(path)]),
        Shape::Bool => call("asBool", vec![Arg::Pos(val), Arg::Pos(path)]),
        Shape::Float => call("asFloat", vec![Arg::Pos(val), Arg::Pos(path)]),
        Shape::Product(tn) => method_call(name(tn), "fromJson", vec![Arg::Pos(val), Arg::Pos(path)]),
        Shape::Refined { name: tn, .. } => {
            method_call(name(tn), "from", vec![Arg::Pos(val), Arg::Pos(path)])
        }
        Shape::Array(elem) => convert_array(val, elem, path, &ty_array(ty_named("Json"))),
    }
}

fn refined_alias_of<'a>(
    td: &'a TypeDef,
    module_id: &str,
    world: &HashMap<String, HashMap<String, TypeDef>>,
    resolve: &HashMap<String, HashMap<String, (String, String)>>,
) -> Option<(&'static str, WhereClause)> {
    let members = td.union_members.as_ref()?;
    if members.len() != 1 {
        return None;
    }
    let mem = &members[0];
    let w = mem.where_clause.clone()?;
    match mem.name.as_str() {
        "Int64" => Some(("Int64", w)),
        "String" => Some(("String", w)),
        "Float64" => Some(("Float64", w)),
        "Bool" => Some(("Bool", w)),
        other => {
            let (def_mod, def_name) = resolve.get(module_id)?.get(other)?.clone();
            let inner = world.get(&def_mod)?.get(&def_name)?;
            let (base, inner_w) = refined_alias_of(inner, &def_mod, world, resolve)?;
            let clause = WhereClause {
                expr: crate::refine::and_clauses(&inner_w.expr, &w.expr),
                span: w.span,
            };
            Some((base, clause))
        }
    }
}

fn gen_from_refined(td: &TypeDef, base: &str, clause: &WhereClause) -> Function {
    let items = where_guard_items("n", base, &clause.expr);
    Function {
        exported: false,
        name: format!("__from_{}", td.name),
        name_span: dummy(),
        params: vec![
            param("n", ty_named(base)),
            param_default("path", ty_named("String"), str_lit("")),
        ],
        return_type: Some(ty_trial(ty_named(&td.name))),
        body: Block {
            items,
            tail: Some(Box::new(call(
                "Pass",
                vec![Arg::Pos(call(
                    &td.name,
                    vec![Arg::Named {
                        name: "value".into(),
                        name_span: dummy(),
                        value: name("n"),
                    }],
                ))],
            ))),
            span: dummy(),
        },
    }
}

fn where_guard_items(val: &str, base: &str, expr: &WhereExpr) -> Vec<BlockItem> {
    match expr {
        WhereExpr::And(a, b) => {
            let mut xs = where_guard_items(val, base, a);
            xs.extend(where_guard_items(val, base, b));
            xs
        }
        WhereExpr::Or(a, b) => {
            let ca = where_cond(val, base, a);
            let cb = where_cond(val, base, b);
            let ok = bin(BinaryOp::Or, ca, cb);
            vec![BlockItem::Expr(if_expr(
                un(UnaryOp::Not, ok),
                block_stmts(vec![BlockItem::Expr(fail_where(val, base, expr))]),
                None,
            ))]
        }
        WhereExpr::Atom(atom) => {
            let cond = pred_cond(val, base, &atom.pred);
            vec![BlockItem::Expr(if_expr(
                un(UnaryOp::Not, cond),
                block_stmts(vec![BlockItem::Expr(fail_atom(val, base, atom))]),
                None,
            ))]
        }
    }
}

fn where_cond(val: &str, base: &str, expr: &WhereExpr) -> Expr {
    match expr {
        WhereExpr::And(a, b) => bin(
            BinaryOp::And,
            where_cond(val, base, a),
            where_cond(val, base, b),
        ),
        WhereExpr::Or(a, b) => bin(
            BinaryOp::Or,
            where_cond(val, base, a),
            where_cond(val, base, b),
        ),
        WhereExpr::Atom(atom) => pred_cond(val, base, &atom.pred),
    }
}

fn pred_cond(val: &str, base: &str, pred: &WherePred) -> Expr {
    let lhs = if matches!(base, "String" | "Array") {
        field(name(val), "count")
    } else {
        name(val)
    };
    match pred {
        WherePred::Ge(l) => bin(BinaryOp::Ge, lhs, lit_expr(l)),
        WherePred::Le(l) => bin(BinaryOp::Le, lhs, lit_expr(l)),
        WherePred::Gt(l) => bin(BinaryOp::Gt, lhs, lit_expr(l)),
        WherePred::Lt(l) => bin(BinaryOp::Lt, lhs, lit_expr(l)),
        WherePred::Eq(l) => bin(BinaryOp::Eq, lhs, lit_expr(l)),
        WherePred::Ne(l) => bin(BinaryOp::Ne, lhs, lit_expr(l)),
        WherePred::Range { lo, hi } => {
            let mut c = match lo {
                Some(l) => bin(BinaryOp::Ge, lhs.clone(), lit_expr(l)),
                None => bool_lit(true),
            };
            if let Some(h) = hi {
                c = bin(BinaryOp::And, c, bin(BinaryOp::Le, lhs, lit_expr(h)));
            }
            c
        }
        WherePred::Lit(l) => bin(BinaryOp::Eq, name(val), lit_expr(l)),
    }
}

fn fail_atom(val: &str, base: &str, atom: &WhereAtom) -> Expr {
    let expected = crate::refine::expected_desc(&atom.pred, base);
    fail_issue(val, &expected, atom.message.as_deref())
}

fn fail_where(val: &str, base: &str, expr: &WhereExpr) -> Expr {
    let expected = match expr {
        WhereExpr::Atom(a) => crate::refine::expected_desc(&a.pred, base),
        _ => "this refinement".into(),
    };
    fail_issue(val, &expected, None)
}

fn fail_issue(val: &str, expected: &str, _tmpl: Option<&str>) -> Expr {
    Expr {
        kind: ExprKind::Return {
            value: Some(Box::new(call(
                "Fail",
                vec![Arg::Pos(call(
                    "issueMismatch",
                    vec![
                        Arg::Pos(name("path")),
                        Arg::Pos(str_lit(expected)),
                        Arg::Pos(interp_val(val)),
                    ],
                ))],
            ))),
        },
        span: dummy(),
    }
}

fn interp_val(val: &str) -> Expr {
    Expr {
        kind: ExprKind::Interpolate(vec![InterpPart::Expr(name(val))]),
        span: dummy(),
    }
}

fn lit_expr(l: &WhereLit) -> Expr {
    match l {
        WhereLit::Int(n) => int_lit(*n),
        WhereLit::Float(n) => Expr {
            kind: ExprKind::Float(*n),
            span: dummy(),
        },
        WhereLit::String(s) => str_lit(s),
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

fn un(op: UnaryOp, expr: Expr) -> Expr {
    Expr {
        kind: ExprKind::Unary {
            op,
            expr: Box::new(expr),
        },
        span: dummy(),
    }
}

fn bool_lit(b: bool) -> Expr {
    Expr {
        kind: ExprKind::Bool(b),
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

fn block_stmts(items: Vec<BlockItem>) -> Block {
    Block {
        items,
        tail: None,
        span: dummy(),
    }
}

fn ensure_import(module: &mut Module, path: &str, names: &[&str]) {
    let mut have: HashSet<String> = HashSet::new();
    for item in &module.items {
        let Item::Import(imp) = item else { continue };
        if imp.path != path {
            continue;
        }
        match &imp.kind {
            ImportKind::Selective(ns) | ImportKind::Combined { names: ns, .. } => {
                for n in ns {
                    have.insert(
                        n.alias
                            .as_ref()
                            .map(|(a, _)| a.clone())
                            .unwrap_or_else(|| n.name.clone()),
                    );
                }
            }
            ImportKind::Namespace { .. } => {}
        }
    }
    let missing: Vec<ImportName> = names
        .iter()
        .filter(|n| !have.contains(**n))
        .map(|n| ImportName {
            name: (*n).into(),
            name_span: dummy(),
            alias: None,
        })
        .collect();
    if missing.is_empty() {
        return;
    }
    for item in &mut module.items {
        let Item::Import(imp) = item else { continue };
        if imp.path != path {
            continue;
        }
        match &mut imp.kind {
            ImportKind::Selective(ns) | ImportKind::Combined { names: ns, .. } => {
                ns.extend(missing);
                return;
            }
            ImportKind::Namespace { .. } => {}
        }
    }
    module.items.insert(
        0,
        Item::Import(Import {
            kind: ImportKind::Selective(missing),
            path: path.into(),
            path_span: dummy(),
        }),
    );
}

// ── AST builders ────────────────────────────────────────────────────────────

fn dummy() -> Span {
    Span::dummy()
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

fn ty_trial(ok: TypeExpr) -> TypeExpr {
    TypeExpr {
        name: "Trial".into(),
        args: vec![TypeArg::Type(ok), TypeArg::Type(ty_named("Issue"))],
        span: dummy(),
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
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

fn param_default(name: &str, ty: TypeExpr, default: Expr) -> Param {
    Param {
        name: name.into(),
        name_span: dummy(),
        positional: false,
        ty,
        default: Some(default),
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

fn try_expr(e: Expr) -> Expr {
    Expr {
        kind: ExprKind::Try(Box::new(e)),
        span: dummy(),
    }
}

fn name(s: &str) -> Expr {
    Expr {
        kind: ExprKind::Name(s.into()),
        span: dummy(),
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

fn interp_int(var: &str) -> Expr {
    Expr {
        kind: ExprKind::Interpolate(vec![InterpPart::Expr(name(var))]),
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

fn let_bind(name: &str, mutable: bool, init: Expr) -> BlockItem {
    BlockItem::Let {
        name: name.into(),
        name_span: dummy(),
        mutable,
        ty: None,
        init,
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

fn assign(name: &str, value: Expr) -> BlockItem {
    BlockItem::Assign {
        target: AssignTarget::Name {
            name: name.into(),
            name_span: dummy(),
        },
        value,
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
    fn generates_read_for_product() {
        let m = parse_desugar(
            r#"
            import { Json } from "std/json"
            type Order(slug: String, qty: Int64)
            "#,
        );
        let names: Vec<_> = m
            .items
            .iter()
            .filter_map(|it| match it {
                Item::Function(f) => Some(f.name.as_str()),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"__read_Order"));
        assert!(names.contains(&"__fromJson_Order"));
    }

    #[test]
    fn skips_union() {
        let m = parse_desugar(
            r#"
            type A
            type B
            type U = A | B
            "#,
        );
        let fns = m
            .items
            .iter()
            .filter(|it| matches!(it, Item::Function(_)))
            .count();
        assert_eq!(fns, 0);
    }
}
