//! D149 / D182: generate the codecs the compiler attaches to a type.
//!
//! For each type in a non-`std/` module, once `std/json` is in the package:
//! - encodable products, unions of products, refined types, and literal unions
//!   get `__toJson_T(v: T) -> Json` and `__encode_T(v: T) -> String` (D182 §1);
//! - readable products, refined types, and literal unions also get
//!   `__fromJson_T(v: Json, path: String = "") -> Trial<T, Issue>` and
//!   `__read_T(s: String) -> Trial<T, Issue>` (`__from_T` for refined /
//!   literal types).
//!
//! Call sites `T.read(s)` / `T.fromJson(v)` / `x.toJson()` are rewritten in MIR
//! to these fns. Generated code names `std/json` items by their mangled names
//! (`std_json_JObj`), so the module needs no import of its own and its names
//! cannot clash; `std/fallible` names are imported when missing.

use crate::ast::*;
use crate::ast_build::*;
use crate::error::{Error, Result};
use crate::loader::{self, BoundImport, Package};
use std::collections::{HashMap, HashSet};

const JSON_PATH: &str = "std/json";
const JSON_MODULE_ID: &str = "std_json";
const FALLIBLE_PATH: &str = "std/fallible";

const FALLIBLE_NAMES: &[&str] = &["Pass", "Fail", "Trial", "Has", "Not", "May"];

/// `std/json` types that are themselves JSON (`Json` and its members).
const JSON_FAMILY: &[&str] = &[
    "Json", "JNull", "JBool", "JInt", "JFloat", "JStr", "JArr", "JObj",
];

type World = HashMap<String, HashMap<String, TypeDef>>;
type Resolve = HashMap<String, HashMap<String, (String, String)>>;

