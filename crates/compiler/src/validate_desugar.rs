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
const HTTP_PATH: &str = "std/http";
const HTTP_MODULE_ID: &str = "std_http";

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

    let forms = package.modules.iter().any(|m| m.id == HTTP_MODULE_ID);
    for m in &mut package.modules {
        if is_std_module(&m.id) {
            continue;
        }
        desugar_module_with(&mut m.ast, &m.id, &world, &resolve, forms)?;
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
    let forms = module_imports_path(module, HTTP_PATH);
    desugar_module_with(module, "local", &world, &resolve, forms)
}

fn is_std_module(id: &str) -> bool {
    id == "std" || id.starts_with("std_")
}

fn desugar_module_with(
    module: &mut Module,
    module_id: &str,
    world: &World,
    resolve: &Resolve,
    forms: bool,
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
        // A `from(json: Json)` clause is the type's reader (D191 §3): the
        // codecs wrap it instead of generating one.
        let custom_reader = json_conversion(td, module_id, resolve)?;
        if let Some((index, fallible)) = custom_reader {
            extra.push(gen_custom_from_json(td, index, fallible));
            extra.push(gen_read(td));
        }
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
        let generated_reader = custom_reader.is_none();
        if let Some((base, clause)) = refined_alias_of(td, module_id, world, resolve) {
            extra.push(gen_from_refined(td, base, &clause));
            encoders(&mut extra, gen_to_json_refined(td, base));
            if generated_reader {
                extra.push(gen_from_json_scalar(td, base));
                extra.push(gen_read(td));
            }
            continue;
        }
        if let Some((base, lits)) = pure_lit_union_of(td) {
            extra.push(gen_from_lit_union(td, base, &lits));
            encoders(&mut extra, gen_to_json_lit_union(td, base));
            if generated_reader {
                extra.push(gen_from_json_scalar(td, base));
                extra.push(gen_read(td));
            }
            continue;
        }
        if let Some(members) = encodable_union(td, module_id, world, resolve, &mut HashSet::new()) {
            encoders(&mut extra, gen_to_json_union(td, &members));
            if generated_reader
                && decodable_union(td, module_id, world, resolve, &mut HashSet::new()).is_some()
            {
                extra.push(gen_from_json_union(td, &members));
                extra.push(gen_read(td));
            }
            continue;
        }
        if encodable_product(td, module_id, world, resolve, &mut HashSet::new()) {
            encoders(&mut extra, gen_to_json(td));
        }
        if !generated_reader || !is_eligible(td, module_id, world, resolve, &mut HashSet::new()) {
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
        let fields = readable_fields(td, module_id, world, resolve)?;
        for field in &fields {
            if let Some(clause) = &field.check {
                extra.push(gen_field_check(td, &field.name, shape_base(&field.shape), clause));
            }
        }
        extra.push(gen_from_json(td, &fields));
        extra.push(gen_read(td));
        if forms && form_readable_product(td, module_id, world, resolve, &mut HashSet::new()) {
            extra.push(gen_from_form(td, &fields));
            extra.push(gen_read_form(td));
        }
    }
    // Body readers first: an `Array<T>` one is an `as` the scan below finds.
    let filled = fill_body_readers(module, module_id, world, resolve, forms);
    for target in cast_targets(module) {
        extra.extend(gen_cast_readers(&target, module_id, world, resolve));
    }
    if extra.is_empty() && !filled {
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
    // Its own `toJson` writes a shape a generated reader would not expect.
    if has_own_method(td, "toJson") {
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
    /// `Map<String, T>`: a JSON object (D182 §5).
    Map(Box<Shape>),
    /// `Json`: any JSON value (D182 §5).
    Json,
    Product(String),
    /// A union of products, chosen by its `"type"` key (D182 §5).
    Union(String),
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
        "Map" if ty.args.len() == 2 => match (&ty.args[0], &ty.args[1]) {
            (TypeArg::Type(key), TypeArg::Type(value)) if key.name == "String" && key.args.is_empty() => {
                let inner = shape_of(value, module_id, world, resolve, visiting)?;
                Some(Shape::Map(Box::new(inner)))
            }
            _ => None,
        },
        // remaining named types: refined aliases / readable products
        other if ty.args.is_empty() => {
            let (def_mod, def_name) = resolve.get(module_id)?.get(other)?.clone();
            if def_mod == JSON_MODULE_ID {
                return (def_name == "Json").then_some(Shape::Json);
            }
            let td = world.get(&def_mod)?.get(&def_name)?;
            if json_conversion(td, &def_mod, resolve).ok().flatten().is_some() {
                return Some(Shape::Product(other.to_string()));
            }
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
            } else if decodable_union(td, &def_mod, world, resolve, visiting).is_some() {
                Some(Shape::Union(other.to_string()))
            } else if is_eligible(td, &def_mod, world, resolve, visiting) {
                Some(Shape::Product(other.to_string()))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// A union of products whose members all read (D182 §5); a member with
/// fields uses its own generated reader.
fn decodable_union(
    td: &TypeDef,
    module_id: &str,
    world: &World,
    resolve: &Resolve,
    visiting: &mut HashSet<String>,
) -> Option<Vec<UnionMember>> {
    if has_own_method(td, "toJson") {
        return None;
    }
    let key = format!("{module_id}::{}", td.name);
    if visiting.contains(&key) {
        return Some(Vec::new());
    }
    let members = encodable_union(td, module_id, world, resolve, visiting)?;
    visiting.insert(key.clone());
    let ok = members.iter().all(|member| {
        member.params.is_empty()
            || resolve
                .get(module_id)
                .and_then(|names| names.get(&member.name))
                .and_then(|(def_mod, def_name)| {
                    world.get(def_mod).and_then(|tys| tys.get(def_name)).map(|m| (def_mod, m))
                })
                .is_some_and(|(def_mod, member_td)| {
                    is_eligible(member_td, def_mod, world, resolve, visiting)
                })
    });
    visiting.remove(&key);
    ok.then_some(members)
}

/// `__read_T(s)`: parse, then `__fromJson_T` (D182 §2).
fn gen_read(td: &TypeDef) -> Function {
    Function {
        exported: false,
        name: format!("__read_{}", td.name),
        name_span: dummy(),
        params: vec![param("s", ty_named("String"))],
        return_type: Some(ty_trial_issues(ty_named(&td.name))),
        body: Block {
            items: vec![let_bind(
                "__tree",
                false,
                try_expr(json_call("readJson", vec![Arg::Pos(name("s"))])),
            )],
            tail: Some(Box::new(call(
                &format!("__fromJson_{}", td.name),
                vec![Arg::Pos(name("__tree")), Arg::Pos(str_lit(""))],
            ))),
            span: dummy(),
        },
    }
}

/// A product field as the readers see it: its shape, its type without a
/// `where`, and its own `where`, checked by `__check_T_field`.
struct ReadableField {
    name: String,
    shape: Shape,
    ty: TypeExpr,
    check: Option<WhereClause>,
    /// The constructor default a missing key takes (D182 §5).
    default: Option<Expr>,
}

impl ReadableField {
    fn check_name(&self, td: &TypeDef) -> Option<String> {
        self.check
            .as_ref()
            .map(|_| format!("__check_{}_{}", td.name, self.name))
    }
}

fn readable_fields(
    td: &TypeDef,
    module_id: &str,
    world: &World,
    resolve: &Resolve,
) -> Result<Vec<ReadableField>> {
    let mut visiting = HashSet::new();
    td.params
        .iter()
        .map(|p| {
            let shape = shape_of(&p.ty, module_id, world, resolve, &mut visiting).ok_or_else(|| {
                Error::at(
                    format!("internal: field `{}` not readable", p.name),
                    p.name_span,
                )
            })?;
            let mut ty = p.ty.clone();
            ty.where_clause = None;
            Ok(ReadableField {
                name: p.name.clone(),
                shape,
                ty,
                check: p.ty.where_clause.clone(),
                default: p.default.clone(),
            })
        })
        .collect()
}

/// `__fromJson_T` for a product: every field is read, and the issues of all
/// of them are reported together (D182 §2).
fn gen_from_json(td: &TypeDef, fields: &[ReadableField]) -> Function {
    let mut items = vec![let_bind(
        "__obj",
        false,
        try_expr(json_call(
            "asObj",
            vec![Arg::Pos(name("v")), Arg::Pos(name("path"))],
        )),
    )];
    for (index, field) in fields.iter().enumerate() {
        let read = read_json_field(field);
        items.push(let_bind_ty(
            &format!("__f{index}"),
            false,
            ty_trial_issues(field.ty.clone()),
            checked_field(&field.name, read, &field.ty, field.check_name(td).as_deref()),
        ));
    }
    Function {
        exported: false,
        name: format!("__fromJson_{}", td.name),
        name_span: dummy(),
        params: vec![
            param("v", json_ty("Json")),
            param_default("path", ty_named("String"), str_lit("")),
        ],
        return_type: Some(ty_trial_issues(ty_named(&td.name))),
        body: build_from_fields(td, fields, items),
    }
}

/// The issues of every field, then the value: the end of a reader's body.
fn build_from_fields(td: &TypeDef, fields: &[ReadableField], mut items: Vec<BlockItem>) -> Block {
    items.extend(collect_issues(fields.len()));
    let ctor_args = fields
        .iter()
        .enumerate()
        .map(|(index, field)| Arg::Named {
            name: field.name.clone(),
            name_span: dummy(),
            value: try_expr(name(&format!("__f{index}"))),
        })
        .collect();
    Block {
        items,
        tail: Some(Box::new(call(
            "Pass",
            vec![Arg::Pos(call(&td.name, ctor_args))],
        ))),
        span: dummy(),
    }
}

// ── Forms (D182 §3) ─────────────────────────────────────────────────────────

/// A product a form can fill: fields of scalars, refined or literal types,
/// `May` or `Array` of those, and nested such products (as `field.sub`).
fn form_readable_product(
    td: &TypeDef,
    module_id: &str,
    world: &World,
    resolve: &Resolve,
    visiting: &mut HashSet<String>,
) -> bool {
    if !plain_product(td) || td.params.is_empty() {
        return false;
    }
    // A product that holds itself would need endless names.
    let key = format!("{module_id}::{}", td.name);
    if !visiting.insert(key.clone()) {
        return false;
    }
    let ok = td.params.iter().all(|p| {
        let Some(shape) = shape_of(&p.ty, module_id, world, resolve, &mut HashSet::new()) else {
            return false;
        };
        match &shape {
            Shape::May(inner) | Shape::Array(inner) => form_scalar(inner),
            Shape::Product(type_name) => resolve
                .get(module_id)
                .and_then(|names| names.get(type_name))
                .and_then(|(def_mod, def_name)| {
                    world.get(def_mod).and_then(|tys| tys.get(def_name)).map(|inner| (def_mod, inner))
                })
                .is_some_and(|(def_mod, inner)| {
                    form_readable_product(inner, def_mod, world, resolve, visiting)
                }),
            other => form_scalar(other),
        }
    });
    visiting.remove(&key);
    ok
}

/// One form value: a scalar, or a refined or literal type over one.
fn form_scalar(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::Str | Shape::Int | Shape::Float | Shape::Bool | Shape::Refined { .. }
    )
}

/// `__readForm_T(text)`: the pairs of a form body or query string, then
/// `__fromForm_T`.
fn gen_read_form(td: &TypeDef) -> Function {
    Function {
        exported: false,
        name: format!("__readForm_{}", td.name),
        name_span: dummy(),
        params: vec![param("text", ty_named("String"))],
        return_type: Some(ty_trial_issues(ty_named(&td.name))),
        body: Block {
            items: vec![],
            tail: Some(Box::new(call(
                &format!("__fromForm_{}", td.name),
                vec![
                    Arg::Pos(http_call("formPairs", vec![Arg::Pos(name("text"))])),
                    Arg::Pos(str_lit("")),
                ],
            ))),
            span: dummy(),
        },
    }
}

/// `__fromForm_T(pairs, path)`: each field from the value named
/// `path.field`, every issue reported together.
fn gen_from_form(td: &TypeDef, fields: &[ReadableField]) -> Function {
    let items = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let read = read_form_field(field);
            let_bind_ty(
                &format!("__f{index}"),
                false,
                ty_trial_issues(field.ty.clone()),
                checked_field(&field.name, read, &field.ty, field.check_name(td).as_deref()),
            )
        })
        .collect();
    Function {
        exported: false,
        name: format!("__fromForm_{}", td.name),
        name_span: dummy(),
        params: vec![
            param("pairs", ty_array(http_ty("FormPair"))),
            param_default("path", ty_named("String"), str_lit("")),
        ],
        return_type: Some(ty_trial_issues(ty_named(&td.name))),
        body: build_from_fields(td, fields, items),
    }
}

/// A field from the form's pairs; its name and path are `__p`. Empty is
/// absent for every type but `String`. An absent `Bool` is `false` and an
/// absent `Array` empty (an unchecked box sends nothing); other absent fields
/// take their default.
fn read_form_field(readable: &ReadableField) -> Expr {
    let (shape, ty) = (&readable.shape, &readable.ty);
    let trial = ty_trial_issues(ty.clone());
    match shape {
        Shape::Product(type_name) => method_call(
            name(type_name),
            "fromForm",
            vec![Arg::Pos(name("pairs")), Arg::Pos(name("__p"))],
        ),
        Shape::Array(elem) => form_array(elem, ty),
        _ => {
            let absent = match shape {
                Shape::Bool => typed("__absent", trial.clone(), call("Pass", vec![Arg::Pos(bool_lit(false))])),
                _ => absent_field(readable),
            };
            let present = form_text(name("__text"), shape, ty, name("__p"));
            let present = if matches!(shape, Shape::Str) {
                present
            } else {
                if_expr(
                    bin(BinaryOp::Eq, field(name("__text"), "count"), int_lit(0)),
                    Block {
                        items: vec![],
                        tail: Some(Box::new(absent.clone())),
                        span: dummy(),
                    },
                    Some(Block {
                        items: vec![],
                        tail: Some(Box::new(present)),
                        span: dummy(),
                    }),
                )
            };
            typed(
                "__field",
                trial,
                match_expr(
                    http_call("formValue", vec![Arg::Pos(name("pairs")), Arg::Pos(name("__p"))]),
                    vec![
                        match_arm_block("Not", vec![], vec![], Some(absent)),
                        match_arm_block("Has", vec![Some("__text".into())], vec![], Some(present)),
                    ],
                ),
            )
        }
    }
}

/// The text of one present value as `ty`: a `Trial<ty, Array<Issue>>`.
fn form_text(text: Expr, shape: &Shape, ty: &TypeExpr, path: Expr) -> Expr {
    let trial = ty_trial_issues(ty.clone());
    match shape {
        Shape::Str => typed("__out", trial, call("Pass", vec![Arg::Pos(text)])),
        Shape::Int => http_call("formInt", vec![Arg::Pos(text), Arg::Pos(path)]),
        Shape::Float => http_call("formFloat", vec![Arg::Pos(text), Arg::Pos(path)]),
        Shape::Bool => typed("__out", trial, call("Pass", vec![Arg::Pos(bool_lit(true))])),
        Shape::Refined { name: type_name, base } => {
            let base_shape = match base.as_str() {
                "String" => Shape::Str,
                "Float64" => Shape::Float,
                "Bool" => Shape::Bool,
                _ => Shape::Int,
            };
            let base_ty = ty_named(shape_base(&base_shape));
            match_expr(
                form_text(text, &base_shape, &base_ty, path.clone()),
                vec![
                    match_arm_block(
                        "Fail",
                        vec![Some("__found".into())],
                        vec![],
                        Some(typed("__out", trial.clone(), call("Fail", vec![Arg::Pos(name("__found"))]))),
                    ),
                    match_arm_block(
                        "Pass",
                        vec![Some("__base".into())],
                        vec![],
                        Some(single_issue(
                            method_call(
                                name(type_name),
                                "__from",
                                vec![Arg::Pos(name("__base")), Arg::Pos(path)],
                            ),
                            &trial,
                        )),
                    ),
                ],
            )
        }
        Shape::May(inner) => {
            let inner_ty = type_arg(ty, 0);
            match_expr(
                form_text(text, inner, &inner_ty, path),
                vec![
                    match_arm_block(
                        "Pass",
                        vec![Some("__some".into())],
                        vec![],
                        Some(typed(
                            "__out",
                            trial.clone(),
                            call(
                                "Pass",
                                vec![Arg::Pos(typed(
                                    "__has",
                                    ty.clone(),
                                    call("Has", vec![Arg::Pos(name("__some"))]),
                                ))],
                            ),
                        )),
                    ),
                    match_arm_block(
                        "Fail",
                        vec![Some("__found".into())],
                        vec![],
                        Some(typed("__out", trial, call("Fail", vec![Arg::Pos(name("__found"))]))),
                    ),
                ],
            )
        }
        // Not form scalars (`form_readable_product` keeps these out).
        Shape::Array(_) | Shape::Map(_) | Shape::Json | Shape::Product(_) | Shape::Union(_) => {
            typed("__out", trial, missing_value())
        }
    }
}

/// `Fail([issueMissing(__p)])` without a type of its own.
fn missing_value() -> Expr {
    fail_with(json_call("issueMissing", vec![Arg::Pos(name("__p"))]))
}

/// One element per value named `__p`, in order; an empty value is skipped
/// unless the elements are strings.
fn form_array(elem: &Shape, ty: &TypeExpr) -> Expr {
    let trial = ty_trial_issues(ty.clone());
    let elem_ty = type_arg(ty, 0);
    let element = form_text(
        name("__text"),
        elem,
        &elem_ty,
        json_call(
            "joinPath",
            vec![Arg::Pos(name("__p")), Arg::Pos(interp_int("__i"))],
        ),
    );
    let mut each = vec![
        let_bind_ty("__one", false, ty_trial_issues(elem_ty.clone()), element),
        BlockItem::Expr(match_expr(
            name("__one"),
            vec![
                match_arm_block(
                    "Pass",
                    vec![Some("__v".into())],
                    vec![BlockItem::Expr(method_call(
                        name("__out"),
                        "append",
                        vec![Arg::Pos(name("__v"))],
                    ))],
                    None,
                ),
                match_arm_block(
                    "Fail",
                    vec![Some("__found".into())],
                    vec![append_all("__bad", "__found")],
                    None,
                ),
            ],
        )),
    ];
    if !matches!(elem, Shape::Str) {
        each = vec![BlockItem::Expr(if_expr(
            bin(BinaryOp::Gt, field(name("__text"), "count"), int_lit(0)),
            block_stmts(each),
            None,
        ))];
    }
    each.push(assign("__i", bin(BinaryOp::Add, name("__i"), int_lit(1))));
    block(
        vec![
            let_bind_ty("__out", true, ty.clone(), empty_array()),
            let_bind_ty("__bad", true, ty_array(json_ty("Issue")), empty_array()),
            let_bind("__i", true, int_lit(0)),
            for_each(
                "__text",
                None,
                http_call("formValues", vec![Arg::Pos(name("pairs")), Arg::Pos(name("__p"))]),
                each,
            ),
        ],
        if_expr(
            bin(BinaryOp::Gt, field(name("__bad"), "count"), int_lit(0)),
            Block {
                items: vec![],
                tail: Some(Box::new(typed(
                    "__result",
                    trial.clone(),
                    call("Fail", vec![Arg::Pos(name("__bad"))]),
                ))),
                span: dummy(),
            },
            Some(Block {
                items: vec![],
                tail: Some(Box::new(typed(
                    "__result",
                    trial,
                    call("Pass", vec![Arg::Pos(name("__out"))]),
                ))),
                span: dummy(),
            }),
        ),
    )
}

/// A call to a `std/http` function by its mangled name.
fn http_call(callee: &str, args: Vec<Arg>) -> Expr {
    call(&format!("{HTTP_MODULE_ID}_{callee}"), args)
}

/// A `std/http` type by its mangled name.
fn http_ty(name: &str) -> TypeExpr {
    ty_named(&format!("{HTTP_MODULE_ID}_{name}"))
}

/// `__check_T_field(value, path) -> Trial<Base, Issue>`: a field's own
/// `where`, as a refined type's `from` checks it.
fn gen_field_check(td: &TypeDef, field: &str, base: &str, clause: &WhereClause) -> Function {
    let items = where_guard_items("value", base, &clause.expr);
    Function {
        exported: false,
        name: format!("__check_{}_{field}", td.name),
        name_span: dummy(),
        params: vec![param("value", ty_named(base)), param("path", ty_named("String"))],
        return_type: Some(ty_trial_issue(ty_named(base))),
        body: Block {
            items,
            tail: Some(Box::new(call("Pass", vec![Arg::Pos(name("value"))]))),
            span: dummy(),
        },
    }
}

/// `mut __issues = []`, every field's issues appended in order, and a
/// `Fail(__issues)` return when there are any.
fn collect_issues(fields: usize) -> Vec<BlockItem> {
    let mut items = vec![let_bind_ty(
        "__issues",
        true,
        ty_array(json_ty("Issue")),
        empty_array(),
    )];
    for index in 0..fields {
        items.push(BlockItem::Expr(match_expr(
            name(&format!("__f{index}")),
            vec![
                match_arm_block(
                    "Fail",
                    vec![Some("__found".into())],
                    vec![append_all("__issues", "__found")],
                    None,
                ),
                else_arm(),
            ],
        )));
    }
    items.push(BlockItem::Expr(if_expr(
        bin(
            BinaryOp::Gt,
            field(name("__issues"), "count"),
            int_lit(0),
        ),
        block_stmts(vec![BlockItem::Expr(return_expr(call(
            "Fail",
            vec![Arg::Pos(name("__issues"))],
        )))]),
        None,
    )));
    items
}

/// A product field read from the object `__obj`; the path is `__p`.
fn read_json_field(field: &ReadableField) -> Expr {
    let (shape, ty) = (&field.shape, &field.ty);
    let absent = absent_field(field);
    match_expr(
        method_call(name("__obj"), "get", vec![Arg::Pos(str_lit(&field.name))]),
        vec![
            match_arm_block("Not", vec![], vec![], Some(absent)),
            match_arm_block(
                "Has",
                vec![Some("__raw".into())],
                vec![],
                Some(convert_json(name("__raw"), shape, ty, name("__p"), 0)),
            ),
        ],
    )
}

/// `{ let __p = joinPath(path, "field"); read }`, then the field's own
/// `where` once the value has its type.
fn checked_field(field_name: &str, read: Expr, ty: &TypeExpr, check: Option<&str>) -> Expr {
    let trial = ty_trial_issues(ty.clone());
    let items = vec![
        let_bind(
            "__p",
            false,
            json_call(
                "joinPath",
                vec![Arg::Pos(name("path")), Arg::Pos(str_lit(field_name))],
            ),
        ),
        let_bind_ty("__read", false, trial.clone(), read),
    ];
    let Some(check) = check else {
        return block(items, name("__read"));
    };
    let checked = match_expr(
        name("__read"),
        vec![
            match_arm_block(
                "Fail",
                vec![Some("__found".into())],
                vec![],
                Some(typed("__out", trial.clone(), call("Fail", vec![Arg::Pos(name("__found"))]))),
            ),
            match_arm_block(
                "Pass",
                vec![Some("__value".into())],
                vec![],
                Some(single_issue(
                    call(check, vec![Arg::Pos(name("__value")), Arg::Pos(name("__p"))]),
                    &trial,
                )),
            ),
        ],
    );
    block(items, checked)
}

/// A missing key: the field's default, `Not` for a `May`, else `Missing`.
fn absent_field(field: &ReadableField) -> Expr {
    let trial = ty_trial_issues(field.ty.clone());
    match (&field.default, &field.shape) {
        (Some(default), _) => typed(
            "__absent",
            trial,
            call(
                "Pass",
                vec![Arg::Pos(typed("__default", field.ty.clone(), default.clone()))],
            ),
        ),
        (None, Shape::May(_)) => typed("__absent", trial, pass_not(&field.ty)),
        (None, _) => missing(&trial),
    }
}

/// `Pass(Not)` for a `May` field that is absent.
fn pass_not(ty: &TypeExpr) -> Expr {
    call(
        "Pass",
        vec![Arg::Pos(typed("__none", ty.clone(), call("Not", vec![])))],
    )
}

/// `Fail([issueMissing(__p)])`
fn missing(trial: &TypeExpr) -> Expr {
    typed(
        "__absent",
        trial.clone(),
        fail_with(json_call("issueMissing", vec![Arg::Pos(name("__p"))])),
    )
}

/// `raw` (a `Json` name) as `ty`, at `path`: a `Trial<ty, Array<Issue>>`.
/// `depth` keeps the names of nested arrays apart.
fn convert_json(raw: Expr, shape: &Shape, ty: &TypeExpr, path: Expr, depth: usize) -> Expr {
    let trial = ty_trial_issues(ty.clone());
    match shape {
        Shape::Str => json_call("asStr", vec![Arg::Pos(raw), Arg::Pos(path)]),
        Shape::Int => json_call("asInt", vec![Arg::Pos(raw), Arg::Pos(path)]),
        Shape::Bool => json_call("asBool", vec![Arg::Pos(raw), Arg::Pos(path)]),
        Shape::Float => json_call("asFloat", vec![Arg::Pos(raw), Arg::Pos(path)]),
        Shape::Product(type_name)
        | Shape::Union(type_name)
        | Shape::Refined { name: type_name, .. } => method_call(
            name(type_name),
            "__fromJson",
            vec![Arg::Pos(raw), Arg::Pos(path)],
        ),
        Shape::Json => typed("__any", trial, call("Pass", vec![Arg::Pos(raw)])),
        Shape::Map(inner) => convert_json_map(raw, inner, ty, path, depth),
        Shape::May(inner) => {
            let inner_ty = type_arg(ty, 0);
            let some = match_expr(
                convert_json(raw.clone(), inner, &inner_ty, path, depth + 1),
                vec![
                    match_arm_block(
                        "Pass",
                        vec![Some("__some".into())],
                        vec![],
                        Some(typed(
                            "__out",
                            trial.clone(),
                            call(
                                "Pass",
                                vec![Arg::Pos(typed(
                                    "__has",
                                    ty.clone(),
                                    call("Has", vec![Arg::Pos(name("__some"))]),
                                ))],
                            ),
                        )),
                    ),
                    match_arm_block(
                        "Fail",
                        vec![Some("__found".into())],
                        vec![],
                        Some(typed("__out", trial.clone(), call("Fail", vec![Arg::Pos(name("__found"))]))),
                    ),
                ],
            );
            typed(
                "__maybe",
                trial.clone(),
                if_expr(
                    json_call("isNull", vec![Arg::Pos(raw)]),
                    Block {
                        items: vec![],
                        tail: Some(Box::new(typed(
                            "__out",
                            trial.clone(),
                            call(
                                "Pass",
                                vec![Arg::Pos(typed("__none", ty.clone(), call("Not", vec![])))],
                            ),
                        ))),
                        span: dummy(),
                    },
                    Some(Block {
                        items: vec![],
                        tail: Some(Box::new(some)),
                        span: dummy(),
                    }),
                ),
            )
        }
        Shape::Array(elem) => {
            let elem_ty = type_arg(ty, 0);
            let at = |base: &str| format!("{base}{depth}");
            let element = convert_json(
                name(&at("__x")),
                elem,
                &elem_ty,
                json_call(
                    "joinPath",
                    vec![Arg::Pos(name(&at("__path"))), Arg::Pos(interp_int(&at("__i")))],
                ),
                depth + 1,
            );
            let each = vec![
                let_bind_ty(&at("__one"), false, ty_trial_issues(elem_ty.clone()), element),
                BlockItem::Expr(match_expr(
                    name(&at("__one")),
                    vec![
                        match_arm_block(
                            "Pass",
                            vec![Some(at("__v"))],
                            vec![BlockItem::Expr(method_call(
                                name(&at("__out")),
                                "append",
                                vec![Arg::Pos(name(&at("__v")))],
                            ))],
                            None,
                        ),
                        match_arm_block(
                            "Fail",
                            vec![Some(at("__found"))],
                            vec![append_all(&at("__bad"), &at("__found"))],
                            None,
                        ),
                    ],
                )),
                assign(&at("__i"), bin(BinaryOp::Add, name(&at("__i")), int_lit(1))),
            ];
            let elements = block(
                vec![
                    let_bind_ty(&at("__out"), true, ty.clone(), empty_array()),
                    let_bind_ty(&at("__bad"), true, ty_array(json_ty("Issue")), empty_array()),
                    let_bind(&at("__i"), true, int_lit(0)),
                    for_each(&at("__x"), None, name(&at("__xs")), each),
                ],
                if_expr(
                    bin(BinaryOp::Gt, field(name(&at("__bad")), "count"), int_lit(0)),
                    Block {
                        items: vec![],
                        tail: Some(Box::new(typed(
                            "__out",
                            trial.clone(),
                            call("Fail", vec![Arg::Pos(name(&at("__bad")))]),
                        ))),
                        span: dummy(),
                    },
                    Some(Block {
                        items: vec![],
                        tail: Some(Box::new(typed(
                            "__out",
                            trial.clone(),
                            call("Pass", vec![Arg::Pos(name(&at("__out")))]),
                        ))),
                        span: dummy(),
                    }),
                ),
            );
            block(
                vec![
                    let_bind(&at("__path"), false, path),
                    let_bind_ty(
                        &at("__list"),
                        false,
                        ty_trial_issues(ty_array(json_ty("Json"))),
                        json_call("asArr", vec![Arg::Pos(raw), Arg::Pos(name(&at("__path")))]),
                    ),
                ],
                typed(
                    &at("__array"),
                    trial.clone(),
                    match_expr(
                        name(&at("__list")),
                        vec![
                            match_arm_block(
                                "Fail",
                                vec![Some("__found".into())],
                                vec![],
                                Some(typed(
                                    "__out",
                                    trial.clone(),
                                    call("Fail", vec![Arg::Pos(name("__found"))]),
                                )),
                            ),
                            match_arm_block(
                                "Pass",
                                vec![Some(at("__xs"))],
                                vec![],
                                Some(elements),
                            ),
                        ],
                    ),
                ),
            )
        }
    }
}

/// `raw` as a `Map<String, T>`: an object whose every value reads as `T`,
/// each at `path.key`.
fn convert_json_map(raw: Expr, inner: &Shape, ty: &TypeExpr, path: Expr, depth: usize) -> Expr {
    let trial = ty_trial_issues(ty.clone());
    let value_ty = type_arg(ty, 1);
    let at = |base: &str| format!("{base}{depth}");
    let value = convert_json(
        name(&at("__x")),
        inner,
        &value_ty,
        json_call(
            "joinPath",
            vec![Arg::Pos(name(&at("__path"))), Arg::Pos(name(&at("__k")))],
        ),
        depth + 1,
    );
    let each = vec![
        let_bind_ty(&at("__one"), false, ty_trial_issues(value_ty.clone()), value),
        BlockItem::Expr(match_expr(
            name(&at("__one")),
            vec![
                match_arm_block(
                    "Pass",
                    vec![Some(at("__v"))],
                    vec![BlockItem::Expr(method_call(
                        name(&at("__out")),
                        "set",
                        vec![Arg::Pos(name(&at("__k"))), Arg::Pos(name(&at("__v")))],
                    ))],
                    None,
                ),
                match_arm_block(
                    "Fail",
                    vec![Some(at("__found"))],
                    vec![append_all(&at("__bad"), &at("__found"))],
                    None,
                ),
            ],
        )),
    ];
    let entries = block(
        vec![
            let_bind_ty(&at("__out"), true, ty.clone(), call("Map", vec![])),
            let_bind_ty(&at("__bad"), true, ty_array(json_ty("Issue")), empty_array()),
            for_each(&at("__k"), Some(&at("__x")), name(&at("__entries")), each),
        ],
        if_expr(
            bin(BinaryOp::Gt, field(name(&at("__bad")), "count"), int_lit(0)),
            Block {
                items: vec![],
                tail: Some(Box::new(typed(
                    "__out",
                    trial.clone(),
                    call("Fail", vec![Arg::Pos(name(&at("__bad")))]),
                ))),
                span: dummy(),
            },
            Some(Block {
                items: vec![],
                tail: Some(Box::new(typed(
                    "__out",
                    trial.clone(),
                    call("Pass", vec![Arg::Pos(name(&at("__out")))]),
                ))),
                span: dummy(),
            }),
        ),
    );
    block(
        vec![
            let_bind(&at("__path"), false, path),
            let_bind_ty(
                &at("__object"),
                false,
                ty_trial_issues(ty_map(ty_named("String"), json_ty("Json"))),
                json_call("asObj", vec![Arg::Pos(raw), Arg::Pos(name(&at("__path")))]),
            ),
        ],
        typed(
            &at("__map"),
            trial.clone(),
            match_expr(
                name(&at("__object")),
                vec![
                    match_arm_block(
                        "Fail",
                        vec![Some("__found".into())],
                        vec![],
                        Some(typed("__out", trial.clone(), call("Fail", vec![Arg::Pos(name("__found"))]))),
                    ),
                    match_arm_block("Pass", vec![Some(at("__entries"))], vec![], Some(entries)),
                ],
            ),
        ),
    )
}

/// `__fromJson_U` for a union of products: the `"type"` key names the
/// member, whose own reader reads the rest of the object (D182 §5).
fn gen_from_json_union(td: &TypeDef, members: &[UnionMember]) -> Function {
    let trial = ty_trial_issues(ty_named(&td.name));
    let mut items = vec![
        let_bind(
            "__obj",
            false,
            try_expr(json_call(
                "asObj",
                vec![Arg::Pos(name("v")), Arg::Pos(name("path"))],
            )),
        ),
        let_bind(
            "__p",
            false,
            json_call(
                "joinPath",
                vec![Arg::Pos(name("path")), Arg::Pos(str_lit("type"))],
            ),
        ),
        let_bind_ty(
            "__tag",
            false,
            ty_trial_issues(ty_named("String")),
            match_expr(
                method_call(name("__obj"), "get", vec![Arg::Pos(str_lit("type"))]),
                vec![
                    match_arm_block("Not", vec![], vec![], Some(missing(&ty_trial_issues(ty_named("String"))))),
                    match_arm_block(
                        "Has",
                        vec![Some("__raw".into())],
                        vec![],
                        Some(json_call("asStr", vec![Arg::Pos(name("__raw")), Arg::Pos(name("__p"))])),
                    ),
                ],
            ),
        ),
        let_bind("__name", false, try_expr(name("__tag"))),
    ];
    for member in members {
        let read = if member.params.is_empty() {
            typed(
                "__out",
                trial.clone(),
                call(
                    "Pass",
                    vec![Arg::Pos(typed("__member", ty_named(&td.name), call(&member.name, vec![])))],
                ),
            )
        } else {
            match_expr(
                method_call(
                    name(&member.name),
                    "__fromJson",
                    vec![Arg::Pos(name("v")), Arg::Pos(name("path"))],
                ),
                vec![
                    match_arm_block(
                        "Pass",
                        vec![Some("__found".into())],
                        vec![],
                        Some(typed(
                            "__out",
                            trial.clone(),
                            call(
                                "Pass",
                                vec![Arg::Pos(typed("__member", ty_named(&td.name), name("__found")))],
                            ),
                        )),
                    ),
                    match_arm_block(
                        "Fail",
                        vec![Some("__found".into())],
                        vec![],
                        Some(typed("__out", trial.clone(), call("Fail", vec![Arg::Pos(name("__found"))]))),
                    ),
                ],
            )
        };
        items.push(BlockItem::Expr(if_expr(
            bin(BinaryOp::Eq, name("__name"), str_lit(&member.name)),
            block_stmts(vec![BlockItem::Expr(return_expr(read))]),
            None,
        )));
    }
    let expected = members
        .iter()
        .map(|member| format!("\"{}\"", member.name))
        .collect::<Vec<_>>()
        .join(" | ");
    Function {
        exported: false,
        name: format!("__fromJson_{}", td.name),
        name_span: dummy(),
        params: vec![
            param("v", json_ty("Json")),
            param_default("path", ty_named("String"), str_lit("")),
        ],
        return_type: Some(trial),
        body: Block {
            items,
            tail: Some(Box::new(fail_with(json_call(
                "issueMismatch",
                vec![
                    Arg::Pos(name("__p")),
                    Arg::Pos(str_lit(&expected)),
                    Arg::Pos(name("__name")),
                ],
            )))),
            span: dummy(),
        },
    }
}

/// `Trial<T, Issue>` (from a `from` or a field check) as
/// `Trial<T, Array<Issue>>`.
fn single_issue(value: Expr, trial: &TypeExpr) -> Expr {
    match_expr(
        value,
        vec![
            match_arm_block(
                "Pass",
                vec![Some("__ok".into())],
                vec![],
                Some(typed("__out", trial.clone(), call("Pass", vec![Arg::Pos(name("__ok"))]))),
            ),
            match_arm_block(
                "Fail",
                vec![Some("__issue".into())],
                vec![],
                Some(typed("__out", trial.clone(), fail_with(name("__issue")))),
            ),
        ],
    )
}

/// `Fail([issue])`
fn fail_with(issue: Expr) -> Expr {
    call(
        "Fail",
        vec![Arg::Pos(Expr {
            kind: ExprKind::ArrayLit(vec![issue]),
            span: dummy(),
        })],
    )
}

/// `for __e in found { into.append(__e) }`
fn append_all(into: &str, found: &str) -> BlockItem {
    for_each(
        "__e",
        None,
        name(found),
        vec![BlockItem::Expr(method_call(
            name(into),
            "append",
            vec![Arg::Pos(name("__e"))],
        ))],
    )
}

/// `{ let label: ty = value; label }`: gives `value` its type.
fn typed(label: &str, ty: TypeExpr, value: Expr) -> Expr {
    block(vec![let_bind_ty(label, false, ty, value)], name(label))
}

fn type_arg(ty: &TypeExpr, index: usize) -> TypeExpr {
    match ty.args.get(index) {
        Some(TypeArg::Type(inner)) => inner.clone(),
        _ => ty_named("Void"),
    }
}

fn else_arm() -> MatchArm {
    MatchArm {
        pattern: Pattern::Else { span: dummy() },
        guard: None,
        body: Block {
            items: vec![],
            tail: None,
            span: dummy(),
        },
    }
}

fn return_expr(value: Expr) -> Expr {
    Expr {
        kind: ExprKind::Return {
            value: Some(Box::new(value)),
        },
        span: dummy(),
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
        Shape::Map(_) | Shape::Product(_) | Shape::Union(_) => "object",
        Shape::Json => "Json",
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
        return_type: Some(ty_trial_issue(ty_named(&td.name))),
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

/// `__fromJson_T` for a refined or literal type: the base value, then `from`.
fn gen_from_json_scalar(td: &TypeDef, base: &str) -> Function {
    let trial = ty_trial_issues(ty_named(&td.name));
    Function {
        exported: false,
        name: format!("__fromJson_{}", td.name),
        name_span: dummy(),
        params: vec![
            param("v", json_ty("Json")),
            param_default("path", ty_named("String"), str_lit("")),
        ],
        return_type: Some(trial.clone()),
        body: Block {
            items: vec![let_bind(
                "__base",
                false,
                try_expr(json_call(
                    as_helper_for_base(base),
                    vec![Arg::Pos(name("v")), Arg::Pos(name("path"))],
                )),
            )],
            tail: Some(Box::new(single_issue(
                method_call(
                    name(&td.name),
                    "__from",
                    vec![Arg::Pos(name("__base")), Arg::Pos(name("path"))],
                ),
                &trial,
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
        return_type: Some(ty_trial_issue(ty_named(&td.name))),
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
        WherePred::Range { lo, hi, inclusive } => {
            let mut c = match lo {
                Some(l) => bin(BinaryOp::Ge, lhs.clone(), lit_expr(l)),
                None => bool_lit(true),
            };
            if let Some(h) = hi {
                // `lo..hi` stops before `hi`, as a `for` range does (D222).
                let below = if *inclusive { BinaryOp::Le } else { BinaryOp::Lt };
                c = bin(BinaryOp::And, c, bin(below, lhs, lit_expr(h)));
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

/// `Trial<ok, Issue>`: one value's check (`from`).
fn ty_trial_issue(ok: TypeExpr) -> TypeExpr {
    ty_trial_of(ok, json_ty("Issue"))
}

/// `Trial<ok, Array<Issue>>`: a read reports every issue (D182 §2).
fn ty_trial_issues(ok: TypeExpr) -> TypeExpr {
    ty_trial_of(ok, ty_array(json_ty("Issue")))
}

fn ty_trial_of(ok: TypeExpr, error: TypeExpr) -> TypeExpr {
    TypeExpr {
        name: "Trial".into(),
        args: vec![TypeArg::Type(ok), TypeArg::Type(error)],
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

/// Every type that appears after `as` in the module, once per reader key
/// (D191).
fn cast_targets(module: &Module) -> Vec<TypeExpr> {
    let mut found: Vec<TypeExpr> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut visit = |ty: &TypeExpr| {
        if seen.insert(cast_reader_key(ty)) {
            found.push(ty.clone());
        }
    };
    for item in &module.items {
        match item {
            Item::Function(f) => casts_in_block(&f.body, &mut visit),
            Item::Type(td) => {
                for method in &td.methods {
                    casts_in_block(&method.body, &mut visit);
                }
            }
            Item::Global(global) => casts_in_expr(&global.init, &mut visit),
            Item::Extern(_) | Item::Import(_) | Item::Extend(_) => {}
        }
    }
    found
}

fn casts_in_block(block: &Block, visit: &mut dyn FnMut(&TypeExpr)) {
    for item in &block.items {
        casts_in_item(item, visit);
    }
    if let Some(tail) = &block.tail {
        casts_in_expr(tail, visit);
    }
}

fn casts_in_item(item: &BlockItem, visit: &mut dyn FnMut(&TypeExpr)) {
    match item {
        BlockItem::Let { init, .. } => casts_in_expr(init, visit),
        BlockItem::Assign { value, .. } => casts_in_expr(value, visit),
        BlockItem::Expr(expr) => casts_in_expr(expr, visit),
    }
}

fn casts_in_args(args: &[Arg], visit: &mut dyn FnMut(&TypeExpr)) {
    for arg in args {
        match arg {
            Arg::Pos(value) | Arg::Named { value, .. } | Arg::Spread { value, .. } => {
                casts_in_expr(value, visit)
            }
        }
    }
}

fn casts_in_expr(expr: &Expr, visit: &mut dyn FnMut(&TypeExpr)) {
    match &expr.kind {
        ExprKind::Cast { expr: inner, ty, .. } => {
            visit(ty);
            casts_in_expr(inner, visit);
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Name(_)
        | ExprKind::MutArg(_)
        | ExprKind::Break
        | ExprKind::Continue => {}
        ExprKind::Interpolate(parts) | ExprKind::TaggedInterp { parts, .. } => {
            for part in parts {
                if let InterpPart::Expr(inner) = part {
                    casts_in_expr(inner, visit);
                }
            }
        }
        ExprKind::Unary { expr: inner, .. }
        | ExprKind::Try(inner)
        | ExprKind::Field { base: inner, .. } => casts_in_expr(inner, visit),
        ExprKind::Binary { left, right, .. } => {
            casts_in_expr(left, visit);
            casts_in_expr(right, visit);
        }
        ExprKind::Call { args, .. } | ExprKind::Start { args, .. } => casts_in_args(args, visit),
        ExprKind::IndirectCall { callee, args } => {
            casts_in_expr(callee, visit);
            casts_in_args(args, visit);
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            casts_in_expr(receiver, visit);
            casts_in_args(args, visit);
        }
        ExprKind::Index { base, index } => {
            casts_in_expr(base, visit);
            casts_in_expr(index, visit);
        }
        ExprKind::ArrayLit(items) => {
            for item in items {
                casts_in_expr(item, visit);
            }
        }
        ExprKind::MapLit(entries) => {
            for entry in entries {
                for inner in entry.exprs() {
                    casts_in_expr(inner, visit);
                }
            }
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            casts_in_expr(cond, visit);
            casts_in_block(then_branch, visit);
            if let Some(branch) = else_branch {
                casts_in_block(branch, visit);
            }
        }
        ExprKind::While { cond, body } => {
            casts_in_expr(cond, visit);
            casts_in_block(body, visit);
        }
        ExprKind::Loop { body } => casts_in_block(body, visit),
        ExprKind::For {
            start, end, body, ..
        } => {
            casts_in_expr(start, visit);
            if let Some(end) = end {
                casts_in_expr(end, visit);
            }
            casts_in_block(body, visit);
        }
        ExprKind::Return { value } => {
            if let Some(value) = value {
                casts_in_expr(value, visit);
            }
        }
        ExprKind::Block(block) => casts_in_block(block, visit),
        ExprKind::Match { scrutinee, arms } => {
            casts_in_expr(scrutinee, visit);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    casts_in_expr(guard, visit);
                }
                casts_in_block(&arm.body, visit);
            }
        }
    }
}

/// The readers a checked `x as T` may call, generated in the module that
/// casts (D191 §1): `__asJson_K` (from a `Json`), `__asBase_K` (into a
/// refined type or a literal union), `__asNarrow_K` (into a narrower
/// integer). MIR picks one by the operand's type; the others go unused.
fn gen_cast_readers(
    target: &TypeExpr,
    module_id: &str,
    world: &World,
    resolve: &Resolve,
) -> Vec<Function> {
    let key = cast_reader_key(target);
    let mut readers = Vec::new();
    if !names_std_type(target, module_id, resolve) {
        if let Some(shape) = shape_of(target, module_id, world, resolve, &mut HashSet::new()) {
            if !matches!(shape, Shape::Json) {
                readers.push(gen_as_json(&key, target, &shape));
            }
        }
    }
    if target.args.is_empty() {
        let defined = resolve
            .get(module_id)
            .and_then(|names| names.get(&target.name))
            .filter(|(def_mod, _)| !is_std_module(def_mod))
            .and_then(|(def_mod, def_name)| {
                world.get(def_mod).and_then(|tys| tys.get(def_name)).map(|td| (def_mod, td))
            });
        if let Some((def_mod, td)) = defined {
            let base = refined_alias_of(td, def_mod, world, resolve)
                .map(|(base, _)| base)
                .or_else(|| pure_lit_union_of(td).map(|(base, _)| base));
            if let Some(base) = base {
                readers.push(gen_as_base(&key, target, base));
            }
        }
        if let Some((min, max)) = narrow_range(&target.name) {
            readers.push(gen_as_narrow(&key, target, min, max));
        }
        if target.name == "Float64" {
            readers.push(gen_as_float(&key, target, "asFloat", "Int64"));
            readers.push(gen_as_float(&key, target, "asFloatUnsigned", "UInt64"));
        }
    }
    readers
}

/// `__asFloat_K(value: Int64) -> Trial<Float64, Array<Issue>>` (and
/// `__asFloatUnsigned_K` for a UInt64): the value when it is exactly a
/// Float64, else `Rejected("", "9007199254740993 does not fit in Float64
/// exactly")` (D224).
fn gen_as_float(key: &str, target: &TypeExpr, kind: &str, source: &str) -> Function {
    let trial = ty_trial_issues(target.clone());
    let rounded = call(
        "Float64",
        vec![Arg::Named {
            name: "rounding".into(),
            name_span: dummy(),
            value: name("value"),
        }],
    );
    let reason = Expr {
        kind: ExprKind::Interpolate(vec![
            InterpPart::Expr(name("value")),
            InterpPart::Lit(" does not fit in Float64 exactly".into()),
        ]),
        span: dummy(),
    };
    let rejected = json_call("issueRejected", vec![Arg::Pos(str_lit("")), Arg::Pos(reason)]);
    Function {
        exported: false,
        name: format!("__{kind}_{key}"),
        name_span: dummy(),
        params: vec![param("value", ty_named(source))],
        return_type: Some(trial.clone()),
        body: Block {
            items: vec![],
            tail: Some(Box::new(if_expr(
                call("__float64Exact", vec![Arg::Pos(name("value"))]),
                Block {
                    items: vec![],
                    tail: Some(Box::new(typed(
                        "__out",
                        trial.clone(),
                        call("Pass", vec![Arg::Pos(rounded)]),
                    ))),
                    span: dummy(),
                },
                Some(Block {
                    items: vec![],
                    tail: Some(Box::new(typed("__out", trial, fail_with(rejected)))),
                    span: dummy(),
                }),
            ))),
            span: dummy(),
        },
    }
}

/// A type that names a `std/` type other than `Json`: those have no
/// generated readers.
fn names_std_type(ty: &TypeExpr, module_id: &str, resolve: &Resolve) -> bool {
    let own = resolve
        .get(module_id)
        .and_then(|names| names.get(&ty.name))
        .is_some_and(|(def_mod, def_name)| {
            is_std_module(def_mod) && !(def_mod == JSON_MODULE_ID && def_name == "Json")
        });
    own || ty.args.iter().any(|arg| match arg {
        TypeArg::Type(inner) => names_std_type(inner, module_id, resolve),
        TypeArg::Int { .. } => false,
    })
}

/// `__asJson_K(value: Json) -> Trial<T, Array<Issue>>`: the reader D182
/// builds for a field of type `T`, at the root.
fn gen_as_json(key: &str, target: &TypeExpr, shape: &Shape) -> Function {
    Function {
        exported: false,
        name: format!("__asJson_{key}"),
        name_span: dummy(),
        params: vec![param("value", json_ty("Json"))],
        return_type: Some(ty_trial_issues(target.clone())),
        body: Block {
            items: vec![],
            tail: Some(Box::new(convert_json(name("value"), shape, target, str_lit(""), 0))),
            span: dummy(),
        },
    }
}

/// `__asBase_K(value: Base) -> Trial<T, Array<Issue>>`: `T.from`, its one
/// issue in an array.
fn gen_as_base(key: &str, target: &TypeExpr, base: &str) -> Function {
    let trial = ty_trial_issues(target.clone());
    Function {
        exported: false,
        name: format!("__asBase_{key}"),
        name_span: dummy(),
        params: vec![param("value", ty_named(base))],
        return_type: Some(trial.clone()),
        body: Block {
            items: vec![],
            tail: Some(Box::new(single_issue(
                method_call(name(&target.name), "__from", vec![Arg::Pos(name("value"))]),
                &trial,
            ))),
            span: dummy(),
        },
    }
}

/// `__asNarrow_K(value: Int64) -> Trial<T, Array<Issue>>`: the value when it
/// fits, else `Rejected("", "300 does not fit in Byte")`.
fn gen_as_narrow(key: &str, target: &TypeExpr, min: i64, max: i64) -> Function {
    let trial = ty_trial_issues(target.clone());
    let fits = bin(
        BinaryOp::And,
        bin(BinaryOp::Ge, name("value"), int_lit(min)),
        bin(BinaryOp::Le, name("value"), int_lit(max)),
    );
    let kept = call(
        &target.name,
        vec![Arg::Named {
            name: "truncating".into(),
            name_span: dummy(),
            value: name("value"),
        }],
    );
    let reason = Expr {
        kind: ExprKind::Interpolate(vec![
            InterpPart::Expr(name("value")),
            InterpPart::Lit(format!(" does not fit in {}", target.name)),
        ]),
        span: dummy(),
    };
    let rejected = json_call("issueRejected", vec![Arg::Pos(str_lit("")), Arg::Pos(reason)]);
    Function {
        exported: false,
        name: format!("__asNarrow_{key}"),
        name_span: dummy(),
        params: vec![param("value", ty_named("Int64"))],
        return_type: Some(trial.clone()),
        body: Block {
            items: vec![],
            tail: Some(Box::new(if_expr(
                fits,
                Block {
                    items: vec![],
                    tail: Some(Box::new(typed("__out", trial.clone(), call("Pass", vec![Arg::Pos(kept)])))),
                    span: dummy(),
                },
                Some(Block {
                    items: vec![],
                    tail: Some(Box::new(typed("__out", trial, fail_with(rejected)))),
                    span: dummy(),
                }),
            ))),
            span: dummy(),
        },
    }
}

/// The values of a narrower integer type, as `Int64`.
fn narrow_range(name: &str) -> Option<(i64, i64)> {
    match name {
        "Int32" => Some((i64::from(i32::MIN), i64::from(i32::MAX))),
        "UInt32" => Some((0, i64::from(u32::MAX))),
        "UInt64" => Some((0, i64::MAX)),
        "Byte" => Some((0, 255)),
        _ => None,
    }
}

/// The type's `from(json: Json)` clause, if it has one (D191 §3): its index and
/// whether it can fail. Such a clause is the type's JSON reader, so it
/// returns the type or `Trial<T, Array<Issue>>`.
fn json_conversion(td: &TypeDef, module_id: &str, resolve: &Resolve) -> Result<Option<(usize, bool)>> {
    for (index, clause) in td.conversions.iter().enumerate() {
        let source = &clause.param.ty;
        let is_json = source.args.is_empty()
            && resolve
                .get(module_id)
                .and_then(|names| names.get(&source.name))
                .is_some_and(|(def_mod, def_name)| def_mod == JSON_MODULE_ID && def_name == "Json");
        if !is_json {
            continue;
        }
        let returns = &clause.return_type;
        if returns.name == td.name {
            return Ok(Some((index, false)));
        }
        let issues = matches!(
            returns.args.get(1),
            Some(TypeArg::Type(error)) if error.name == "Array"
                && matches!(error.args.first(), Some(TypeArg::Type(item)) if item.name == "Issue")
        );
        if !issues {
            return Err(Error::at(
                format!(
                    "a `from(json: Json)` clause is the JSON reader of `{0}`, so it returns `{0}` or `Trial<{0}, Array<Issue>>`",
                    td.name
                ),
                returns.span,
            ));
        }
        return Ok(Some((index, true)));
    }
    Ok(None)
}

/// `__fromJson_T(v, path)` over the type's own `from(json: Json)` clause. The
/// clause reports paths from its own root; they are moved under `path`.
fn gen_custom_from_json(td: &TypeDef, index: usize, fallible: bool) -> Function {
    let trial = ty_trial_issues(ty_named(&td.name));
    let converted = call(
        &crate::conversion_desugar::conversion_fn_name(&td.name, index),
        vec![Arg::Pos(name("v"))],
    );
    let tail = if fallible {
        match_expr(
            converted,
            vec![
                match_arm_block(
                    "Pass",
                    vec![Some("__found".into())],
                    vec![],
                    Some(typed("__out", trial.clone(), call("Pass", vec![Arg::Pos(name("__found"))]))),
                ),
                match_arm_block(
                    "Fail",
                    vec![Some("__issues".into())],
                    vec![],
                    Some(typed(
                        "__out",
                        trial.clone(),
                        call(
                            "Fail",
                            vec![Arg::Pos(json_call(
                                "underPath",
                                vec![Arg::Pos(name("__issues")), Arg::Pos(name("path"))],
                            ))],
                        ),
                    )),
                ),
            ],
        )
    } else {
        typed("__out", trial, call("Pass", vec![Arg::Pos(converted)]))
    };
    Function {
        exported: false,
        name: format!("__fromJson_{}", td.name),
        name_span: dummy(),
        params: vec![
            param("v", json_ty("Json")),
            param_default("path", ty_named("String"), str_lit("")),
        ],
        return_type: Some(ty_trial_issues(ty_named(&td.name))),
        body: Block {
            items: vec![],
            tail: Some(Box::new(tail)),
            span: dummy(),
        },
    }
}

/// Give each `__routeBody_K(request)` stub the route desugar added its body
/// (D191 §5): the query or a form body through `T.readForm` when `T` reads
/// from a form, a JSON body through `T.read`; an `Array<T>` body reads JSON
/// as a top-level array, `readJson(body)? as Array<T>` (D218). Returns
/// whether any was filled.
fn fill_body_readers(
    module: &mut Module,
    module_id: &str,
    world: &World,
    resolve: &Resolve,
    forms: bool,
) -> bool {
    let mut filled = false;
    for item in &mut module.items {
        let Item::Function(f) = item else {
            continue;
        };
        if !f.name.starts_with(crate::route_desugar::BODY_READER_PREFIX)
            || !f.body.items.is_empty()
            || f.body.tail.is_some()
        {
            continue;
        }
        let Some(TypeArg::Type(target)) = f.return_type.as_ref().and_then(|t| t.args.first()) else {
            continue;
        };
        let target = target.clone();
        let trial = ty_trial_issues(target.clone());
        let array = target.name == "Array";
        let shown = match target.args.first() {
            Some(TypeArg::Type(element)) if array => format!("Array<{}>", element.name),
            _ => target.name.clone(),
        };
        let form_ok = !array
            && forms
            && resolve
                .get(module_id)
                .and_then(|names| names.get(&target.name))
                .and_then(|(def_mod, def_name)| {
                    world.get(def_mod).and_then(|tys| tys.get(def_name)).map(|td| (def_mod, td))
                })
                .is_some_and(|(def_mod, td)| {
                    form_readable_product(td, def_mod, world, resolve, &mut HashSet::new())
                });
        let body_text = method_call(field(name("request"), "body"), "toString", vec![]);
        let form_read = |text: Expr| {
            if form_ok {
                method_call(name(&target.name), "readForm", vec![Arg::Pos(text)])
            } else {
                typed(
                    "__out",
                    trial.clone(),
                    fail_with(json_call(
                        "issueRejected",
                        vec![
                            Arg::Pos(str_lit("")),
                            Arg::Pos(str_lit(&format!("{shown} is read from JSON only"))),
                        ],
                    )),
                )
            }
        };
        let read_json = if array {
            Expr {
                kind: ExprKind::Cast {
                    expr: Box::new(try_expr(json_call(
                        "readJson",
                        vec![Arg::Pos(body_text.clone())],
                    ))),
                    ty: target.clone(),
                    propagate: false,
                },
                span: dummy(),
            }
        } else {
            method_call(name(&target.name), "read", vec![Arg::Pos(body_text.clone())])
        };
        let when = |kind: &str, value: Expr| {
            BlockItem::Expr(if_expr(
                bin(BinaryOp::Eq, name("__kind"), str_lit(kind)),
                block_stmts(vec![BlockItem::Expr(return_expr(value))]),
                None,
            ))
        };
        f.body = Block {
            items: vec![
                let_bind("__kind", false, http_call("bodyKind", vec![Arg::Pos(name("request"))])),
                when("query", form_read(field(name("request"), "query"))),
                when("form", form_read(body_text)),
            ],
            tail: Some(Box::new(read_json)),
            span: dummy(),
        };
        filled = true;
    }
    filled
}