pub fn desugar_package(package: &mut Package) -> Result<()> {
    if !package.modules.iter().any(|m| m.id == JSON_MODULE_ID) {
        return Ok(());
    }
    let import_maps = loader::build_import_maps(package)?;
    let mut world: World = HashMap::new();
    for m in &package.modules {
        let mut tys = HashMap::new();
        for item in &m.ast.items {
            if let Item::Type(td) = item {
                tys.insert(td.name.clone(), td.clone());
            }
        }
        world.insert(m.id.clone(), tys);
    }
    let mut resolve: Resolve = HashMap::new();
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

/// Single-file / LSP path: generate codecs for local types when the module
/// imports `std/json`.
pub fn desugar_module(module: &mut Module) -> Result<()> {
    if !module_imports_path(module, JSON_PATH) {
        return Ok(());
    }
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
    world: &World,
    resolve: &Resolve,
) -> Result<()> {
    let mut extra: Vec<Function> = Vec::new();
    let types: Vec<TypeDef> = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Type(td) => Some(td.clone()),
            _ => None,
        })
        .collect();
    for td in &types {
        let own_to_json = has_own_method(td, "toJson");
        let own_encode = has_own_method(td, "encode");
        let encoders = |extra: &mut Vec<Function>, to_json: Function| {
            if !own_to_json {
                extra.push(to_json);
            }
            if !own_encode {
                extra.push(gen_encode(td));
            }
        };
        if let Some((base, clause)) = refined_alias_of(td, module_id, world, resolve) {
            extra.push(gen_from_refined(td, base, &clause));
            encoders(&mut extra, gen_to_json_refined(td, base));
            continue;
        }
        if let Some((base, lits)) = pure_lit_union_of(td) {
            extra.push(gen_from_lit_union(td, base, &lits));
            encoders(&mut extra, gen_to_json_lit_union(td, base));
            extra.push(gen_from_json_scalar(td, base));
            extra.push(gen_read(td));
            continue;
        }
        if let Some(members) = encodable_union(td, module_id, world, resolve, &mut HashSet::new()) {
            encoders(&mut extra, gen_to_json_union(td, &members));
            continue;
        }
        if encodable_product(td, module_id, world, resolve, &mut HashSet::new()) {
            encoders(&mut extra, gen_to_json(td));
        }
        if !is_eligible(td, module_id, world, resolve, &mut HashSet::new()) {
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
        extra.push(gen_from_json(td, module_id, world, resolve)?);
        extra.push(gen_read(td));
    }
    if extra.is_empty() {
        return Ok(());
    }
    let defined: HashSet<&str> = types.iter().map(|td| td.name.as_str()).collect();
    let fallible: Vec<&str> = FALLIBLE_NAMES
        .iter()
        .copied()
        .filter(|name| !defined.contains(name))
        .collect();
    ensure_import(module, FALLIBLE_PATH, &fallible);
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

/// A field type whose values `toJson` can write (D182 §1). Structure
/// (`Array`, `Map<String, T>`, `May`) is converted by MIR; named types need a
/// generated (or their own) encoder.
fn encodable_ty(
    ty: &TypeExpr,
    module_id: &str,
    world: &World,
    resolve: &Resolve,
    visiting: &mut HashSet<String>,
) -> bool {
    if ty.fn_type.is_some() || ty.union_members.is_some() || ty.lit.is_some() {
        return false;
    }
    let arg = |index: usize| match ty.args.get(index) {
        Some(TypeArg::Type(inner)) => Some(inner),
        _ => None,
    };
    match (ty.name.as_str(), ty.args.len()) {
        ("String" | "Int64" | "Int32" | "UInt32" | "UInt64" | "Byte" | "UInt8" | "Float64"
        | "Bool", 0) => true,
        ("Array" | "May", 1) => {
            arg(0).is_some_and(|inner| encodable_ty(inner, module_id, world, resolve, visiting))
        }
        ("Map", 2) => {
            arg(0).is_some_and(|key| key.name == "String" && key.args.is_empty())
                && arg(1).is_some_and(|value| {
                    encodable_ty(value, module_id, world, resolve, visiting)
                })
        }
        (_, 0) => {
            let Some((def_mod, def_name)) = resolve.get(module_id).and_then(|m| m.get(&ty.name))
            else {
                return false;
            };
            if def_mod == JSON_MODULE_ID {
                return JSON_FAMILY.contains(&def_name.as_str());
            }
            if is_std_module(def_mod) {
                return false;
            }
            let Some(td) = world.get(def_mod).and_then(|tys| tys.get(def_name)) else {
                return false;
            };
            has_own_method(td, "toJson")
                || refined_alias_of(td, def_mod, world, resolve).is_some()
                || pure_lit_union_of(td).is_some()
                || encodable_union(td, def_mod, world, resolve, visiting).is_some()
                || encodable_product(td, def_mod, world, resolve, visiting)
        }
        _ => false,
    }
}

/// An exported method the type defines itself: its own `toJson` replaces the
/// generated one (D182 §6).
fn has_own_method(td: &TypeDef, method: &str) -> bool {
    td.methods.iter().any(|m| m.name == method && m.exported)
}

/// A plain product (constructor params, no body fields, not generic, not a
/// resource) whose fields all encode.
fn plain_product(td: &TypeDef) -> bool {
    td.union_members.is_none()
        && td.type_params.is_empty()
        && td.fields.is_empty()
        && !td.methods.iter().any(|m| m.name == "drop")
}

fn encodable_product(
    td: &TypeDef,
    module_id: &str,
    world: &World,
    resolve: &Resolve,
    visiting: &mut HashSet<String>,
) -> bool {
    if !plain_product(td) || td.params.is_empty() {
        return false;
    }
    let key = format!("{module_id}::{}", td.name);
    if !visiting.insert(key.clone()) {
        return true;
    }
    let ok = td
        .params
        .iter()
        .all(|p| encodable_ty(&p.ty, module_id, world, resolve, visiting));
    visiting.remove(&key);
    ok
}

/// One member of an encodable union: its name and its fields (none for a
/// member with no constructor params).
struct UnionMember {
    name: String,
    params: Vec<Param>,
}

/// A union whose members are all plain products whose fields encode
/// (D182 §1: written as the member's object with a `"type"` key). A union
/// reached again through its own members is assumed to encode.
fn encodable_union(
    td: &TypeDef,
    module_id: &str,
    world: &World,
    resolve: &Resolve,
    visiting: &mut HashSet<String>,
) -> Option<Vec<UnionMember>> {
    let members = td.union_members.as_ref()?;
    if members.len() < 2 || !td.type_params.is_empty() {
        return None;
    }
    let key = format!("{module_id}::{}", td.name);
    if !visiting.insert(key.clone()) {
        return Some(Vec::new());
    }
    let result = union_members_of(members, module_id, world, resolve, visiting);
    visiting.remove(&key);
    result
}

fn union_members_of(
    members: &[TypeExpr],
    module_id: &str,
    world: &World,
    resolve: &Resolve,
    visiting: &mut HashSet<String>,
) -> Option<Vec<UnionMember>> {
    let mut out = Vec::new();
    for member in members {
        if member.lit.is_some()
            || member.where_clause.is_some()
            || member.union_members.is_some()
            || member.fn_type.is_some()
            || !member.args.is_empty()
        {
            return None;
        }
        let (def_mod, def_name) = resolve.get(module_id)?.get(&member.name)?.clone();
        if is_std_module(&def_mod) {
            return None;
        }
        let member_td = world.get(&def_mod)?.get(&def_name)?;
        if !plain_product(member_td) {
            return None;
        }
        if !member_td
            .params
            .iter()
            .all(|p| encodable_ty(&p.ty, &def_mod, world, resolve, visiting))
        {
            return None;
        }
        out.push(UnionMember {
            name: member.name.clone(),
            params: member_td.params.clone(),
        });
    }
    Some(out)
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
    May(Box<Shape>),
    Product(String),
    Refined { name: String, base: String },
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
        "May" if ty.args.len() == 1 => match &ty.args[0] {
            TypeArg::Type(elem) => {
                let inner = shape_of(elem, module_id, world, resolve, visiting)?;
                Some(Shape::May(Box::new(inner)))
            }
            TypeArg::Int { .. } => None,
        },
        // remaining named types: refined aliases / readable products
        other => {
            let (def_mod, def_name) = resolve.get(module_id)?.get(other)?.clone();
            let td = world.get(&def_mod)?.get(&def_name)?;
            if let Some((base, _)) = refined_alias_of(td, &def_mod, world, resolve) {
                Some(Shape::Refined {
                    name: other.to_string(),
                    base: base.to_string(),
                })
            } else if let Some((base, _)) = pure_lit_union_of(td) {
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
                try_expr(json_call("readJson", vec![Arg::Pos(name("s"))])),
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
        try_expr(json_call("asObj",
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
        let mut ann = p.ty.clone();
        ann.where_clause = None;
        items.push(let_bind_ty(
            &p.name,
            false,
            ann,
            field_value(&p.name, &shape, &p.ty),
        ));
        if let Some(w) = &p.ty.where_clause {
            let base = shape_base(&shape);
            let pvar = format!("{}Path", p.name);
            items.push(let_bind(
                &pvar,
                false,
                json_call("joinPath",
                    vec![Arg::Pos(name("path")), Arg::Pos(str_lit(&p.name))],
                ),
            ));
            items.extend(where_guard_items_at(&p.name, base, &w.expr, &pvar));
        }
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
            param("v", json_ty("Json")),
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
        Shape::Str => try_expr(json_call("strField",
            vec![
                Arg::Pos(name("m")),
                Arg::Pos(str_lit(field)),
                Arg::Pos(name("path")),
            ],
        )),
        Shape::Int => try_expr(json_call("intField",
            vec![
                Arg::Pos(name("m")),
                Arg::Pos(str_lit(field)),
                Arg::Pos(name("path")),
            ],
        )),
        Shape::Bool => try_expr(json_call("boolField",
            vec![
                Arg::Pos(name("m")),
                Arg::Pos(str_lit(field)),
                Arg::Pos(name("path")),
            ],
        )),
        Shape::Float => try_expr(json_call("floatField",
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
            let raw = try_expr(json_call(helper,
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
                    Arg::Pos(json_call("joinPath",
                        vec![Arg::Pos(name("path")), Arg::Pos(str_lit(field))],
                    )),
                ],
            ))
        }
        Shape::Product(tn) => {
            let raw = try_expr(json_call("field",
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
                    Arg::Pos(json_call("joinPath",
                        vec![Arg::Pos(name("path")), Arg::Pos(str_lit(field))],
                    )),
                ],
            ))
        }
        Shape::Array(elem) => {
            let raw = try_expr(json_call("field",
                vec![
                    Arg::Pos(name("m")),
                    Arg::Pos(str_lit(field)),
                    Arg::Pos(name("path")),
                ],
            ));
            let arr_path = json_call("joinPath",
                vec![Arg::Pos(name("path")), Arg::Pos(str_lit(field))],
            );
            convert_array(raw, elem, arr_path, ty)
        }
        Shape::May(inner) => may_field_value(field, inner),
    }
}

fn may_field_value(field: &str, inner: &Shape) -> Expr {
    let path = json_call("joinPath",
        vec![Arg::Pos(name("path")), Arg::Pos(str_lit(field))],
    );
    let inner_ty = ty_of_shape(inner);
    let may_ty = ty_may(inner_ty.clone());
    match_expr(
        method_call(name("m"), "get", vec![Arg::Pos(str_lit(field))]),
        vec![
            match_arm_block(
                "Not",
                vec![],
                vec![let_bind_ty("absent", false, may_ty.clone(), call("Not", vec![]))],
                Some(name("absent")),
            ),
            match_arm_block(
                "Has",
                vec![Some("raw".into())],
                vec![],
                Some(if_expr(
                    json_call("isNull", vec![Arg::Pos(name("raw"))]),
                    Block {
                        items: vec![let_bind_ty(
                            "absent",
                            false,
                            may_ty.clone(),
                            call("Not", vec![]),
                        )],
                        tail: Some(Box::new(name("absent"))),
                        span: dummy(),
                    },
                    Some(Block {
                        items: vec![
                            let_bind_ty(
                                "one",
                                false,
                                inner_ty,
                                try_expr(convert_value(name("raw"), inner, path)),
                            ),
                            let_bind_ty(
                                "present",
                                false,
                                may_ty,
                                call("Has", vec![Arg::Pos(name("one"))]),
                            ),
                        ],
                        tail: Some(Box::new(name("present"))),
                        span: dummy(),
                    }),
                )),
            ),
        ],
    )
}

fn convert_array(raw: Expr, elem: &Shape, path: Expr, ty: &TypeExpr) -> Expr {
    let out_ty = ty.clone();
    Expr {
        kind: ExprKind::Block(Block {
            items: vec![
                let_bind(
                    "xs",
                    false,
                    try_expr(json_call("asArr", vec![Arg::Pos(raw), Arg::Pos(path.clone())])),
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
                                        json_call("joinPath",
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
        Shape::Str => json_call("asStr", vec![Arg::Pos(val), Arg::Pos(path)]),
        Shape::Int => json_call("asInt", vec![Arg::Pos(val), Arg::Pos(path)]),
        Shape::Bool => json_call("asBool", vec![Arg::Pos(val), Arg::Pos(path)]),
        Shape::Float => json_call("asFloat", vec![Arg::Pos(val), Arg::Pos(path)]),
        Shape::Product(tn) => method_call(name(tn), "fromJson", vec![Arg::Pos(val), Arg::Pos(path)]),
        Shape::Refined { name: tn, .. } => {
            method_call(name(tn), "from", vec![Arg::Pos(val), Arg::Pos(path)])
        }
        Shape::Array(elem) => convert_array(val, elem, path, &ty_array(json_ty("Json"))),
        Shape::May(inner) => {
            if_expr(
                json_call("isNull", vec![Arg::Pos(val.clone())]),
                Block {
                    items: vec![],
                    tail: Some(Box::new(call("Not", vec![]))),
                    span: dummy(),
                },
                Some(Block {
                    items: vec![],
                    tail: Some(Box::new(call(
                        "Has",
                        vec![Arg::Pos(try_expr(convert_value(val, inner, path)))],
                    ))),
                    span: dummy(),
                }),
            )
        }
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

fn shape_base(shape: &Shape) -> &'static str {
    match shape {
        Shape::Str => "String",
        Shape::Int => "Int64",
        Shape::Bool => "Bool",
        Shape::Float => "Float64",
        Shape::Array(_) => "Array",
        Shape::May(_) => "optional",
        Shape::Product(_) => "object",
        Shape::Refined { base, .. } => match base.as_str() {
            "String" => "String",
            "Float64" => "Float64",
            "Bool" => "Bool",
            _ => "Int64",
        },
    }
}

fn json_ctor_for_base(base: &str) -> &'static str {
    match base {
        "String" => "JStr",
        "Float64" => "JFloat",
        "Bool" => "JBool",
        _ => "JInt",
    }
}

fn as_helper_for_base(base: &str) -> &'static str {
    match base {
        "String" => "asStr",
        "Float64" => "asFloat",
        "Bool" => "asBool",
        _ => "asInt",
    }
}

/// `type T = "espresso" | "mocha"` / `0 | 1` / `true | false`.
fn pure_lit_union_of(td: &TypeDef) -> Option<(&'static str, Vec<TypeLit>)> {
    let members = td.union_members.as_ref()?;
    if members.is_empty() {
        return None;
    }
    let mut lits = Vec::new();
    for m in members {
        if m.where_clause.is_some() || m.union_members.is_some() {
            return None;
        }
        lits.push(m.lit.clone()?);
    }
    let base = match &lits[0] {
        TypeLit::String(_) => "String",
        TypeLit::Int(_) => "Int64",
        TypeLit::Bool(_) => "Bool",
    };
    let same = lits.iter().all(|l| match (l, base) {
        (TypeLit::String(_), "String") => true,
        (TypeLit::Int(_), "Int64") => true,
        (TypeLit::Bool(_), "Bool") => true,
        _ => false,
    });
    if same {
        Some((base, lits))
    } else {
        None
    }
}

fn typelit_expr(lit: &TypeLit) -> Expr {
    match lit {
        TypeLit::String(s) => str_lit(s),
        TypeLit::Int(n) => int_lit(*n),
        TypeLit::Bool(b) => bool_lit(*b),
    }
}

fn typelit_expected(lits: &[TypeLit]) -> String {
    lits.iter()
        .map(|l| match l {
            TypeLit::String(s) => format!("\"{s}\""),
            TypeLit::Int(n) => n.to_string(),
            TypeLit::Bool(b) => b.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn gen_from_lit_union(td: &TypeDef, _base: &str, lits: &[TypeLit]) -> Function {
    let expected = typelit_expected(lits);
    let mut items = Vec::new();
    for lit in lits {
        items.push(BlockItem::Expr(if_expr(
            bin(BinaryOp::Eq, name("n"), typelit_expr(lit)),
            block_stmts(vec![BlockItem::Expr(Expr {
                kind: ExprKind::Return {
                    value: Some(Box::new(call(
                        "Pass",
                        vec![Arg::Pos(typelit_expr(lit))],
                    ))),
                },
                span: dummy(),
            })]),
            None,
        )));
    }
    Function {
        exported: false,
        name: format!("__from_{}", td.name),
        name_span: dummy(),
        params: vec![
            param("n", ty_named(_base)),
            param_default("path", ty_named("String"), str_lit("")),
        ],
        return_type: Some(ty_trial(ty_named(&td.name))),
        body: Block {
            items,
            tail: Some(Box::new(call(
                "Fail",
                vec![Arg::Pos(json_call("issueMismatch",
                    vec![
                        Arg::Pos(name("path")),
                        Arg::Pos(str_lit(&expected)),
                        Arg::Pos(interp_val("n")),
                    ],
                ))],
            ))),
            span: dummy(),
        },
    }
}

fn gen_to_json_lit_union(td: &TypeDef, base: &str) -> Function {
    Function {
        exported: false,
        name: format!("__toJson_{}", td.name),
        name_span: dummy(),
        params: vec![param("v", ty_named(&td.name))],
        return_type: Some(json_ty("Json")),
        body: Block {
            items: vec![],
            tail: Some(Box::new(json_call(json_ctor_for_base(base),
                vec![Arg::Pos(name("v"))],
            ))),
            span: dummy(),
        },
    }
}

fn gen_from_json_scalar(td: &TypeDef, base: &str) -> Function {
    let from = format!("__fromJson_{}", td.name);
    Function {
        exported: false,
        name: from,
        name_span: dummy(),
        params: vec![
            param("v", json_ty("Json")),
            param_default("path", ty_named("String"), str_lit("")),
        ],
        return_type: Some(ty_trial(ty_named(&td.name))),
        body: Block {
            items: vec![let_bind(
                "raw",
                false,
                try_expr(json_call(as_helper_for_base(base),
                    vec![Arg::Pos(name("v")), Arg::Pos(name("path"))],
                )),
            )],
            tail: Some(Box::new(method_call(
                name(&td.name),
                "from",
                vec![Arg::Pos(name("raw")), Arg::Pos(name("path"))],
            ))),
            span: dummy(),
        },
    }
}

fn gen_encode(td: &TypeDef) -> Function {
    Function {
        exported: false,
        name: format!("__encode_{}", td.name),
        name_span: dummy(),
        params: vec![param("v", ty_named(&td.name))],
        return_type: Some(ty_named("String")),
        body: Block {
            items: vec![],
            tail: Some(Box::new(json_call("encode",
                vec![Arg::Pos(method_call(name("v"), "toJson", vec![]))],
            ))),
            span: dummy(),
        },
    }
}

fn gen_to_json_refined(td: &TypeDef, base: &str) -> Function {
    Function {
        exported: false,
        name: format!("__toJson_{}", td.name),
        name_span: dummy(),
        params: vec![param("v", ty_named(&td.name))],
        return_type: Some(json_ty("Json")),
        body: Block {
            items: vec![],
            tail: Some(Box::new(json_call(json_ctor_for_base(base),
                vec![Arg::Pos(field(name("v"), "value"))],
            ))),
            span: dummy(),
        },
    }
}

/// `{ "a": v.a.toJson(), … }`; a `May` field that is `Not` is left out.
fn gen_to_json(td: &TypeDef) -> Function {
    let mut items = vec![json_object_start()];
    for p in &td.params {
        items.push(set_field_json(&p.name, field(name("v"), &p.name), &p.ty));
    }
    Function {
        exported: false,
        name: format!("__toJson_{}", td.name),
        name_span: dummy(),
        params: vec![param("v", ty_named(&td.name))],
        return_type: Some(json_ty("Json")),
        body: Block {
            items,
            tail: Some(Box::new(json_object_end())),
            span: dummy(),
        },
    }
}

/// D182 §1: `{ "type": "Member", …its fields }` for the member `v` holds.
fn gen_to_json_union(td: &TypeDef, members: &[UnionMember]) -> Function {
    let arms = members
        .iter()
        .map(|member| {
            let binds: Vec<String> = (0..member.params.len()).map(|i| format!("__{i}")).collect();
            let mut items = vec![BlockItem::Expr(method_call(
                name("__m"),
                "set",
                vec![
                    Arg::Pos(str_lit("type")),
                    Arg::Pos(json_call("JStr", vec![Arg::Pos(str_lit(&member.name))])),
                ],
            ))];
            for (p, bind) in member.params.iter().zip(&binds) {
                items.push(set_field_json(&p.name, name(bind), &p.ty));
            }
            match_arm_block(
                &member.name,
                binds.into_iter().map(Some).collect(),
                items,
                None,
            )
        })
        .collect();
    Function {
        exported: false,
        name: format!("__toJson_{}", td.name),
        name_span: dummy(),
        params: vec![param("v", ty_named(&td.name))],
        return_type: Some(json_ty("Json")),
        body: Block {
            items: vec![json_object_start(), BlockItem::Expr(match_expr(name("v"), arms))],
            tail: Some(Box::new(json_object_end())),
            span: dummy(),
        },
    }
}

/// `mut __m: Map<String, Json> = Map()`
fn json_object_start() -> BlockItem {
    let_bind_ty(
        "__m",
        true,
        ty_map(ty_named("String"), json_ty("Json")),
        call("Map", vec![]),
    )
}

/// `JObj(__m)`
fn json_object_end() -> Expr {
    json_call("JObj", vec![Arg::Pos(name("__m"))])
}

/// `__m.set("key", value.toJson())`, or for a `May` field only when it holds
/// a value.
fn set_field_json(key: &str, value: Expr, ty: &TypeExpr) -> BlockItem {
    let set = |value: Expr| {
        BlockItem::Expr(method_call(
            name("__m"),
            "set",
            vec![
                Arg::Pos(str_lit(key)),
                Arg::Pos(method_call(value, "toJson", vec![])),
            ],
        ))
    };
    if ty.name == "May" && ty.args.len() == 1 {
        BlockItem::Expr(match_expr(
            value,
            vec![
                match_arm_block("Not", vec![], vec![], None),
                match_arm_block("Has", vec![Some("__x".into())], vec![set(name("__x"))], None),
            ],
        ))
    } else {
        set(value)
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
    where_guard_items_at(val, base, expr, "path")
}

fn where_guard_items_at(val: &str, base: &str, expr: &WhereExpr, path_var: &str) -> Vec<BlockItem> {
    match expr {
        WhereExpr::And(a, b) => {
            let mut xs = where_guard_items_at(val, base, a, path_var);
            xs.extend(where_guard_items_at(val, base, b, path_var));
            xs
        }
        WhereExpr::Or(a, b) => {
            let ca = where_cond(val, base, a);
            let cb = where_cond(val, base, b);
            let ok = bin(BinaryOp::Or, ca, cb);
            vec![BlockItem::Expr(if_expr(
                un(UnaryOp::Not, ok),
                block_stmts(vec![BlockItem::Expr(fail_where_at(val, base, expr, path_var))]),
                None,
            ))]
        }
        WhereExpr::Atom(atom) => {
            let cond = pred_cond(val, base, &atom.pred);
            vec![BlockItem::Expr(if_expr(
                un(UnaryOp::Not, cond),
                block_stmts(vec![BlockItem::Expr(fail_atom_at(val, base, atom, path_var))]),
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

fn fail_atom_at(val: &str, base: &str, atom: &WhereAtom, path_var: &str) -> Expr {
    let expected = crate::refine::expected_desc(&atom.pred, base);
    fail_issue_at(val, &expected, atom.message.as_deref(), path_var)
}

fn fail_where_at(val: &str, base: &str, expr: &WhereExpr, path_var: &str) -> Expr {
    let expected = crate::refine::expected_clause(expr, base);
    fail_issue_at(val, &expected, None, path_var)
}

fn fail_issue_at(val: &str, expected: &str, tmpl: Option<&str>, path_var: &str) -> Expr {
    let issue = if let Some(t) = tmpl {
        json_call("issueRejected",
            vec![
                Arg::Pos(name(path_var)),
                Arg::Pos(template_expr_at(t, val, expected, path_var)),
            ],
        )
    } else {
        json_call("issueMismatch",
            vec![
                Arg::Pos(name(path_var)),
                Arg::Pos(str_lit(expected)),
                Arg::Pos(interp_val(val)),
            ],
        )
    };
    Expr {
        kind: ExprKind::Return {
            value: Some(Box::new(call("Fail", vec![Arg::Pos(issue)]))),
        },
        span: dummy(),
    }
}

fn template_expr_at(tmpl: &str, val: &str, expected: &str, path_var: &str) -> Expr {
    let mut parts: Vec<InterpPart> = Vec::new();
    let mut rest = tmpl;
    while !rest.is_empty() {
        if let Some(i) = rest.find('{') {
            if i > 0 {
                parts.push(InterpPart::Lit(rest[..i].into()));
            }
            if let Some(j) = rest[i + 1..].find('}') {
                let hole = &rest[i + 1..i + 1 + j];
                match hole {
                    "path" => parts.push(InterpPart::Expr(name(path_var))),
                    "expected" => parts.push(InterpPart::Lit(expected.into())),
                    "value" => parts.push(InterpPart::Expr(name(val))),
                    _ => parts.push(InterpPart::Lit(format!("{{{hole}}}")))
                }
                rest = &rest[i + 2 + j..];
            } else {
                parts.push(InterpPart::Lit(rest.into()));
                break;
            }
        } else {
            parts.push(InterpPart::Lit(rest.into()));
            break;
        }
    }
    if parts.is_empty() {
        return str_lit("");
    }
    if parts.len() == 1 {
        if let InterpPart::Lit(s) = &parts[0] {
            return str_lit(s);
        }
    }
    Expr {
        kind: ExprKind::Interpolate(parts),
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


fn ty_of_shape(shape: &Shape) -> TypeExpr {
    match shape {
        Shape::Str => ty_named("String"),
        Shape::Int => ty_named("Int64"),
        Shape::Bool => ty_named("Bool"),
        Shape::Float => ty_named("Float64"),
        Shape::Array(elem) => ty_array(ty_of_shape(elem)),
        Shape::May(inner) => ty_may(ty_of_shape(inner)),
        Shape::Product(n) | Shape::Refined { name: n, .. } => ty_named(n),
    }
}

fn ty_trial(ok: TypeExpr) -> TypeExpr {
    TypeExpr {
        name: "Trial".into(),
        args: vec![TypeArg::Type(ok), TypeArg::Type(json_ty("Issue"))],
        span: dummy(),
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    }
}

/// A call to a `std/json` function or constructor by its mangled name.
fn json_call(callee: &str, args: Vec<Arg>) -> Expr {
    call(&format!("{JSON_MODULE_ID}_{callee}"), args)
}

/// A `std/json` type by its mangled name.
fn json_ty(name: &str) -> TypeExpr {
    ty_named(&format!("{JSON_MODULE_ID}_{name}"))
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
    fn generates_from_for_lit_union() {
        let m = parse_desugar(
            r#"
            import { Json } from "std/json"
            type DrinkSlug = "espresso" | "mocha"
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
        assert!(names.contains(&"__from_DrinkSlug"));
        assert!(names.contains(&"__fromJson_DrinkSlug"));
        assert!(names.contains(&"__read_DrinkSlug"));
    }

    #[test]
    fn generates_read_for_may_field() {
        let m = parse_desugar(
            r#"
            import { Json } from "std/json"
            import { May } from "std/fallible"
            type Opt(note: May<String>, n: Int64)
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
        assert!(names.contains(&"__read_Opt"));
        assert!(names.contains(&"__fromJson_Opt"));
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
