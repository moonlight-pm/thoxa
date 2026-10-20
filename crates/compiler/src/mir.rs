//! Mid-level IR: typed enough for RC insert + C codegen.
//! R8: multi-file modules, import/export, module-level let/mut.

use crate::ast::{
    self, accessor_of, getter_name, setter_name, AccessorKind, TraitRef, Arg, AssignTarget, BinaryOp, Block, BlockItem, Expr, ExprKind, InterpPart, Item, MapEntry,
    MatchArm, Method, Pattern, TypeArg, TypeDef, TypeExpr, TypeLit, TypeMark, UnaryOp,
};
use crate::error::{Error, Result};
use crate::loader::{self, BoundImport, Package, SourceModule};
use crate::span::Span;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

mod codec;
mod convert;
mod generic;
mod order;

/// Synthetic union types inferred for unannotated `if` lub (D119).
#[derive(Default)]
struct SyntheticRegistry {
    /// mangled name → (type info, layout)
    entries: HashMap<String, (TypeInfo, TypeLayout)>,
    /// User method calls whose receiver cannot be written (D163). Each is an
    /// error only if the method writes its receiver, which is known once
    /// every body in the package is lowered.
    receiver_checks: Vec<ReceiverCheck>,
    /// Getters, each an error if it writes its receiver (D228): reading a
    /// property never changes the value, as reading a field does not.
    getter_checks: Vec<GetterCheck>,
    /// Types whose `==` is a generated function `__eq_{type}`, because they
    /// contain themselves (through an Array, a Map, or a union arm), so
    /// expanding their comparison inline would not end (D212).
    eq_functions: Vec<String>,
    /// Conformances that hold per instance (D228): a generic type's, keyed by
    /// its mangled template name, and the built-in types' (`Array`, `Int64`).
    conditional: HashMap<String, Vec<ast::ConditionalConformance>>,
    /// Generic functions' templates, by mangled name (D228 §5).
    generic_fns: HashMap<String, generic::GenericFn>,
    /// The instances made so far, by name, with their signatures.
    fn_instances: HashMap<String, FnSig>,
    /// Instances still to lower.
    instance_jobs: Vec<generic::InstanceJob>,
    /// Generic types' templates, for instances made while lowering.
    generics: HashMap<String, GenericDef>,
    /// Methods of the generic types' instances made while lowering.
    late_mono_jobs: Vec<MonoMethodJob>,
    /// Each type's associated types (D228 §7), by its mangled name: what a
    /// generic function's `S.Item` is for an `S`.
    assoc: HashMap<String, HashMap<String, Ty>>,
    /// `mixed T`'s members that name an associated type, by the union.
    mixed_closed: HashMap<String, HashSet<String>>,
    /// `mixed T`'s trait and the members of its lineage, by the union
    /// (D235).
    mixed_members: HashMap<String, (TraitRef, Vec<ast::TraitMember>)>,
    /// Those members' signatures, resolved on first call.
    mixed_sigs: HashMap<String, HashMap<String, MethodSig>>,
    /// The members a `mixed T` gained as values of them converted into it
    /// (D235): a generic type's instances and built-in types, by the union.
    mixed_added: HashMap<String, Vec<SumVariant>>,
    /// The functions calls through `mixed` values make, made once every
    /// body is lowered, over the members known then (D235).
    mixed_functions: Vec<MixedFunction>,
    /// Built-in types' associated types, by the type's name (D228 §7).
    built_in_assoc: crate::trait_desugar::BuiltInAssoc,
    /// Methods made from templates (`map_String`), by their type (D228 §5).
    method_instances: HashMap<String, HashMap<String, MethodSig>>,
    /// Those methods' bodies, still to lower.
    method_jobs: Vec<generic::MethodJob>,
}

/// A call's written type arguments (D230), resolved in its module.
fn resolve_type_args(ctx: &mut LowerCtx, type_args: &[TypeExpr]) -> Result<Vec<Ty>> {
    let name_map = ctx.name_to_type.clone();
    let mut out = Vec::new();
    for te in type_args {
        generic::ensure_instances(ctx, te, &name_map)?;
        out.push(resolve_ty(te, &ctx.types, &name_map)?);
    }
    Ok(out)
}

/// Type arguments written on a call that takes none.
fn no_type_args(explicit: &[Ty], name: &str, span: Span) -> Result<()> {
    if explicit.is_empty() {
        return Ok(());
    }
    Err(Error::at(format!("`{name}` takes no type arguments"), span))
}

/// `Box<Int64>(value: 1)` and `Map<String, Int64>()`: a generic type's
/// constructor with its instance written out is the constructor that
/// instance is expected (D230). `None` for a generic function, which takes
/// them itself.
fn explicit_type_call(
    ctx: &mut LowerCtx,
    callee: &str,
    args: &[Arg],
    span: Span,
    expect: Option<&Ty>,
    type_args: &[TypeExpr],
    explicit: &[Ty],
) -> Result<Option<(Place, Ty)>> {
    if ctx.lookup(callee).is_some() {
        return Err(Error::at(format!("`{callee}` is a function value: it takes no type arguments"), span));
    }
    if callee == "Map" {
        let [key, val] = explicit else {
            return Err(Error::at("`Map` takes 2 type arguments: `Map<K, V>()`", span));
        };
        let ty = Ty::Map {
            key: Box::new(key.clone()),
            val: Box::new(val.clone()),
        };
        return lower_map_new(ctx, args, span, Some(&ty)).map(Some);
    }
    // `Has<Int64>(5)` makes the `May<Int64>` it is a member of; a `Pass` or
    // `Fail` names only one side of its `Trial`.
    match callee {
        "Has" if explicit.len() == 1 => {
            let may = TypeExpr {
                name: "std_fallible_May".into(),
                args: type_args.iter().cloned().map(TypeArg::Type).collect(),
                span,
                lit: None,
                mark: None,
                union_members: None,
                fn_type: None,
                where_clause: None,
            };
            let name_map = ctx.name_to_type.clone();
            generic::ensure_instances(ctx, &may, &name_map)?;
            let may = resolve_ty(&may, &ctx.types, &name_map)?;
            return lower_call(ctx, callee, args, span, Some(expect.unwrap_or(&may)), &[]).map(Some);
        }
        "Pass" | "Fail" => {
            return Err(Error::at(
                format!("`{callee}` takes its `Trial` from where it is used, not from type arguments"),
                span,
            )
            .with_help("annotate the value: `let result: Trial<Int64, String> = Pass(5)`"))
        }
        _ => {}
    }
    let mangled = ctx.name_to_fn.get(callee).cloned().unwrap_or_else(|| callee.to_string());
    if ctx.synthetics.borrow().generic_fns.contains_key(&mangled) {
        return Ok(None);
    }
    let base = ctx.name_to_type.get(callee).cloned().unwrap_or_else(|| callee.to_string());
    if ctx.synthetics.borrow().generics.contains_key(&base) {
        let written = TypeExpr {
            name: callee.to_string(),
            args: type_args.iter().cloned().map(TypeArg::Type).collect(),
            span,
            lit: None,
            mark: None,
            union_members: None,
            fn_type: None,
            where_clause: None,
        };
        let name_map = ctx.name_to_type.clone();
        generic::ensure_instances(ctx, &written, &name_map)?;
        let instance = resolve_ty(&written, &ctx.types, &name_map)?;
        let _ = expect;
        return lower_call(ctx, callee, args, span, Some(&instance), &[]).map(Some);
    }
    Err(Error::at(format!("`{callee}` takes no type arguments"), span))
}

/// The types and methods made while lowering, added to a context's `types`:
/// generic types' instances, and methods made from templates.
fn seed_synthetics(types: &mut HashMap<String, TypeInfo>, synthetics: &SyntheticRegistry) {
    for (name, (info, _)) in synthetics.entries.iter() {
        types.entry(name.clone()).or_insert_with(|| info.clone());
    }
    for (mixed, added) in &synthetics.mixed_added {
        add_mixed_variants(types, mixed, added);
    }
    for (type_name, methods) in &synthetics.method_instances {
        if let Some(info) = types.get_mut(type_name) {
            for (name, sig) in methods {
                info.methods.entry(name.clone()).or_insert_with(|| sig.clone());
            }
        }
    }
}

/// Join `added` to `mixed`'s members in `types`, each once (D235).
fn add_mixed_variants(types: &mut HashMap<String, TypeInfo>, mixed: &str, added: &[SumVariant]) {
    if let Some(sum) = types.get_mut(mixed).and_then(|info| info.sum.as_mut()) {
        for variant in added {
            if sum.variants.iter().all(|known| known.tag != variant.tag) {
                sum.variants.push(variant.clone());
            }
        }
    }
}

/// A function calls through a `mixed T` value make (D235), made over the
/// union's members once every body is lowered.
#[derive(Clone)]
struct MixedFunction {
    name: String,
    mixed: String,
    kind: MixedKind,
}

#[derive(Clone)]
enum MixedKind {
    /// One of the trait's members, called on the value inside.
    Member { method: String, sig: MethodSig },
    /// `compare`'s sign: by member type, the types ranked by name, then by
    /// the values.
    Compare,
}

/// Whether a Map keyed by the type `name` calls its own `hash` and `equals`
/// (D228): it is `Hashable` and writes them, rather than taking the
/// compiler's field-by-field ones.
fn own_key_ops(
    types: &HashMap<String, TypeInfo>,
    conditional: &HashMap<String, Vec<ast::ConditionalConformance>>,
    name: &str,
) -> (bool, bool) {
    let Some(info) = types.get(name) else {
        return (false, false);
    };
    if !ty_conforms(types, conditional, &Ty::User(name.to_string()), &core_trait("Hashable")) {
        return (false, false);
    }
    let own = |member: &str| info.methods.contains_key(member) && !info.derived.contains(member);
    (own("hash"), own("equals"))
}

/// A generic type's instance for `args` meets its parameters' bounds.
fn instance_meets_bounds(
    types: &HashMap<String, TypeInfo>,
    conditional: &HashMap<String, Vec<ast::ConditionalConformance>>,
    generics: &HashMap<String, GenericDef>,
    tables: &AssocTables,
    base: &str,
    args: &[Ty],
) -> Result<()> {
    let Some(gdef) = generics.get(base) else {
        return Ok(());
    };
    let shown_instance = || {
        let shown: Vec<String> = args.iter().map(display_ty).collect();
        format!("{}<{}>", gdef.source_name, shown.join(", "))
    };
    // Its parameters' associated types meet theirs (D237), but a checking
    // instance's are archetypes.
    if !gdef.derived.is_empty() && gdef.checking.iter().all(|(each, ..)| each != args) {
        let mut subst: HashMap<String, Ty> = gdef.type_params.iter().cloned().zip(args.iter().cloned()).collect();
        derived_subst(gdef, &mut subst, types, generics, tables, &mut Vec::new(), gdef.name_span)?;
        // `S.Item` before `S.Cursor.Item`, the path the trait fixes it as.
        let mut by_name: Vec<&ast::DerivedParam> = gdef.derived.iter().collect();
        by_name.sort_by_key(|param| param.name.len());
        for param in by_name {
            let Some(ty) = subst.get(&param.name) else { continue };
            for bound in &param.bounds {
                if !ty_conforms(types, conditional, ty, bound) {
                    return Err(Error::at(
                        format!(
                            "`{}`: `{}` must be `{}`, and {} is not",
                            shown_instance(),
                            param.name,
                            bound.name,
                            display_ty(ty)
                        ),
                        gdef.name_span,
                    ));
                }
            }
            if let Some(fixed) = &param.fixed {
                let written = subst_type_expr(fixed, &subst);
                let want = resolve_ty(&written, types, &gdef.name_map)?;
                if want != *ty {
                    return Err(Error::at(
                        format!(
                            "`{}`: `{}` must be {}, and it is {}",
                            shown_instance(),
                            param.name,
                            display_ty(&want),
                            display_ty(ty)
                        ),
                        gdef.name_span,
                    ));
                }
            }
        }
    }
    for ((param, bounds), arg) in gdef.type_params.iter().zip(&gdef.bounds).zip(args) {
        for bound in bounds {
            if !ty_conforms(types, conditional, arg, bound) {
                let shown: Vec<String> = args.iter().map(display_ty).collect();
                return Err(Error::at(
                    format!(
                        "`{}<{}>`: `{param}` must be `{}`, and {} is not",
                        gdef.source_name,
                        shown.join(", "),
                        bound.name,
                        display_ty(arg)
                    ),
                    gdef.name_span,
                ));
            }
        }
    }
    Ok(())
}

/// A trait std's core module declares (`Equatable`, `Hashable`, `Ordered`).
fn core_trait(name: &str) -> TraitRef {
    TraitRef {
        module: "std_core".into(),
        name: name.into(),
    }
}

/// Whether values of `ty` conform to `trait_ref` (D228): declared, through a
/// conditional conformance whose bounds its arguments meet, or, for a
/// refinement, through its base. A union of literals is `Equatable` and
/// `Hashable` as its literals' type is.
fn ty_conforms(
    types: &HashMap<String, TypeInfo>,
    conditional: &HashMap<String, Vec<ast::ConditionalConformance>>,
    ty: &Ty,
    trait_ref: &TraitRef,
) -> bool {
    fn walk(
        types: &HashMap<String, TypeInfo>,
        conditional: &HashMap<String, Vec<ast::ConditionalConformance>>,
        ty: &Ty,
        trait_ref: &TraitRef,
        depth: u32,
    ) -> bool {
        // A type that contains itself meets its own bound.
        if depth > 32 {
            return true;
        }
        let holds = |key: &str, args: &[Ty]| {
            conditional.get(key).is_some_and(|list| {
                list.iter().any(|each| {
                    each.trait_ref == *trait_ref
                        && each.bounds.iter().zip(args).all(|(bounds, arg)| {
                            bounds
                                .iter()
                                .all(|bound| walk(types, conditional, arg, bound, depth + 1))
                        })
                })
            })
        };
        match ty {
            Ty::User(name) => {
                let Some(info) = types.get(name) else {
                    return false;
                };
                if info.traits.contains(trait_ref) {
                    return true;
                }
                if let Some(refined) = &info.refined {
                    return walk(types, conditional, &refined.base, trait_ref, depth + 1);
                }
                if let Some((base, args)) = &info.instance {
                    return holds(base, args);
                }
                if let Some(sum) = &info.sum {
                    let literal = sum.variants.iter().all(|variant| variant.lit.is_some());
                    let synthetic = name.starts_with('$');
                    if (literal && trait_ref.name != "Ordered") || synthetic {
                        return sum.variants.iter().all(|variant| match &variant.lit {
                            Some(TypeLit::Int(_)) => holds("Int64", &[]),
                            Some(TypeLit::Bool(_)) => holds("Bool", &[]),
                            Some(TypeLit::String(_)) => holds("String", &[]),
                            None => walk(types, conditional, &variant.ty, trait_ref, depth + 1),
                        });
                    }
                }
                false
            }
            Ty::Array { elem } => holds("Array", std::slice::from_ref(elem.as_ref())),
            Ty::Map { key, val } => holds("Map", &[key.as_ref().clone(), val.as_ref().clone()]),
            Ty::Int64 => holds("Int64", &[]),
            Ty::Int32 => holds("Int32", &[]),
            Ty::UInt64 => holds("UInt64", &[]),
            Ty::UInt32 => holds("UInt32", &[]),
            Ty::Byte => holds("Byte", &[]),
            Ty::Float64 => holds("Float64", &[]),
            Ty::Bool => holds("Bool", &[]),
            Ty::String => holds("String", &[]),
            Ty::Bytes => holds("Bytes", &[]),
            Ty::Buffer => holds("Buffer", &[]),
            Ty::Void => holds("Void", &[]),
            Ty::Node => holds("Node", &[]),
            _ => false,
        }
    }
    walk(types, conditional, ty, trait_ref, 0)
}

impl LowerCtx {
    /// Take the generic types' instances made since this context was, as a
    /// probe of an argument's type makes them (D228 §5).
    fn take_new_instances(&mut self) {
        seed_synthetics(&mut self.types, &self.synthetics.borrow());
    }

    /// Whether `ty` conforms to std's core trait `name` (D228).
    fn conforms(&self, ty: &Ty, name: &str) -> bool {
        ty_conforms(&self.types, &self.synthetics.borrow().conditional, ty, &core_trait(name))
    }

    /// Whether `ty` declares std/json's `Encodable` or `Decodable` (D228).
    fn json_declares(&self, ty: &Ty, name: &str) -> bool {
        let trait_ref = TraitRef {
            module: "std_json".into(),
            name: name.into(),
        };
        ty_conforms(&self.types, &self.synthetics.borrow().conditional, ty, &trait_ref)
    }
}

/// The first field (or a union's member) of `type_name` whose type is a
/// declared type that does not declare `name` (D228): why it has no codec.
fn undeclared_part(ctx: &LowerCtx, type_name: &str, name: &str) -> Option<(String, Ty)> {
    fn inner(ty: &Ty) -> &Ty {
        match ty {
            Ty::Array { elem } => inner(elem),
            Ty::Map { val, .. } => inner(val),
            _ => ty,
        }
    }
    let info = ctx.types.get(type_name)?;
    let parts: Vec<(String, Ty)> = match &info.sum {
        Some(sum) => sum
            .variants
            .iter()
            .filter(|variant| variant.lit.is_none())
            .map(|variant| (variant.name.clone(), variant.ty.clone()))
            .collect(),
        None => info.fields.iter().map(|field| (field.name.clone(), field.ty.clone())).collect(),
    };
    parts.into_iter().find(|(_, ty)| {
        let ty = inner(ty);
        matches!(ty, Ty::User(_)) && !ctx.json_declares(ty, name)
    })
}

/// "`Point` is not `Encodable`", with how to declare it (D228).
fn not_codec(ty: &Ty, name: &str, span: Span) -> Error {
    let shown = display_ty(ty);
    Error::at(format!("`{shown}` is not `{name}`"), span).with_help(format!(
        "declare `conform {shown} to {name}`, with `import {{ {name} }} from \"std/json\"`"
    ))
}

/// A user method called on an immutable binding, a field, or a temporary.
#[derive(Debug, Clone)]
struct ReceiverCheck {
    /// The method's MIR function name (`{type}__{method}`).
    function: String,
    method: String,
    /// What the receiver is, for the message: "immutable binding `x`", …
    receiver: String,
    span: Span,
    module_id: String,
}

/// A getter's function, for the check that it does not write its receiver.
#[derive(Debug, Clone)]
struct GetterCheck {
    function: String,
    property: String,
    span: Span,
    module_id: String,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub types: Vec<TypeLayout>,
    /// Extern declarations (for prototypes / includes in cgen).
    pub externs: Vec<ExternDecl>,
    /// Module-level statics (`export let`, private `mut`, …).
    pub globals: Vec<GlobalVar>,
    /// Statements run once before `main` (module value initializers, dep order).
    pub init: Vec<Stmt>,
    pub functions: Vec<Function>,
}

/// Keep only the functions a program can reach from `main` and its module
/// initializers (D197). Every loaded module is lowered whole, std/string
/// included, but a program carries only the code it calls. A `Resource`'s
/// `drop` (`Type__drop`) stays: the runtime calls it.
pub fn retain_reachable(program: &mut Program) {
    let mut reached: HashSet<String> = HashSet::new();
    let mut pending: Vec<String> = Vec::new();
    for function in &program.functions {
        if function.is_main {
            pending.push(function.name.clone());
        }
    }
    for layout in &program.types {
        if layout.has_drop {
            pending.push(format!("{}__drop", layout.name));
        }
        // A Map's key ops call a key type's own `hash` / `equals` (D228).
        if layout.own_hash {
            pending.push(format!("{}__hash", layout.name));
        }
        if layout.own_equals {
            pending.push(format!("{}__equals", layout.name));
        }
    }
    referenced_fns(&program.init, &mut pending);
    let bodies: HashMap<&str, &Vec<Stmt>> = program
        .functions
        .iter()
        .map(|function| (function.name.as_str(), &function.body))
        .collect();
    while let Some(name) = pending.pop() {
        if !reached.insert(name.clone()) {
            continue;
        }
        if let Some(body) = bodies.get(name.as_str()) {
            referenced_fns(body, &mut pending);
        }
    }
    program
        .functions
        .retain(|function| reached.contains(&function.name));
}

/// The functions `stmts` call, start as tasks, or take as values.
fn referenced_fns(stmts: &[Stmt], out: &mut Vec<String>) {
    let callee_fn = |callee: &Callee| match callee {
        Callee::User(name) => Some(name.clone()),
        Callee::Method { type_name, method } => Some(format!("{type_name}__{method}")),
        Callee::Builtin(_) | Callee::Extern { .. } | Callee::Indirect { .. } => None,
    };
    for stmt in stmts {
        let value = match stmt {
            Stmt::Assign { value, .. } | Stmt::Eval(value) => Some(value),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                referenced_fns(then_body, out);
                referenced_fns(else_body, out);
                None
            }
            Stmt::Loop { body } | Stmt::ForRange { body, .. } | Stmt::StoreBack { body, .. } => {
                referenced_fns(body, out);
                None
            }
            Stmt::Start { callee, .. } => {
                out.extend(callee_fn(callee));
                None
            }
            _ => None,
        };
        match value {
            Some(RValue::Call { callee, .. }) => out.extend(callee_fn(callee)),
            Some(RValue::MakeFn { target, .. }) => out.push(match target {
                FnTarget::Free { mangled } => mangled.clone(),
                FnTarget::Method {
                    type_name, method, ..
                } => format!("{type_name}__{method}"),
            }),
            _ => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct GlobalVar {
    pub name: String,
    pub ty: Ty,
}

/// A bound C function (library `"c"` only in R6).
#[derive(Debug, Clone)]
pub struct ExternDecl {
    pub thoxa_name: String,
    pub c_symbol: String,
    pub params: Vec<(String, Ty)>,
    pub ret: Ty,
}

#[derive(Debug, Clone)]
pub struct TypeLayout {
    pub name: String,
    /// Field name (source) → C local-style sanitized field name, type.
    pub fields: Vec<FieldLayout>,
    /// D118: when set, this type is a union alias (tagged sum).
    pub sum: Option<Vec<SumVariantLayout>>,
    /// Pure-literal-only unions: C type is this narrow scalar (tag as value), not a struct.
    /// Byte-up: Byte / UInt16 / UInt32 / UInt64 by arm count (D118 §6).
    pub sum_scalar: Option<Ty>,
    /// D155: product has `fn drop()` — values are heap boxes (header + payload).
    pub has_drop: bool,
    /// Its own `hash` / `equals`, not the derived ones (D228): a Map keyed
    /// by it calls them instead of hashing and comparing field by field.
    pub own_hash: bool,
    pub own_equals: bool,
}

#[derive(Debug, Clone)]
pub struct FieldLayout {
    pub name: String,
    pub c_name: String,
    pub ty: Ty,
}

/// One arm of a user union type (D118).
#[derive(Debug, Clone)]
pub struct SumVariantLayout {
    pub tag: u32,
    /// Source constructor / member name (e.g. `Up`, `Circle`).
    pub name: String,
    /// Payload type (`User` for nominal members; `Void` if empty product).
    pub payload_ty: Ty,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub is_main: bool,
    /// Optional implicit receiver for methods: (c_param_name, type).
    pub receiver: Option<(String, Ty)>,
    pub params: Vec<(String, Ty)>,
    /// Parallel to `params`: passed by address (`mut` parameter, D199).
    /// Empty when none is.
    pub mut_params: Vec<bool>,
    pub body: Vec<Stmt>,
    pub ret: Option<Place>,
    pub ret_ty: Ty,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Assign {
        place: Place,
        ty: Ty,
        value: RValue,
    },
    /// `base.field = value` (C-layout field store; truncates via Cast in value if needed).
    FieldAssign {
        base: Place,
        field: String,
        ty: Ty,
        value: Place,
    },
    /// `base.field[index] = value` (fixed-array element store). `len` is the
    /// array length; cgen checks the index against it at runtime.
    IndexAssign {
        base: Place,
        field: String,
        index: Place,
        ty: Ty,
        value: Place,
        len: u64,
    },
    /// `name[index] = value` (dynamic Array element store).
    DynIndexAssign {
        base: Place,
        index: Place,
        elem_ty: Ty,
        value: Place,
    },
    /// `name[index] = value` (first-class fixed Array<T,N> local — D117).
    FixedIndexAssign {
        base: Place,
        index: Place,
        ty: Ty,
        value: Place,
        len: u64,
    },
    Eval(RValue),
    If {
        cond: Place,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    /// Infinite loop: `while (1) { body }`. `break` / `continue` target this.
    Loop {
        body: Vec<Stmt>,
    },
    /// C-style counted range after `index` is initialized to the start value:
    /// `for (; index < end; index++)` or `<=` when inclusive.
    ForRange {
        index: Place,
        end: Place,
        inclusive: bool,
        body: Vec<Stmt>,
    },
    Break,
    Continue,
    /// Early / `?` return. `None` = bare `return` from a Void function.
    Return {
        value: Option<Place>,
    },
    /// Bump refcount of a heap object (or live Trial heap payload) at `place`.
    Dup {
        place: Place,
        ty: Ty,
    },
    /// Drop a heap object (or live Trial heap payload) at `place` (refcount--).
    Drop {
        place: Place,
        ty: Ty,
    },
    /// Stores a receiver back where it came from after a call that may have
    /// changed it in place. C runs `body` either way; it writes the
    /// enclosing function's receiver only when one of `callees` writes
    /// theirs (D163, D233).
    StoreBack {
        callees: Vec<String>,
        body: Vec<Stmt>,
    },
    /// `--> name(args)` — enqueue a task; do not wait for the result.
    Start {
        callee: Callee,
        args: Vec<Place>,
        arg_tys: Vec<Ty>,
        ret_ty: Ty,
    },
}

#[derive(Debug, Clone)]
pub enum RValue {
    Use(Place),
    Int(i64),
    /// IEEE-754 binary64 bits as a Thoxa `Float64` value.
    Float(f64),
    Bool(bool),
    /// Heap string from UTF-8 bytes (owned, refcount 1).
    StringLit(String),
    /// Null heap pointer (for uninit String slots).
    Null,
    Unary {
        op: UnaryOp,
        arg: Place,
    },
    Binary {
        op: BinaryOp,
        left: Place,
        right: Place,
    },
    Call {
        callee: Callee,
        args: Vec<Place>,
    },
    /// `&place`: an `Address<T>` to a `mut` C-layout struct local.
    AddressOf(Place),
    /// Construct a user type from field places (declaration order).
    StructLit {
        type_name: String,
        fields: Vec<Place>,
    },
    /// Zero-initialize a C-layout / user struct (`TypeName()`).
    StructZero {
        type_name: String,
    },
    /// `base.field`
    Field {
        base: Place,
        field: String,
    },
    /// `base.field[index]` — fixed-array element load (index checked against `len`).
    Index {
        base: Place,
        field: String,
        index: Place,
        len: u64,
    },
    /// Inject a member value into a union (D118).
    SumInject {
        sum_type: String,
        tag: u32,
        payload: Place,
        payload_ty: Ty,
    },
    /// Read tag from a union place (`sum_type` selects scalar vs struct layout).
    SumTag {
        place: Place,
        sum_type: String,
    },
    /// Extract payload for `tag` from a union place into a typed temporary.
    SumPayload {
        base: Place,
        tag: u32,
        payload_ty: Ty,
    },
    /// `name[index]` — first-class fixed Array<T,N> local element load (D117).
    FixedIndex {
        base: Place,
        index: Place,
        len: u64,
    },
    /// Fixed Array<T,N> local initializer from element places (D117).
    FixedArrayLit {
        elems: Vec<Place>,
        elem_ty: Ty,
        len: u64,
    },
    /// Dynamic Array literal from element places (homogeneous).
    ArrayLit {
        elems: Vec<Place>,
        elem_ty: Ty,
    },
    /// `Array<Int64>` literal of integer constants: one C array, not a
    /// temporary per element (the Unicode tables in std).
    IntArray(Vec<i64>),
    /// Typed dynamic-array element load (User: memcpy + field-dup into place).
    ArrayGet {
        arr: Place,
        index: Place,
        elem_ty: Ty,
    },
    /// Functional append of a typed element (User uses blob + field-dup).
    ArrayAppend {
        arr: Place,
        item: Place,
        elem_ty: Ty,
    },
    /// Functional dropLast (User field-dups surviving elements).
    ArrayDropLast {
        arr: Place,
        elem_ty: Ty,
    },
    /// Functional `insert` for User elements (blob + field-dup).
    ArrayInsertAt {
        arr: Place,
        at: Place,
        item: Place,
        elem_ty: Ty,
    },
    /// Functional removeAt for User elements (field-dup survivors).
    ArrayRemoveAt {
        arr: Place,
        at: Place,
        elem_ty: Ty,
    },
    /// Empty `Map()` at a typed slot.
    MapNew {
        key_ty: Ty,
        val_ty: Ty,
    },
    /// COW insert/overwrite; result is the unique map (write back onto the place).
    MapSet {
        map: Place,
        key: Place,
        val: Place,
        key_ty: Ty,
        val_ty: Ty,
    },
    /// COW remove (missing is a no-op).
    MapRemove {
        map: Place,
        key: Place,
        key_ty: Ty,
        val_ty: Ty,
    },
    /// Lookup: writes `out` when present; this rvalue is 1/0 found.
    MapGet {
        map: Place,
        key: Place,
        out: Place,
        val_ty: Ty,
    },
    /// Insertion-order key at `index` (i64 bits / pointer).
    MapKeyAt {
        map: Place,
        index: Place,
    },
    /// Insertion-order value at `index`.
    MapValAt {
        map: Place,
        index: Place,
        val_ty: Ty,
    },
    MapKeys {
        map: Place,
        key_ty: Ty,
    },
    MapValues {
        map: Place,
        val_ty: Ty,
    },
    /// Convert Int64/Bool/Array → owned String.
    ToString {
        value: Place,
        from: Ty,
    },
    /// Form a function value (D132): free / unbound / bound.
    MakeFn {
        recv: Option<Place>,
        recv_ty: Option<Ty>,
        target: FnTarget,
    },
    /// Non-consuming concat → owned String.
    StringConcat {
        left: Place,
        right: Place,
    },
    /// Numeric / trivial cast between compatible types.
    Cast {
        value: Place,
        from: Ty,
        to: Ty,
    },
    /// Implicit integer narrowing (D167): panics when `to` cannot hold the
    /// value. `T(truncating: x)` is a plain `Cast`.
    NarrowChecked {
        value: Place,
        from: Ty,
        to: Ty,
    },
}

#[derive(Debug, Clone)]
pub enum Callee {
    Builtin(String),
    User(String),
    /// Method: Type_method — C name built in cgen.
    Method {
        type_name: String,
        method: String,
    },
    /// Direct C symbol (libc). Args already coerced to declared types.
    Extern {
        c_symbol: String,
        /// Parameter types (for C marshaling of String/Bytes).
        arg_tys: Vec<Ty>,
        /// Return type (for C→Thoxa marshaling, e.g. `char*` → String).
        ret_ty: Ty,
    },
    /// Call through a function value (D132). `args` are the user arguments;
    /// cgen prepends `fn.recv`.
    Indirect {
        fun: Place,
        arg_tys: Vec<Ty>,
        ret_ty: Ty,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Void,
    /// Diverging control flow (`loop` with no `break`). Bottom type: unifies with
    /// any expected type so a bare infinite loop can be a function's last expression
    /// without a dummy `Pass(Void)` / `0` tail (D128).
    Never,
    Int32,
    Int64,
    UInt32,
    UInt64,
    /// `Byte` / `UInt8` — fixed 8-bit unsigned.
    Byte,
    /// IEEE-754 binary64 (`double`).
    Float64,
    Bool,
    /// Heap RC String (`ThoxaString*`).
    String,
    /// Heap RC Bytes (`ThoxaBytes*`) — fixed length, mutable contents.
    Bytes,
    /// Heap RC growable Buffer (`ThoxaBuffer*`) — COW on mutate when shared.
    Buffer,
    /// Heap RC HTML tree (`ThoxaHtml*`) — builtin `Node` (JSX).
    Node,
    /// Function pointer / bound method (D132). Unboxed `{recv, code}` pair.
    Fn {
        params: Vec<Ty>,
        ret: Box<Ty>,
    },
    /// Dynamic growable Array (`ThoxaArray*`) — heap RC; Swift/Buffer COW writes (D133).
    Array {
        elem: Box<Ty>,
    },
    /// Dynamic map (`ThoxaMap*`) — heap RC; Swift/Buffer COW writes (D134).
    Map {
        key: Box<Ty>,
        val: Box<Ty>,
    },
    /// Fixed inline array field type `Array<T, N>` (not a first-class value).
    FixedArray {
        elem: Box<Ty>,
        len: u64,
    },
    /// FFI pointer `Address<T>` (param types only in R7).
    Address(Box<Ty>),
    /// User product / union / monomorph (including std/fallible May/Trial).
    User(String),
}

/// User-facing display of a MIR type (for hover / diagnostics).
///
/// Monomorphs like `Fallible_Trial__Void__String` render as `Trial<Void, String>`.
pub fn display_ty(ty: &Ty) -> String {
    match ty {
        Ty::Void => "Void".into(),
        Ty::Never => "Never".into(),
        Ty::Int32 => "Int32".into(),
        Ty::Int64 => "Int64".into(),
        Ty::UInt32 => "UInt32".into(),
        Ty::UInt64 => "UInt64".into(),
        Ty::Byte => "Byte".into(),
        Ty::Float64 => "Float64".into(),
        Ty::Bool => "Bool".into(),
        Ty::String => "String".into(),
        Ty::Bytes => "Bytes".into(),
        Ty::Buffer => "Buffer".into(),
        Ty::Node => "Node".into(),
        Ty::Array { elem } => format!("Array<{}>", display_ty(elem)),
        Ty::Map { key, val } => format!("Map<{}, {}>", display_ty(key), display_ty(val)),
        Ty::FixedArray { elem, len } => format!("Array<{}, {len}>", display_ty(elem)),
        Ty::Address(inner) => format!("Address<{}>", display_ty(inner)),
        Ty::User(n) => display_user_ty(n),
        Ty::Fn { params, ret } => {
            let ps: Vec<String> = params.iter().map(display_ty).collect();
            format!("({}) -> {}", ps.join(", "), display_ty(ret))
        }
    }
}

/// What a function value points at (D132).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FnTarget {
    Free {
        mangled: String,
    },
    Method {
        type_name: String,
        method: String,
        bound: bool,
    },
}

fn display_user_ty(mangled: &str) -> String {
    // `mixed T`'s union, `Mixed_T` (D228 §6).
    if is_mixed(mangled) {
        return format!("mixed {}", display_type_base(mangled));
    }
    // D131 synthetic singleton literal types.
    if let Some(rest) = mangled.strip_prefix("$lit$") {
        return display_lit_key(rest);
    }
    // D131 / D119 synthetic member-set unions.
    if let Some(rest) = mangled.strip_prefix("$union$") {
        let parts: Vec<String> = rest.split('$').map(display_lit_key).collect();
        return parts.join(" | ");
    }
    // Mono: Base__Arg1__Arg2… (args may themselves contain `__` only if nested User
    // was flattened via ty_mono_suffix, which replaces `-`/`$` but keeps `__`).
    let parts: Vec<&str> = mangled.split("__").collect();
    let base = display_type_base(parts[0]);
    if parts.len() == 1 {
        return base;
    }
    let args: Vec<String> = parts[1..].iter().map(|a| display_mono_arg(a)).collect();
    format!("{base}<{}>", args.join(", "))
}

/// Display helper for synthetic lit/union keys (`liti_0`, `litb_true`, `lits_home`, `ty_…`).
fn display_lit_key(key: &str) -> String {
    if let Some(n) = key.strip_prefix("liti_") {
        return n.to_string();
    }
    if let Some(b) = key.strip_prefix("litb_") {
        return b.to_string();
    }
    if let Some(s) = key.strip_prefix("lits_") {
        return format!("\"{s}\"");
    }
    if let Some(t) = key.strip_prefix("ty_") {
        return display_type_base(t);
    }
    key.to_string()
}

fn display_type_base(mangled_base: &str) -> String {
    // A generic function's archetype for an associated type,
    // `Param_count_I_Item`, is `I.Item` (D228 §7).
    if let Some((_, rest)) = mangled_base.split_once("Param_") {
        if let Some((_, path)) = rest.split_once('_') {
            if path.contains('_') && path.split('_').all(|segment| segment.starts_with(|c: char| c.is_ascii_uppercase())) {
                return path.replace('_', ".");
            }
        }
    }
    // Entry types are bare names; non-entry are `{mod}_{Name}`. Prefer last
    // PascalCase segment.
    mangled_base
        .rsplit('_')
        .find(|s| {
            s.chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false)
        })
        .unwrap_or(mangled_base)
        .to_string()
}

fn display_mono_arg(arg: &str) -> String {
    match arg {
        "Void" | "Never" | "Int32" | "Int64" | "UInt32" | "UInt64" | "Byte" | "Float64"
        | "Bool" | "String" | "Bytes" | "Buffer" => arg.to_string(),
        a if a.starts_with("Array_") => {
            format!("Array<{}>", display_mono_arg(&a["Array_".len()..]))
        }
        a if a.starts_with("Fixed_") => {
            // Fixed_{elem}_{len}
            let rest = &a["Fixed_".len()..];
            if let Some((elem, len)) = rest.rsplit_once('_') {
                format!("Array<{}, {len}>", display_mono_arg(elem))
            } else {
                a.to_string()
            }
        }
        a if a.starts_with("Addr_") => {
            format!("Address<{}>", display_mono_arg(&a["Addr_".len()..]))
        }
        a if a.contains("__") => display_user_ty(a),
        a => display_type_base(a),
    }
}

/// Map free-function / method keys to return-type display strings (D129 hover).
///
/// Keys:
/// - free function: source/mangled entry name (`run`, `main`)
/// - method: `TypeName.method` using the demangled type base
pub fn function_return_displays(program: &Program) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for f in &program.functions {
        let ret = display_ty(&f.ret_ty);
        if let Some((_, recv_ty)) = &f.receiver {
            if let Ty::User(ty_name) = recv_ty {
                let method = f
                    .name
                    .strip_prefix(&format!("{ty_name}__"))
                    .unwrap_or(f.name.as_str());
                let owner = display_type_base(ty_name);
                out.insert(format!("{owner}.{method}"), ret);
            }
        } else {
            // Entry free fns: source name. Non-entry: `{mod}_{name}` → trailing camelCase.
            out.insert(free_fn_display_key(&f.name, f.is_main), ret);
        }
    }
    out
}

fn free_fn_display_key(mangled: &str, is_main: bool) -> String {
    if is_main || mangled == "main" {
        return "main".into();
    }
    // Entry free fns keep their source name (no module prefix).
    if !mangled.contains('_') {
        return mangled.to_string();
    }
    // Non-entry: `{modId}_{name}` — modId may contain `_`. Prefer longest camelCase suffix.
    if let Some((head, tail)) = mangled.rsplit_once('_') {
        if tail
            .chars()
            .next()
            .map(|c| c.is_ascii_lowercase())
            .unwrap_or(false)
        {
            let _ = head;
            return tail.to_string();
        }
    }
    mangled.to_string()
}

fn type_has_drop(types: &HashMap<String, TypeInfo>, ty: &Ty) -> bool {
    match ty {
        Ty::User(name) => types.get(name).is_some_and(|info| info.resource),
        _ => false,
    }
}

/// True when a local of this type needs an end-of-scope drop.
/// User types always participate (cgen walks fields; no-op when none are heap).
fn needs_drop(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::String
            | Ty::Bytes
            | Ty::Buffer
            | Ty::Node
            | Ty::Array { .. }
            | Ty::Map { .. }
            | Ty::User(_)
            | Ty::Fn { .. }
    )
}

/// True when evaluating `value` reads `place` (must not Drop it first).
fn rvalue_uses_place(value: &RValue, place: &Place) -> bool {
    let uses = |p: &Place| p == place;
    match value {
        RValue::Use(p) => uses(p),
        RValue::Int(_)
        | RValue::Float(_)
        | RValue::Bool(_)
        | RValue::StringLit(_)
        | RValue::Null
        | RValue::StructZero { .. }
        | RValue::MapNew { .. } => false,
        RValue::Unary { arg, .. }
        | RValue::ToString { value: arg, .. }
        | RValue::Cast { value: arg, .. }
        | RValue::NarrowChecked { value: arg, .. }
        | RValue::AddressOf(arg) => uses(arg),
        RValue::Binary { left, right, .. } | RValue::StringConcat { left, right } => {
            uses(left) || uses(right)
        }
        RValue::Call { callee, args } => {
            args.iter().any(uses) || matches!(callee, Callee::Indirect { fun, .. } if uses(fun))
        }
        RValue::StructLit { fields, .. }
        | RValue::FixedArrayLit { elems: fields, .. }
        | RValue::ArrayLit { elems: fields, .. } => fields.iter().any(uses),
        RValue::IntArray(_) => false,
        RValue::Field { base, .. }
        | RValue::SumTag { place: base, .. }
        | RValue::SumPayload { base, .. } => uses(base),
        RValue::Index { base, index, .. }
        | RValue::FixedIndex { base, index, .. }
        | RValue::ArrayGet {
            arr: base, index, ..
        } => uses(base) || uses(index),
        RValue::SumInject { payload, .. } => uses(payload),
        RValue::ArrayAppend { arr, item, .. } => uses(arr) || uses(item),
        RValue::ArrayDropLast { arr, .. } => uses(arr),
        RValue::ArrayInsertAt { arr, at, item, .. } => uses(arr) || uses(at) || uses(item),
        RValue::ArrayRemoveAt { arr, at, .. } => uses(arr) || uses(at),
        RValue::MapSet { map, key, val, .. } => uses(map) || uses(key) || uses(val),
        RValue::MapRemove { map, key, .. } => uses(map) || uses(key),
        RValue::MapGet { map, key, out, .. } => uses(map) || uses(key) || uses(out),
        RValue::MapKeyAt { map, index } | RValue::MapValAt { map, index, .. } => {
            uses(map) || uses(index)
        }
        RValue::MapKeys { map, .. } | RValue::MapValues { map, .. } => uses(map),
        RValue::MakeFn { recv, .. } => recv.as_ref().is_some_and(uses),
    }
}

/// True when `ty` is the diverging bottom type (no value produced).
fn is_never(ty: &Ty) -> bool {
    matches!(ty, Ty::Never)
}

/// Element types supported in dynamic `Array<T>` (phase A).
/// User is allowed when fields are constructor-param scalars/heap (checked at resolve).
fn is_array_elem_ty(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Int64
            | Ty::Float64
            | Ty::Bool
            | Ty::String
            | Ty::Bytes
            | Ty::Node
            | Ty::User(_)
            | Ty::Array { .. }
            | Ty::Map { .. }
    )
}

fn array_elem_is_heap(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::String | Ty::Bytes | Ty::Node | Ty::Array { .. } | Ty::Map { .. }
    )
}

/// String, Int64, Float64, Bool, or a user type (D179, D214; what a user
/// type holds is checked where a map is made, see `map_key_problem`).
fn is_map_key_ty(ty: &Ty) -> bool {
    matches!(ty, Ty::String | Ty::Int64 | Ty::Float64 | Ty::Bool | Ty::User(_))
}

/// D179: why values of `ty` cannot be (part of) a Map key, or `None`. A key
/// hashes and compares by value, so it may hold integers, Float64, Bool,
/// String, Bytes, and arrays, products, and unions of those (D214).
fn map_key_problem(ctx: &LowerCtx, ty: &Ty) -> Option<String> {
    let types = &ctx.types;
    if is_int(ty) || matches!(ty, Ty::Bool | Ty::Float64 | Ty::String | Ty::Bytes | Ty::Void) {
        return None;
    }
    // An array hashes its count and elements in order (D214).
    if let Ty::Array { elem } = ty {
        return map_key_problem(ctx, elem);
    }
    let Ty::User(name) = ty else {
        return Some(format!("it holds {}", display_ty(ty)));
    };
    let shown = display_ty(ty);
    let Some(info) = types.get(name) else {
        return Some(format!("`{shown}` is not a known type"));
    };
    if info.resource {
        return Some(format!("`{shown}` is a `Resource`"));
    }
    // A key's type says it hashes (D228).
    if !ctx.conforms(ty, "Hashable") {
        return Some(format!("`{shown}` is not `Hashable`"));
    }
    if info.zero_init {
        return Some(format!("`{shown}` is a C-layout type"));
    }
    if let Some(sum) = &info.sum {
        return sum
            .variants
            .iter()
            .filter(|variant| variant.lit.is_none())
            .find_map(|variant| map_key_problem(ctx, &variant.ty));
    }
    info.fields
        .iter()
        .find_map(|field| map_key_problem(ctx, &field.ty))
}

/// Refuse a map whose key type cannot be hashed (D179).
fn check_map_key(ctx: &LowerCtx, key_ty: &Ty, span: Span) -> Result<()> {
    match map_key_problem(ctx, key_ty) {
        None => Ok(()),
        Some(problem) => Err(Error::at(
            format!("cannot use {} as a Map key: {problem}", display_ty(key_ty)),
            span,
        )
        .with_help(
            "a key hashes and compares by value: integers, Float64, Bool, String, \
             Bytes, and arrays, products, and unions of those (no Map or Node)",
        )),
    }
}

fn is_map_val_ty(ty: &Ty) -> bool {
    is_array_elem_ty(ty)
}

fn array_elem_is_user(ty: &Ty) -> bool {
    matches!(ty, Ty::User(_))
}

/// Constructor-param User types: scalar/heap fields, plus Array/Map/Node/User.
fn is_array_user_elem(ty: &Ty, types: &HashMap<String, TypeInfo>) -> bool {
    match ty {
        Ty::User(name) => {
            let Some(info) = types.get(name) else {
                return false;
            };
            // C-layout (zero_init) types stay out of Array for now.
            if info.zero_init {
                return false;
            }
            info.fields.iter().all(|f| is_ctor_field_ty(&f.ty, types))
        }
        _ => true,
    }
}

/// D48 / D135: constructor-param fields (not C-layout body fields, not fn types).
fn is_ctor_field_ty(ty: &Ty, types: &HashMap<String, TypeInfo>) -> bool {
    match ty {
        Ty::Float64 | Ty::Bool | Ty::String | Ty::Bytes | Ty::Node => true,
        ty if is_int(ty) => true,
        Ty::Array { .. } | Ty::Map { .. } => true,
        Ty::User(name) => types.get(name).is_some_and(|info| !info.zero_init),
        _ => false,
    }
}

fn is_int(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Int32 | Ty::Int64 | Ty::UInt32 | Ty::UInt64 | Ty::Byte
    )
}

fn is_float(ty: &Ty) -> bool {
    matches!(ty, Ty::Float64)
}

/// Int widths among themselves, or Int → Float64 (never the reverse).
fn can_numeric_cast(from: &Ty, to: &Ty) -> bool {
    (is_int(from) && is_int(to)) || (is_int(from) && is_float(to))
}

fn is_abi_scalar(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Int32 | Ty::Int64 | Ty::UInt32 | Ty::UInt64 | Ty::Byte | Ty::Float64
    )
}

fn is_abi_field_ty(ty: &Ty) -> bool {
    match ty {
        t if is_abi_scalar(t) => true,
        Ty::FixedArray { elem, .. } => is_abi_scalar(elem),
        _ => false,
    }
}

fn is_ffi_ty(ty: &Ty) -> bool {
    match ty {
        Ty::Int32
        | Ty::Int64
        | Ty::UInt32
        | Ty::UInt64
        | Ty::Byte
        | Ty::Float64
        | Ty::String
        | Ty::Bytes
        | Ty::Void => true,
        Ty::Address(inner) => matches!(inner.as_ref(), Ty::User(_)),
        _ => false,
    }
}

#[derive(Clone)]
struct Binding {
    ty: Ty,
    mutable: bool,
    /// C place name (local `v_x` or module global `g_Mod_x`).
    place: Place,
    /// Method field write-back (D107): reassignment also stores through the
    /// receiver (`_self.field = …`). `field` is the sanitized C field name.
    writeback: Option<(Place, String)>,
}

#[derive(Clone)]
struct FnSig {
    params: Vec<(String, Ty)>,
    /// Parallel to `params`: default expression when the call omits this argument.
    defaults: Vec<Option<Expr>>,
    ret: Ty,
    /// If set, this name is an `extern fn` (call C, no Thoxa body).
    extern_symbol: Option<String>,
    /// Mangled MIR/C function name (may differ from source name for non-entry modules).
    mangled: String,
    /// Module where the function is defined — default exprs resolve here (D116).
    def_mod: String,
    /// D131 multi-clause merge — cannot take as a value (D132).
    multi_clause: bool,
    /// Parallel to `params`: a `mut` parameter, passed by address (D199).
    /// Empty when none is.
    mutable: Vec<bool>,
    /// Parallel to `params`: marked `!`, so passed by position; the rest
    /// are passed by label (D200). Empty when unknown (not checked).
    positional: Vec<bool>,
}

#[derive(Clone)]
struct MethodSig {
    /// Method params (not including receiver).
    params: Vec<(String, Ty)>,
    /// Parallel to `params`: default expression when the call omits this argument (D98/D106).
    defaults: Vec<Option<Expr>>,
    ret: Ty,
    exported: bool,
    /// Module of the enclosing type — default exprs resolve here (D116).
    def_mod: String,
    /// D131 multi-clause merge — cannot take as a value (D132).
    multi_clause: bool,
    /// Parallel to `params`: a `mut` parameter, passed by address (D199).
    mutable: Vec<bool>,
    /// Parallel to `params`: marked `!` (D200).
    positional: Vec<bool>,
    /// A generic type's instance has this method only when each of these
    /// type arguments (the parameter, its argument) meets its bound: the
    /// conformance it comes from holds per instance (D228).
    requires: Vec<(String, Ty, TraitRef)>,
}

/// One clause of a multi-clause function/method (D131).
#[derive(Clone)]
struct MultiClause {
    /// Formal names (source order) for this clause's body.
    param_names: Vec<String>,
    /// Formal types for this clause (refinement under the arm).
    param_tys: Vec<Ty>,
    /// Positional `!` markers (must match across clauses).
    positional: Vec<bool>,
    /// Optional per-clause `-> T` local expect.
    return_type: Option<TypeExpr>,
    body: Block,
    name_span: Span,
    exported: bool,
}

#[derive(Clone)]
struct FieldInfo {
    name: String,
    ty: Ty,
    /// `mut` field (required for field assign / Address coercion of whole struct uses place mut).
    mutable: bool,
    /// Constructor-param default (`type T(x: Int64 = 0)`); ignored for body fields.
    default: Option<Expr>,
    /// Constructor param marked `!`: passed by position, not by label (D200).
    positional: bool,
    /// Field-level `where` (validation experiment).
    #[allow(dead_code)]
    where_clause: Option<crate::ast::WhereClause>,
}

/// Per-module free-name resolution maps (for default-arg scope — D116).
#[derive(Clone, Default)]
struct ModuleNameMaps {
    name_to_type: HashMap<String, String>,
    name_to_fn: HashMap<String, String>,
    name_to_global: HashMap<String, GlobalInfo>,
    namespaces: HashMap<String, String>,
}

#[derive(Clone)]
struct TypeInfo {
    fields: Vec<FieldInfo>,
    methods: HashMap<String, MethodSig>,
    /// True when constructed with `TypeName()` zero-init (body fields, R7).
    zero_init: bool,
    /// Defining module — ctor field defaults resolve here (D116).
    def_mod: String,
    /// D118: union alias arms (ordered). `None` for ordinary product / C-layout types.
    sum: Option<SumInfo>,
    /// `type Qty = Int64 where …` — one-field wrapper (`value`) over `base`.
    refined: Option<RefinedInfo>,
    /// Conforms to `Resource` (D228): a shared heap box whose `drop` runs
    /// with its last copy (D155).
    resource: bool,
    /// Traits the type declares it conforms to (D228). An instance of a
    /// generic type conforms through its template's conditional
    /// conformances instead (`instance`).
    traits: Vec<TraitRef>,
    /// Methods whose bodies are the compiler's field-by-field defaults.
    derived: HashSet<String>,
    /// An instance of a generic type: its template's mangled name and its
    /// type arguments, in order.
    instance: Option<(String, Vec<Ty>)>,
    /// Its methods with their own type parameters (D228 §5), by name.
    templates: HashMap<String, MethodTemplate>,
}

/// A method with its own type parameters (D228 §5): a call infers them and
/// makes the type's method for those arguments (`map_String`), lowered as
/// any method is.
#[derive(Clone)]
struct MethodTemplate {
    /// For a generic type's instance, with the instance's arguments in.
    method: Method,
    /// Where its body resolves.
    def_mod: String,
}

#[derive(Clone)]
struct RefinedInfo {
    base: Ty,
    clause: crate::ast::WhereClause,
}

/// Resolved union alias (D118).
#[derive(Clone)]
struct SumInfo {
    variants: Vec<SumVariant>,
    /// Tag of residual arm (`?` mark), if any.
    residual_tag: Option<u32>,
    /// Tag of success-extract arm (`!` mark), if any.
    extract_tag: Option<u32>,
}

/// Generic type template (`type Box<T>(…)` / `type Opt<T> = …`).
#[derive(Clone)]
struct GenericDef {
    source_name: String,
    mangled_base: String,
    type_params: Vec<String>,
    def_mod: String,
    kind: GenericKind,
    name_span: Span,
    /// Each type parameter's bounds, which an instance's arguments meet
    /// (D228 §5).
    bounds: Vec<Vec<TraitRef>>,
    /// Its defining module's type names, which its fields and members are
    /// written with, wherever an instance is made (`May`'s `Has`).
    name_map: HashMap<String, String>,
    /// Its conformances' associated types, written with its parameters
    /// (`type Cursor = StackCursor<T>`), and their bounds (D228 §7).
    assoc: Vec<(String, TypeExpr, Vec<TraitRef>)>,
    /// Its checking instances' arguments, archetypes, each with the bounds a
    /// conformance adds for the members it checks (D228 §5), and the
    /// archetypes of its parameters' associated types (D237).
    checking: Vec<(Vec<Ty>, Vec<Vec<TraitRef>>, HashMap<String, Ty>)>,
    /// Its parameters' associated types, `S.Item`, read off each instance's
    /// arguments in order, with what they must meet (D237).
    derived: Vec<ast::DerivedParam>,
}

#[derive(Clone)]
enum GenericKind {
    Product {
        params: Vec<ast::Param>,
        methods: Vec<Method>,
        /// `from` clauses, instantiated per instance (D217).
        conversions: Vec<ast::Conversion>,
    },
    Union {
        members: Vec<TypeExpr>,
        /// Its conformances' members (D238).
        methods: Vec<Method>,
    },
}

/// Monomorphized method to lower after type mono materialization.
#[derive(Clone)]
struct MonoMethodJob {
    mono_type: String,
    method: Method,
    subst: HashMap<String, Ty>,
    def_mod: String,
    /// A `from` clause of the instance (D217): `method` is the free function
    /// `__conversion_{instance}_N`, not a method.
    conversion: bool,
}

#[derive(Clone)]
struct SumVariant {
    tag: u32,
    /// Source name for patterns and diagnostics (`Up`, `0`, `"x"`).
    name: String,
    /// Member type (`User` for nominal; `Void` for pure literal arms).
    ty: Ty,
    /// Literal arm (`0 | 1`) — D118 pure-literal / mixed unions.
    lit: Option<TypeLit>,
    /// Type-level mark on this arm (`!` / `?`); tags also stored on SumInfo.
    #[allow(dead_code)]
    mark: Option<TypeMark>,
    /// Type-arg extract mark (`Hold<Int64!>`): success `?` unwraps the sole field (D118).
    /// Whole-arm mark (`Hold!`) sets extract_tag but leaves this false.
    unwrap_payload: bool,
}

/// Module-level value (let/mut).
#[derive(Clone)]
struct GlobalInfo {
    ty: Ty,
    mutable: bool,
    place: Place,
}

struct LowerCtx {
    stmts: Vec<Stmt>,
    temps: u32,
    env: Vec<HashMap<String, Binding>>,
    /// C local names already bound in this function. A second binding of the
    /// same source name (shadowing, or a sibling scope) gets a suffixed C name
    /// so it cannot overwrite or retype the first (see `local_place`).
    bound_locals: HashSet<String>,
    fns: HashMap<String, FnSig>,
    types: HashMap<String, TypeInfo>,
    /// Mangled type name → info (same as types; key is always mangled).
    /// Local/import name resolution uses `name_to_type`.
    name_to_type: HashMap<String, String>,
    /// Local name → mangled fn key in `fns`.
    name_to_fn: HashMap<String, String>,
    /// Local name → global (module-level let/mut in this module or imported let).
    name_to_global: HashMap<String, GlobalInfo>,
    /// Namespace binding → module id.
    namespaces: HashMap<String, String>,
    /// module_id → (export name → mangled type / fn / global key)
    module_exports: HashMap<String, ModuleExports>,
    /// Per-module name maps for default-arg resolution in the defining module (D116).
    package_maps: HashMap<String, ModuleNameMaps>,
    fn_ret: Ty,
    /// Current module id being lowered.
    module_id: String,
    is_entry: bool,
    /// Nesting depth of `loop` / `while` / `for` (for `break` / `continue`).
    loop_depth: u32,
    /// Heap / Trial-with-heap places from `Assign` so far (for drops on early `return` / `?`).
    /// Same policy as end-of-fn `collect_drop_places` — **not** params (caller-owned).
    drop_places: Vec<(String, Ty)>,
    /// Shared synthetic union types for unannotated if lub (D119).
    synthetics: Rc<RefCell<SyntheticRegistry>>,
    /// D129: body probe collecting exit types (discard MIR; no packaging checks).
    inferring: bool,
    /// Mangled name of the function being inferred (self-recursion detect).
    infer_self: Option<String>,
    /// Other unannotated functions still awaiting inference (cycle detect).
    /// Methods are keyed by their C name, `Type__method` (D174).
    infer_blocked: HashSet<String>,
    /// The first function awaiting inference that this probe reached, shared
    /// with nested probes. Set means the probe waits for a later round.
    infer_waited: Rc<RefCell<Option<String>>>,
    /// Residual `?` exits collapsed to success-Void form.
    infer_residuals: Vec<Ty>,
    /// Normal `return` / block-tail value exits (not Never).
    infer_values: Vec<Ty>,
    /// Payload types of bare `Pass(x)` / `Has(x)` typed from a `?` residual:
    /// they refine its success side (D129 §4).
    infer_successes: Vec<Ty>,
    /// Generic templates for residual collapse (D129).
    generics: HashMap<String, GenericDef>,
    /// Experiment: body of a desugared route clause (`__Type_route_N`).
    route_clause: bool,
    /// Method fields the call being lowered takes `mut` (D206), set by
    /// `resolve_call_args_full`: `emit_call_result` stores each back through
    /// `_self` after the call.
    mut_field_args: Vec<Binding>,
    /// The type whose method body is being lowered: a bare call to one of its
    /// methods is a call on the receiver, and its private methods are callable.
    method_type: Option<String>,
    /// The method's fields by name, as bound at its start: `self.name` reaches
    /// one even when a local shadows it (D228).
    method_fields: HashMap<String, Binding>,
    /// The method whose body is being lowered (`get_name` in a getter): its
    /// own property's stored field is `field` there (D228).
    current_method: Option<String>,
    /// A body a trait provides (D228): only the trait's members are visible
    /// by bare name or through `self`, not the conforming type's fields or
    /// its own methods.
    trait_scope: Option<HashSet<String>>,
    /// Types whose `==` is being expanded inline; one met again calls its
    /// generated function instead (D212).
    eq_expanding: Vec<String>,
}

#[derive(Clone, Default)]
struct ModuleExports {
    types: HashMap<String, String>, // source name → mangled type
    fns: HashMap<String, String>,   // source name → mangled fn
    /// source name → global info (place + type)
    globals: HashMap<String, GlobalInfo>,
}

impl LowerCtx {
    fn new(
        fns: HashMap<String, FnSig>,
        types: HashMap<String, TypeInfo>,
        name_to_type: HashMap<String, String>,
        name_to_fn: HashMap<String, String>,
        name_to_global: HashMap<String, GlobalInfo>,
        namespaces: HashMap<String, String>,
        module_exports: HashMap<String, ModuleExports>,
        package_maps: HashMap<String, ModuleNameMaps>,
        fn_ret: Ty,
        module_id: String,
        is_entry: bool,
        synthetics: Rc<RefCell<SyntheticRegistry>>,
    ) -> Self {
        let mut types = types;
        // Seed with synthetics created by earlier functions in this package.
        seed_synthetics(&mut types, &synthetics.borrow());
        Self {
            stmts: Vec::new(),
            temps: 0,
            env: vec![HashMap::new()],
            bound_locals: HashSet::new(),
            fns,
            types,
            name_to_type,
            name_to_fn,
            name_to_global,
            namespaces,
            module_exports,
            package_maps,
            fn_ret,
            module_id,
            is_entry,
            loop_depth: 0,
            drop_places: Vec::new(),
            synthetics,
            inferring: false,
            infer_self: None,
            infer_blocked: HashSet::new(),
            infer_waited: Rc::new(RefCell::new(None)),
            infer_residuals: Vec::new(),
            infer_values: Vec::new(),
            infer_successes: Vec::new(),
            generics: HashMap::new(),
            route_clause: false,
            mut_field_args: Vec::new(),
            method_type: None,
            method_fields: HashMap::new(),
            current_method: None,
            trait_scope: None,
            eq_expanding: Vec::new(),
        }
    }

    /// D129 / D174: while probing for a return type, a call to `key` (a free
    /// function's mangled name, or `Type__method`) whose own return type is
    /// not known yet cannot be typed. The probe stops and waits.
    fn infer_check_callee(&self, key: &str, shown: &str, span: Span) -> Result<()> {
        if !self.inferring {
            return Ok(());
        }
        if self.infer_self.as_deref() != Some(key) && !self.infer_blocked.contains(key) {
            return Ok(());
        }
        self.infer_waited
            .borrow_mut()
            .get_or_insert_with(|| key.to_string());
        Err(Error::at(
            format!(
                "cannot infer return type of recursive call to `{shown}`; \
                 add an explicit `->` annotation"
            ),
            span,
        ))
    }

    fn type_mismatch_msg(&self, expect: &Ty, found: &Ty) -> String {
        if self.route_clause {
            if matches!(expect, Ty::String) && display_ty(found) == "Response" {
                return "route clause returns Response; annotate `-> Response` \
                        to pass it through"
                    .into();
            }
            format!(
                "route clause: expected String or Response, found {}",
                display_ty(found)
            )
        } else {
            format!("expected {expect:?}, found {found:?}")
        }
    }

    /// Run `f` with free-name maps from `def_mod` (default-arg defining scope — D116).
    fn with_def_mod_maps<R>(
        &mut self,
        def_mod: &str,
        f: impl FnOnce(&mut LowerCtx) -> Result<R>,
    ) -> Result<R> {
        if def_mod == self.module_id || def_mod.is_empty() {
            return f(self);
        }
        let Some(maps) = self.package_maps.get(def_mod).cloned() else {
            return f(self);
        };
        let saved_ty = std::mem::replace(&mut self.name_to_type, maps.name_to_type);
        let saved_fn = std::mem::replace(&mut self.name_to_fn, maps.name_to_fn);
        let saved_g = std::mem::replace(&mut self.name_to_global, maps.name_to_global);
        let saved_ns = std::mem::replace(&mut self.namespaces, maps.namespaces);
        let result = f(self);
        self.name_to_type = saved_ty;
        self.name_to_fn = saved_fn;
        self.name_to_global = saved_g;
        self.namespaces = saved_ns;
        result
    }

    fn push_scope(&mut self) {
        self.env.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.env.pop();
    }

    fn define(&mut self, name: &str, ty: Ty, mutable: bool, place: Place) {
        self.define_full(name, ty, mutable, place, None);
    }

    fn define_full(
        &mut self,
        name: &str,
        ty: Ty,
        mutable: bool,
        place: Place,
        writeback: Option<(Place, String)>,
    ) {
        self.bound_locals.insert(place.name.clone());
        self.env.last_mut().unwrap().insert(
            name.to_string(),
            Binding {
                ty,
                mutable,
                place,
                writeback,
            },
        );
    }

    fn lookup(&self, name: &str) -> Option<Binding> {
        for scope in self.env.iter().rev() {
            if let Some(b) = scope.get(name) {
                return Some(b.clone());
            }
        }
        // Module-level globals (this module or selective imports).
        if let Some(g) = self.name_to_global.get(name) {
            return Some(Binding {
                ty: g.ty.clone(),
                mutable: g.mutable,
                place: g.place.clone(),
                writeback: None,
            });
        }
        None
    }

    fn resolve_type_name(&self, name: &str) -> Option<String> {
        if name == "Attr" {
            return Some("Attr".into());
        }
        if let Some(mangled) = name.strip_prefix(ast::RESOLVED_TYPE) {
            return self.types.contains_key(mangled).then(|| mangled.to_string());
        }
        if let Some(mangled) = self.name_to_type.get(name) {
            return Some(mangled.clone());
        }
        // Generated codecs name std types by their mangled name (D182); a
        // name a program writes never contains `_` beside other characters.
        (name.len() > 1 && name.contains('_') && self.types.contains_key(name))
            .then(|| name.to_string())
    }

    fn fresh(&mut self, prefix: &str) -> Place {
        let id = self.temps;
        self.temps += 1;
        Place {
            name: format!("_{prefix}{id}"),
        }
    }

    fn assign(&mut self, place: Place, ty: Ty, value: RValue) {
        if let RValue::Use(ref p) = value {
            if p == &place {
                return;
            }
            // Copy of a heap value or Trial-with-heap: bump so source and dest both own.
            if needs_drop(&ty) {
                self.stmts.push(Stmt::Dup {
                    place: p.clone(),
                    ty: ty.clone(),
                });
            }
        }
        // Loop / re-bind: the C local is reused. Drop the previous occupant
        // first (NULL-init makes the first Drop a no-op). Skip when the RHS
        // still reads `place` (COW MapSet / Array append write-back).
        if needs_drop(&ty) && !rvalue_uses_place(&value, &place) {
            self.stmts.push(Stmt::Drop {
                place: place.clone(),
                ty: ty.clone(),
            });
        }
        self.note_drop_place(&place, &ty);
        self.stmts.push(Stmt::Assign { place, ty, value });
    }

    /// Assign without registering a drop (borrow / intermediate sum payload extract).
    fn assign_no_drop(&mut self, place: Place, ty: Ty, value: RValue) {
        self.stmts.push(Stmt::Assign { place, ty, value });
    }

    /// Record a place that owns a heap / Trial-with-heap value (for early-return drops).
    /// Module globals (`g_…`) are never end-of-fn dropped — they outlive the function.
    /// Registering them here double-frees after `mut g = new_array` (kilo textRows crash).
    fn note_drop_place(&mut self, place: &Place, ty: &Ty) {
        if !needs_drop(ty) {
            return;
        }
        if self.is_global_place(place) {
            return;
        }
        // A `mut` parameter's slot belongs to the caller (D199).
        if place.name.starts_with("(*") {
            return;
        }
        self.drop_places.push((place.name.clone(), ty.clone()));
    }

    /// Type of `place` when it owns a value dropped at function exit.
    fn drop_place_ty(&self, place: &Place) -> Option<Ty> {
        self.drop_places
            .iter()
            .find(|(name, _)| name == &place.name)
            .map(|(_, ty)| ty.clone())
    }

    fn is_global_place(&self, place: &Place) -> bool {
        // Module-mangled globals are `g_<Module>_<name>` (see `mangle_global`).
        place.name.starts_with("g_")
            || self
                .name_to_global
                .values()
                .any(|g| g.place.name == place.name)
    }

    /// C place for a new binding of `name`: `v_<name>` the first time in this
    /// function, then `v_<name>_2`, `v_<name>_3`, … Source names cannot contain
    /// `_`, so the suffixed names never collide with another binding.
    fn local_place(&mut self, name: &str) -> Place {
        let base = sanitize_name(name);
        let mut candidate = base.clone();
        let mut suffix = 2;
        while self.bound_locals.contains(&candidate) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        self.bound_locals.insert(candidate.clone());
        Place { name: candidate }
    }

    /// Bind a name to an already-lowered value (fresh ownership from `place`).
    fn bind_name(&mut self, name: &str, ty: Ty, mutable: bool, place: Place) {
        let bound = self.local_place(name);
        self.assign(bound.clone(), ty.clone(), RValue::Use(place));
        self.define(name, ty, mutable, bound);
    }

    /// Reassign a mut binding; drops the old heap value if any.
    /// Method field locals (D107) also FieldAssign through the receiver.
    fn reassign_name(&mut self, name: &str, name_span: Span, value: &Expr) -> Result<()> {
        let found = self.lookup(name);
        // A property with a setter writes through it (D228).
        if let Some((type_name, setter)) =
            property_accessor(self, name, AccessorKind::Set, found.as_ref())
        {
            let args = [Arg::Pos(value.clone())];
            lower_sibling_call(self, &type_name, &setter, &args, name_span)?;
            return Ok(());
        }
        if found.is_none() {
            if let Some(error) = own_accessor_error(self, name, name_span) {
                return Err(error);
            }
        }
        let binding =
            found.ok_or_else(|| Error::at(format!("undefined name `{name}`"), name_span))?;
        if !binding.mutable {
            return Err(Error::at(
                format!("cannot assign to immutable binding `{name}`"),
                name_span,
            ));
        }
        self.reassign_binding(binding, name, value)
    }

    /// Store `value` in a mutable binding (and through the receiver, for a
    /// method's field).
    fn reassign_binding(&mut self, binding: Binding, name: &str, value: &Expr) -> Result<()> {
        let expect = binding.ty.clone();
        let (place, ty) = lower_expr_expect(self, value, Some(&expect))?;
        // Converts as the binding's annotation would (D192).
        let Some(place) = coerce_to_expected(self, place, &ty, &expect, value)? else {
            return Err(Error::at(
                format!("cannot reassign `{name}`: expected {expect:?}, found {ty:?}"),
                value.span,
            ));
        };
        let ty = expect.clone();
        let bound = binding.place.clone();
        // `assign` Drops the old heap occupant, then Dups the RHS temp so both own.
        // FixedArray: cgen copies element-wise on `Use` (C arrays are not assignable).
        self.assign(bound.clone(), ty.clone(), RValue::Use(place));
        // Write through receiver for method field bindings.
        if let Some((base, field)) = binding.writeback {
            self.stmts.push(Stmt::FieldAssign {
                base,
                field,
                ty: binding.ty.clone(),
                value: bound,
            });
        }
        Ok(())
    }
}

/// [`in_module_file`] for the module with this id.
fn in_module_id(error: Error, package: &Package, module_id: &str) -> Error {
    match package.modules.iter().find(|module| module.id == module_id) {
        Some(module) => in_module_file(error, &module.path),
        None => error,
    }
}

/// A body-lowering error's span is an offset into the module being lowered:
/// record that file so the CLI and LSP show the right file and line.
fn in_module_file(mut error: Error, path: &std::path::Path) -> Error {
    if error.path.is_none() {
        error.path = Some(path.to_path_buf());
    }
    for more in &mut error.more {
        if more.path.is_none() {
            more.path = Some(path.to_path_buf());
        }
    }
    error
}

/// D107 write-back for in-place collection writes (`items.append(x)`,
/// `items[i] = x`, `tags[k] = v`) on a method's field local: store the
/// updated local through the receiver, as `reassign_name` does for `=`.
fn write_back_field(ctx: &mut LowerCtx, binding: &Binding) {
    if let Some((base, field)) = &binding.writeback {
        ctx.stmts.push(Stmt::FieldAssign {
            base: base.clone(),
            field: field.clone(),
            ty: binding.ty.clone(),
            value: binding.place.clone(),
        });
    }
}

/// Options for MIR lowering.
#[derive(Debug, Clone, Copy)]
pub struct LowerOptions {
    /// When true (build/run default), the package must define `fn main`.
    /// When false (LSP / `thoxa check` on library modules), skip that gate.
    pub require_main: bool,
}

impl Default for LowerOptions {
    fn default() -> Self {
        Self { require_main: true }
    }
}

/// Lower a single-file module (wrapper used by tests / single-file programs).
pub fn lower_module(module: &ast::Module) -> Result<Program> {
    lower_module_with(module, LowerOptions::default())
}

/// Lower a single-file module with explicit options (e.g. library check, no main).
pub fn lower_module_with(module: &ast::Module, opts: LowerOptions) -> Result<Program> {
    let is_entry = opts.require_main;
    let mut pkg = Package {
        project_root: PathBuf::from("."),
        modules: vec![SourceModule {
            id: "Main".into(),
            path: PathBuf::from("main.thx"),
            ast: module.clone(),
            is_entry,
        }],
    };
    crate::loader::include_core_modules(&mut pkg)?;
    lower_package_with(&pkg, opts)
}

/// Lower a multi-file package into one MIR program (requires `fn main`).
pub fn lower_package(package: &Package) -> Result<Program> {
    lower_package_with(package, LowerOptions::default())
}

/// Lower a multi-file package with explicit options.
pub fn lower_package_with(package: &Package, opts: LowerOptions) -> Result<Program> {
    lower_package_full(package, opts)
}

fn lower_package_full(package: &Package, opts: LowerOptions) -> Result<Program> {
    let lower_started = std::time::Instant::now();
    // D150: type-body `GET "/…"` → handlers + `route` (names Request/Response/ok).
    let mut package = package.clone();
    // `extend` first: a generic `extend` block's methods become generic
    // functions, which the trait pass resolves (D228 §5).
    crate::extend_desugar::desugar_package(&mut package)?;
    let trait_output = crate::trait_desugar::desugar_package(&mut package)?;
    let built_in_conformances = trait_output.built_in;
    crate::conversion_desugar::desugar_package(&mut package)?;
    crate::route_desugar::desugar_package(&mut package)?;
    crate::validate_desugar::desugar_package(&mut package)?;
    let package = &package;

    // May/Trial live in `std/fallible` and must be imported (no ambient inject).
    let import_maps = loader::build_import_maps(package)?;

    // ---------- Phase 1: register types (mangled names) ----------
    let mut types_info: HashMap<String, TypeInfo> = HashMap::new();
    let mut type_layouts: Vec<TypeLayout> = Vec::new();
    seed_html_types(&mut types_info, &mut type_layouts);
    // (module_id, mangled_type_name, &TypeDef) — concrete (non-generic) only
    let mut type_defs: Vec<(String, String, &TypeDef)> = Vec::new();
    // Generic templates: mangled base name → def (D118 / May prelude path).
    let mut generics: HashMap<String, GenericDef> = HashMap::new();
    // Conformances that hold per instance (D228): a generic type's, by its
    // mangled name, and the built-in types', by theirs.
    let mut conditional: HashMap<String, Vec<ast::ConditionalConformance>> = built_in_conformances;
    let mut module_exports: HashMap<String, ModuleExports> = HashMap::new();

    for m in &package.modules {
        module_exports.entry(m.id.clone()).or_default();
        for item in &m.ast.items {
            let Item::Type(td) = item else { continue };
            let mangled = mangle_type(&m.id, &td.name, m.is_entry);
            if types_info.contains_key(&mangled) || generics.contains_key(&mangled) {
                return Err(Error::at(
                    format!("duplicate type `{}`", td.name),
                    td.name_span,
                ));
            }
            if !td.type_params.is_empty() {
                // Template only — monomorphs are materialized on use.
                if td.type_params.len() > 4 {
                    return Err(Error::at(
                        "at most 4 type parameters are supported for now",
                        td.name_span,
                    ));
                }
                let kind = if let Some(members) = &td.union_members {
                    if !td.params.is_empty() || !td.fields.is_empty() {
                        return Err(Error::at(
                            "union type cannot have constructor params or body fields",
                            td.name_span,
                        ));
                    }
                    // Its methods are its conformances' members (D238).
                    GenericKind::Union {
                        members: members.clone(),
                        methods: td.methods.clone(),
                    }
                } else if !td.fields.is_empty() {
                    return Err(Error::at(
                        "generic types cannot use C-layout body fields yet",
                        td.name_span,
                    ));
                } else {
                    GenericKind::Product {
                        params: td.params.clone(),
                        methods: td.methods.clone(),
                        conversions: td.conversions.clone(),
                    }
                };
                if !td.conditional.is_empty() {
                    conditional.insert(mangled.clone(), td.conditional.clone());
                }
                generics.insert(
                    mangled.clone(),
                    GenericDef {
                        source_name: td.name.clone(),
                        mangled_base: mangled.clone(),
                        type_params: td.type_params.clone(),
                        def_mod: m.id.clone(),
                        bounds: td.bounds.clone(),
                        kind,
                        name_span: td.name_span,
                        name_map: HashMap::new(),
                        assoc: td.assoc.clone(),
                        checking: td
                            .checking
                            .iter()
                            .map(|checking| {
                                let args = checking
                                    .archetypes
                                    .iter()
                                    .map(|archetype| Ty::User(mangle_type(&m.id, archetype, m.is_entry)))
                                    .collect();
                                let derived = checking
                                    .derived
                                    .iter()
                                    .map(|param| {
                                        (param.name.clone(), Ty::User(mangle_type(&m.id, &param.archetype, m.is_entry)))
                                    })
                                    .collect();
                                (args, checking.requires.clone(), derived)
                            })
                            .collect(),
                        derived: td.checking.first().map(|checking| checking.derived.clone()).unwrap_or_default(),
                    },
                );
                if td.exported {
                    module_exports
                        .get_mut(&m.id)
                        .unwrap()
                        .types
                        .insert(td.name.clone(), mangled);
                }
                continue;
            }
            if matches!(
                td.name.as_str(),
                "Attr" | "Element" | "Text" | "Fragment" | "Node"
            ) {
                return Err(Error::at(
                    format!("`{}` is a built-in HTML type", td.name),
                    td.name_span,
                ));
            }
            // Field types may reference other types; resolve after all types registered.
            type_defs.push((m.id.clone(), mangled.clone(), td));
            types_info.insert(
                mangled.clone(),
                TypeInfo {
                    fields: Vec::new(),
                    methods: HashMap::new(),
                    zero_init: !td.fields.is_empty() && td.union_members.is_none(),
                    def_mod: m.id.clone(),
                    sum: None,
                    refined: None,
                    resource: td.traits.iter().any(|r| r.is_resource()),
                    traits: td.traits.clone(),
                    derived: td.derived.iter().cloned().collect(),
                    instance: None,
                    templates: HashMap::new(),
                },
            );
            if td.exported {
                module_exports
                    .get_mut(&m.id)
                    .unwrap()
                    .types
                    .insert(td.name.clone(), mangled);
            }
        }
    }

    // Per-module local type name map for resolve (source name → mangled), including imports.
    let mut per_mod_types: HashMap<String, HashMap<String, String>> = HashMap::new();
    for m in &package.modules {
        let mut map = HashMap::new();
        for item in &m.ast.items {
            if let Item::Type(td) = item {
                map.insert(td.name.clone(), mangle_type(&m.id, &td.name, m.is_entry));
            }
        }
        if let Some(imports) = import_maps.get(&m.id) {
            for (local, b) in imports {
                if let BoundImport::Name {
                    from_module,
                    original,
                } = b
                {
                    if let Some(mangled) = module_exports
                        .get(from_module)
                        .and_then(|e| e.types.get(original))
                    {
                        map.insert(local.clone(), mangled.clone());
                    }
                }
            }
        }
        // std/core's types (`Order`) are visible by name everywhere (D228).
        if let Some(core) = module_exports.get("std_core") {
            for (name, mangled) in &core.types {
                map.entry(name.clone()).or_insert_with(|| mangled.clone());
            }
        }
        // `mixed T` as this module spells it (D228 §6).
        for ((module, trait_name), mangled) in &trait_output.mixed {
            if *module == m.id {
                map.insert(format!("{}:{trait_name}", ast::MIXED), mangled.clone());
            }
        }
        per_mod_types.insert(m.id.clone(), map);
    }

    for gdef in generics.values_mut() {
        if let Some(map) = per_mod_types.get(&gdef.def_mod) {
            gdef.name_map = map.clone();
        }
    }
    // Seed common fallible monomorphs first so nested apps (Trial<May<…>>) resolve.
    seed_fallible_monos(
        &mut types_info,
        &mut type_layouts,
        &generics,
        &per_mod_types,
    )?;
    // Then materialize every GenericApp found in the package (including nested).
    let mut mono_method_jobs: Vec<MonoMethodJob> = Vec::new();
    // What instances read their parameters' associated types from (D237).
    let mut tables = AssocTables {
        built_in: trait_output.built_in_assoc.clone(),
        core_names: per_mod_types.get("std_core").cloned().unwrap_or_default(),
        ..AssocTables::default()
    };
    for (mod_id, mangled, td) in &type_defs {
        if td.assoc.is_empty() || !td.type_params.is_empty() {
            continue;
        }
        let names = per_mod_types.get(mod_id).cloned().unwrap_or_default();
        let written = td.assoc.iter().map(|(name, te, _)| (name.clone(), te.clone())).collect();
        tables.written.insert(mangled.clone(), (names, written));
    }
    monomorphize_package(
        package,
        &mut types_info,
        &mut type_layouts,
        &generics,
        &per_mod_types,
        &tables,
        &mut mono_method_jobs,
    )?;
    // Each instance's methods once, generic unions' included (D238).
    let mut method_jobs_made: HashSet<String> = HashSet::new();
    mono_method_jobs.retain(|job| job.conversion || method_jobs_made.insert(job_function_name(job)));
    mono_method_jobs.extend(union_method_jobs(&types_info, &generics, &mut method_jobs_made));
    // Generic types' checking instances (D228 §5), by name: the template,
    // and the bounds a conformance adds for the members checked there.
    let checking: HashMap<String, (String, Vec<Vec<TraitRef>>)> = generics
        .iter()
        .flat_map(|(base, gdef)| {
            gdef.checking
                .iter()
                .map(move |(args, requires, _)| (mono_type_name(base, args), (base.clone(), requires.clone())))
        })
        .collect();
    mono_method_jobs.sort_by_key(|job| !checking.contains_key(&job.mono_type));

    // Fill type fields with resolved types (concrete AST defs only).
    // Independent type-shape errors are collected for LSP multi-diag.
    let mut type_reg_errors: Vec<Error> = Vec::new();
    // Refined aliases (`type Qty = Int64 where …`) may nest; resolve until stable.
    let mut refined_progress = true;
    let mut refined_failed: HashSet<String> = HashSet::new();
    while refined_progress {
        refined_progress = false;
        for (mod_id, mangled, td) in &type_defs {
            if refined_failed.contains(mangled) {
                continue;
            }
            if types_info
                .get(mangled)
                .and_then(|i| i.refined.as_ref())
                .is_some()
            {
                continue;
            }
            let Some(members) = &td.union_members else {
                continue;
            };
            let name_map = per_mod_types.get(mod_id).cloned().unwrap_or_default();
            match try_refined_alias(members, td, &types_info, &name_map) {
                Ok(Some(refined)) => {
                    let fields = vec![FieldInfo {
                        name: "value".into(),
                        ty: refined.base.clone(),
                        mutable: false,
                        default: None,
                        positional: false,
                        where_clause: None,
                    }];
                    let info = types_info.get_mut(mangled).unwrap();
                    info.fields = fields.clone();
                    info.zero_init = false;
                    info.sum = None;
                    info.refined = Some(refined);
                    type_layouts.push(TypeLayout {
                        name: mangled.clone(),
                        fields: fields
                            .iter()
                            .map(|f| FieldLayout {
                                name: f.name.clone(),
                                c_name: sanitize_field(&f.name),
                                ty: f.ty.clone(),
                            })
                            .collect(),
                        sum: None,
                        sum_scalar: None,
                        has_drop: false,
                        own_hash: false,
                        own_equals: false,
                    });
                    refined_progress = true;
                }
                Ok(None) => {}
                Err(e) => {
                    refined_failed.insert((*mangled).clone());
                    type_reg_errors.push(in_module_id(e, package, mod_id));
                }
            }
        }
    }
    if !type_reg_errors.is_empty() {
        return Err(errors_to_multi(type_reg_errors));
    }
    for (mod_id, mangled, td) in &type_defs {
        if types_info
            .get(mangled)
            .and_then(|i| i.refined.as_ref())
            .is_some()
        {
            continue;
        }
        let name_map = per_mod_types.get(mod_id).cloned().unwrap_or_default();
        match resolve_type_def_shape(td, &types_info, &name_map) {
            Ok((fields, zero_init, sum_info, refined)) => {
                let info = types_info.get_mut(mangled).unwrap();
                info.fields = fields.clone();
                info.zero_init = zero_init;
                info.sum = sum_info.clone();
                info.refined = refined;
                let sum_layout = sum_info.as_ref().map(|s| {
                    s.variants
                        .iter()
                        .map(|v| SumVariantLayout {
                            tag: v.tag,
                            name: v.name.clone(),
                            payload_ty: v.ty.clone(),
                        })
                        .collect()
                });
                // Pure-literal unions: narrow scalar storage (D118 §6).
                let sum_scalar = sum_info.as_ref().and_then(|s| {
                    if s.variants.is_empty() || !s.variants.iter().all(|v| v.lit.is_some()) {
                        return None;
                    }
                    Some(pure_lit_scalar_ty(s.variants.len()))
                });
                type_layouts.push(TypeLayout {
                    name: mangled.clone(),
                    fields: fields
                        .iter()
                        .map(|f| FieldLayout {
                            name: f.name.clone(),
                            c_name: sanitize_field(&f.name),
                            ty: f.ty.clone(),
                        })
                        .collect(),
                    sum: sum_layout,
                    sum_scalar,
                    has_drop: false,
                    own_hash: false,
                    own_equals: false,
                });
            }
            Err(e) => type_reg_errors.push(in_module_id(e, package, mod_id)),
        }
    }
    type_reg_errors.extend(user_field_cycle_errors(&types_info, &type_defs));
    if !type_reg_errors.is_empty() {
        return Err(errors_to_multi(type_reg_errors));
    }

    // D131: bare singleton literal types + inline `A | B` in formals / annotations.
    materialize_package_typespace(package, &mut types_info, &mut type_layouts, &per_mod_types)?;

    // Methods (signature type-reg; multi-clause merge — D131).
    let mut method_reg_errors: Vec<Error> = Vec::new();
    // (type_mangled, method_name) → clause bodies for multi-clause lower
    let mut multi_methods: HashMap<(String, String), Vec<MultiClause>> = HashMap::new();
    for (mod_id, mangled, td) in &type_defs {
        // Group methods by name (source order).
        let mut groups: HashMap<String, Vec<&Method>> = HashMap::new();
        let mut group_order: Vec<String> = Vec::new();
        for m in &td.methods {
            // A method with type parameters is a template (D228 §5).
            if !m.generic.is_empty() {
                let def_mod = method_scope(m, mod_id).to_string();
                types_info.get_mut(mangled).unwrap().templates.insert(
                    m.name.clone(),
                    MethodTemplate {
                        method: m.clone(),
                        def_mod,
                    },
                );
                continue;
            }
            if !groups.contains_key(&m.name) {
                group_order.push(m.name.clone());
            }
            groups.entry(m.name.clone()).or_default().push(m);
        }
        for method_name in group_order {
            let clauses = groups.get(&method_name).unwrap();
            let scope = method_scope(clauses[0], mod_id);
            let name_map = with_self_type(
                per_mod_types.get(scope).cloned().unwrap_or_default(),
                mangled,
            );
            match merge_method_clauses(
                clauses,
                scope,
                &mut types_info,
                &mut type_layouts,
                &name_map,
            ) {
                Ok((sig, multi)) => {
                    types_info
                        .get_mut(mangled)
                        .unwrap()
                        .methods
                        .insert(method_name.clone(), sig);
                    if let Some(mc) = multi {
                        multi_methods.insert((mangled.clone(), method_name), mc);
                    }
                }
                Err(e) => method_reg_errors.push(in_module_id(e, package, scope)),
            }
        }
    }
    if !method_reg_errors.is_empty() {
        return Err(errors_to_multi(method_reg_errors));
    }

    // D155 / D228: a `Resource` product is heap-boxed; mark layouts.
    let mut drop_errors: Vec<Error> = Vec::new();
    for layout in &mut type_layouts {
        let Some(info) = types_info.get(&layout.name) else {
            continue;
        };
        if !info.resource {
            continue;
        }
        if layout.sum.is_some() {
            drop_errors.push(Error::new(format!(
                "union type `{}` cannot be a `Resource`",
                layout.name
            )));
            continue;
        }
        if info.zero_init {
            drop_errors.push(Error::new(format!(
                "C-layout type `{}` cannot be a `Resource`",
                layout.name
            )));
            continue;
        }
        layout.has_drop = true;
    }
    // D228: a Map keyed by a type with its own `hash` / `equals` calls them.
    for layout in &mut type_layouts {
        (layout.own_hash, layout.own_equals) = own_key_ops(&types_info, &conditional, &layout.name);
    }
    if !drop_errors.is_empty() {
        return Err(errors_to_multi(drop_errors));
    }

    // D228 §4: the compiler's default `==`, hash, and order go field by
    // field (a union's member by member), so each part must conform too.
    let mut default_errors: Vec<Error> = Vec::new();
    for (mod_id, mangled, td) in &type_defs {
        let info = &types_info[mangled];
        for (trait_name, member) in [("Equatable", Some("equals")), ("Hashable", None), ("Ordered", Some("compare"))] {
            let trait_ref = core_trait(trait_name);
            if !info.traits.contains(&trait_ref) {
                continue;
            }
            if info.resource {
                default_errors.push(in_module_id(
                    Error::at(format!("`{}` is a `Resource`, not a value, so it cannot be `{trait_name}`", td.name), td.name_span),
                    package,
                    mod_id,
                ));
                continue;
            }
            // Equal values hash alike: its own `equals` means its own `hash`.
            if trait_name == "Hashable"
                && info.methods.contains_key("equals")
                && !info.derived.contains("equals")
                && info.derived.contains("hash")
            {
                default_errors.push(in_module_id(
                    Error::at(
                        format!("`{}` has its own `equals`, so its `Hashable` conformance needs its own `hash`: equal values must hash alike", td.name),
                        td.name_span,
                    ),
                    package,
                    mod_id,
                ));
                continue;
            }
            if member.is_some_and(|member| info.methods.contains_key(member) && !info.derived.contains(member)) {
                continue;
            }
            let parts: Vec<(&str, String, Ty)> = match &info.sum {
                Some(sum) => sum
                    .variants
                    .iter()
                    .filter(|variant| variant.lit.is_none() && has_storage(&types_info, &variant.ty))
                    .map(|variant| ("member", variant.name.clone(), variant.ty.clone()))
                    .collect(),
                None => info
                    .fields
                    .iter()
                    .filter(|field| field.ty != Ty::Void)
                    .map(|field| ("field", field.name.clone(), field.ty.clone()))
                    .collect(),
            };
            for (kind, name, ty) in parts {
                if !ty_conforms(&types_info, &conditional, &ty, &trait_ref) {
                    default_errors.push(in_module_id(
                        Error::at(
                            format!(
                                "`{}` cannot be `{trait_name}` field by field: {kind} `{name}` is {}, which is not `{trait_name}`",
                                td.name,
                                display_ty(&ty)
                            ),
                            td.name_span,
                        ),
                        package,
                        mod_id,
                    ));
                }
            }
        }
    }
    // A generic type's instances hash structurally unless a conformance
    // writes `hash`: one with its own `equals` must, too.
    for (mangled, gdef) in &generics {
        let GenericKind::Product { methods, .. } = &gdef.kind else { continue };
        let hashable = conditional
            .get(mangled)
            .is_some_and(|list| list.iter().any(|each| each.trait_ref == core_trait("Hashable")));
        let has = |name: &str| methods.iter().any(|method| method.name == name);
        if hashable && has("equals") && !has("hash") {
            default_errors.push(in_module_id(
                Error::at(
                    format!("`{}` has its own `equals`, so its `Hashable` conformance needs its own `hash`: equal values must hash alike", gdef.source_name),
                    gdef.name_span,
                ),
                package,
                &gdef.def_mod,
            ));
        }
    }
    if !default_errors.is_empty() {
        return Err(errors_to_multi(default_errors));
    }

    // ---------- Phase 2: functions + externs ----------
    let mut fns: HashMap<String, FnSig> = HashMap::new();
    let mut extern_decls: Vec<ExternDecl> = Vec::new();
    // (module, mangled, &Function, is_entry)
    let mut free_fns: Vec<(String, String, &ast::Function, bool)> = Vec::new();
    // mangled free-fn name → multi-clause bodies (D131)
    let mut multi_free: HashMap<String, Vec<MultiClause>> = HashMap::new();
    // Generic functions' templates (D228 §5).
    let mut generic_templates: HashMap<String, generic::GenericFn> = HashMap::new();

    for m in &package.modules {
        // Signature errors are spans in this module: attach its path.
        let registered = (|| -> Result<()> {
            let name_map = per_mod_types.get(&m.id).cloned().unwrap_or_default();
            // Collect free functions by name for multi-clause merge (D131).
            let mut free_groups: HashMap<String, Vec<&ast::Function>> = HashMap::new();
            let mut free_order: Vec<String> = Vec::new();
            for item in &m.ast.items {
                if let Item::Function(f) = item {
                    if !free_groups.contains_key(&f.name) {
                        free_order.push(f.name.clone());
                    }
                    free_groups.entry(f.name.clone()).or_default().push(f);
                }
            }
            for fname in free_order {
                let clauses = free_groups.get(&fname).unwrap();
                let mangled = if m.is_entry && fname == "main" {
                    "main".to_string()
                } else {
                    mangle_fn(&m.id, &fname, m.is_entry)
                };
                // A generic function is a template; its instances are made
                // at the calls (D228 §5).
                if !clauses[0].generic.is_empty() {
                    if clauses.len() > 1 {
                        return Err(Error::at(
                            format!("generic function `{fname}` cannot be multi-clause"),
                            clauses[1].name_span,
                        ));
                    }
                    if clauses[0].exported {
                        module_exports
                            .get_mut(&m.id)
                            .unwrap()
                            .fns
                            .insert(fname.clone(), mangled.clone());
                    }
                    generic_templates.insert(
                        mangled,
                        generic::GenericFn {
                            function: clauses[0].clone(),
                            module: m.id.clone(),
                            is_entry: m.is_entry,
                        },
                    );
                    continue;
                }
                match merge_free_clauses(
                    clauses,
                    m.is_entry,
                    &m.id,
                    &mut types_info,
                    &mut type_layouts,
                    &name_map,
                ) {
                    Ok((mut sig, multi)) => {
                        let exported = multi
                            .as_ref()
                            .map(|c| c.iter().any(|x| x.exported))
                            .unwrap_or(clauses[0].exported);
                        sig.mangled = mangled.clone();
                        if fns.insert(mangled.clone(), sig).is_some() {
                            return Err(Error::at(
                                format!("duplicate function `{fname}`"),
                                clauses[0].name_span,
                            ));
                        }
                        if exported {
                            module_exports
                                .get_mut(&m.id)
                                .unwrap()
                                .fns
                                .insert(fname.clone(), mangled.clone());
                        }
                        if let Some(mc) = multi {
                            multi_free.insert(mangled.clone(), mc);
                        }
                        free_fns.push((m.id.clone(), mangled, clauses[0], m.is_entry));
                    }
                    Err(e) => return Err(e),
                }
            }
            for item in &m.ast.items {
                if let Item::Extern(e) = item {
                    if e.library != "c" {
                        return Err(Error::at(
                            format!("only library \"c\" is supported (got \"{}\")", e.library),
                            e.library_span,
                        ));
                    }
                    let ret = match e.return_type.as_ref() {
                        Some(t) => resolve_ty(t, &types_info, &name_map)?,
                        None => Ty::Void,
                    };
                    if !is_ffi_ty(&ret) {
                        return Err(Error::at(
                            format!("type {ret:?} is not FFI-marshalable"),
                            e.return_type
                                .as_ref()
                                .map(|t| t.span)
                                .unwrap_or(e.name_span),
                        ));
                    }
                    let mut params = Vec::new();
                    let mut defaults = Vec::new();
                    for p in &e.params {
                        let ty = resolve_ty(&p.ty, &types_info, &name_map)?;
                        if matches!(ty, Ty::Void) || !is_ffi_ty(&ty) {
                            return Err(Error::at(
                                format!("type {ty:?} is not FFI-marshalable as a parameter"),
                                p.ty.span,
                            ));
                        }
                        params.push((p.name.clone(), ty));
                        defaults.push(p.default.clone());
                    }
                    // Same ordering rule as free functions (call-site defaults).
                    let mut seen_default = false;
                    for (i, d) in defaults.iter().enumerate() {
                        if d.is_some() {
                            seen_default = true;
                        } else if seen_default {
                            return Err(Error::at(
                                format!(
                                    "parameter `{}` without a default follows a defaulted parameter",
                                    params[i].0
                                ),
                                e.params[i].name_span,
                            ));
                        }
                    }
                    let c_symbol = e.c_symbol.clone().unwrap_or_else(|| e.name.clone());
                    let mangled = mangle_fn(&m.id, &e.name, m.is_entry);
                    if fns
                        .insert(
                            mangled.clone(),
                            FnSig {
                                params: params.clone(),
                                defaults,
                                ret: ret.clone(),
                                extern_symbol: Some(c_symbol.clone()),
                                mangled: mangled.clone(),
                                def_mod: m.id.clone(),
                                multi_clause: false,
                                mutable: Vec::new(),
                                positional: vec![true; e.params.len()],
                            },
                        )
                        .is_some()
                    {
                        return Err(Error::at(
                            format!("duplicate function `{}`", e.name),
                            e.name_span,
                        ));
                    }
                    if e.exported {
                        module_exports
                            .get_mut(&m.id)
                            .unwrap()
                            .fns
                            .insert(e.name.clone(), mangled);
                    }
                    extern_decls.push(ExternDecl {
                        thoxa_name: e.name.clone(),
                        c_symbol,
                        params,
                        ret,
                    });
                }
            }
            Ok(())
        })();
        if let Err(error) = registered {
            return Err(in_module_file(error, &m.path));
        }
    }

    // A generic instance's `from` clauses are free functions of its module,
    // with the type arguments substituted (D217).
    // Each with its instance and its template's member (D228 §5).
    let mut mono_conversions: Vec<(String, String, ast::Function, bool, (String, (String, String)))> = Vec::new();
    for job in mono_method_jobs.iter().filter(|job| job.conversion) {
        let module = package
            .modules
            .iter()
            .find(|module| module.id == job.def_mod)
            .unwrap();
        let name_map = per_mod_types.get(&job.def_mod).cloned().unwrap_or_default();
        let method = subst_method(&job.method, &job.subst);
        let function = ast::Function {
            exported: method.exported,
            name: method.name,
            name_span: method.name_span,
            params: method.params,
            return_type: method.return_type,
            body: method.body,
            type_params: Vec::new(),
            generic: Vec::new(),
        };
        let mangled = mangle_fn(&job.def_mod, &function.name, module.is_entry);
        let (mut sig, _) = merge_free_clauses(
            &[&function],
            module.is_entry,
            &job.def_mod,
            &mut types_info,
            &mut type_layouts,
            &name_map,
        )
        .map_err(|error| in_module_file(error, &module.path))?;
        sig.mangled = mangled.clone();
        fns.insert(mangled.clone(), sig);
        if function.exported {
            module_exports
                .get_mut(&job.def_mod)
                .unwrap()
                .fns
                .insert(function.name.clone(), mangled.clone());
        }
        let member = (job.mono_type.clone(), template_member(&types_info, job));
        mono_conversions.push((job.def_mod.clone(), mangled, function, module.is_entry, member));
    }

    if opts.require_main && !fns.contains_key("main") {
        let error = Error::new("program requires `fn main`");
        return Err(match package.modules.iter().find(|module| module.is_entry) {
            Some(entry) => error.with_path(&entry.path),
            None => error,
        });
    }

    // ---------- Phase 3: globals (module-level let/mut) ----------
    // Placeholders: type of global may reference types; inits lowered later.
    let mut all_globals: HashMap<String, GlobalInfo> = HashMap::new(); // place name → info
                                                                       // (module_id, &Global, place name)
    let mut global_defs: Vec<(String, &ast::Global, String)> = Vec::new();

    for m in &package.modules {
        let name_map = per_mod_types.get(&m.id).cloned().unwrap_or_default();
        for item in &m.ast.items {
            let Item::Global(g) = item else { continue };
            let place_name = mangle_global(&m.id, &g.name);
            // Infer type by lowering init in a throwaway context later; for registration
            // we need the type first — lower init once for type only in phase 4.
            global_defs.push((m.id.clone(), g, place_name.clone()));
            // Temporary Void; filled after init type inference.
            let _ = name_map;
            all_globals.insert(
                place_name,
                GlobalInfo {
                    ty: Ty::Void, // filled later
                    mutable: g.mutable,
                    place: Place {
                        name: mangle_global(&m.id, &g.name),
                    },
                },
            );
        }
    }

    // Infer global types by lowering inits in order (module dep order).
    let mut program_globals: Vec<GlobalVar> = Vec::new();
    let mut init_stmts: Vec<Stmt> = Vec::new();
    // Every global's init lands in one C function: number temps across all of
    // them, or `_obj0` from two modules would share one (mistyped) local.
    let mut init_temps = 0;
    // A generic type's instances meet its parameters' bounds (D228 §5).
    let mut bound_errors = Vec::new();
    for info in types_info.values() {
        if let Some((base, args)) = &info.instance {
            if let Err(error) = instance_meets_bounds(&types_info, &conditional, &generics, &tables, base, args) {
                bound_errors.push(in_module_id(error, package, &info.def_mod));
            }
        }
    }
    // Each type's associated types (D228 §7), which meet their bounds.
    let mut assoc_types: HashMap<String, HashMap<String, Ty>> = HashMap::new();
    for (mod_id, mangled, td) in &type_defs {
        if td.assoc.is_empty() {
            continue;
        }
        let name_map = per_mod_types.get(mod_id).cloned().unwrap_or_default();
        let mut resolved = HashMap::new();
        for (name, te, bounds) in &td.assoc {
            let ty = match resolve_ty(te, &types_info, &name_map) {
                Ok(ty) => ty,
                Err(error) => {
                    bound_errors.push(in_module_id(error, package, mod_id));
                    continue;
                }
            };
            for bound in bounds {
                if !ty_conforms(&types_info, &conditional, &ty, bound) {
                    bound_errors.push(in_module_id(
                        Error::at(
                            format!(
                                "`{}`'s `{name}` is {}, which is not `{}`",
                                td.name,
                                display_ty(&ty),
                                bound.name
                            ),
                            te.span,
                        ),
                        package,
                        mod_id,
                    ));
                }
            }
            resolved.insert(name.clone(), ty);
        }
        assoc_types.insert(mangled.clone(), resolved);
    }
    if !bound_errors.is_empty() {
        return Err(errors_to_multi(bound_errors));
    }

    // Each generic function is checked once, as its instance for its own
    // archetypes (D228 §5).
    let checking_jobs: Vec<generic::InstanceJob> = generic_templates
        .iter()
        .map(|(mangled, template)| {
            let subst: HashMap<String, Ty> = template
                .function
                .generic
                .iter()
                .map(|param| {
                    (
                        param.name.clone(),
                        Ty::User(mangle_type(&template.module, &param.archetype, template.is_entry)),
                    )
                })
                .collect();
            generic::InstanceJob {
                template: mangled.clone(),
                instance: generic::instance_name(mangled, &template.function, &subst),
                subst,
                archetypal: true,
            }
        })
        .collect();
    let synthetics = Rc::new(RefCell::new(SyntheticRegistry {
        conditional: std::mem::take(&mut conditional),
        generic_fns: generic_templates,
        instance_jobs: checking_jobs,
        generics: generics.clone(),
        assoc: assoc_types,
        mixed_closed: trait_output.mixed_closed.clone(),
        mixed_members: trait_output.mixed_members.clone(),
        built_in_assoc: trait_output.built_in_assoc.clone(),
        ..SyntheticRegistry::default()
    }));
    // Name maps of the modules whose globals are done, so a default argument
    // resolves in its defining module (D116) inside a module initializer.
    let mut init_package_maps: HashMap<String, ModuleNameMaps> = HashMap::new();

    for m in &package.modules {
        let (name_to_type, name_to_fn, name_to_global, namespaces) = build_module_name_maps(
            m,
            &import_maps,
            &module_exports,
            &all_globals,
            &per_mod_types,
            &fns,
        )?;

        for item in &m.ast.items {
            let Item::Global(g) = item else { continue };
            let place_name = mangle_global(&m.id, &g.name);
            let mut ctx = LowerCtx::new(
                fns.clone(),
                types_info.clone(),
                name_to_type.clone(),
                name_to_fn.clone(),
                name_to_global.clone(),
                namespaces.clone(),
                module_exports.clone(),
                init_package_maps.clone(),
                Ty::Void,
                m.id.clone(),
                m.is_entry,
                Rc::clone(&synthetics),
            );
            ctx.temps = init_temps;
            let lowered = match &g.ty {
                Some(annotation) => (|| {
                    let expect = resolve_ty(annotation, &ctx.types, &ctx.name_to_type)?;
                    let (place, ty) = lower_expr_expect(&mut ctx, &g.init, Some(&expect))?;
                    let place =
                        coerce_annotated(&mut ctx, place, &ty, &expect, &g.init, &g.name, g.name_span)?;
                    Ok((place, expect))
                })(),
                None => lower_expr(&mut ctx, &g.init),
            };
            let (place, ty) = lowered.map_err(|error| in_module_file(error, &m.path))?;
            init_temps = ctx.temps;
            // Move init value into global place.
            let global_place = Place {
                name: place_name.clone(),
            };
            ctx.assign(global_place.clone(), ty.clone(), RValue::Use(place));
            // Drop init temps (`_s0` from `"stderr"`) but not the global (D122).
            insert_function_drops(&mut ctx.stmts, &Some(global_place), &ty, &ctx.drop_places);
            init_stmts.extend(ctx.stmts);

            let ginfo = GlobalInfo {
                ty: ty.clone(),
                mutable: g.mutable,
                place: Place {
                    name: place_name.clone(),
                },
            };
            all_globals.insert(place_name.clone(), ginfo.clone());
            if g.exported {
                module_exports
                    .get_mut(&m.id)
                    .unwrap()
                    .globals
                    .insert(g.name.clone(), ginfo);
            }
            program_globals.push(GlobalVar {
                name: place_name,
                ty,
            });
        }
        let (name_to_type, name_to_fn, name_to_global, namespaces) = build_module_name_maps(
            m,
            &import_maps,
            &module_exports,
            &all_globals,
            &per_mod_types,
            &fns,
        )?;
        init_package_maps.insert(
            m.id.clone(),
            ModuleNameMaps {
                name_to_type,
                name_to_fn,
                name_to_global,
                namespaces,
            },
        );
    }

    let bodies_started = std::time::Instant::now();
    // ---------- Phase 4: lower methods + free functions ----------
    // Build per-module name maps for default-arg defining-module scope (D116).
    let mut package_maps: HashMap<String, ModuleNameMaps> = HashMap::new();
    for m in &package.modules {
        let (name_to_type, name_to_fn, name_to_global, namespaces) = build_module_name_maps(
            m,
            &import_maps,
            &module_exports,
            &all_globals,
            &per_mod_types,
            &fns,
        )?;
        package_maps.insert(
            m.id.clone(),
            ModuleNameMaps {
                name_to_type,
                name_to_fn,
                name_to_global,
                namespaces,
            },
        );
    }

    // D129 / D174: infer unannotated function and method returns before body lower.
    infer_unannotated_returns(
        &mut fns,
        &mut types_info,
        &mut type_layouts,
        &generics,
        &free_fns,
        &multi_free,
        &type_defs,
        &multi_methods,
        &mono_method_jobs,
        &package_maps,
        &module_exports,
        &synthetics,
        package,
    )?;

    let mut functions = Vec::new();
    // Collect independent body-lower errors so LSP can show more than one (E7+).
    let mut body_errors: Vec<Error> = Vec::new();

    // A trait's archetype first (D228): an error in a body it provides is
    // reported once, against the trait, not again for each conforming type.
    let mut lower_order: Vec<&(String, String, &TypeDef)> = type_defs.iter().collect();
    lower_order.sort_by_key(|(_, _, td)| !is_archetype(td));
    let mut archetype_errors = false;
    for (type_mod, mangled_ty, td) in lower_order {
        let mut seen_methods: HashSet<String> = HashSet::new();
        for method in td.methods.iter().filter(|method| method.generic.is_empty()) {
            if !seen_methods.insert(method.name.clone()) {
                continue; // multi-clause: lower once per name
            }
            if archetype_errors && !is_archetype(td) && method.provided.is_some() {
                continue;
            }
            let mod_id = &method_scope(method, type_mod).to_string();
            let m = package.modules.iter().find(|x| x.id == *mod_id).unwrap();
            let maps = package_maps.get(mod_id).unwrap();
            let result = if let Some(clauses) =
                multi_methods.get(&(mangled_ty.clone(), method.name.clone()))
            {
                lower_multi_method(
                    mangled_ty,
                    &method.name,
                    clauses,
                    &fns,
                    &types_info,
                    &maps.name_to_type,
                    &maps.name_to_fn,
                    &maps.name_to_global,
                    &maps.namespaces,
                    &module_exports,
                    &package_maps,
                    mod_id,
                    m.is_entry,
                    &synthetics,
                )
            } else {
                lower_method(
                    mangled_ty,
                    method,
                    &fns,
                    &types_info,
                    &maps.name_to_type,
                    &maps.name_to_fn,
                    &maps.name_to_global,
                    &maps.namespaces,
                    &module_exports,
                    &package_maps,
                    mod_id,
                    m.is_entry,
                    &synthetics,
                )
            };
            match result {
                Ok(f) => functions.push(f),
                Err(e) => {
                    archetype_errors |= is_archetype(td);
                    body_errors.push(in_module_file(e, &m.path));
                }
            }
        }
    }

    // Monomorphized methods on generic products (no multi-clause on generic
    // templates in v0); an instance has a conformance's only when it meets
    // the conformance's bounds. The checking instances' come first (D228
    // §5): a member that fails there is not lowered for the real ones.
    let mut failed_members: HashSet<(String, String)> = HashSet::new();
    for job in mono_method_jobs
        .iter()
        .filter(|job| !job.conversion && job_applies(&types_info, &synthetics, job))
    {
        let checks = checking.get(&job.mono_type);
        if checks.is_some_and(|(_, requires)| !checks_member(requires, &job.method)) {
            continue;
        }
        let member = template_member(&types_info, job);
        if checks.is_none() && failed_members.contains(&member) {
            continue;
        }
        let m = package
            .modules
            .iter()
            .find(|x| x.id == job.def_mod)
            .unwrap();
        let maps = package_maps.get(&job.def_mod).unwrap();
        let method = subst_method(&job.method, &job.subst);
        match lower_method(
            &job.mono_type,
            &method,
            &fns,
            &types_info,
            &maps.name_to_type,
            &maps.name_to_fn,
            &maps.name_to_global,
            &maps.namespaces,
            &module_exports,
            &package_maps,
            &job.def_mod,
            m.is_entry,
            &synthetics,
        ) {
            Ok(f) => functions.push(f),
            Err(e) => {
                if checks.is_some() {
                    failed_members.insert(member);
                }
                body_errors.push(in_module_file(e, &m.path));
            }
        }
    }

    for (mod_id, mangled, f, is_entry) in &free_fns {
        let maps = package_maps.get(mod_id).unwrap();
        let module_path = package
            .modules
            .iter()
            .find(|x| x.id == *mod_id)
            .map(|x| x.path.clone());
        let result = if let Some(clauses) = multi_free.get(mangled) {
            lower_multi_free(
                f,
                mangled,
                clauses,
                *is_entry,
                &fns,
                &types_info,
                &maps.name_to_type,
                &maps.name_to_fn,
                &maps.name_to_global,
                &maps.namespaces,
                &module_exports,
                &package_maps,
                mod_id,
                &synthetics,
            )
        } else {
            lower_function(
                f,
                mangled,
                *is_entry,
                &fns,
                &types_info,
                &maps.name_to_type,
                &maps.name_to_fn,
                &maps.name_to_global,
                &maps.namespaces,
                &module_exports,
                &package_maps,
                mod_id,
                &synthetics,
            )
        };
        match result {
            Ok(fun) => functions.push(fun),
            Err(e) => body_errors.push(match &module_path {
                Some(path) => in_module_file(e, path),
                None => e,
            }),
        }
    }

    for (mod_id, mangled, function, is_entry, member) in &mono_conversions {
        // A `from` clause is checked once, with the type's own bounds.
        let checks = match checking.get(&member.0) {
            Some((_, requires)) if !requires.iter().all(Vec::is_empty) => continue,
            Some(_) => true,
            None => false,
        };
        if !checks && failed_members.contains(&member.1) {
            continue;
        }
        let maps = package_maps.get(mod_id).unwrap();
        let module_path = package
            .modules
            .iter()
            .find(|module| module.id == *mod_id)
            .map(|module| module.path.clone());
        match lower_function(
            function,
            mangled,
            *is_entry,
            &fns,
            &types_info,
            &maps.name_to_type,
            &maps.name_to_fn,
            &maps.name_to_global,
            &maps.namespaces,
            &module_exports,
            &package_maps,
            mod_id,
            &synthetics,
        ) {
            Ok(fun) => functions.push(fun),
            Err(e) => {
                if checks {
                    failed_members.insert(member.1.clone());
                }
                body_errors.push(match &module_path {
                    Some(path) => in_module_file(e, path),
                    None => e,
                });
            }
        }
    }

    // Generic functions' instances (D228 §5): each template's checking one
    // first; a template whose body fails there makes no others. Lowering one
    // may ask for more, and for the methods of generic types' instances.
    let mut lowered_instances: HashSet<String> = HashSet::new();
    let mut failed_templates: HashSet<String> = HashSet::new();
    let mut failed_methods: HashSet<(String, String)> = HashSet::new();
    // Each method template checked once against its archetypes (D228 §5): a
    // generic type's on its checking instance.
    let mut with_templates: Vec<(String, String)> = types_info
        .iter()
        .filter(|(name, info)| !info.templates.is_empty() && (info.instance.is_none() || checking.contains_key(*name)))
        .map(|(name, info)| (name.clone(), info.def_mod.clone()))
        .collect();
    with_templates.sort();
    for (type_name, def_mod) in with_templates {
        let maps = package_maps.get(&def_mod).cloned().unwrap_or_default();
        let mut check_ctx = LowerCtx::new(
            fns.clone(),
            types_info.clone(),
            maps.name_to_type,
            maps.name_to_fn,
            maps.name_to_global,
            maps.namespaces,
            module_exports.clone(),
            package_maps.clone(),
            Ty::Void,
            def_mod.clone(),
            false,
            Rc::clone(&synthetics),
        );
        if let Err(e) = generic::check_templates(&mut check_ctx, &type_name) {
            body_errors.push(in_module_id(e, package, &def_mod));
        }
    }
    let entry_module = package
        .modules
        .iter()
        .find(|module| module.is_entry)
        .map(|module| module.id.clone())
        .unwrap_or_else(|| "Main".into());
    let mut mixed_made: HashMap<String, usize> = HashMap::new();
    // The instance methods lowered so far, by function name: an instance
    // made again elsewhere does not lower them twice (D238).
    let mut lowered_method_jobs: HashSet<String> = mono_method_jobs
        .iter()
        .filter(|job| !job.conversion)
        .map(job_function_name)
        .collect();
    loop {
        let job = {
            let mut synthetics = synthetics.borrow_mut();
            let index = synthetics
                .instance_jobs
                .iter()
                .position(|job| job.archetypal)
                .or(if synthetics.instance_jobs.is_empty() { None } else { Some(0) });
            match index {
                Some(index) => Some(synthetics.instance_jobs.remove(index)),
                None => None,
            }
        };
        let Some(job) = job else {
            let mut late = std::mem::take(&mut synthetics.borrow_mut().late_mono_jobs);
            // Generic unions' instances made while lowering (D238).
            {
                let mut types_now = types_info.clone();
                seed_synthetics(&mut types_now, &synthetics.borrow());
                let generics_now = synthetics.borrow().generics.clone();
                late.extend(union_method_jobs(&types_now, &generics_now, &mut method_jobs_made));
            }
            late.retain(|job| job.conversion || !lowered_method_jobs.contains(&job_function_name(job)));
            lowered_method_jobs.extend(late.iter().filter(|job| !job.conversion).map(job_function_name));
            let mut made_methods = std::mem::take(&mut synthetics.borrow_mut().method_jobs);
            if late.is_empty() && made_methods.is_empty() {
                // Calls through `mixed` values (D235), over every member known
                // once nothing else is left to lower; making them may add
                // jobs, and those may add members.
                let made = lower_mixed_functions(
                    &fns,
                    &types_info,
                    &package_maps,
                    &module_exports,
                    &entry_module,
                    &synthetics,
                    &mut mixed_made,
                );
                if made.is_empty() {
                    break;
                }
                for function in made {
                    match function {
                        Ok(function) => {
                            functions.retain(|known| known.name != function.name);
                            functions.push(function);
                        }
                        Err(error) => body_errors.push(error),
                    }
                }
                continue;
            }
            // Late instances' bounds are checked with the types made so far.
            let mut types_now = types_info.clone();
            seed_synthetics(&mut types_now, &synthetics.borrow());
            for job in late.iter().filter(|job| {
                !job.conversion
                    && job_applies(&types_now, &synthetics, job)
                    && !failed_members.contains(&template_member(&types_info, job))
            }) {
                let m = package.modules.iter().find(|x| x.id == job.def_mod).unwrap();
                let maps = package_maps.get(&job.def_mod).unwrap();
                let method = subst_method(&job.method, &job.subst);
                match lower_method(
                    &job.mono_type,
                    &method,
                    &fns,
                    &types_info,
                    &maps.name_to_type,
                    &maps.name_to_fn,
                    &maps.name_to_global,
                    &maps.namespaces,
                    &module_exports,
                    &package_maps,
                    &job.def_mod,
                    m.is_entry,
                    &synthetics,
                ) {
                    Ok(f) => functions.push(f),
                    Err(e) => body_errors.push(in_module_file(e, &m.path)),
                }
            }
            // Methods made from templates (D228 §5): each template's checking
            // one first; a template whose body fails there makes no others.
            made_methods.sort_by_key(|job| !job.archetypal);
            for job in &made_methods {
                if !job.archetypal && failed_methods.contains(&job.template) {
                    continue;
                }
                let m = package.modules.iter().find(|x| x.id == job.def_mod).unwrap();
                let maps = package_maps.get(&job.def_mod).unwrap();
                match lower_method(
                    &job.type_name,
                    &job.method,
                    &fns,
                    &types_info,
                    &maps.name_to_type,
                    &maps.name_to_fn,
                    &maps.name_to_global,
                    &maps.namespaces,
                    &module_exports,
                    &package_maps,
                    &job.def_mod,
                    m.is_entry,
                    &synthetics,
                ) {
                    Ok(f) => functions.push(f),
                    Err(e) => {
                        if job.archetypal {
                            failed_methods.insert(job.template.clone());
                        }
                        body_errors.push(in_module_file(e, &m.path));
                    }
                }
            }
            continue;
        };
        if !lowered_instances.insert(job.instance.clone()) {
            continue;
        }
        if !job.archetypal && failed_templates.contains(&job.template) {
            continue;
        }
        let template = synthetics.borrow().generic_fns[&job.template].clone();
        let m = package.modules.iter().find(|x| x.id == template.module).unwrap();
        let maps = package_maps.get(&template.module).unwrap();
        // A checking instance's fixed associated types (D231) are their types,
        // with the archetypes for the type parameters they name.
        let mut job = job;
        if job.archetypal && template.function.generic.iter().any(|param| param.fixed.is_some()) {
            let mut fixed_ctx = LowerCtx::new(
                fns.clone(),
                types_info.clone(),
                maps.name_to_type.clone(),
                maps.name_to_fn.clone(),
                maps.name_to_global.clone(),
                maps.namespaces.clone(),
                module_exports.clone(),
                package_maps.clone(),
                Ty::Void,
                template.module.clone(),
                false,
                Rc::clone(&synthetics),
            );
            for param in &template.function.generic {
                let Some(fixed) = &param.fixed else { continue };
                let written = subst_type_expr(fixed, &job.subst);
                let name_map = maps.name_to_type.clone();
                let resolved = generic::ensure_instances(&mut fixed_ctx, &written, &name_map)
                    .and_then(|()| resolve_ty(&written, &fixed_ctx.types, &name_map));
                match resolved {
                    Ok(ty) => {
                        job.subst.insert(param.name.clone(), ty);
                    }
                    Err(e) => body_errors.push(in_module_file(e, &m.path)),
                }
            }
        }
        let function = subst_function(&template.function, &job.subst);
        let mut known = fns.clone();
        known.extend(synthetics.borrow().fn_instances.clone());
        // Its signature first: it may name generic types' instances nothing
        // made yet, which then join the synthetic types.
        if !known.contains_key(&job.instance) {
            let mut sig_ctx = LowerCtx::new(
                known.clone(),
                types_info.clone(),
                maps.name_to_type.clone(),
                maps.name_to_fn.clone(),
                maps.name_to_global.clone(),
                maps.namespaces.clone(),
                module_exports.clone(),
                package_maps.clone(),
                Ty::Void,
                template.module.clone(),
                false,
                Rc::clone(&synthetics),
            );
            match generic::instance_sig(&mut sig_ctx, &template, &job.instance, &job.subst) {
                Ok(sig) => {
                    synthetics.borrow_mut().fn_instances.insert(job.instance.clone(), sig.clone());
                    known.insert(job.instance.clone(), sig);
                }
                Err(e) => {
                    failed_templates.insert(job.template.clone());
                    body_errors.push(in_module_file(e, &m.path));
                    continue;
                }
            }
        }
        let mut types_now = types_info.clone();
        for (name, (info, _)) in synthetics.borrow().entries.iter() {
            types_now.entry(name.clone()).or_insert_with(|| info.clone());
        }
        match lower_function(
            &function,
            &job.instance,
            false,
            &known,
            &types_now,
            &maps.name_to_type,
            &maps.name_to_fn,
            &maps.name_to_global,
            &maps.namespaces,
            &module_exports,
            &package_maps,
            &template.module,
            &synthetics,
        ) {
            Ok(fun) => functions.push(fun),
            Err(e) => {
                if job.archetypal {
                    failed_templates.insert(job.template.clone());
                }
                body_errors.push(in_module_file(e, &m.path));
            }
        }
    }

    // The `==` functions of types that contain themselves (D212).
    let entry_mod_id = package
        .modules
        .iter()
        .find(|module| module.is_entry)
        .map(|module| module.id.clone())
        .unwrap_or_else(|| "Main".into());
    match lower_eq_functions(
        &fns,
        &types_info,
        &package_maps,
        &module_exports,
        &entry_mod_id,
        &synthetics,
        &mut HashSet::new(),
    ) {
        Ok(generated) => functions.extend(generated),
        Err(e) => body_errors.push(e),
    }

    if std::env::var_os("THOXA_PROFILE").is_some() {
        eprintln!(
            "thoxa profile: mir-bodies {} fns {:?}",
            functions.len(),
            bodies_started.elapsed()
        );
    }
    let write_errors = {
        let synthetics = synthetics.borrow();
        receiver_errors(&functions, &synthetics.receiver_checks, &synthetics.getter_checks)
    };
    for error in write_errors {
        let path = package
            .modules
            .iter()
            .find(|module| module.id == error.1)
            .map(|module| module.path.clone());
        body_errors.push(match &path {
            Some(path) => in_module_file(error.0, path),
            None => error.0,
        });
    }
    if !body_errors.is_empty() {
        // Build/run still report the first error; LSP expands `more` via check_source.
        return Err(errors_to_multi(body_errors));
    }

    // Synthetic unions from unannotated if lub (D119).
    for (_, (_, layout)) in synthetics.borrow().entries.iter() {
        type_layouts.push(layout.clone());
    }
    // The members `mixed` unions gained while lowering (D235).
    for (mixed, added) in &synthetics.borrow().mixed_added {
        if let Some(layout) = type_layouts.iter_mut().find(|layout| layout.name == *mixed) {
            let variants = layout.sum.get_or_insert_with(Vec::new);
            for variant in added {
                variants.push(SumVariantLayout {
                    tag: variant.tag,
                    name: variant.name.clone(),
                    payload_ty: variant.ty.clone(),
                });
            }
        }
    }
    // The types only generic code's checks use are not the program's.
    let archetypes: HashSet<String> = type_defs
        .iter()
        .filter(|(_, _, td)| td.name.starts_with("Param_") || is_archetype(td))
        .map(|(_, mangled, _)| mangled.clone())
        .collect();
    let mut instances: HashMap<String, Vec<Ty>> = HashMap::new();
    for (name, info) in types_info
        .iter()
        .chain(synthetics.borrow().entries.iter().map(|(name, (info, _))| (name, info)))
    {
        if let Some((_, args)) = &info.instance {
            instances.insert(name.clone(), args.clone());
        }
    }
    let type_layouts = without_checking_types(type_layouts, archetypes, &instances);

    if std::env::var_os("THOXA_PROFILE").is_some() {
        eprintln!("thoxa profile: mir-total {:?}", lower_started.elapsed());
    }
    Ok(Program {
        types: type_layouts,
        externs: extern_decls,
        globals: program_globals,
        init: init_stmts,
        functions,
    })
}

fn union_member_field_ok(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Int64
            | Ty::Int32
            | Ty::UInt32
            | Ty::UInt64
            | Ty::Byte
            | Ty::Float64
            | Ty::Bool
            | Ty::Void
            | Ty::String
            | Ty::Bytes
            | Ty::User(_)
            | Ty::Array { .. }
            | Ty::Map { .. }
    )
}

/// Resolve concrete type fields / union shape (type-reg phase).
fn resolve_type_def_shape(
    td: &TypeDef,
    types_info: &HashMap<String, TypeInfo>,
    name_map: &HashMap<String, String>,
) -> Result<(Vec<FieldInfo>, bool, Option<SumInfo>, Option<RefinedInfo>)> {
    let mut fields: Vec<FieldInfo> = Vec::new();
    let zero_init;
    let mut sum_info: Option<SumInfo> = None;

    // Union alias: `type Dir = Up | Down` (D118). Its methods are its
    // conformances' members (D238).
    if let Some(members) = &td.union_members {
        if !td.params.is_empty() || !td.fields.is_empty() {
            return Err(Error::at(
                "union type cannot have constructor params or body fields",
                td.name_span,
            ));
        }
        if let Some(refined) = try_refined_alias(members, td, types_info, name_map)? {
            let fields = vec![FieldInfo {
                name: "value".into(),
                ty: refined.base.clone(),
                mutable: false,
                default: None,
                positional: false,
                where_clause: None,
            }];
            return Ok((fields, false, None, Some(refined)));
        }
        zero_init = false;
        let mut variants = Vec::new();
        let mut seen_names = HashSet::new();
        let mut residual_tag: Option<u32> = None;
        let mut extract_tag: Option<u32> = None;
        for mem in members {
            // Whole-arm marks (`Done!`, `Quit?`) and type-arg marks (`Hold<Int64!>`,
            // `Fail<String?>`) both count toward residual/extract tags (D118).
            let arg_extract = type_expr_has_arg_mark(mem, TypeMark::Extract);
            let arg_residual = type_expr_has_arg_mark(mem, TypeMark::Residual);
            let whole_extract = matches!(mem.mark, Some(TypeMark::Extract));
            let whole_residual = matches!(mem.mark, Some(TypeMark::Residual));
            let mark = mem.mark.or(if arg_extract {
                Some(TypeMark::Extract)
            } else if arg_residual {
                Some(TypeMark::Residual)
            } else {
                None
            });
            // Type-arg `!` → unwrap sole field; whole-arm `!` alone → yield arm type.
            let unwrap_payload = arg_extract;

            if (whole_residual || arg_residual) && residual_tag.is_some() {
                return Err(Error::at(
                    "union may have at most one residual mark (`?`)",
                    mem.span,
                ));
            }
            if (whole_extract || arg_extract) && extract_tag.is_some() {
                return Err(Error::at(
                    "union may have at most one extract mark (`!`)",
                    mem.span,
                ));
            }

            // Pure literal arm: `0 | 1 | "x" | true` (D118).
            if let Some(lit) = &mem.lit {
                let vname = match lit {
                    TypeLit::Int(n) => n.to_string(),
                    TypeLit::Bool(b) => b.to_string(),
                    TypeLit::String(s) => format!("\"{s}\""),
                };
                if !seen_names.insert(vname.clone()) {
                    return Err(Error::at(
                        format!("duplicate union member `{vname}`"),
                        mem.span,
                    ));
                }
                let tag = variants.len() as u32;
                if whole_residual || arg_residual {
                    residual_tag = Some(tag);
                }
                if whole_extract || arg_extract {
                    extract_tag = Some(tag);
                }
                variants.push(SumVariant {
                    tag,
                    name: vname,
                    ty: Ty::Void,
                    lit: Some(lit.clone()),
                    mark,
                    unwrap_payload: false,
                });
                continue;
            }
            let ty = resolve_ty(mem, &types_info, &name_map)?;
            let Ty::User(ref member_name) = ty else {
                return Err(Error::at(
                    format!("union member must be a named type or literal, got {ty:?}"),
                    mem.span,
                ));
            };
            // Flatten nested union aliases: `Outer = Inner | C` → members of Inner + C
            // (D118; transparent composition). Nested marks on the sum itself are rejected.
            if let Some(minfo) = types_info.get(member_name) {
                if let Some(inner_sum) = &minfo.sum {
                    if whole_extract || whole_residual || arg_extract || arg_residual {
                        return Err(Error::at(
                                format!(
                                    "cannot place extract marks on nested union member `{member_name}` (mark atoms instead)"
                                ),
                                mem.span,
                            ));
                    }
                    if inner_sum.residual_tag.is_some() || inner_sum.extract_tag.is_some() {
                        return Err(Error::at(
                                format!(
                                    "nested union `{member_name}` has residual/extract marks; flatten marks onto the outer union"
                                ),
                                mem.span,
                            ));
                    }
                    for iv in &inner_sum.variants {
                        let vname = iv.name.clone();
                        if !seen_names.insert(vname.clone()) {
                            return Err(Error::at(
                                    format!(
                                        "duplicate union member `{vname}` (from nested `{member_name}`)"
                                    ),
                                    mem.span,
                                ));
                        }
                        let nested_tag = variants.len() as u32;
                        variants.push(SumVariant {
                            tag: nested_tag,
                            name: vname,
                            ty: iv.ty.clone(),
                            lit: iv.lit.clone(),
                            mark: None,
                            unwrap_payload: iv.unwrap_payload,
                        });
                    }
                    continue;
                }
            }
            // Prefer source name from type expr for patterns.
            let vname = if mem.name.is_empty() {
                member_name.clone()
            } else {
                mem.name.clone()
            };
            if !seen_names.insert(vname.clone()) {
                return Err(Error::at(
                    format!("duplicate union member `{vname}`"),
                    mem.span,
                ));
            }
            // Product arm: scalars (incl. Float64 / D140), String/Bytes, User, Array, Map.
            if let Some(minfo) = types_info.get(member_name) {
                for f in &minfo.fields {
                    if !union_member_field_ok(&f.ty) {
                        return Err(Error::at(
                                format!(
                                    "union member `{vname}` field `{}` must be a scalar, String/Bytes, User, Array, or Map (got {:?})",
                                    f.name, f.ty
                                ),
                                mem.span,
                            ));
                    }
                }
                if unwrap_payload {
                    if minfo.fields.len() != 1 {
                        return Err(Error::at(
                                format!(
                                    "type-arg extract mark (`!`) on `{vname}` requires a single-field product"
                                ),
                                mem.span,
                            ));
                    }
                }
            } else if unwrap_payload {
                return Err(Error::at(
                    format!("type-arg extract mark (`!`) requires a product type, got `{vname}`"),
                    mem.span,
                ));
            }
            let tag = variants.len() as u32;
            if whole_residual || arg_residual {
                residual_tag = Some(tag);
            }
            if whole_extract || arg_extract {
                extract_tag = Some(tag);
            }
            variants.push(SumVariant {
                tag,
                name: vname,
                ty,
                lit: None,
                mark,
                unwrap_payload,
            });
        }
        sum_info = Some(SumInfo {
            variants,
            residual_tag,
            extract_tag,
        });
    } else if !td.fields.is_empty() {
        zero_init = true;
        for f in &td.fields {
            let ty = resolve_ty(&f.ty, &types_info, &name_map)?;
            if !is_abi_field_ty(&ty) {
                return Err(Error::at(
                    format!(
                        "C-layout field `{}` must be an ABI scalar or Array<T, N>, got {ty:?}",
                        f.name
                    ),
                    f.ty.span,
                ));
            }
            if fields.iter().any(|x| x.name == f.name) {
                return Err(Error::at(
                    format!("duplicate field `{}`", f.name),
                    f.name_span,
                ));
            }
            fields.push(FieldInfo {
                name: f.name.clone(),
                ty,
                mutable: f.mutable,
                default: None,
                positional: false,
                where_clause: None,
            });
        }
    } else {
        zero_init = false;
        let mut seen_default = false;
        for p in &td.params {
            let ty = resolve_ty(&p.ty, &types_info, &name_map)?;
            // D48/D135: scalars + heap + Array/Map + Node + constructor-param User
            // (including sums). C-layout and function types stay out.
            if !is_ctor_field_ty(&ty, types_info) {
                return Err(Error::at(
                    "constructor-param fields must be an integer type, Float64, Bool, String, Bytes, Node, Array, Map, or a constructor-param type",
                    p.ty.span,
                ));
            }
            if p.default.is_some() {
                seen_default = true;
            } else if seen_default {
                return Err(Error::at(
                        format!(
                            "constructor parameter `{}` without a default follows a defaulted parameter",
                            p.name
                        ),
                        p.name_span,
                    ));
            }
            fields.push(FieldInfo {
                name: p.name.clone(),
                ty,
                mutable: false,
                default: p.default.clone(),
                positional: p.positional,
                where_clause: p.ty.where_clause.clone(),
            });
        }
    }
    Ok((fields, zero_init, sum_info, None))
}

fn try_refined_alias(
    members: &[TypeExpr],
    td: &TypeDef,
    types_info: &HashMap<String, TypeInfo>,
    name_map: &HashMap<String, String>,
) -> Result<Option<RefinedInfo>> {
    if members.len() != 1 {
        return Ok(None);
    }
    let mem = &members[0];
    let extra = mem.where_clause.clone();
    let base_ty = resolve_ty(
        &TypeExpr {
            where_clause: None,
            ..mem.clone()
        },
        types_info,
        name_map,
    );
    let Ok(base_ty) = base_ty else {
        return Ok(None);
    };
    let (base, clause) = match &base_ty {
        Ty::Int64 | Ty::Float64 | Ty::String | Ty::Bool => {
            let Some(w) = extra else {
                return Ok(None);
            };
            (base_ty, w)
        }
        Ty::User(n) => {
            let Some(inner) = types_info.get(n).and_then(|i| i.refined.clone()) else {
                return Ok(None);
            };
            let clause = if let Some(w) = extra {
                crate::ast::WhereClause {
                    expr: crate::refine::and_clauses(&inner.clause.expr, &w.expr),
                    span: w.span,
                }
            } else {
                inner.clause
            };
            (inner.base, clause)
        }
        _ => return Ok(None),
    };
    crate::refine::check_templates(&clause)?;
    let base_name = match &base {
        Ty::Int64 => "Int64",
        Ty::Float64 => "Float64",
        Ty::String => "String",
        Ty::Bool => "Bool",
        _ => "value",
    };
    crate::refine::reject_empty(base_name, &clause, td.name_span)?;
    Ok(Some(RefinedInfo { base, clause }))
}

fn errors_to_multi(errors: Vec<Error>) -> Error {
    Error::multi(errors)
}

/// Inline User fields cannot form a cycle (C struct would be infinite).
/// `Array` / `Map` are pointers, so `type Tree(kids: Array<Tree>)` is fine.
fn user_field_cycle_errors(
    types: &HashMap<String, TypeInfo>,
    type_defs: &[(String, String, &TypeDef)],
) -> Vec<Error> {
    let span_of = |mangled: &str| -> Span {
        type_defs
            .iter()
            .find(|(_, name, _)| name == mangled)
            .map(|(_, _, td)| td.name_span)
            .unwrap_or_else(Span::dummy)
    };
    fn visit(
        name: &str,
        stack: &mut Vec<String>,
        types: &HashMap<String, TypeInfo>,
    ) -> Option<Vec<String>> {
        if let Some(i) = stack.iter().position(|s| s == name) {
            let mut cycle = stack[i..].to_vec();
            cycle.push(name.to_string());
            return Some(cycle);
        }
        let Some(info) = types.get(name) else {
            return None;
        };
        if info.zero_init {
            return None;
        }
        stack.push(name.to_string());
        for f in &info.fields {
            if let Ty::User(inner) = &f.ty {
                if let Some(c) = visit(inner, stack, types) {
                    stack.pop();
                    return Some(c);
                }
            }
        }
        if let Some(sum) = &info.sum {
            for v in &sum.variants {
                if let Ty::User(inner) = &v.ty {
                    if let Some(c) = visit(inner, stack, types) {
                        stack.pop();
                        return Some(c);
                    }
                }
            }
        }
        stack.pop();
        None
    }
    let mut errors = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for name in types.keys() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(cycle) = visit(name, &mut Vec::new(), types) {
            for n in &cycle {
                seen.insert(n.clone());
            }
            let shown = cycle.join(" → ");
            errors.push(Error::at(
                format!(
                    "constructor-param type cannot contain itself through fields ({shown}); use Array or Map for a recursive shape"
                ),
                span_of(&cycle[0]),
            ));
        }
    }
    errors
}

fn mangle_type(mod_id: &str, name: &str, is_entry: bool) -> String {
    if is_entry {
        name.to_string()
    } else {
        format!("{mod_id}_{name}")
    }
}

/// How to make `ty` conform to `traits`: a type parameter takes `bound` where
/// it is declared (D228 §5), any other type a conformance, written `how`.
fn conform_help(ty: &Ty, traits: &str, bound: &str, how: &str) -> String {
    let shown = display_ty(ty);
    if !is_param_archetype(ty) {
        format!("declare `conform {shown} to {traits}`: {how}")
    } else if shown.contains('.') {
        format!("bound it where it is declared: `where {shown}: {bound}`")
    } else {
        format!("bound it where it is declared: `<{shown}: {bound}>`")
    }
}

/// Whether `ty` is a type parameter's archetype (`Param_largest_T`), which
/// has only its bounds' members (D228 §5).
fn is_param_archetype(ty: &Ty) -> bool {
    matches!(ty, Ty::User(name) if name.starts_with("Param_") || name.contains("_Param_"))
}

/// "no field `rank` on `Card`"; for a trait's archetype, the trait has no such
/// member (D228).
fn no_member_message(type_mangled: &str, member: &str, kind: &str) -> String {
    let shown = display_ty(&Ty::User(type_mangled.to_string()));
    if type_mangled.starts_with("Self_") || type_mangled.contains("_Self_") {
        format!("`{shown}` has no member `{member}` (a trait's body sees only the trait)")
    } else if is_param_archetype(&Ty::User(type_mangled.to_string())) {
        format!("`{shown}` has no member `{member}` (generic code sees only its type parameters' bounds)")
    } else {
        format!("no {kind} `{member}` on `{shown}`")
    }
}

/// Construction goes through setters (D228): once the value `place` exists,
/// each field's setter runs on that field's argument.
fn run_constructor_setters(
    ctx: &mut LowerCtx,
    info: &TypeInfo,
    ty: &Ty,
    place: &Place,
    arguments: &[Place],
) -> Result<()> {
    let Ty::User(type_name) = ty else {
        return Ok(());
    };
    for (field, argument) in info.fields.iter().zip(arguments) {
        let setter = setter_name(&field.name);
        if !info.methods.contains_key(&setter) {
            continue;
        }
        let value = ctx.fresh("setarg");
        ctx.assign(value.clone(), field.ty.clone(), RValue::Use(argument.clone()));
        emit_call_result(
            ctx,
            Callee::Method {
                type_name: type_name.clone(),
                method: setter,
            },
            vec![place.clone(), value],
            Ty::Void,
        )?;
    }
    Ok(())
}

/// The union `trait_desugar` makes for `mixed T` (`Mixed_T`; no source name
/// has an `_`), D228 §6.
fn is_mixed(type_name: &str) -> bool {
    type_name.starts_with("Mixed_") || type_name.contains("_Mixed_")
}

/// `value.member(…)` on a `mixed T` (D228 §6, D235): a call to the
/// function made for the member once every body is lowered, which calls the
/// member of whichever type the value holds, on the value in place. Its
/// signature is the trait's; one that takes a `Self` cannot be called, and
/// one that returns a `Self` returns a `mixed T`.
#[allow(clippy::too_many_arguments)]
fn lower_mixed_call(
    ctx: &mut LowerCtx,
    receiver: Place,
    receiver_write: ReceiverWrite,
    mixed: &str,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    let shown = display_ty(&Ty::User(mixed.to_string()));
    if ctx
        .synthetics
        .borrow()
        .mixed_closed
        .get(mixed)
        .is_some_and(|members| members.contains(method))
    {
        return Err(Error::at(
            format!("`{method}` names an associated type, which each conforming type names for itself, so it cannot be called through {shown}"),
            span,
        ));
    }
    let sig = mixed_member_sig(ctx, mixed, method, span)?
        .ok_or_else(|| Error::at(format!("{shown} has no member `{method}`"), span))?;
    let mixed_ty = Ty::User(mixed.to_string());
    if sig.params.iter().any(|(_, ty)| *ty == mixed_ty) {
        return Err(Error::at(
            format!("`{method}` takes a `Self`, so it cannot be called through {shown}: the other side's type is unknown"),
            span,
        ));
    }
    let arg_places = resolve_call_args_full(
        ctx,
        &sig.params,
        &sig.defaults,
        &sig.mutable,
        &sig.positional,
        args,
        span,
        &sig.def_mod,
    )?;
    let function = request_mixed_function(
        ctx,
        mixed,
        MixedKind::Member {
            method: method.to_string(),
            sig: sig.clone(),
        },
    );
    let mut all_args = vec![Place {
        name: format!("&{}", receiver.name),
    }];
    all_args.extend(arg_places);
    let result = emit_call_result(ctx, Callee::User(function.clone()), all_args, sig.ret.clone())?;
    store_receiver(ctx, receiver_write, receiver, &[function], method, span);
    Ok(result)
}

/// The signature of `mixed`'s member `method`, from its trait's lineage,
/// resolved in the trait's module with `Self` the union; `None` when the
/// trait has no such member.
fn mixed_member_sig(ctx: &mut LowerCtx, mixed: &str, method: &str, span: Span) -> Result<Option<MethodSig>> {
    if let Some(sig) = ctx.synthetics.borrow().mixed_sigs.get(mixed).and_then(|sigs| sigs.get(method)) {
        return Ok(Some(sig.clone()));
    }
    let Some((trait_ref, members)) = ctx.synthetics.borrow().mixed_members.get(mixed).cloned() else {
        return Ok(None);
    };
    let Some(member) = members.iter().find(|member| member.name == method) else {
        return Ok(None);
    };
    let name_map = ctx
        .package_maps
        .get(&trait_ref.module)
        .map(|maps| with_self_type(maps.name_to_type.clone(), mixed))
        .ok_or_else(|| Error::at(format!("internal: no names for `{}`", trait_ref.module), span))?;
    let mut resolve = |te: &TypeExpr| -> Result<Ty> {
        generic::ensure_instances(ctx, te, &name_map)?;
        resolve_ty(te, &ctx.types, &name_map)
    };
    let mut params = Vec::new();
    for param in &member.params {
        params.push((param.name.clone(), resolve(&param.ty)?));
    }
    let ret = match &member.return_type {
        Some(te) => resolve(te)?,
        None => Ty::Void,
    };
    let sig = MethodSig {
        params,
        defaults: member.params.iter().map(|param| param.default.clone()).collect(),
        ret,
        exported: true,
        def_mod: trait_ref.module.clone(),
        multi_clause: false,
        mutable: member.params.iter().map(|param| param.mutable).collect(),
        positional: member.params.iter().map(|param| param.positional).collect(),
        requires: Vec::new(),
    };
    ctx.synthetics
        .borrow_mut()
        .mixed_sigs
        .entry(mixed.to_string())
        .or_default()
        .insert(method.to_string(), sig.clone());
    Ok(Some(sig))
}

/// The function for `kind` on `mixed` (D235), asked for once: its name.
fn request_mixed_function(ctx: &mut LowerCtx, mixed: &str, kind: MixedKind) -> String {
    let name = match &kind {
        MixedKind::Member { method, .. } => format!("__mixed_{mixed}_{method}"),
        MixedKind::Compare => format!("__mixed_{mixed}__compare"),
    };
    let mut synthetics = ctx.synthetics.borrow_mut();
    if synthetics.mixed_functions.iter().all(|function| function.name != name) {
        synthetics.mixed_functions.push(MixedFunction {
            name: name.clone(),
            mixed: mixed.to_string(),
            kind,
        });
    }
    name
}

/// Make each requested `mixed` function (D235) over its union's members
/// known now. `made` holds how many members each was made with; one made
/// with fewer is made again, replacing the old. Returns what was made.
#[allow(clippy::too_many_arguments)]
fn lower_mixed_functions(
    fns: &HashMap<String, FnSig>,
    types: &HashMap<String, TypeInfo>,
    package_maps: &HashMap<String, ModuleNameMaps>,
    module_exports: &HashMap<String, ModuleExports>,
    entry_mod_id: &str,
    synthetics: &Rc<RefCell<SyntheticRegistry>>,
    made: &mut HashMap<String, usize>,
) -> Vec<Result<Function>> {
    let requests = synthetics.borrow().mixed_functions.clone();
    let mut out = Vec::new();
    for request in requests {
        let mut seeded = types.clone();
        seed_synthetics(&mut seeded, &synthetics.borrow());
        let Some(info) = seeded.get(&request.mixed).cloned() else { continue };
        let variants: Vec<SumVariant> = info
            .sum
            .map(|sum| sum.variants)
            .unwrap_or_default()
            .into_iter()
            .filter(|variant| !mentions_archetype(&variant.ty))
            .collect();
        if made.get(&request.name) == Some(&variants.len()) {
            continue;
        }
        made.insert(request.name.clone(), variants.len());
        let module_id = Some(info.def_mod.clone())
            .filter(|module| package_maps.contains_key(module))
            .unwrap_or_else(|| entry_mod_id.to_string());
        let maps = &package_maps[&module_id];
        let ctx = LowerCtx::new(
            fns.clone(),
            seeded,
            maps.name_to_type.clone(),
            maps.name_to_fn.clone(),
            maps.name_to_global.clone(),
            maps.namespaces.clone(),
            module_exports.clone(),
            package_maps.clone(),
            Ty::Void,
            module_id,
            false,
            Rc::clone(synthetics),
        );
        out.push(lower_mixed_function(ctx, &request, &variants));
    }
    out
}

/// One `mixed` function's body: on the tag of `*self`, the member of the type
/// it holds, on the value in place (or, for `Compare`, the sign of `*self`
/// against `*other`).
fn lower_mixed_function(mut ctx: LowerCtx, request: &MixedFunction, variants: &[SumVariant]) -> Result<Function> {
    let span = Span::dummy();
    let mixed_ty = Ty::User(request.mixed.clone());
    let (params, mutable, ret) = match &request.kind {
        MixedKind::Member { sig, .. } => {
            let mutable = (0..sig.params.len()).map(|index| sig.mutable.get(index) == Some(&true)).collect();
            (sig.params.clone(), mutable, sig.ret.clone())
        }
        MixedKind::Compare => (vec![("other".to_string(), mixed_ty.clone())], vec![true], Ty::Int64),
    };
    ctx.fn_ret = ret.clone();
    bind_param(&mut ctx, "self", mixed_ty.clone(), true);
    for ((name, ty), is_mut) in params.iter().zip(&mutable) {
        bind_param(&mut ctx, name, ty.clone(), *is_mut);
    }
    let receiver = Place {
        name: format!("(*{})", sanitize_name("self")),
    };
    let tag = ctx.fresh("mixtag");
    ctx.assign(
        tag.clone(),
        Ty::Int64,
        RValue::SumTag {
            place: receiver.clone(),
            sum_type: request.mixed.clone(),
        },
    );
    let result = ctx.fresh("mixed");
    if ret != Ty::Void {
        init_place(&mut ctx, &result, &ret)?;
    }
    let payload = |place: &Place, variant: &SumVariant| Place {
        name: format!("{}.u.v{}", place.name, variant.tag),
    };
    match &request.kind {
        MixedKind::Member { method, sig } => {
            let args: Vec<Arg> = sig
                .params
                .iter()
                .enumerate()
                .map(|(index, (name, _))| {
                    let kind = if mutable[index] {
                        ExprKind::MutArg(name.clone())
                    } else {
                        ExprKind::Name(name.clone())
                    };
                    let value = Expr { kind, span };
                    if sig.positional.get(index) == Some(&true) {
                        Arg::Pos(value)
                    } else {
                        Arg::Named {
                            name: name.clone(),
                            name_span: span,
                            value,
                        }
                    }
                })
                .collect();
            for variant in variants {
                let this_arm = tag_is(&mut ctx, &tag, variant.tag);
                let outside = std::mem::take(&mut ctx.stmts);
                ctx.push_scope();
                let held = format!("__held{}", variant.tag);
                ctx.define_full(&held, variant.ty.clone(), true, payload(&receiver, variant), None);
                let lowered = lower_method_call(&mut ctx, &crate::ast_build::name(&held), method, span, &args);
                let (value, value_ty) = lowered.map_err(|error| {
                    error.with_help(format!("a {} holds a {}", display_ty(&mixed_ty), display_ty(&variant.ty)))
                })?;
                if ret != Ty::Void {
                    // A member that returns `Self` returns the mixed value.
                    let value = if value_ty == ret {
                        value
                    } else {
                        try_sum_coerce(&mut ctx, value, &value_ty, &ret, None, span)?.ok_or_else(|| {
                            Error::at(
                                format!(
                                    "`{method}` of {} returns {}, not {}",
                                    display_ty(&variant.ty),
                                    display_ty(&value_ty),
                                    display_ty(&ret)
                                ),
                                span,
                            )
                        })?
                    };
                    ctx.assign(result.clone(), ret.clone(), RValue::Use(value));
                }
                ctx.pop_scope();
                let then_body = std::mem::replace(&mut ctx.stmts, outside);
                ctx.stmts.push(Stmt::If {
                    cond: this_arm,
                    then_body,
                    else_body: Vec::new(),
                });
            }
        }
        MixedKind::Compare => {
            let other = Place {
                name: format!("(*{})", sanitize_name("other")),
            };
            let other_tag = ctx.fresh("mixtag");
            ctx.assign(
                other_tag.clone(),
                Ty::Int64,
                RValue::SumTag {
                    place: other.clone(),
                    sum_type: request.mixed.clone(),
                },
            );
            // Different types order by their names.
            let mut ranked: Vec<&SumVariant> = variants.iter().collect();
            ranked.sort_by(|a, b| a.name.cmp(&b.name));
            let mut ranks = Vec::new();
            for side in [&tag, &other_tag] {
                let rank = ctx.fresh("mixrank");
                ctx.assign(rank.clone(), Ty::Int64, RValue::Int(0));
                for (position, variant) in ranked.iter().enumerate() {
                    let this_arm = tag_is(&mut ctx, side, variant.tag);
                    let outside = std::mem::take(&mut ctx.stmts);
                    ctx.assign(rank.clone(), Ty::Int64, RValue::Int(position as i64));
                    let then_body = std::mem::replace(&mut ctx.stmts, outside);
                    ctx.stmts.push(Stmt::If {
                        cond: this_arm,
                        then_body,
                        else_body: Vec::new(),
                    });
                }
                ranks.push(rank);
            }
            ctx.assign(result.clone(), Ty::Int64, RValue::Int(0));
            for (op, sign) in [(BinaryOp::Lt, -1), (BinaryOp::Gt, 1)] {
                let differs = ctx.fresh("mixcmp");
                ctx.assign(
                    differs.clone(),
                    Ty::Bool,
                    RValue::Binary {
                        op,
                        left: ranks[0].clone(),
                        right: ranks[1].clone(),
                    },
                );
                let outside = std::mem::take(&mut ctx.stmts);
                ctx.assign(result.clone(), Ty::Int64, RValue::Int(sign));
                let then_body = std::mem::replace(&mut ctx.stmts, outside);
                ctx.stmts.push(Stmt::If {
                    cond: differs,
                    then_body,
                    else_body: Vec::new(),
                });
            }
            // The same type: its values' order.
            let same = ctx.fresh("mixsame");
            ctx.assign(
                same.clone(),
                Ty::Bool,
                RValue::Binary {
                    op: BinaryOp::Eq,
                    left: ranks[0].clone(),
                    right: ranks[1].clone(),
                },
            );
            let outside = std::mem::take(&mut ctx.stmts);
            for variant in variants {
                let this_arm = tag_is(&mut ctx, &tag, variant.tag);
                let before = std::mem::take(&mut ctx.stmts);
                let sign = order::lower_compare(&mut ctx, payload(&receiver, variant), payload(&other, variant), &variant.ty, span)?;
                ctx.assign(result.clone(), Ty::Int64, RValue::Use(sign));
                let then_body = std::mem::replace(&mut ctx.stmts, before);
                ctx.stmts.push(Stmt::If {
                    cond: this_arm,
                    then_body,
                    else_body: Vec::new(),
                });
            }
            let then_body = std::mem::replace(&mut ctx.stmts, outside);
            ctx.stmts.push(Stmt::If {
                cond: same,
                then_body,
                else_body: Vec::new(),
            });
        }
    }
    let ret_place = (ret != Ty::Void).then_some(result);
    let mut body = ctx.stmts;
    insert_function_drops(&mut body, &ret_place, &ret, &ctx.drop_places);
    let mut all_params = vec![(sanitize_name("self"), mixed_ty)];
    all_params.extend(params.iter().map(|(name, ty)| (sanitize_name(name), ty.clone())));
    let mut mut_params = vec![true];
    mut_params.extend(mutable);
    Ok(Function {
        name: request.name.clone(),
        is_main: false,
        receiver: None,
        params: all_params,
        mut_params,
        body,
        ret: ret_place,
        ret_ty: ret,
    })
}

/// Whether the tag in `tag` is `want`.
fn tag_is(ctx: &mut LowerCtx, tag: &Place, want: u32) -> Place {
    let wanted = ctx.fresh("mixarm");
    ctx.assign(wanted.clone(), Ty::Int64, RValue::Int(want as i64));
    let is = ctx.fresh("mixis");
    ctx.assign(
        is.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Eq,
            left: tag.clone(),
            right: wanted,
        },
    );
    is
}

/// The type `trait_desugar` makes to check a trait's provided bodies once
/// (`Self_Named`; no source name has an `_`).
fn is_archetype(td: &TypeDef) -> bool {
    td.name.starts_with("Self_")
}

/// The module whose names a method's signature and body see: its trait's for
/// a body a trait provides or a conformance's member written there (D238),
/// else its type's (D228).
fn method_scope<'a>(method: &'a Method, type_module: &'a str) -> &'a str {
    match (&method.provided, &method.scope_module) {
        (Some(provided), _) => provided.module.as_str(),
        (None, Some(module)) => module.as_str(),
        (None, None) => type_module,
    }
}

/// A module's type names, plus `Self` for the type whose method is resolved
/// (D228).
fn with_self_type(mut name_map: HashMap<String, String>, type_mangled: &str) -> HashMap<String, String> {
    name_map.insert("Self".to_string(), type_mangled.to_string());
    name_map
}

fn mangle_fn(mod_id: &str, name: &str, is_entry: bool) -> String {
    if is_entry {
        name.to_string()
    } else {
        format!("{mod_id}_{name}")
    }
}

fn mangle_global(mod_id: &str, name: &str) -> String {
    format!("g_{mod_id}_{name}")
}

fn attr_ty() -> Ty {
    Ty::User("Attr".into())
}

fn attr_array_ty() -> Ty {
    Ty::Array {
        elem: Box::new(attr_ty()),
    }
}

fn node_array_ty() -> Ty {
    Ty::Array {
        elem: Box::new(Ty::Node),
    }
}

fn field_info(name: &str, ty: Ty) -> FieldInfo {
    FieldInfo {
        name: name.into(),
        ty,
        mutable: false,
        default: None,
        positional: false,
        where_clause: None,
    }
}

/// Ambient `Attr` plus match-only `Element` / `Text` / `Fragment` field lists.
fn seed_html_types(types_info: &mut HashMap<String, TypeInfo>, type_layouts: &mut Vec<TypeLayout>) {
    let attr_fields = vec![
        field_info("name", Ty::String),
        field_info("value", Ty::String),
    ];
    types_info.insert(
        "Attr".into(),
        TypeInfo {
            fields: attr_fields.clone(),
            methods: HashMap::new(),
            zero_init: false,
            def_mod: String::new(),
            sum: None,
            refined: None,
            resource: false,
            traits: Vec::new(),
            derived: HashSet::new(),
            instance: None,
            templates: HashMap::new(),
        },
    );
    type_layouts.push(TypeLayout {
        name: "Attr".into(),
        fields: vec![
            FieldLayout {
                name: "name".into(),
                c_name: "name".into(),
                ty: Ty::String,
            },
            FieldLayout {
                name: "value".into(),
                c_name: "value".into(),
                ty: Ty::String,
            },
        ],
        sum: None,
        sum_scalar: None,
        has_drop: false,
        own_hash: false,
        own_equals: false,
    });
    types_info.insert(
        "Element".into(),
        TypeInfo {
            fields: vec![
                field_info("tag", Ty::String),
                field_info("attributes", attr_array_ty()),
                field_info("children", node_array_ty()),
            ],
            methods: HashMap::new(),
            zero_init: false,
            def_mod: String::new(),
            sum: None,
            refined: None,
            resource: false,
            traits: Vec::new(),
            derived: HashSet::new(),
            instance: None,
            templates: HashMap::new(),
        },
    );
    types_info.insert(
        "Text".into(),
        TypeInfo {
            fields: vec![field_info("text", Ty::String)],
            methods: HashMap::new(),
            zero_init: false,
            def_mod: String::new(),
            sum: None,
            refined: None,
            resource: false,
            traits: Vec::new(),
            derived: HashSet::new(),
            instance: None,
            templates: HashMap::new(),
        },
    );
    types_info.insert(
        "Fragment".into(),
        TypeInfo {
            fields: vec![field_info("children", node_array_ty())],
            methods: HashMap::new(),
            zero_init: false,
            def_mod: String::new(),
            sum: None,
            refined: None,
            resource: false,
            traits: Vec::new(),
            derived: HashSet::new(),
            instance: None,
            templates: HashMap::new(),
        },
    );
}

fn node_sum_info() -> SumInfo {
    SumInfo {
        variants: vec![
            SumVariant {
                tag: 0,
                name: "Element".into(),
                ty: Ty::User("Element".into()),
                lit: None,
                mark: None,
                unwrap_payload: false,
            },
            SumVariant {
                tag: 1,
                name: "Text".into(),
                ty: Ty::User("Text".into()),
                lit: None,
                mark: None,
                unwrap_payload: false,
            },
            SumVariant {
                tag: 2,
                name: "Fragment".into(),
                ty: Ty::User("Fragment".into()),
                lit: None,
                mark: None,
                unwrap_payload: false,
            },
        ],
        residual_tag: None,
        extract_tag: None,
    }
}

fn build_module_name_maps(
    m: &SourceModule,
    import_maps: &HashMap<String, HashMap<String, BoundImport>>,
    module_exports: &HashMap<String, ModuleExports>,
    all_globals: &HashMap<String, GlobalInfo>,
    per_mod_types: &HashMap<String, HashMap<String, String>>,
    fns: &HashMap<String, FnSig>,
) -> Result<(
    HashMap<String, String>,
    HashMap<String, String>,
    HashMap<String, GlobalInfo>,
    HashMap<String, String>,
)> {
    let mut name_to_type = per_mod_types.get(&m.id).cloned().unwrap_or_default();
    let mut name_to_fn: HashMap<String, String> = HashMap::new();
    let mut name_to_global: HashMap<String, GlobalInfo> = HashMap::new();
    let mut namespaces: HashMap<String, String> = HashMap::new();

    // Local functions
    for item in &m.ast.items {
        match item {
            Item::Function(f) => {
                let mangled = if m.is_entry && f.name == "main" {
                    "main".to_string()
                } else {
                    mangle_fn(&m.id, &f.name, m.is_entry)
                };
                name_to_fn.insert(f.name.clone(), mangled);
            }
            Item::Extern(e) => {
                let mangled = mangle_fn(&m.id, &e.name, m.is_entry);
                name_to_fn.insert(e.name.clone(), mangled);
            }
            Item::Global(g) => {
                let place = mangle_global(&m.id, &g.name);
                if let Some(info) = all_globals.get(&place) {
                    name_to_global.insert(g.name.clone(), info.clone());
                }
            }
            // `extend` blocks are free functions by now (D196).
            Item::Type(_) | Item::Import(_) | Item::Extend(_) | Item::Trait(_) | Item::Conform(_) => {}
        }
    }

    // Imports
    if let Some(imports) = import_maps.get(&m.id) {
        for (local, b) in imports {
            match b {
                BoundImport::Name {
                    from_module,
                    original,
                } => {
                    let exp = module_exports.get(from_module);
                    if let Some(t) = exp.and_then(|e| e.types.get(original)) {
                        name_to_type.insert(local.clone(), t.clone());
                    } else if let Some(f) = exp.and_then(|e| e.fns.get(original)) {
                        name_to_fn.insert(local.clone(), f.clone());
                    } else if let Some(info) = exp.and_then(|e| e.globals.get(original)) {
                        name_to_global.insert(local.clone(), info.clone());
                    }
                    // If not yet in exports (global type not filled), try type map from defining module
                    if !name_to_type.contains_key(local)
                        && !name_to_fn.contains_key(local)
                        && !name_to_global.contains_key(local)
                    {
                        if let Some(tmap) = per_mod_types.get(from_module) {
                            if let Some(t) = tmap.get(original) {
                                name_to_type.insert(local.clone(), t.clone());
                            }
                        }
                        let fmangled = mangle_fn(from_module, original, false);
                        if fns.contains_key(&fmangled) {
                            name_to_fn.insert(local.clone(), fmangled);
                        }
                    }
                }
                BoundImport::Namespace { from_module } => {
                    namespaces.insert(local.clone(), from_module.clone());
                }
            }
        }
    }

    Ok((name_to_type, name_to_fn, name_to_global, namespaces))
}

fn resolve_return_ty(
    f: &ast::Function,
    is_entry: bool,
    types: &HashMap<String, TypeInfo>,
    name_map: &HashMap<String, String>,
) -> Result<Ty> {
    let is_main = is_entry && f.name == "main";
    match f.return_type.as_ref() {
        // D129: unannotated functions, `main` too (D223), are inferred later;
        // placeholder Void until then.
        None => Ok(Ty::Void),
        Some(t) => {
            let ty = resolve_ty(t, types, name_map)?;
            if is_main && ty != Ty::Int64 {
                return Err(Error::at("`main` must return Int64", f.name_span));
            }
            Ok(ty)
        }
    }
}

// ── D131 multi-clause functions ────────────────────────────────────────────

fn merge_free_clauses(
    clauses: &[&ast::Function],
    is_entry: bool,
    mod_id: &str,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    name_map: &HashMap<String, String>,
) -> Result<(FnSig, Option<Vec<MultiClause>>)> {
    debug_assert!(!clauses.is_empty());
    if clauses.len() == 1 {
        let f = clauses[0];
        let ret = resolve_return_ty(f, is_entry, types, name_map)?;
        let mut params = Vec::new();
        let mut defaults = Vec::new();
        let mut mutable = Vec::new();
        let mut positional = Vec::new();
        for p in &f.params {
            params.push((p.name.clone(), resolve_ty(&p.ty, types, name_map)?));
            defaults.push(p.default.clone());
            mutable.push(p.mutable);
            positional.push(p.positional);
        }
        let mut seen_default = false;
        for (i, d) in defaults.iter().enumerate() {
            if d.is_some() {
                seen_default = true;
            } else if seen_default {
                return Err(Error::at(
                    format!(
                        "parameter `{}` without a default follows a defaulted parameter",
                        params[i].0
                    ),
                    f.params[i].name_span,
                ));
            }
        }
        return Ok((
            FnSig {
                params,
                defaults,
                ret,
                extern_symbol: None,
                mangled: String::new(), // filled by caller insert key
                def_mod: mod_id.to_string(),
                multi_clause: false,
                mutable,
                positional,
            },
            None,
        ));
    }

    if let Some(param) = clauses.iter().flat_map(|c| &c.params).find(|p| p.mutable) {
        return Err(Error::at(
            "a multi-clause function cannot take a `mut` parameter",
            param.name_span,
        ));
    }
    // Multi-clause: no defaults; equal arity; export consistency; any-type dispatch.
    if is_entry && clauses[0].name == "main" {
        return Err(Error::at(
            "multi-clause `main` is not supported",
            clauses[0].name_span,
        ));
    }

    let arity = clauses[0].params.len();
    let mut export_any = false;
    let mut export_all = true;
    let mut resolved: Vec<MultiClause> = Vec::new();

    for f in clauses {
        if f.params.len() != arity {
            return Err(Error::at(
                format!(
                    "multi-clause function `{}` clauses must have the same arity (found {} and {})",
                    f.name,
                    arity,
                    f.params.len()
                ),
                f.name_span,
            ));
        }
        if f.exported {
            export_any = true;
        } else {
            export_all = false;
        }
        for p in &f.params {
            if p.default.is_some() {
                return Err(Error::at(
                    format!(
                        "multi-clause function `{}` cannot use parameter defaults (clause `{}`)",
                        f.name, p.name
                    ),
                    p.name_span,
                ));
            }
        }
        let mut param_names = Vec::new();
        let mut param_tys = Vec::new();
        let mut positional = Vec::new();
        for p in &f.params {
            param_names.push(p.name.clone());
            param_tys.push(resolve_ty(&p.ty, types, name_map)?);
            positional.push(p.positional);
        }
        resolved.push(MultiClause {
            param_names,
            param_tys,
            positional,
            return_type: f.return_type.clone(),
            body: f.body.clone(),
            name_span: f.name_span,
            exported: f.exported,
        });
    }

    if export_any && !export_all {
        return Err(Error::at(
            format!(
                "multi-clause function `{}`: if any clause is `export`, all must be",
                clauses[0].name
            ),
            clauses[0].name_span,
        ));
    }

    // Positional `!` consistency.
    let pos0 = &resolved[0].positional;
    for c in &resolved[1..] {
        if &c.positional != pos0 {
            return Err(Error::at(
                format!(
                    "multi-clause function `{}`: positional `!` markers must match across clauses",
                    clauses[0].name
                ),
                c.name_span,
            ));
        }
    }

    // Domain cover / duplicate checks (source order).
    for i in 0..resolved.len() {
        for j in 0..i {
            if domain_equivalent(types, &resolved[j].param_tys, &resolved[i].param_tys) {
                return Err(Error::at(
                    format!(
                        "duplicate multi-clause for `{}` (equivalent parameter types)",
                        clauses[0].name
                    ),
                    resolved[i].name_span,
                ));
            }
            if domain_covers(types, &resolved[j].param_tys, &resolved[i].param_tys) {
                return Err(Error::at(
                    format!(
                        "unreachable multi-clause for `{}` (earlier clause covers this domain)",
                        clauses[0].name
                    ),
                    resolved[i].name_span,
                ));
            }
        }
    }

    // Merged domain: per-index join.
    let mut domain_params: Vec<(String, Ty)> = Vec::new();
    for i in 0..arity {
        let name = resolved[0].param_names[i].clone();
        let tys: Vec<Ty> = resolved.iter().map(|c| c.param_tys[i].clone()).collect();
        // Shared if all equivalent.
        let first = tys[0].clone();
        if tys.iter().all(|t| ty_eq(types, t, &first)) {
            domain_params.push((name, first));
        } else {
            let joined = join_domain_tys(types, layouts, &tys, resolved[0].name_span, mod_id)?;
            domain_params.push((name, joined));
        }
    }

    // Return type: placeholder Void; D129 / annotations applied later for free fns.
    // If every clause has an explicit `->`, join those annotations as an initial ret.
    let mut ret = Ty::Void;
    let mut has_ann = false;
    let mut ann_tys = Vec::new();
    for c in &resolved {
        if let Some(rt) = &c.return_type {
            has_ann = true;
            ann_tys.push(resolve_ty(rt, types, name_map)?);
        }
    }
    if has_ann {
        if ann_tys.len() == resolved.len() {
            ret = ann_tys[0].clone();
            for t in &ann_tys[1..] {
                ret = join_return_tys(types, layouts, &ret, t, resolved[0].name_span, mod_id)?;
            }
        }
        // Mixed annotate/unannotate: leave Void for D129-style probe of merged body.
    }

    Ok((
        FnSig {
            params: domain_params,
            defaults: vec![None; arity],
            ret,
            extern_symbol: None,
            mangled: String::new(),
            def_mod: mod_id.to_string(),
            multi_clause: true,
            mutable: Vec::new(),
            positional: clauses[0].params.iter().map(|p| p.positional).collect(),
        },
        Some(resolved),
    ))
}

fn merge_method_clauses(
    clauses: &[&Method],
    mod_id: &str,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    name_map: &HashMap<String, String>,
) -> Result<(MethodSig, Option<Vec<MultiClause>>)> {
    debug_assert!(!clauses.is_empty());
    if clauses.len() == 1 {
        let m = clauses[0];
        let ret = match m.return_type.as_ref() {
            Some(t) => resolve_ty(t, types, name_map)?,
            None => Ty::Void,
        };
        let mut params = Vec::new();
        let mut defaults = Vec::new();
        let mut mutable = Vec::new();
        let mut positional = Vec::new();
        for p in &m.params {
            params.push((p.name.clone(), resolve_ty(&p.ty, types, name_map)?));
            defaults.push(p.default.clone());
            mutable.push(p.mutable);
            positional.push(p.positional);
        }
        let mut seen_default = false;
        for (i, d) in defaults.iter().enumerate() {
            if d.is_some() {
                seen_default = true;
            } else if seen_default {
                return Err(Error::at(
                    format!(
                        "parameter `{}` without a default follows a defaulted parameter",
                        params[i].0
                    ),
                    m.params[i].name_span,
                ));
            }
        }
        return Ok((
            MethodSig {
                params,
                defaults,
                ret,
                exported: m.exported,
                def_mod: mod_id.to_string(),
                multi_clause: false,
                mutable,
                positional,
                requires: Vec::new(),
            },
            None,
        ));
    }

    if let Some(param) = clauses.iter().flat_map(|c| &c.params).find(|p| p.mutable) {
        return Err(Error::at(
            "a multi-clause function cannot take a `mut` parameter",
            param.name_span,
        ));
    }
    let arity = clauses[0].params.len();
    let mut export_any = false;
    let mut export_all = true;
    let mut resolved: Vec<MultiClause> = Vec::new();

    for m in clauses {
        if m.params.len() != arity {
            return Err(Error::at(
                format!(
                    "multi-clause method `{}` clauses must have the same arity (found {} and {})",
                    m.name,
                    arity,
                    m.params.len()
                ),
                m.name_span,
            ));
        }
        if m.exported {
            export_any = true;
        } else {
            export_all = false;
        }
        for p in &m.params {
            if p.default.is_some() {
                return Err(Error::at(
                    format!(
                        "multi-clause method `{}` cannot use parameter defaults",
                        m.name
                    ),
                    p.name_span,
                ));
            }
        }
        let mut param_names = Vec::new();
        let mut param_tys = Vec::new();
        let mut positional = Vec::new();
        for p in &m.params {
            param_names.push(p.name.clone());
            param_tys.push(resolve_ty(&p.ty, types, name_map)?);
            positional.push(p.positional);
        }
        resolved.push(MultiClause {
            param_names,
            param_tys,
            positional,
            return_type: m.return_type.clone(),
            body: m.body.clone(),
            name_span: m.name_span,
            exported: m.exported,
        });
    }

    if export_any && !export_all {
        return Err(Error::at(
            format!(
                "multi-clause method `{}`: if any clause is `export`, all must be",
                clauses[0].name
            ),
            clauses[0].name_span,
        ));
    }

    let pos0 = &resolved[0].positional;
    for c in &resolved[1..] {
        if &c.positional != pos0 {
            return Err(Error::at(
                format!(
                    "multi-clause method `{}`: positional `!` markers must match across clauses",
                    clauses[0].name
                ),
                c.name_span,
            ));
        }
    }

    for i in 0..resolved.len() {
        for j in 0..i {
            if domain_equivalent(types, &resolved[j].param_tys, &resolved[i].param_tys) {
                return Err(Error::at(
                    format!(
                        "duplicate multi-clause for method `{}` (equivalent parameter types)",
                        clauses[0].name
                    ),
                    resolved[i].name_span,
                ));
            }
            if domain_covers(types, &resolved[j].param_tys, &resolved[i].param_tys) {
                return Err(Error::at(
                    format!(
                        "unreachable multi-clause for method `{}` (earlier clause covers this domain)",
                        clauses[0].name
                    ),
                    resolved[i].name_span,
                ));
            }
        }
    }

    let mut domain_params: Vec<(String, Ty)> = Vec::new();
    for i in 0..arity {
        let name = resolved[0].param_names[i].clone();
        let first = resolved[0].param_tys[i].clone();
        if resolved
            .iter()
            .all(|c| ty_eq(types, &c.param_tys[i], &first))
        {
            domain_params.push((name, first));
        } else {
            let tys: Vec<Ty> = resolved.iter().map(|c| c.param_tys[i].clone()).collect();
            let joined = join_domain_tys(types, layouts, &tys, resolved[0].name_span, mod_id)?;
            domain_params.push((name, joined));
        }
    }

    // Methods: default Void unless all clauses annotate (then join).
    let mut ret = Ty::Void;
    let mut ann_tys = Vec::new();
    for c in &resolved {
        if let Some(rt) = &c.return_type {
            ann_tys.push(resolve_ty(rt, types, name_map)?);
        }
    }
    if ann_tys.len() == resolved.len() && !ann_tys.is_empty() {
        ret = ann_tys[0].clone();
        for t in &ann_tys[1..] {
            ret = join_return_tys(types, layouts, &ret, t, resolved[0].name_span, mod_id)?;
        }
    } else if !ann_tys.is_empty() {
        // Some annotated: join annotated + Void for unannotated → keep join of annotated only
        // (bodies still checked against local expects during lower).
        ret = ann_tys[0].clone();
        for t in &ann_tys[1..] {
            ret = join_return_tys(types, layouts, &ret, t, resolved[0].name_span, mod_id)?;
        }
    }

    Ok((
        MethodSig {
            params: domain_params,
            defaults: vec![None; arity],
            ret,
            exported: export_any,
            def_mod: mod_id.to_string(),
            multi_clause: true,
            mutable: Vec::new(),
            positional: clauses[0].params.iter().map(|p| p.positional).collect(),
            requires: Vec::new(),
        },
        Some(resolved),
    ))
}

/// True when `general` covers `specific` for multi-clause reachability (D131).
fn ty_covers(types: &HashMap<String, TypeInfo>, general: &Ty, specific: &Ty) -> bool {
    if ty_eq(types, general, specific) {
        return true;
    }
    // Wide bases cover their literal singletons / pure-lit sums.
    if matches!(general, Ty::String) && is_pure_lit_sum_of_kind(types, specific, LitKind::String) {
        return true;
    }
    if matches!(general, Ty::Int64) && is_pure_lit_sum_of_kind(types, specific, LitKind::Int) {
        return true;
    }
    if matches!(general, Ty::Bool) && is_pure_lit_sum_of_kind(types, specific, LitKind::Bool) {
        return true;
    }
    // Specific injects into general (subset sum / atom into union).
    can_coerce_ty(types, specific, general)
}

fn domain_covers(types: &HashMap<String, TypeInfo>, earlier: &[Ty], later: &[Ty]) -> bool {
    earlier.len() == later.len()
        && earlier
            .iter()
            .zip(later)
            .all(|(e, l)| ty_covers(types, e, l))
}

fn domain_equivalent(types: &HashMap<String, TypeInfo>, a: &[Ty], b: &[Ty]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| ty_eq(types, x, y))
}

#[derive(Clone, Copy)]
enum LitKind {
    Int,
    Bool,
    String,
}

fn is_pure_lit_sum_of_kind(types: &HashMap<String, TypeInfo>, ty: &Ty, kind: LitKind) -> bool {
    let Some(sum) = sum_info_of(types, ty) else {
        return false;
    };
    if sum.variants.is_empty() {
        return false;
    }
    sum.variants.iter().all(|v| match (&v.lit, kind) {
        (Some(TypeLit::Int(_)), LitKind::Int) => true,
        (Some(TypeLit::Bool(_)), LitKind::Bool) => true,
        (Some(TypeLit::String(_)), LitKind::String) => true,
        _ => false,
    })
}

fn join_domain_tys(
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    tys: &[Ty],
    span: Span,
    def_mod: &str,
) -> Result<Ty> {
    let mut acc = tys[0].clone();
    for t in &tys[1..] {
        if ty_eq(types, &acc, t) {
            continue;
        }
        if ty_covers(types, &acc, t) {
            continue; // acc already general
        }
        if ty_covers(types, t, &acc) {
            acc = t.clone();
            continue;
        }
        // Member-set join when both are union-compatible.
        if collect_union_members(types, &acc).is_some() && collect_union_members(types, t).is_some()
        {
            acc = ensure_member_set_type(types, layouts, &[acc.clone(), t.clone()], span, def_mod)?;
            continue;
        }
        // Wide + lit already handled by ty_covers; remaining → error.
        return Err(Error::at(
            format!(
                "cannot join multi-clause parameter types {} and {} (annotate or simplify)",
                display_ty(&acc),
                display_ty(t)
            ),
            span,
        ));
    }
    Ok(acc)
}

fn join_return_tys(
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    a: &Ty,
    b: &Ty,
    span: Span,
    def_mod: &str,
) -> Result<Ty> {
    if ty_eq(types, a, b) {
        return Ok(a.clone());
    }
    if is_never(a) {
        return Ok(b.clone());
    }
    if is_never(b) {
        return Ok(a.clone());
    }
    if collect_union_members(types, a).is_some() && collect_union_members(types, b).is_some() {
        return ensure_member_set_type(types, layouts, &[a.clone(), b.clone()], span, def_mod);
    }
    // Scalar lub not supported for multi-clause returns without annotation.
    Err(Error::at(
        format!(
            "cannot join multi-clause return types {} and {}; annotate each clause with compatible `->`",
            display_ty(a),
            display_ty(b)
        ),
        span,
    ))
}

// ── D129 return-type inference ─────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeParamRole {
    Success,
    Residual,
    /// Appears in both marks, or unmarked only — cannot collapse.
    Ambiguous,
}

fn type_param_role(param: &str, members: &[TypeExpr]) -> TypeParamRole {
    let mut success = false;
    let mut residual = false;
    for m in members {
        walk_type_param_marks(m, param, &mut success, &mut residual);
    }
    match (success, residual) {
        (true, false) => TypeParamRole::Success,
        (false, true) => TypeParamRole::Residual,
        (false, false) | (true, true) => TypeParamRole::Ambiguous,
    }
}

/// Walk union member type exprs; note `!` / `?` marks on type-param leaves (`P!`, `F?`).
fn walk_type_param_marks(te: &TypeExpr, param: &str, success: &mut bool, residual: &mut bool) {
    if te.name == param && te.args.is_empty() {
        match te.mark {
            Some(TypeMark::Extract) => *success = true,
            Some(TypeMark::Residual) => *residual = true,
            None => {}
        }
    }
    for arg in &te.args {
        if let TypeArg::Type(inner) = arg {
            walk_type_param_marks(inner, param, success, residual);
        }
    }
    if let Some(members) = &te.union_members {
        for m in members {
            walk_type_param_marks(m, param, success, residual);
        }
    }
    if let Some(ft) = &te.fn_type {
        for (_, p) in &ft.params {
            walk_type_param_marks(p, param, success, residual);
        }
        walk_type_param_marks(&ft.ret, param, success, residual);
    }
}

fn residual_extract_of_sum(
    types: &HashMap<String, TypeInfo>,
    sum: &SumInfo,
    span: Span,
) -> Result<Ty> {
    let tag = sum.residual_tag.ok_or_else(|| {
        Error::at(
            "`?` requires a residual mark (`?`) on one union member",
            span,
        )
    })?;
    let v = sum
        .variants
        .iter()
        .find(|v| v.tag == tag)
        .ok_or_else(|| Error::at("internal: bad residual tag", span))?;
    if v.unwrap_payload {
        match &v.ty {
            Ty::User(n) => {
                let fields = types.get(n).map(|i| i.fields.as_slice()).unwrap_or(&[]);
                if fields.len() == 1 {
                    Ok(fields[0].ty.clone())
                } else if fields.is_empty() {
                    Ok(Ty::Void)
                } else {
                    Err(Error::at(
                        "type-arg residual extract requires a single-field product arm",
                        span,
                    ))
                }
            }
            other => Ok(other.clone()),
        }
    } else {
        match &v.ty {
            Ty::User(n) => {
                let fields = types.get(n).map(|i| i.fields.as_slice()).unwrap_or(&[]);
                if fields.is_empty() {
                    Ok(Ty::Void)
                } else if fields.len() == 1 {
                    Ok(fields[0].ty.clone())
                } else {
                    Ok(v.ty.clone())
                }
            }
            Ty::Void => Ok(Ty::Void),
            other => Ok(other.clone()),
        }
    }
}

/// Residual `?` exit contribution: success type-args → Void, residual extract kept (D129).
fn collapse_residual_exit(
    types: &mut HashMap<String, TypeInfo>,
    generics: &HashMap<String, GenericDef>,
    layouts: &mut Vec<TypeLayout>,
    scrut_ty: &Ty,
    span: Span,
) -> Result<Ty> {
    let Ty::User(mono) = scrut_ty else {
        return Err(Error::at(
            "`?` expects a Trial, May, or residual-marked union value",
            span,
        ));
    };
    let sum = types.get(mono).and_then(|i| i.sum.clone()).ok_or_else(|| {
        Error::at(
            "`?` expects a Trial, May, or residual-marked union value",
            span,
        )
    })?;
    if sum.residual_tag.is_none() {
        return Err(Error::at(
            "`?` requires a residual mark (`?`) on one union member",
            span,
        ));
    }
    let f_ty = residual_extract_of_sum(types, &sum, span)?;

    // Prefer generic residual union monomorph: Void success params, residual = F.
    let mut bases: Vec<(&String, &GenericDef)> = generics
        .iter()
        .filter(|(b, g)| is_mono_of_base(b, mono) && matches!(g.kind, GenericKind::Union { .. }))
        .collect();
    // Longest base prefix wins (more specific mangled base).
    bases.sort_by_key(|(b, _)| std::cmp::Reverse(b.len()));
    if let Some((base, gdef)) = bases.first() {
        if let GenericKind::Union { members, .. } = &gdef.kind {
            let mut args = Vec::with_capacity(gdef.type_params.len());
            for p in &gdef.type_params {
                match type_param_role(p, members) {
                    TypeParamRole::Success => args.push(Ty::Void),
                    TypeParamRole::Residual => args.push(f_ty.clone()),
                    TypeParamRole::Ambiguous => {
                        return Err(Error::at(
                            format!(
                                "cannot infer return type from `?` on `{mono}` \
                                 (type parameter `{p}` is not a clear success/residual slot); \
                                 add an explicit `->` annotation"
                            ),
                            span,
                        ));
                    }
                }
            }
            let collapsed = mono_type_name(base, &args);
            ensure_union_mono(types, layouts, generics, base, &args, &gdef.def_mod, span)?;
            return Ok(Ty::User(collapsed));
        }
    }
    // Non-generic residual union: whole type.
    Ok(scrut_ty.clone())
}

fn ensure_union_mono(
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    generics: &HashMap<String, GenericDef>,
    base: &str,
    args: &[Ty],
    def_mod: &str,
    span: Span,
) -> Result<()> {
    let mono = mono_type_name(base, args);
    if types.contains_key(&mono) {
        return Ok(());
    }
    // Name map: identity for monomorph resolve of nested builtins.
    let name_map = HashMap::new();
    let pending = vec![(base.to_string(), args.to_vec(), name_map, span)];
    let mut ignore_methods = Vec::new();
    monomorphize_pending(pending, types, layouts, generics, &AssocTables::default(), &mut ignore_methods)?;
    if !types.contains_key(&mono) {
        // Retry with def_mod empty map still — seed may need Fallible inners only.
        let _ = def_mod;
        return Err(Error::at(
            format!(
                "cannot materialize monomorph `{mono}` for return-type inference; \
                 add an explicit `->` annotation"
            ),
            span,
        ));
    }
    Ok(())
}

fn unify_infer_tys(tys: &[Ty], span: Span) -> Result<Ty> {
    let mut acc = Ty::Never;
    for t in tys {
        acc = match (&acc, t) {
            (Ty::Never, x) => x.clone(),
            (x, Ty::Never) => x.clone(),
            (x, y) if x == y => x.clone(),
            (x, y) => {
                return Err(Error::at(
                    format!(
                        "cannot infer a single return type from exits ({x:?} vs {y:?}); \
                         add an explicit `->` annotation"
                    ),
                    span,
                ));
            }
        };
    }
    if is_never(&acc) {
        Ok(Ty::Void)
    } else {
        Ok(acc)
    }
}

fn refine_residual_with_success(
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    generics: &HashMap<String, GenericDef>,
    residual_ty: &Ty,
    success_ty: &Ty,
    span: Span,
) -> Result<Ty> {
    if is_never(success_ty) || *success_ty == Ty::Void {
        return Ok(residual_ty.clone());
    }
    let Ty::User(mono) = residual_ty else {
        return unify_infer_tys(&[residual_ty.clone(), success_ty.clone()], span);
    };
    let sum = match types.get(mono).and_then(|i| i.sum.clone()) {
        Some(s) if s.residual_tag.is_some() => s,
        _ => return unify_infer_tys(&[residual_ty.clone(), success_ty.clone()], span),
    };
    let f_ty = residual_extract_of_sum(types, &sum, span)?;

    let mut bases: Vec<(&String, &GenericDef)> = generics
        .iter()
        .filter(|(b, g)| is_mono_of_base(b, mono) && matches!(g.kind, GenericKind::Union { .. }))
        .collect();
    bases.sort_by_key(|(b, _)| std::cmp::Reverse(b.len()));
    if let Some((base, gdef)) = bases.first() {
        if let GenericKind::Union { members, .. } = &gdef.kind {
            let mut args = Vec::with_capacity(gdef.type_params.len());
            for p in &gdef.type_params {
                match type_param_role(p, members) {
                    TypeParamRole::Success => args.push(success_ty.clone()),
                    TypeParamRole::Residual => args.push(f_ty.clone()),
                    TypeParamRole::Ambiguous => {
                        return Err(Error::at(
                            "cannot refine inferred residual return type; add an explicit `->` annotation",
                            span,
                        ));
                    }
                }
            }
            let refined = mono_type_name(base, &args);
            ensure_union_mono(types, layouts, generics, base, &args, &gdef.def_mod, span)?;
            return Ok(Ty::User(refined));
        }
    }
    unify_infer_tys(&[residual_ty.clone(), success_ty.clone()], span)
}

fn combine_inferred_return(
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    generics: &HashMap<String, GenericDef>,
    residuals: &[Ty],
    values: &[Ty],
    successes: &[Ty],
    span: Span,
) -> Result<Ty> {
    let values: Vec<Ty> = values.iter().filter(|t| !is_never(t)).cloned().collect();
    if residuals.is_empty() {
        return unify_infer_tys(&values, span);
    }
    let mut r = unify_infer_tys(residuals, span)?;
    if !values.is_empty() {
        let s = unify_infer_tys(&values, span)?;
        // Full residual-union value exits (e.g. `return Pass(…)`) unify with residual
        // collapses — do not treat the whole Trial as a success *payload*.
        if !is_residual_union(types, &s) {
            let mut plain = values;
            plain.extend(successes.iter().cloned());
            let s = unify_infer_tys(&plain, span)?;
            return refine_residual_with_success(types, layouts, generics, &r, &s, span);
        }
        r = unify_infer_tys(&[r, s], span)?;
    }
    if successes.is_empty() {
        return Ok(r);
    }
    // `x?` then `Pass(value)`: the payload is the success type.
    let s = unify_infer_tys(successes, span)?;
    refine_residual_with_success(types, layouts, generics, &r, &s, span)
}

/// What a body calls, to order return inference (D129 / D174).
#[derive(Default)]
struct CallNames {
    /// Free functions, by the name the body uses.
    functions: HashSet<String>,
    /// Methods, by name alone: the receiver's type is not known before lowering.
    methods: HashSet<String>,
}

/// Functions and methods `block` calls, by name.
fn collect_local_fn_calls(block: &Block, out: &mut CallNames) {
    for it in &block.items {
        match it {
            BlockItem::Let { init, .. } => collect_local_fn_calls_expr(init, out),
            BlockItem::Assign { value, .. } => collect_local_fn_calls_expr(value, out),
            BlockItem::Expr(e) => collect_local_fn_calls_expr(e, out),
        }
    }
    if let Some(t) = &block.tail {
        collect_local_fn_calls_expr(t, out);
    }
}

fn collect_local_fn_calls_expr(expr: &Expr, out: &mut CallNames) {
    match &expr.kind {
        ExprKind::Call { callee, args, .. } | ExprKind::Start { callee, args, .. } => {
            out.functions.insert(callee.clone());
            for a in args {
                match a {
                    Arg::Pos(e) | Arg::Named { value: e, .. } | Arg::Spread { value: e, .. } => {
                        collect_local_fn_calls_expr(e, out)
                    }
                }
            }
        }
        ExprKind::IndirectCall { callee, args } => {
            collect_local_fn_calls_expr(callee, out);
            for a in args {
                match a {
                    Arg::Pos(e) | Arg::Named { value: e, .. } | Arg::Spread { value: e, .. } => {
                        collect_local_fn_calls_expr(e, out)
                    }
                }
            }
        }
        ExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            out.methods.insert(method.clone());
            collect_local_fn_calls_expr(receiver, out);
            for a in args {
                match a {
                    Arg::Pos(e) | Arg::Named { value: e, .. } | Arg::Spread { value: e, .. } => {
                        collect_local_fn_calls_expr(e, out)
                    }
                }
            }
        }
        ExprKind::Field { base, .. } => collect_local_fn_calls_expr(base, out),
        ExprKind::Index { base, index } => {
            collect_local_fn_calls_expr(base, out);
            collect_local_fn_calls_expr(index, out);
        }
        ExprKind::Unary { expr: inner, .. } => collect_local_fn_calls_expr(inner, out),
        ExprKind::Binary { left, right, .. } => {
            collect_local_fn_calls_expr(left, out);
            collect_local_fn_calls_expr(right, out);
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_local_fn_calls_expr(cond, out);
            collect_local_fn_calls(then_branch, out);
            if let Some(e) = else_branch {
                collect_local_fn_calls(e, out);
            }
        }
        ExprKind::While { cond, body } => {
            collect_local_fn_calls_expr(cond, out);
            collect_local_fn_calls(body, out);
        }
        ExprKind::Loop { body } => collect_local_fn_calls(body, out),
        ExprKind::For {
            start, end, body, ..
        } => {
            collect_local_fn_calls_expr(start, out);
            if let Some(e) = end {
                collect_local_fn_calls_expr(e, out);
            }
            collect_local_fn_calls(body, out);
        }
        ExprKind::Return { value } => {
            if let Some(v) = value {
                collect_local_fn_calls_expr(v, out);
            }
        }
        ExprKind::Try(inner) | ExprKind::Cast { expr: inner, .. } => {
            collect_local_fn_calls_expr(inner, out)
        }
        ExprKind::Block(b) => collect_local_fn_calls(b, out),
        ExprKind::Match { scrutinee, arms } => {
            collect_local_fn_calls_expr(scrutinee, out);
            for arm in arms {
                collect_local_fn_calls(&arm.body, out);
                if let Some(g) = &arm.guard {
                    collect_local_fn_calls_expr(g, out);
                }
            }
        }
        ExprKind::ArrayLit(elems) => {
            for e in elems {
                collect_local_fn_calls_expr(e, out);
            }
        }
        ExprKind::MapLit(entries) => {
            for e in entries.iter().flat_map(|entry| entry.exprs()) {
                collect_local_fn_calls_expr(e, out);
            }
        }
        ExprKind::Interpolate(parts) | ExprKind::TaggedInterp { parts, .. } => {
            for p in parts {
                if let InterpPart::Expr(e) = p {
                    collect_local_fn_calls_expr(e, out);
                }
            }
        }
        ExprKind::Int(_) | ExprKind::BigInt(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Name(_)
        | ExprKind::MutArg(_)
        | ExprKind::Break
        | ExprKind::Continue => {}
    }
}

fn infer_unannotated_returns(
    fns: &mut HashMap<String, FnSig>,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    generics: &HashMap<String, GenericDef>,
    free_fns: &[(String, String, &ast::Function, bool)],
    multi_free: &HashMap<String, Vec<MultiClause>>,
    type_defs: &[(String, String, &TypeDef)],
    multi_methods: &HashMap<(String, String), Vec<MultiClause>>,
    mono_method_jobs: &[MonoMethodJob],
    package_maps: &HashMap<String, ModuleNameMaps>,
    module_exports: &HashMap<String, ModuleExports>,
    synthetics: &Rc<RefCell<SyntheticRegistry>>,
    package: &Package,
) -> Result<()> {
    let module_is_entry = |mod_id: &str| {
        package
            .modules
            .iter()
            .any(|module| module.id == mod_id && module.is_entry)
    };
    // A generic type's methods are probed per instance, on bodies with the
    // type arguments substituted (as `lower_method` lowers them).
    let mono_methods: Vec<(&MonoMethodJob, Method)> = mono_method_jobs
        .iter()
        .filter(|job| {
            !job.conversion && job.method.return_type.is_none() && job_applies(types, synthetics, job)
        })
        .map(|job| (job, subst_method(&job.method, &job.subst)))
        .collect();

    // Candidates: unannotated free functions (`main` too, D223) and, since
    // D174, unannotated methods (`drop` is always Void).
    let mut candidates: Vec<InferCandidate> = Vec::new();
    for (mod_id, mangled, f, is_entry) in free_fns {
        // D131: a multi-clause function joins its clauses' types; one whose
        // clauses all annotate was joined when it was merged.
        if let Some(clauses) = multi_free.get(mangled) {
            if clauses.iter().all(|c| c.return_type.is_some()) {
                continue;
            }
            candidates.push(InferCandidate {
                module_id: mod_id.clone(),
                is_entry: *is_entry,
                key: mangled.clone(),
                shown: f.name.clone(),
                name_span: f.name_span,
                bodies: clauses.iter().map(|clause| &clause.body).collect(),
                target: InferTarget::Multi {
                    clauses,
                    method: None,
                },
            });
            continue;
        }
        if f.return_type.is_some() {
            continue;
        }
        candidates.push(InferCandidate {
            module_id: mod_id.clone(),
            is_entry: *is_entry,
            key: mangled.clone(),
            shown: f.name.clone(),
            name_span: f.name_span,
            bodies: vec![&f.body],
            target: InferTarget::Free(f),
        });
    }
    for (job, method) in &mono_methods {
        candidates.push(InferCandidate {
            module_id: job.def_mod.clone(),
            is_entry: module_is_entry(&job.def_mod),
            key: format!("{}__{}", job.mono_type, method.name),
            shown: format!("{}.{}", display_ty(&Ty::User(job.mono_type.clone())), method.name),
            name_span: method.name_span,
            bodies: vec![&method.body],
            target: InferTarget::Method {
                type_mangled: job.mono_type.clone(),
                method,
            },
        });
    }
    for (type_mod, type_mangled, td) in type_defs {
        let mut seen: HashSet<&str> = HashSet::new();
        for method in td.methods.iter().filter(|method| method.generic.is_empty()) {
            if !seen.insert(method.name.as_str()) {
                continue;
            }
            let mod_id = &method_scope(method, type_mod).to_string();
            let is_entry = module_is_entry(mod_id);
            let key = format!("{type_mangled}__{}", method.name);
            let shown = format!("{}.{}", td.name, method.name);
            // A multi-clause method joins its clauses, as a function does (D211).
            if let Some(clauses) = multi_methods.get(&(type_mangled.clone(), method.name.clone())) {
                if clauses.iter().all(|c| c.return_type.is_some()) {
                    continue;
                }
                candidates.push(InferCandidate {
                    module_id: mod_id.clone(),
                    is_entry,
                    key,
                    shown,
                    name_span: method.name_span,
                    bodies: clauses.iter().map(|clause| &clause.body).collect(),
                    target: InferTarget::Multi {
                        clauses,
                        method: Some(type_mangled.clone()),
                    },
                });
                continue;
            }
            if method.return_type.is_some() {
                continue;
            }
            candidates.push(InferCandidate {
                module_id: mod_id.clone(),
                is_entry,
                key,
                shown,
                name_span: method.name_span,
                bodies: vec![&method.body],
                target: InferTarget::Method {
                    type_mangled: type_mangled.clone(),
                    method,
                },
            });
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }

    // Probe callees first, so most bodies are probed once. A method call
    // matches every candidate method of that name (the receiver's type is
    // not known yet), so this order is a guess; the rounds below decide.
    let index_of: HashMap<String, usize> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.key.clone(), index))
        .collect();
    let mut methods_named: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let name = match &candidate.target {
            InferTarget::Method { method, .. } => Some(method.name.clone()),
            InferTarget::Multi {
                method: Some(type_mangled),
                ..
            } => candidate
                .key
                .strip_prefix(&format!("{type_mangled}__"))
                .map(str::to_string),
            _ => None,
        };
        if let Some(name) = name {
            methods_named.entry(name).or_default().push(index);
        }
    }
    let callees: Vec<Vec<usize>> = candidates
        .iter()
        .map(|candidate| {
            let maps = package_maps.get(&candidate.module_id).unwrap();
            let mut names = CallNames::default();
            for body in &candidate.bodies {
                collect_local_fn_calls(body, &mut names);
            }
            let mut found: Vec<usize> = names
                .functions
                .iter()
                .filter_map(|name| maps.name_to_fn.get(name))
                .filter_map(|mangled| index_of.get(mangled).copied())
                .collect();
            for name in &names.methods {
                found.extend(methods_named.get(name).into_iter().flatten().copied());
            }
            // Inside a method a bare call may be a sibling (D208).
            let owner = match &candidate.target {
                InferTarget::Method { type_mangled, .. } => Some(type_mangled),
                InferTarget::Multi { method, .. } => method.as_ref(),
                InferTarget::Free(_) => None,
            };
            if let Some(type_mangled) = owner {
                for name in &names.functions {
                    found.extend(index_of.get(&format!("{type_mangled}__{name}")).copied());
                }
            }
            found.sort_unstable();
            found.dedup();
            found
        })
        .collect();
    let mut order = Vec::with_capacity(candidates.len());
    let mut visited = vec![false; candidates.len()];
    for root in 0..candidates.len() {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        let mut stack = vec![(root, 0usize)];
        while let Some((node, next)) = stack.pop() {
            match callees[node].get(next) {
                Some(&callee) => {
                    stack.push((node, next + 1));
                    if !visited[callee] {
                        visited[callee] = true;
                        stack.push((callee, 0));
                    }
                }
                None => order.push(node),
            }
        }
    }

    // Rounds: a probe that reaches a candidate still unresolved waits for the
    // next round. A round where every probe waits leaves only cycles and the
    // bodies that wait on them.
    let mut infer_errors: Vec<Error> = Vec::new();
    let mut unresolved: HashSet<String> = candidates.iter().map(|c| c.key.clone()).collect();
    let mut pending = order;
    loop {
        let mut waiting = Vec::new();
        let mut waits_on: HashMap<usize, usize> = HashMap::new();
        let mut progressed = false;
        for index in pending {
            let candidate = &candidates[index];
            let maps = package_maps.get(&candidate.module_id).unwrap();
            let mut blocked = unresolved.clone();
            blocked.remove(&candidate.key);
            let waited: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
            let probe = ProbeEnv {
                fns: &*fns,
                generics,
                maps,
                module_exports,
                package_maps,
                module_id: &candidate.module_id,
                is_entry: candidate.is_entry,
                synthetics,
                key: &candidate.key,
                blocked: &blocked,
                waited: &waited,
            };
            let outcome = match &candidate.target {
                InferTarget::Free(f) => probe_function_return(f, &probe, types, layouts),
                InferTarget::Method {
                    type_mangled,
                    method,
                } => probe_method_return(type_mangled, method, &probe, types, layouts),
                InferTarget::Multi { clauses, method } => {
                    probe_multi_return(clauses, method.as_deref(), &probe, types, layouts)
                }
            };
            let waited_on = waited.borrow().clone();
            if let Some(callee) = waited_on {
                waiting.push(index);
                waits_on.insert(index, index_of[&callee]);
                continue;
            }
            progressed = true;
            unresolved.remove(&candidate.key);
            match outcome {
                Ok(ret) => match &candidate.target {
                    InferTarget::Free(_) | InferTarget::Multi { method: None, .. } => {
                        if let Some(sig) = fns.get_mut(&candidate.key) {
                            sig.ret = ret;
                        }
                    }
                    InferTarget::Multi {
                        method: Some(type_mangled),
                        ..
                    } => {
                        let name = &candidate.key[type_mangled.len() + 2..];
                        if let Some(sig) = types
                            .get_mut(type_mangled)
                            .and_then(|info| info.methods.get_mut(name))
                        {
                            sig.ret = ret;
                        }
                    }
                    InferTarget::Method {
                        type_mangled,
                        method,
                    } => {
                        if let Some(sig) = types
                            .get_mut(type_mangled)
                            .and_then(|info| info.methods.get_mut(&method.name))
                        {
                            sig.ret = ret;
                        }
                    }
                },
                Err(e) => infer_errors.push(in_module_id(e, package, &candidate.module_id)),
            }
        }
        if waiting.is_empty() {
            break;
        }
        if !progressed {
            // Each body left waits on another body left: following the waits
            // from any of them ends in a cycle. Name the bodies on cycles.
            let mut on_cycle: HashSet<usize> = HashSet::new();
            for &start in &waiting {
                let mut path = Vec::new();
                let mut node = start;
                while !path.contains(&node) {
                    path.push(node);
                    node = waits_on[&node];
                }
                let entry = node;
                loop {
                    on_cycle.insert(node);
                    node = waits_on[&node];
                    if node == entry {
                        break;
                    }
                }
            }
            for index in waiting.iter().filter(|index| on_cycle.contains(index)) {
                let candidate = &candidates[*index];
                let error = Error::at(
                    format!(
                        "cannot infer return type of `{}` (recursive or mutually recursive); \
                         add an explicit `->` annotation",
                        candidate.shown
                    ),
                    candidate.name_span,
                );
                infer_errors.push(in_module_id(error, package, &candidate.module_id));
            }
            break;
        }
        pending = waiting;
    }

    // D223: an unannotated `main` returns the exit status, `Int64`, or
    // nothing (it then exits 0).
    if let Some((mod_id, _, f, _)) = free_fns
        .iter()
        .find(|(_, _, f, is_entry)| *is_entry && f.name == "main" && f.return_type.is_none())
    {
        let ret = fns.get("main").map(|sig| sig.ret.clone()).unwrap_or(Ty::Void);
        if !matches!(ret, Ty::Int64 | Ty::Void | Ty::Never) {
            let error = Error::at(
                format!(
                    "`main` ends with a {} value: it returns its exit status (`Int64`) or nothing",
                    display_ty(&ret)
                ),
                f.name_span,
            )
            .with_help("end `main` with a status such as `0`, or with a statement");
            infer_errors.push(in_module_id(error, package, mod_id));
        }
    }
    if !infer_errors.is_empty() {
        return Err(errors_to_multi(infer_errors));
    }
    Ok(())
}

/// A function or method whose return type is inferred (D129 / D174).
struct InferCandidate<'a> {
    module_id: String,
    is_entry: bool,
    /// Mangled name of a free function, or `Type__method`.
    key: String,
    /// The name diagnostics show: `area`, or `Box.area`.
    shown: String,
    name_span: Span,
    /// Every body it has (a multi-clause one has one per clause), for the
    /// call order.
    bodies: Vec<&'a Block>,
    target: InferTarget<'a>,
}

enum InferTarget<'a> {
    Free(&'a ast::Function),
    Method {
        type_mangled: String,
        method: &'a Method,
    },
    /// A multi-clause function, or method of `method` (D131, D211).
    Multi {
        clauses: &'a [MultiClause],
        method: Option<String>,
    },
}

/// What a return-type probe lowers against.
struct ProbeEnv<'a> {
    fns: &'a HashMap<String, FnSig>,
    generics: &'a HashMap<String, GenericDef>,
    maps: &'a ModuleNameMaps,
    module_exports: &'a HashMap<String, ModuleExports>,
    package_maps: &'a HashMap<String, ModuleNameMaps>,
    module_id: &'a str,
    is_entry: bool,
    synthetics: &'a Rc<RefCell<SyntheticRegistry>>,
    /// The candidate being probed.
    key: &'a str,
    /// The other candidates not inferred yet.
    blocked: &'a HashSet<String>,
    /// Set when the probe reaches a blocked candidate (or its own key).
    waited: &'a Rc<RefCell<Option<String>>>,
}

/// Probe each multi-clause body and join exit types (D131 + D129). A clause
/// with `-> T` contributes `T`; a method's clauses see its fields. Probed in
/// the rounds, so a clause that calls a candidate not inferred yet waits.
fn probe_multi_return(
    clauses: &[MultiClause],
    method_type: Option<&str>,
    env: &ProbeEnv,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
) -> Result<Ty> {
    let mut joined: Option<Ty> = None;
    for clause in clauses {
        let clause_ret = match &clause.return_type {
            Some(rt) => resolve_ty(rt, types, &env.maps.name_to_type)?,
            None => {
                let mut ctx = probe_ctx(env, types);
                if let Some(type_mangled) = method_type {
                    bind_method_fields(&mut ctx, type_mangled);
                }
                for (name, ty) in clause.param_names.iter().zip(clause.param_tys.iter()) {
                    let place = Place {
                        name: sanitize_name(name),
                    };
                    ctx.define(name, ty.clone(), false, place);
                }
                probe_body_return(ctx, &clause.body, clause.name_span, env.generics, types, layouts)?
            }
        };
        joined = Some(match joined {
            None => clause_ret,
            Some(prev) => join_return_tys(
                types,
                layouts,
                &prev,
                &clause_ret,
                clause.name_span,
                env.module_id,
            )?,
        });
    }
    Ok(joined.unwrap_or(Ty::Void))
}

/// A lowering context that collects exit types instead of building MIR.
fn probe_ctx(env: &ProbeEnv, types: &HashMap<String, TypeInfo>) -> LowerCtx {
    let mut ctx = LowerCtx::new(
        env.fns.clone(),
        types.clone(),
        env.maps.name_to_type.clone(),
        env.maps.name_to_fn.clone(),
        env.maps.name_to_global.clone(),
        env.maps.namespaces.clone(),
        env.module_exports.clone(),
        env.package_maps.clone(),
        Ty::Void,
        env.module_id.to_string(),
        env.is_entry,
        Rc::clone(env.synthetics),
    );
    ctx.inferring = true;
    ctx.infer_self = Some(env.key.to_string());
    ctx.infer_blocked = env.blocked.clone();
    ctx.infer_waited = Rc::clone(env.waited);
    ctx.generics = env.generics.clone();
    ctx
}

/// Lower `body` in a probe context and combine its exits into one type.
fn probe_body_return(
    mut ctx: LowerCtx,
    body: &Block,
    name_span: Span,
    generics: &HashMap<String, GenericDef>,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
) -> Result<Ty> {
    // Probe body; capture exits. Errors (bare Pass/Fail, etc.) surface here.
    let _ = lower_block_value(&mut ctx, body, &Ty::Void)?;

    // Merge any monomorphs / synthetics created during probe back.
    for (name, info) in ctx.types.iter() {
        if !types.contains_key(name) {
            types.insert(name.clone(), info.clone());
        }
    }
    for (name, (info, layout)) in ctx.synthetics.borrow().entries.iter() {
        if !types.contains_key(name) {
            types.insert(name.clone(), info.clone());
            layouts.push(layout.clone());
        }
    }

    combine_inferred_return(
        types,
        layouts,
        generics,
        &ctx.infer_residuals,
        &ctx.infer_values,
        &ctx.infer_successes,
        name_span,
    )
}

fn probe_function_return(
    f: &ast::Function,
    env: &ProbeEnv,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
) -> Result<Ty> {
    let mut ctx = probe_ctx(env, types);
    for p in &f.params {
        let ty = resolve_ty(&p.ty, &ctx.types, &ctx.name_to_type)?;
        bind_param(&mut ctx, &p.name, ty, p.mutable);
    }
    probe_body_return(ctx, &f.body, f.name_span, env.generics, types, layouts)
}

/// D174: a method's body sees its fields and parameters as `lower_method`
/// binds them.
fn probe_method_return(
    type_mangled: &str,
    method: &Method,
    env: &ProbeEnv,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
) -> Result<Ty> {
    let mut ctx = probe_ctx(env, types);
    define_method_scope(&mut ctx, type_mangled, method)?;
    probe_body_return(
        ctx,
        &method.body,
        method.name_span,
        env.generics,
        types,
        layouts,
    )
}

fn resolve_ty(
    t: &TypeExpr,
    types: &HashMap<String, TypeInfo>,
    name_map: &HashMap<String, String>,
) -> Result<Ty> {
    // `mixed T`: the union of `T`'s conforming types (D228 §6).
    if t.name == ast::MIXED {
        let trait_name = match t.args.first() {
            Some(TypeArg::Type(arg)) => arg.name.clone(),
            _ => String::new(),
        };
        return match name_map.get(&format!("{}:{trait_name}", ast::MIXED)) {
            Some(mangled) => Ok(Ty::User(mangled.clone())),
            None => Err(Error::at(format!("unknown trait `{trait_name}`"), t.span)),
        };
    }
    if let Some(ft) = &t.fn_type {
        let mut params = Vec::new();
        for (_, p) in &ft.params {
            params.push(resolve_ty(p, types, name_map)?);
        }
        let ret = resolve_ty(&ft.ret, types, name_map)?;
        return Ok(Ty::Fn {
            params,
            ret: Box::new(ret),
        });
    }
    // Inline member-set `A | B` (D131) — must be pre-materialized.
    if let Some(members) = &t.union_members {
        if members.is_empty() {
            return Err(Error::at("empty union type", t.span));
        }
        if members.len() == 1 {
            return resolve_ty(&members[0], types, name_map);
        }
        let mut mem_tys = Vec::new();
        for m in members {
            mem_tys.push(resolve_ty(m, types, name_map)?);
        }
        let name = member_set_type_name(types, &mem_tys);
        if types.contains_key(&name) {
            return Ok(Ty::User(name));
        }
        return Err(Error::at(
            "internal: inline union type was not materialized",
            t.span,
        ));
    }
    // Bare singleton literal type (D131).
    if let Some(lit) = &t.lit {
        let name = lit_type_name(lit);
        if types.contains_key(&name) {
            return Ok(Ty::User(name));
        }
        return Err(Error::at(
            format!(
                "internal: literal type `{}` was not materialized",
                display_type_lit(lit)
            ),
            t.span,
        ));
    }
    match (t.name.as_str(), t.args.as_slice()) {
        ("Int32", []) => Ok(Ty::Int32),
        ("Int64", []) => Ok(Ty::Int64),
        ("UInt32", []) => Ok(Ty::UInt32),
        ("UInt64", []) => Ok(Ty::UInt64),
        ("Byte" | "UInt8", []) => Ok(Ty::Byte),
        ("Float64", []) => Ok(Ty::Float64),
        ("Bool", []) => Ok(Ty::Bool),
        ("Void", []) => Ok(Ty::Void),
        ("String", []) => Ok(Ty::String),
        ("Bytes", []) => Ok(Ty::Bytes),
        ("Buffer", []) => Ok(Ty::Buffer),
        ("Node" | "Element" | "Text" | "Fragment", []) => Ok(Ty::Node),
        ("Attr", []) => Ok(Ty::User("Attr".into())),
        // May / Trial are std/fallible generic unions (D118) — resolved via monomorphs.
        ("Array", [TypeArg::Type(elem)]) => {
            let et = resolve_ty(elem, types, name_map)?;
            if !is_array_elem_ty(&et) {
                return Err(Error::at(
                    format!(
                        "Array element type not yet supported: {et:?} (Int64, Float64, Bool, String, Bytes, User)"
                    ),
                    elem.span,
                ));
            }
            if array_elem_is_user(&et) && !is_array_user_elem(&et, types) {
                return Err(Error::at(
                    format!(
                        "Array element User type must be constructor-param with Int64/Bool/String/Bytes/Node/Array/Map/User fields: {et:?}"
                    ),
                    elem.span,
                ));
            }
            Ok(Ty::Array { elem: Box::new(et) })
        }
        ("Map", [TypeArg::Type(k), TypeArg::Type(v)]) => {
            let kt = resolve_ty(k, types, name_map)?;
            let vt = resolve_ty(v, types, name_map)?;
            if !is_map_key_ty(&kt) {
                return Err(Error::at(
                    format!(
                        "Map key type not supported: {} (String, Int64, Float64, Bool, or a user type)",
                        display_ty(&kt)
                    ),
                    k.span,
                ));
            }
            if !is_map_val_ty(&vt) {
                return Err(Error::at(
                    format!("Map value type not yet supported: {vt:?}"),
                    v.span,
                ));
            }
            if array_elem_is_user(&vt) && !is_array_user_elem(&vt, types) {
                return Err(Error::at(
                    format!(
                        "Map value User type must be constructor-param with Int64/Bool/String/Bytes/Node/Array/Map/User fields: {vt:?}"
                    ),
                    v.span,
                ));
            }
            Ok(Ty::Map {
                key: Box::new(kt),
                val: Box::new(vt),
            })
        }
        ("Map", _) => Err(Error::at("Map expects Map<K, V>", t.span)),
        (
            "Array",
            [TypeArg::Type(elem), TypeArg::Int {
                value: n,
                span: nspan,
            }],
        ) => {
            if *n < 0 {
                return Err(Error::at("Array length must be non-negative", *nspan));
            }
            let et = resolve_ty(elem, types, name_map)?;
            if !is_abi_scalar(&et) {
                return Err(Error::at(
                    format!("fixed Array element must be an ABI scalar, got {et:?}"),
                    elem.span,
                ));
            }
            Ok(Ty::FixedArray {
                elem: Box::new(et),
                len: *n as u64,
            })
        }
        ("Array", _) => Err(Error::at(
            "Array expects Array<T> (dynamic) or Array<T, N> (fixed field)",
            t.span,
        )),
        ("Address", [TypeArg::Type(inner)]) => {
            let pointee = resolve_ty(inner, types, name_map)?;
            match &pointee {
                Ty::User(_) => Ok(Ty::Address(Box::new(pointee))),
                other => Err(Error::at(
                    format!("Address pointee must be a user struct, got {other:?}"),
                    inner.span,
                )),
            }
        }
        ("Address", _) => Err(Error::at("Address expects one type argument", t.span)),
        // A substitution's type is resolved already, wherever it is read.
        (name, []) if name.starts_with(ast::RESOLVED_TYPE) => {
            let mangled = &name[ast::RESOLVED_TYPE.len_utf8()..];
            if types.contains_key(mangled) {
                Ok(Ty::User(mangled.to_string()))
            } else {
                Err(unknown_type_error(mangled, t.span))
            }
        }
        (name, []) => {
            let mangled = name_map
                .get(name)
                .cloned()
                .unwrap_or_else(|| name.to_string());
            if types.contains_key(&mangled) {
                Ok(Ty::User(mangled))
            } else if types.contains_key(name) {
                Ok(Ty::User(name.to_string()))
            } else {
                Err(unknown_type_error(name, t.span))
            }
        }
        // Type application: monomorph (`Box<Int64>`) or identity mark app (`Hold<Int64!>`).
        (name, args) if !args.is_empty() => {
            let mangled = name_map
                .get(name)
                .cloned()
                .unwrap_or_else(|| name.to_string());
            // Prefer monomorph if already materialised (`Box__Int64`).
            let mut arg_tys = Vec::new();
            let mut all_type_args = true;
            for arg in args {
                match arg {
                    TypeArg::Type(arg_te) => {
                        arg_tys.push(resolve_ty(arg_te, types, name_map)?);
                    }
                    TypeArg::Int { .. } => {
                        all_type_args = false;
                        break;
                    }
                }
            }
            if all_type_args && !arg_tys.is_empty() {
                let mono = mono_type_name(&mangled, &arg_tys);
                if types.contains_key(&mono) {
                    return Ok(Ty::User(mono));
                }
                // Not visible in this module at all: a plain unknown type.
                if !name_map.contains_key(name)
                    && !types.contains_key(&mangled)
                    && !types.contains_key(name)
                {
                    return Err(unknown_type_error(name, t.span));
                }
                // Generic base with no mono yet — should have been eager-monomorphized.
                if !types.contains_key(&mangled) && !types.contains_key(name) {
                    return Err(Error::at(
                        format!(
                            "generic type `{name}` is not instantiated for these arguments (internal monomorph missing: {mono})"
                        ),
                        t.span,
                    ));
                }
            }
            // Identity monomorphic product app for type-arg marks: `Hold<Int64!>` ≡ `Hold`.
            let key = if types.contains_key(&mangled) {
                mangled
            } else if types.contains_key(name) {
                name.to_string()
            } else {
                return Err(Error::at(format!("unknown type `{name}`"), t.span));
            };
            let info = types.get(&key).unwrap();
            if info.sum.is_some() {
                return Err(Error::at(
                    format!("type application on union `{name}` is not supported"),
                    t.span,
                ));
            }
            if info.fields.len() != args.len() {
                return Err(Error::at(
                    format!(
                        "type `{name}` has {} field(s), but {} type argument(s) were given",
                        info.fields.len(),
                        args.len()
                    ),
                    t.span,
                ));
            }
            for (i, arg) in args.iter().enumerate() {
                let TypeArg::Type(arg_te) = arg else {
                    return Err(Error::at(
                        "product type application expects type arguments (not const ints)",
                        t.span,
                    ));
                };
                let arg_ty = resolve_ty(arg_te, types, name_map)?;
                if arg_ty != info.fields[i].ty {
                    return Err(Error::at(
                        format!(
                            "type argument {} for `{name}`: expected {:?}, found {arg_ty:?}",
                            i + 1,
                            info.fields[i].ty
                        ),
                        arg_te.span,
                    ));
                }
            }
            Ok(Ty::User(key))
        }
        (other, _) => Err(Error::at(format!("unknown type `{other}`"), t.span)),
    }
}

fn mono_type_name(base: &str, args: &[Ty]) -> String {
    let mut s = base.to_string();
    for a in args {
        s.push_str("__");
        s.push_str(&ty_mono_suffix(a));
    }
    s
}

fn ty_mono_suffix(ty: &Ty) -> String {
    match ty {
        Ty::Void => "Void".into(),
        Ty::Never => "Never".into(),
        Ty::Int32 => "Int32".into(),
        Ty::Int64 => "Int64".into(),
        Ty::UInt32 => "UInt32".into(),
        Ty::UInt64 => "UInt64".into(),
        Ty::Byte => "Byte".into(),
        Ty::Float64 => "Float64".into(),
        Ty::Bool => "Bool".into(),
        Ty::String => "String".into(),
        Ty::Bytes => "Bytes".into(),
        Ty::Buffer => "Buffer".into(),
        Ty::Node => "Node".into(),
        Ty::User(n) => n.replace(['-', '$'], "_"),
        Ty::Array { elem } => format!("Array_{}", ty_mono_suffix(elem)),
        Ty::Map { key, val } => format!("Map_{}_{}", ty_mono_suffix(key), ty_mono_suffix(val)),
        Ty::FixedArray { elem, len } => format!("Fixed_{}_{len}", ty_mono_suffix(elem)),
        Ty::Address(inner) => format!("Addr_{}", ty_mono_suffix(inner)),
        Ty::Fn { params, ret } => {
            let ps: Vec<String> = params.iter().map(ty_mono_suffix).collect();
            format!("Fn_{}_{}", ps.join("_"), ty_mono_suffix(ret))
        }
    }
}

fn is_mono_of_base(base: &str, mono: &str) -> bool {
    mono == base || mono.starts_with(&format!("{base}__"))
}

/// Pick monomorph for a type constructor call from an expected type.
/// Returns `Some(mono)` or `None` if `base` is already a concrete type name.
fn resolve_ctor_mono(
    ctx: &LowerCtx,
    base: &str,
    source_name: &str,
    expect: Option<&Ty>,
    span: Span,
) -> Result<Option<String>> {
    if ctx.types.contains_key(base) {
        return Ok(None); // concrete type
    }
    // Generic: need expect to select monomorph.
    let Some(exp) = expect else {
        return Err(Error::at(
            format!(
                "generic type `{source_name}` requires a type annotation (e.g. let x: {source_name}<Int64> = …)"
            ),
            span,
        ));
    };
    // Direct monomorph expect: Box__Int64
    if let Ty::User(n) = exp {
        if is_mono_of_base(base, n) && ctx.types.contains_key(n) {
            return Ok(Some(n.clone()));
        }
        // Expect is a union containing a monomorph of this product. A
        // `mixed` value may hold several, so its arguments decide (D235).
        if let Some(sum) = ctx.types.get(n).and_then(|i| i.sum.as_ref()).filter(|_| !is_mixed(n)) {
            for v in &sum.variants {
                if let Ty::User(pn) = &v.ty {
                    if is_mono_of_base(base, pn) && ctx.types.contains_key(pn) {
                        return Ok(Some(pn.clone()));
                    }
                }
            }
        }
    }
    Err(Error::at(
        format!("cannot construct generic `{source_name}` for expected type {exp:?}"),
        span,
    ))
}

/// Ensure May/Trial monomorphs used by inference and Bytes.find exist.
fn seed_fallible_monos(
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    generics: &HashMap<String, GenericDef>,
    per_mod_types: &HashMap<String, HashMap<String, String>>,
) -> Result<()> {
    // Seed monomorphs for every module that imported (or defined) May/Trial.
    let span = Span::new(0, 0);
    let inners = [Ty::Int64, Ty::String, Ty::Bool, Ty::Bytes, Ty::Void];
    let mut pending = Vec::new();
    for name_map in per_mod_types.values() {
        if let Some(may_base) = name_map.get("May") {
            for inn in &inners {
                pending.push((may_base.clone(), vec![inn.clone()], name_map.clone(), span));
            }
        }
        if let Some(trial_base) = name_map.get("Trial") {
            for ok in &inners {
                for err in &inners {
                    pending.push((
                        trial_base.clone(),
                        vec![ok.clone(), err.clone()],
                        name_map.clone(),
                        span,
                    ));
                }
            }
        }
    }
    let mut ignore_methods = Vec::new();
    monomorphize_pending(pending, types, layouts, generics, &AssocTables::default(), &mut ignore_methods)?;
    Ok(())
}

fn monomorphize_pending(
    mut pending: Vec<(String, Vec<Ty>, HashMap<String, String>, Span)>,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    generics: &HashMap<String, GenericDef>,
    tables: &AssocTables,
    mono_methods: &mut Vec<MonoMethodJob>,
) -> Result<()> {
    let mut guard = 0;
    // Products made, whose methods' signatures resolve once every instance
    // they name is made.
    let mut made: Vec<(String, String, HashMap<String, Ty>, HashMap<String, String>)> = Vec::new();
    while !pending.is_empty() {
        guard += 1;
        if guard > 10_000 {
            return Err(Error::new(
                "internal: monomorphization did not converge (too many iterations)",
            ));
        }
        let (base, arg_tys, name_map, span) = pending.remove(0);
        let mono = mono_type_name(&base, &arg_tys);
        if types.contains_key(&mono) {
            continue;
        }
        let Some(gdef) = generics.get(&base) else {
            continue;
        };
        let name_map = if gdef.name_map.is_empty() { name_map } else { gdef.name_map.clone() };
        if gdef.type_params.len() != arg_tys.len() {
            return Err(Error::at(
                format!(
                    "type `{}` expects {} type argument(s), found {}",
                    gdef.source_name,
                    gdef.type_params.len(),
                    arg_tys.len()
                ),
                span,
            ));
        }
        let mut subst: HashMap<String, Ty> = HashMap::new();
        for (p, ty) in gdef.type_params.iter().zip(arg_tys.iter()) {
            subst.insert(p.clone(), ty.clone());
        }
        // `S.Item` (D237): a checking instance's archetype, else read off
        // the argument.
        match gdef.checking.iter().find(|(args, ..)| *args == arg_tys) {
            Some((_, _, derived)) => subst.extend(derived.iter().map(|(name, ty)| (name.clone(), ty.clone()))),
            None => {
                derived_subst(gdef, &mut subst, types, generics, tables, &mut pending, span)?;
            }
        }
        match &gdef.kind {
            GenericKind::Product {
                params,
                methods,
                conversions,
            } => {
                instantiate_product_mono(
                    mono.clone(),
                    gdef,
                    params,
                    methods,
                    conversions,
                    &subst,
                    types,
                    layouts,
                    &name_map,
                    generics,
                    &mut pending,
                    span,
                )?;
                made.push((mono, base.clone(), subst, name_map));
            }
            GenericKind::Union { members, methods } => {
                instantiate_union_mono(
                    mono.clone(),
                    gdef,
                    members,
                    &subst,
                    types,
                    layouts,
                    &name_map,
                    generics,
                    &mut pending,
                    span,
                )?;
                // Its members' signatures name instances to make first.
                for method in methods.iter().filter(|method| method.generic.is_empty()) {
                    for te in method_sig_type_exprs(method) {
                        let te = subst_type_expr(te, &subst);
                        enqueue_type_app_mono(&te, types, &name_map, generics, &mut pending, span);
                    }
                }
                made.push((mono, base.clone(), subst, name_map));
            }
        }
    }
    for (mono, base, subst, name_map) in made {
        let gdef = &generics[&base];
        let (methods, conversions): (&[Method], &[ast::Conversion]) = match &gdef.kind {
            GenericKind::Product { methods, conversions, .. } => (methods, conversions),
            GenericKind::Union { methods, .. } => (methods, &[]),
        };
        instantiate_product_methods(&mono, gdef, methods, conversions, &subst, types, &name_map, mono_methods)?;
    }
    Ok(())
}

/// Materialize monomorphs for every generic application in the package.
fn monomorphize_package(
    package: &Package,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    generics: &HashMap<String, GenericDef>,
    per_mod_types: &HashMap<String, HashMap<String, String>>,
    tables: &AssocTables,
    mono_methods: &mut Vec<MonoMethodJob>,
) -> Result<()> {
    if generics.is_empty() {
        return Ok(());
    }
    // Worklist of (base_mangled, arg_tys, name_map, span)
    let mut pending: Vec<(String, Vec<Ty>, HashMap<String, String>, Span)> = Vec::new();
    // Each generic product's checking instances first (D228 §5), so their
    // methods are lowered before any real instance's.
    let mut bases: Vec<&String> = generics.keys().collect();
    bases.sort();
    for base in bases {
        let gdef = &generics[base];
        for (args, ..) in &gdef.checking {
            pending.push((base.clone(), args.clone(), gdef.name_map.clone(), gdef.name_span));
        }
    }

    for m in &package.modules {
        let mut apps = Vec::new();
        for item in &m.ast.items {
            collect_item_type_apps(item, &mut apps);
        }
        let mut scoped: Vec<(TypeExpr, Span, &str)> = apps.into_iter().map(|(te, span)| (te, span, m.id.as_str())).collect();
        // A member a trait provides is written in the trait's module (D228).
        for item in &m.ast.items {
            let Item::Type(td) = item else { continue };
            for method in &td.methods {
                let Some(provided) = &method.provided else { continue };
                let mut apps = Vec::new();
                for param in &method.params {
                    collect_type_apps(&param.ty, &mut apps);
                }
                if let Some(ret) = &method.return_type {
                    collect_type_apps(ret, &mut apps);
                }
                scoped.extend(apps.into_iter().map(|(te, span)| (te, span, provided.module.as_str())));
            }
        }
        let empty = HashMap::new();
        for (te, span, module) in scoped {
            let name_map = per_mod_types.get(module).unwrap_or(&empty);
            // `m[k]` / `.get` is `May<V>` (D134). Seed it from every `Map<K, V>`.
            if te.name == "Map" && te.args.len() == 2 {
                if let Some(may_base) = name_map.get("May") {
                    let mut arg_tys = Vec::new();
                    let mut ok = true;
                    for arg in &te.args {
                        match arg {
                            TypeArg::Type(inner) => match resolve_ty(inner, types, &name_map) {
                                Ok(ty) => arg_tys.push(ty),
                                Err(_) => {
                                    ok = false;
                                    break;
                                }
                            },
                            TypeArg::Int { .. } => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok && arg_tys.len() == 2 {
                        pending.push((
                            may_base.clone(),
                            vec![arg_tys[1].clone()],
                            name_map.clone(),
                            span,
                        ));
                    }
                }
            }
            if te.args.is_empty() || is_builtin_generic(&te.name) {
                continue;
            }
            let base = name_map
                .get(&te.name)
                .cloned()
                .unwrap_or_else(|| te.name.clone());
            if !generics.contains_key(&base) {
                continue;
            }
            let mut arg_tys = Vec::new();
            let mut ok = true;
            for arg in &te.args {
                match arg {
                    TypeArg::Type(inner) => match resolve_ty(inner, types, &name_map) {
                        Ok(ty) => arg_tys.push(ty),
                        Err(_) => {
                            ok = false;
                            break;
                        }
                    },
                    TypeArg::Int { .. } => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && !arg_tys.is_empty() {
                pending.push((base, arg_tys, name_map.clone(), span));
            }
        }
    }

    monomorphize_pending(pending, types, layouts, generics, tables, mono_methods)
}

fn is_builtin_generic(name: &str) -> bool {
    // May/Trial are user generics from std/fallible (not builtins).
    matches!(
        name,
        "Array"
            | "Map"
            | "Address"
            | "Int32"
            | "Int64"
            | "UInt32"
            | "UInt64"
            | "Byte"
            | "UInt8"
            | "Float64"
            | "Bool"
            | "String"
            | "Bytes"
            | "Buffer"
            | "Void"
    )
}

pub(crate) fn collect_item_type_apps(item: &Item, out: &mut Vec<(TypeExpr, Span)>) {
    match item {
        Item::Type(td) => {
            if let Some(members) = &td.union_members {
                for m in members {
                    collect_type_apps(m, out);
                }
            }
            for p in &td.params {
                collect_type_apps(&p.ty, out);
            }
            for f in &td.fields {
                collect_type_apps(&f.ty, out);
            }
            for m in &td.methods {
                for p in &m.params {
                    collect_type_apps(&p.ty, out);
                }
                if let Some(r) = &m.return_type {
                    collect_type_apps(r, out);
                }
            }
            for (_, ty, _) in &td.assoc {
                collect_type_apps(ty, out);
            }
        }
        Item::Function(f) => {
            for p in &f.params {
                collect_type_apps(&p.ty, out);
            }
            if let Some(r) = &f.return_type {
                collect_type_apps(r, out);
            }
            collect_block_type_apps(&f.body, out);
        }
        Item::Extern(e) => {
            for p in &e.params {
                collect_type_apps(&p.ty, out);
            }
            if let Some(r) = &e.return_type {
                collect_type_apps(r, out);
            }
        }
        Item::Global(g) => {
            if let Some(t) = &g.ty {
                collect_type_apps(t, out);
            }
        }
        Item::Import(_) | Item::Extend(_) | Item::Trait(_) | Item::Conform(_) => {}
    }
}

/// Type applications in a body's `let` annotations, nested blocks included
/// (an `if` / `match` arm, a loop, a block expression).
fn collect_block_type_apps(block: &Block, out: &mut Vec<(TypeExpr, Span)>) {
    for it in &block.items {
        match it {
            BlockItem::Let { ty, init, .. } => {
                if let Some(t) = ty {
                    collect_type_apps(t, out);
                }
                collect_expr_type_apps(init, out);
            }
            BlockItem::Assign { value, .. } => collect_expr_type_apps(value, out),
            BlockItem::Expr(e) => collect_expr_type_apps(e, out),
        }
    }
    if let Some(tail) = &block.tail {
        collect_expr_type_apps(tail, out);
    }
}

fn collect_expr_type_apps(expr: &Expr, out: &mut Vec<(TypeExpr, Span)>) {
    let args_of = |args: &[Arg], out: &mut Vec<(TypeExpr, Span)>| {
        for a in args {
            match a {
                Arg::Pos(e) | Arg::Named { value: e, .. } | Arg::Spread { value: e, .. } => {
                    collect_expr_type_apps(e, out)
                }
            }
        }
    };
    match &expr.kind {
        ExprKind::Block(body) | ExprKind::Loop { body } => collect_block_type_apps(body, out),
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_type_apps(cond, out);
            collect_block_type_apps(then_branch, out);
            if let Some(e) = else_branch {
                collect_block_type_apps(e, out);
            }
        }
        ExprKind::While { cond, body } => {
            collect_expr_type_apps(cond, out);
            collect_block_type_apps(body, out);
        }
        ExprKind::For {
            start, end, body, ..
        } => {
            collect_expr_type_apps(start, out);
            if let Some(e) = end {
                collect_expr_type_apps(e, out);
            }
            collect_block_type_apps(body, out);
        }
        ExprKind::Match { scrutinee, arms } => {
            collect_expr_type_apps(scrutinee, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_expr_type_apps(g, out);
                }
                collect_block_type_apps(&arm.body, out);
            }
        }
        ExprKind::Call { args, .. } | ExprKind::Start { args, .. } => args_of(args, out),
        ExprKind::IndirectCall { callee, args } => {
            collect_expr_type_apps(callee, out);
            args_of(args, out);
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            collect_expr_type_apps(receiver, out);
            args_of(args, out);
        }
        ExprKind::Field { base, .. } | ExprKind::Try(base) => collect_expr_type_apps(base, out),
        ExprKind::Unary { expr: inner, .. } => collect_expr_type_apps(inner, out),
        // `x as Pair<Int64>` names an instance (D217).
        ExprKind::Cast { expr: inner, ty, .. } => {
            collect_expr_type_apps(inner, out);
            collect_type_apps(ty, out);
        }
        ExprKind::Index { base, index } => {
            collect_expr_type_apps(base, out);
            collect_expr_type_apps(index, out);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_expr_type_apps(left, out);
            collect_expr_type_apps(right, out);
        }
        ExprKind::Return { value: Some(v) } => collect_expr_type_apps(v, out),
        ExprKind::ArrayLit(elems) => {
            for e in elems {
                collect_expr_type_apps(e, out);
            }
        }
        ExprKind::MapLit(entries) => {
            for e in entries.iter().flat_map(|entry| entry.exprs()) {
                collect_expr_type_apps(e, out);
            }
        }
        ExprKind::Interpolate(parts) | ExprKind::TaggedInterp { parts, .. } => {
            for part in parts {
                if let InterpPart::Expr(e) = part {
                    collect_expr_type_apps(e, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_type_apps(te: &TypeExpr, out: &mut Vec<(TypeExpr, Span)>) {
    if !te.name.is_empty() && !te.args.is_empty() {
        out.push((te.clone(), te.span));
    }
    for a in &te.args {
        if let TypeArg::Type(inner) = a {
            collect_type_apps(inner, out);
        }
    }
    if let Some(members) = &te.union_members {
        for m in members {
            collect_type_apps(m, out);
        }
    }
    if let Some(ft) = &te.fn_type {
        for (_, p) in &ft.params {
            collect_type_apps(p, out);
        }
        collect_type_apps(&ft.ret, out);
    }
}

fn ty_to_type_expr(ty: &Ty, span: Span) -> TypeExpr {
    let (name, args) = match ty {
        Ty::Int64 => ("Int64".into(), vec![]),
        Ty::Int32 => ("Int32".into(), vec![]),
        Ty::UInt32 => ("UInt32".into(), vec![]),
        Ty::UInt64 => ("UInt64".into(), vec![]),
        Ty::Byte => ("Byte".into(), vec![]),
        Ty::Float64 => ("Float64".into(), vec![]),
        Ty::Bool => ("Bool".into(), vec![]),
        Ty::String => ("String".into(), vec![]),
        Ty::Bytes => ("Bytes".into(), vec![]),
        Ty::Buffer => ("Buffer".into(), vec![]),
        Ty::Node => ("Node".into(), vec![]),
        Ty::Void => ("Void".into(), vec![]),
        Ty::Never => ("Never".into(), vec![]),
        Ty::User(n) => (format!("{}{n}", ast::RESOLVED_TYPE), vec![]),
        Ty::Array { elem } => (
            "Array".into(),
            vec![TypeArg::Type(ty_to_type_expr(elem, span))],
        ),
        Ty::Map { key, val } => (
            "Map".into(),
            vec![
                TypeArg::Type(ty_to_type_expr(key, span)),
                TypeArg::Type(ty_to_type_expr(val, span)),
            ],
        ),
        Ty::Fn { params, ret } => {
            return TypeExpr {
                name: String::new(),
                args: vec![],
                span,
                lit: None,
                mark: None,
                union_members: None,
                fn_type: Some(Box::new(crate::ast::FnType {
                    params: params
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (format!("a{i}"), ty_to_type_expr(p, span)))
                        .collect(),
                    ret: ty_to_type_expr(ret, span),
                })),
                where_clause: None,
            };
        }
        other => (format!("{other:?}"), vec![]),
    };
    TypeExpr {
        name,
        args,
        span,
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    }
}

/// What a substitution puts in place of a type name: a type parameter's
/// argument, or a conformance's associated type as written (D228 §7).
pub(crate) trait TypeSubst {
    fn lookup(&self, name: &str, span: Span) -> Option<TypeExpr>;
}

impl TypeSubst for HashMap<String, Ty> {
    fn lookup(&self, name: &str, span: Span) -> Option<TypeExpr> {
        self.get(name).map(|ty| ty_to_type_expr(ty, span))
    }
}

impl TypeSubst for HashMap<String, TypeExpr> {
    fn lookup(&self, name: &str, span: Span) -> Option<TypeExpr> {
        self.get(name).map(|te| TypeExpr { span, ..te.clone() })
    }
}

pub(crate) fn subst_type_expr<S: TypeSubst + ?Sized>(te: &TypeExpr, subst: &S) -> TypeExpr {
    if te.lit.is_some() {
        return te.clone();
    }
    if let Some(ft) = &te.fn_type {
        return TypeExpr {
            name: String::new(),
            args: Vec::new(),
            span: te.span,
            lit: None,
            mark: te.mark,
            union_members: None,
            fn_type: Some(Box::new(crate::ast::FnType {
                params: ft
                    .params
                    .iter()
                    .map(|(n, p)| (n.clone(), subst_type_expr(p, subst)))
                    .collect(),
                ret: subst_type_expr(&ft.ret, subst),
            })),
            where_clause: te.where_clause.clone(),
        };
    }
    if let Some(members) = &te.union_members {
        return TypeExpr {
            name: String::new(),
            args: Vec::new(),
            span: te.span,
            lit: None,
            mark: te.mark,
            union_members: Some(members.iter().map(|m| subst_type_expr(m, subst)).collect()),
            fn_type: None,
            where_clause: te.where_clause.clone(),
        };
    }
    if te.args.is_empty() {
        if let Some(mut out) = subst.lookup(&te.name, te.span) {
            out.mark = te.mark;
            return out;
        }
    }
    let args = te
        .args
        .iter()
        .map(|a| match a {
            TypeArg::Type(inner) => TypeArg::Type(subst_type_expr(inner, subst)),
            TypeArg::Int { value, span } => TypeArg::Int {
                value: *value,
                span: *span,
            },
        })
        .collect();
    TypeExpr {
        name: te.name.clone(),
        args,
        span: te.span,
        lit: None,
        mark: te.mark,
        union_members: None,
        fn_type: None,
        where_clause: te.where_clause.clone(),
    }
}

/// An instance's template and its type arguments in the template's order.
fn instance_of(gdef: &GenericDef, subst: &HashMap<String, Ty>) -> (String, Vec<Ty>) {
    let args = gdef
        .type_params
        .iter()
        .map(|param| subst.get(param).cloned().unwrap_or(Ty::Void))
        .collect();
    (gdef.mangled_base.clone(), args)
}

fn instantiate_product_mono(
    mono: String,
    gdef: &GenericDef,
    params: &[ast::Param],
    methods: &[Method],
    conversions: &[ast::Conversion],
    subst: &HashMap<String, Ty>,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    name_map: &HashMap<String, String>,
    generics: &HashMap<String, GenericDef>,
    pending: &mut Vec<(String, Vec<Ty>, HashMap<String, String>, Span)>,
    span: Span,
) -> Result<()> {
    let mut fields = Vec::new();
    for p in params {
        let te = subst_type_expr(&p.ty, subst);
        // Request nested monos if needed.
        if !te.args.is_empty() && !is_builtin_generic(&te.name) {
            if let Some(base) = name_map.get(&te.name).or(Some(&te.name)) {
                if generics.contains_key(base) {
                    let mut arg_tys = Vec::new();
                    for a in &te.args {
                        if let TypeArg::Type(inner) = a {
                            arg_tys.push(resolve_ty(inner, types, name_map)?);
                        }
                    }
                    if arg_tys.len() == te.args.len() {
                        pending.push((base.clone(), arg_tys, name_map.clone(), span));
                    }
                }
            }
        }
        let ty = resolve_ty(&te, types, name_map)?;
        // Allow User (File, nested May/Trial, …) and Void (Trial ok = Void).
        // Narrower integers are plain scalars too (`Trial<Byte, …>` from a
        // checked `as`, D191).
        let ok = matches!(
            ty,
            Ty::Int64
                | Ty::Int32
                | Ty::UInt32
                | Ty::UInt64
                | Ty::Byte
                | Ty::Float64
                | Ty::Bool
                | Ty::String
                | Ty::Bytes
                | Ty::Array { .. }
                | Ty::Map { .. }
                | Ty::User(_)
                | Ty::Void
        );
        if !ok {
            return Err(Error::at(
                format!(
                    "generic product field `{}` must be an integer type, Float64, Bool, String, Bytes, Array, Map, User, or Void (got {ty:?})",
                    p.name
                ),
                p.ty.span,
            ));
        }
        fields.push(FieldInfo {
            name: p.name.clone(),
            ty,
            mutable: false,
            default: p.default.clone(),
            positional: p.positional,
            where_clause: p.ty.where_clause.clone(),
        });
    }

    // Insert type first so method returns of `Self` monomorph (e.g. `Box<T>`) resolve.
    let layout = TypeLayout {
        name: mono.clone(),
        fields: fields
            .iter()
            .map(|f| FieldLayout {
                name: f.name.clone(),
                c_name: sanitize_field(&f.name),
                ty: f.ty.clone(),
            })
            .collect(),
        sum: None,
        sum_scalar: None,
        has_drop: false,
        own_hash: false,
        own_equals: false,
    };
    types.insert(
        mono.clone(),
        TypeInfo {
            fields: fields.clone(),
            methods: HashMap::new(),
            zero_init: false,
            def_mod: gdef.def_mod.clone(),
            sum: None,
            refined: None,
            resource: false,
            traits: Vec::new(),
            derived: HashSet::new(),
            instance: Some(instance_of(gdef, subst)),
            templates: HashMap::new(),
        },
    );
    layouts.push(layout);

    // Enqueue monomorphs referenced by method and `from` clause signatures
    // (after type-param subst); a template's are made per call.
    for m in methods.iter().filter(|m| m.generic.is_empty()) {
        for te in method_sig_type_exprs(m) {
            let te = subst_type_expr(te, subst);
            enqueue_type_app_mono(&te, types, name_map, generics, pending, span);
        }
    }
    for clause in conversions {
        for te in [&clause.param.ty, &clause.return_type] {
            let te = subst_type_expr(te, subst);
            enqueue_type_app_mono(&te, types, name_map, generics, pending, span);
        }
    }

    Ok(())
}

/// An instance's methods and `from` clauses, once every instance their
/// signatures name has been made: their signatures, and the jobs that lower
/// their bodies.
#[allow(clippy::too_many_arguments)]
fn instantiate_product_methods(
    mono: &str,
    gdef: &GenericDef,
    methods: &[Method],
    conversions: &[ast::Conversion],
    subst: &HashMap<String, Ty>,
    types: &mut HashMap<String, TypeInfo>,
    name_map: &HashMap<String, String>,
    mono_methods: &mut Vec<MonoMethodJob>,
) -> Result<()> {
    let mono = mono.to_string();
    // `Self` in a signature is the instance (D228).
    let mut name_map = name_map.clone();
    name_map.insert("Self".into(), mono.clone());
    let name_map = &name_map;
    let mut method_sigs = HashMap::new();
    for m in methods {
        if !m.generic.is_empty() {
            let template = MethodTemplate {
                method: subst_method(m, subst),
                def_mod: gdef.def_mod.clone(),
            };
            types.get_mut(&mono).unwrap().templates.insert(m.name.clone(), template);
            continue;
        }
        let ret = match m.return_type.as_ref() {
            Some(t) => resolve_ty(&subst_type_expr(t, subst), types, name_map)?,
            None => Ty::Void,
        };
        let mut mparams = Vec::new();
        let mut defaults = Vec::new();
        for p in &m.params {
            mparams.push((
                p.name.clone(),
                resolve_ty(&subst_type_expr(&p.ty, subst), types, name_map)?,
            ));
            defaults.push(p.default.clone());
        }
        let mut seen_default = false;
        for (i, d) in defaults.iter().enumerate() {
            if d.is_some() {
                seen_default = true;
            } else if seen_default {
                return Err(Error::at(
                    format!(
                        "parameter `{}` without a default follows a defaulted parameter",
                        mparams[i].0
                    ),
                    m.params[i].name_span,
                ));
            }
        }
        method_sigs.insert(
            m.name.clone(),
            MethodSig {
                params: mparams,
                defaults,
                ret,
                exported: m.exported,
                def_mod: gdef.def_mod.clone(),
                multi_clause: false,
                mutable: m.params.iter().map(|p| p.mutable).collect(),
                positional: m.params.iter().map(|p| p.positional).collect(),
                requires: method_requires(gdef, m, subst),
            },
        );
        mono_methods.push(MonoMethodJob {
            mono_type: mono.clone(),
            method: m.clone(),
            subst: subst.clone(),
            def_mod: gdef.def_mod.clone(),
            conversion: false,
        });
    }

    types.get_mut(&mono).unwrap().methods = method_sigs;
    // Each `from` clause is a free function of this instance, named as `as`
    // looks it up, `__conversion_{instance}_N` (D217).
    let instance = type_source_name(types.get(&mono).unwrap(), &mono);
    for (index, clause) in conversions.iter().enumerate() {
        mono_methods.push(MonoMethodJob {
            mono_type: mono.clone(),
            method: Method {
                exported: clause.exported,
                name: crate::conversion_desugar::conversion_fn_name(&instance, index),
                name_span: clause.span,
                // `as` calls it by position (D200).
                params: vec![ast::Param {
                    positional: true,
                    ..clause.param.clone()
                }],
                return_type: Some(clause.return_type.clone()),
                body: clause.body.clone(),
                type_params: Vec::new(),
                provided: None,
                scope_module: None,
                requires: Vec::new(),
                generic: Vec::new(),
            },
            subst: subst.clone(),
            def_mod: gdef.def_mod.clone(),
            conversion: true,
        });
    }
    Ok(())
}

/// What an instance's arguments must meet for it to have `method` (D228):
/// its conformance's bounds, each with the parameter's argument.
fn method_requires(gdef: &GenericDef, method: &Method, subst: &HashMap<String, Ty>) -> Vec<(String, Ty, TraitRef)> {
    let mut out = Vec::new();
    for (param, bounds) in gdef.type_params.iter().zip(&method.requires) {
        let Some(arg) = subst.get(param) else { continue };
        for bound in bounds {
            out.push((param.clone(), arg.clone(), bound.clone()));
        }
    }
    out
}

/// `ty`'s associated type `name` (D228 §7), as its conformance names it: a
/// declared type's, recorded before lowering; a generic type's instance's,
/// its template's with the instance's arguments, worked out on first use.
fn assoc_type(ctx: &mut LowerCtx, ty: &Ty, name: &str, span: Span) -> Result<Option<Ty>> {
    let Ty::User(mangled) = ty else {
        return built_in_assoc_type(ctx, ty, name, span);
    };
    let recorded = ctx
        .synthetics
        .borrow()
        .assoc
        .get(mangled)
        .and_then(|types| types.get(name))
        .cloned();
    if recorded.is_some() {
        return Ok(recorded);
    }
    let Some((base, args)) = ctx.types.get(mangled).and_then(|info| info.instance.clone()) else {
        return Ok(None);
    };
    let Some(gdef) = ctx.synthetics.borrow().generics.get(&base).cloned() else {
        return Ok(None);
    };
    let Some((_, written, _)) = gdef.assoc.iter().find(|(each, ..)| each == name) else {
        return Ok(None);
    };
    let subst: HashMap<String, Ty> = gdef.type_params.iter().cloned().zip(args).collect();
    let written = TypeExpr {
        span,
        ..subst_type_expr(written, &subst)
    };
    generic::ensure_instances(ctx, &written, &gdef.name_map)?;
    let found = resolve_ty(&written, &ctx.types, &gdef.name_map)?;
    let conditional = ctx.synthetics.borrow().conditional.clone();
    for bound in gdef.assoc.iter().find(|(each, ..)| each == name).map(|(_, _, bounds)| bounds).into_iter().flatten() {
        if !ty_conforms(&ctx.types, &conditional, &found, bound) {
            return Err(Error::at(
                format!(
                    "`{}`'s `{name}` is {}, which is not `{}`",
                    display_ty(ty),
                    display_ty(&found),
                    bound.name
                ),
                span,
            ));
        }
    }
    ctx.synthetics
        .borrow_mut()
        .assoc
        .entry(mangled.clone())
        .or_default()
        .insert(name.to_string(), found.clone());
    Ok(Some(found))
}

/// A built-in type's associated type (D228 §7): its conformance's, with the
/// type's arguments (an `Array<Int64>`'s `Cursor` is `ArrayCursor<Int64>`).
fn built_in_assoc_type(ctx: &mut LowerCtx, ty: &Ty, name: &str, span: Span) -> Result<Option<Ty>> {
    let (base, args): (String, Vec<Ty>) = match ty {
        Ty::Array { elem } => ("Array".into(), vec![elem.as_ref().clone()]),
        Ty::Map { key, val } => ("Map".into(), vec![key.as_ref().clone(), val.as_ref().clone()]),
        other => match extension_key(other) {
            Some(key) => (key, Vec::new()),
            None => return Ok(None),
        },
    };
    let entry = ctx
        .synthetics
        .borrow()
        .built_in_assoc
        .get(&base)
        .and_then(|entries| entries.iter().find(|(_, each, ..)| each == name).cloned());
    let Some((params, _, written, bounds)) = entry else {
        return Ok(None);
    };
    let subst: HashMap<String, Ty> = params.into_iter().zip(args).collect();
    let written = TypeExpr {
        span,
        ..subst_type_expr(&written, &subst)
    };
    // Written in MIR names, so any module's map reads it.
    let name_map = ctx
        .package_maps
        .get("std_core")
        .map(|maps| maps.name_to_type.clone())
        .unwrap_or_default();
    generic::ensure_instances(ctx, &written, &name_map)?;
    let found = resolve_ty(&written, &ctx.types, &name_map)?;
    let conditional = ctx.synthetics.borrow().conditional.clone();
    for bound in &bounds {
        if !ty_conforms(&ctx.types, &conditional, &found, bound) {
            return Err(Error::at(
                format!(
                    "`{}`'s `{name}` is {}, which is not `{}`",
                    display_ty(ty),
                    display_ty(&found),
                    bound.name
                ),
                span,
            ));
        }
    }
    Ok(Some(found))
}

/// A method of a generic type's instance that a conformance with bounds
/// gives (D228): its arguments must meet them.
fn check_requires(ctx: &LowerCtx, type_name: &str, method: &str, sig: &MethodSig, span: Span) -> Result<()> {
    let conditional = ctx.synthetics.borrow().conditional.clone();
    for (param, arg, bound) in &sig.requires {
        if !ty_conforms(&ctx.types, &conditional, arg, bound) {
            return Err(Error::at(
                format!(
                    "`{}` has `{}` only where `{param}` is `{}`, and {} is not",
                    display_ty(&Ty::User(type_name.to_string())),
                    ast::member_display(method),
                    bound.name,
                    display_ty(arg)
                ),
                span,
            ));
        }
    }
    Ok(())
}

/// Whether a generic type's instance has the method a job lowers: its
/// conformance's bounds, if any, are met (D228).
fn job_applies(types: &HashMap<String, TypeInfo>, synthetics: &Rc<RefCell<SyntheticRegistry>>, job: &MonoMethodJob) -> bool {
    let Some(sig) = types.get(&job.mono_type).and_then(|info| info.methods.get(&job.method.name)) else {
        return true;
    };
    requires_met(types, &synthetics.borrow().conditional, &sig.requires)
}

/// The layouts without the types only generic code's checks use (D228 §5):
/// archetypes, the instances made with them, and the types that hold one
/// (`May<T>` in a generic function). Only those checks name them, and no
/// program runs one, so they are not emitted, nor their members kept.
fn without_checking_types(
    layouts: Vec<TypeLayout>,
    mut checking: HashSet<String>,
    instances: &HashMap<String, Vec<Ty>>,
) -> Vec<TypeLayout> {
    fn names_any(ty: &Ty, names: &HashSet<String>) -> bool {
        match ty {
            Ty::User(name) => names.contains(name),
            Ty::Array { elem } | Ty::FixedArray { elem, .. } => names_any(elem, names),
            Ty::Address(inner) => names_any(inner, names),
            Ty::Map { key, val } => names_any(key, names) || names_any(val, names),
            Ty::Fn { params, ret } => params.iter().any(|param| names_any(param, names)) || names_any(ret, names),
            _ => false,
        }
    }
    loop {
        let before = checking.len();
        for layout in &layouts {
            if checking.contains(&layout.name) {
                continue;
            }
            let holds = layout.fields.iter().any(|field| names_any(&field.ty, &checking))
                || layout.sum.iter().flatten().any(|variant| names_any(&variant.payload_ty, &checking))
                || instances
                    .get(&layout.name)
                    .is_some_and(|args| args.iter().any(|arg| names_any(arg, &checking)));
            if holds {
                checking.insert(layout.name.clone());
            }
        }
        if checking.len() == before {
            break;
        }
    }
    layouts.into_iter().filter(|layout| !checking.contains(&layout.name)).collect()
}

/// Whether a checking instance (D228 §5) lowers `method`: one written under
/// its bounds, and not a trait's provided member, which is checked at the
/// trait.
fn checks_member(requires: &[Vec<TraitRef>], method: &Method) -> bool {
    let plain = |lists: &[Vec<TraitRef>]| lists.iter().all(Vec::is_empty);
    method.provided.is_none() && (method.requires == requires || plain(&method.requires) && plain(requires))
}

/// A generic instance's method or `from` clause as its template's member:
/// the template, and the member's name (`from 0` for the first clause). A
/// member whose checking instance fails is not lowered for the real ones
/// (D228 §5).
fn template_member(types: &HashMap<String, TypeInfo>, job: &MonoMethodJob) -> (String, String) {
    let base = types
        .get(&job.mono_type)
        .and_then(|info| info.instance.as_ref())
        .map_or_else(|| job.mono_type.clone(), |(base, _)| base.clone());
    let member = if job.conversion {
        let index = job.method.name.rsplit('_').next().unwrap_or_default();
        format!("from {index}")
    } else {
        job.method.name.clone()
    };
    (base, member)
}

/// Whether an instance's arguments meet what `requires` asks (D228).
fn requires_met(
    types: &HashMap<String, TypeInfo>,
    conditional: &HashMap<String, Vec<ast::ConditionalConformance>>,
    requires: &[(String, Ty, TraitRef)],
) -> bool {
    requires
        .iter()
        .all(|(_, arg, bound)| ty_conforms(types, conditional, arg, bound))
}

fn method_sig_type_exprs(m: &Method) -> Vec<&TypeExpr> {
    let mut out = Vec::new();
    for p in &m.params {
        out.push(&p.ty);
    }
    if let Some(r) = &m.return_type {
        out.push(r);
    }
    out
}

/// Queue the generic instances `te` names, nested ones too, and give the
/// type it is: an instance by the name it will have once made, so an
/// instance that names another (`May<Pair<A, B>>`) is queued with it.
fn enqueue_type_app_mono(
    te: &TypeExpr,
    types: &HashMap<String, TypeInfo>,
    name_map: &HashMap<String, String>,
    generics: &HashMap<String, GenericDef>,
    pending: &mut Vec<(String, Vec<Ty>, HashMap<String, String>, Span)>,
    span: Span,
) -> Option<Ty> {
    let mut bare = te.clone();
    bare.mark = None;
    if te.args.is_empty() {
        return resolve_ty(&bare, types, name_map).ok();
    }
    let mut arg_tys = Vec::new();
    for arg in &te.args {
        match arg {
            TypeArg::Type(inner) => arg_tys.push(enqueue_type_app_mono(inner, types, name_map, generics, pending, span)),
            TypeArg::Int { .. } => arg_tys.push(None),
        }
    }
    let base = name_map.get(&te.name).cloned().unwrap_or_else(|| te.name.clone());
    if is_builtin_generic(&te.name) || !generics.contains_key(&base) {
        return match (te.name.as_str(), arg_tys.as_slice()) {
            ("Array", [Some(elem)]) => Some(Ty::Array {
                elem: Box::new(elem.clone()),
            }),
            ("Map", [Some(key), Some(val)]) => Some(Ty::Map {
                key: Box::new(key.clone()),
                val: Box::new(val.clone()),
            }),
            _ => resolve_ty(&bare, types, name_map).ok(),
        };
    }
    let arg_tys: Vec<Ty> = arg_tys.into_iter().collect::<Option<_>>()?;
    let mono = mono_type_name(&base, &arg_tys);
    if !types.contains_key(&mono) {
        pending.push((base, arg_tys, name_map.clone(), span));
    }
    Some(Ty::User(mono))
}

/// An instance's parameters' associated types, `S.Item`, into `subst`,
/// read off its arguments in order, owners first (D237).
fn derived_subst(
    gdef: &GenericDef,
    subst: &mut HashMap<String, Ty>,
    types: &HashMap<String, TypeInfo>,
    generics: &HashMap<String, GenericDef>,
    tables: &AssocTables,
    pending: &mut Vec<(String, Vec<Ty>, HashMap<String, String>, Span)>,
    span: Span,
) -> Result<()> {
    for param in &gdef.derived {
        let Some(owner_ty) = subst.get(&param.owner).cloned() else { continue };
        let Some(ty) = assoc_of(&owner_ty, &param.assoc, types, generics, tables, pending, span) else {
            return Err(Error::at(
                format!(
                    "`{}` needs `{}`, and {} names no `{}`",
                    gdef.source_name,
                    param.name,
                    display_ty(&owner_ty),
                    param.assoc
                ),
                span,
            ));
        };
        subst.insert(param.name.clone(), ty);
    }
    Ok(())
}

/// The function a method job lowers, `{instance}__{method}`.
fn job_function_name(job: &MonoMethodJob) -> String {
    format!("{}__{}", job.mono_type, job.method.name)
}

/// The method jobs of generic unions' instances that no path that made them
/// kept (D238): seeding `May` and return inference make instances without
/// their jobs, and an instance may be made more than once. Each method once,
/// by `known`.
fn union_method_jobs(
    types: &HashMap<String, TypeInfo>,
    generics: &HashMap<String, GenericDef>,
    known: &mut HashSet<String>,
) -> Vec<MonoMethodJob> {
    let mut names: Vec<&String> = types.keys().collect();
    names.sort();
    let mut jobs = Vec::new();
    for name in names {
        let Some((base, args)) = &types[name].instance else { continue };
        let Some(gdef) = generics.get(base) else { continue };
        let GenericKind::Union { methods, .. } = &gdef.kind else { continue };
        let subst: HashMap<String, Ty> = gdef.type_params.iter().cloned().zip(args.iter().cloned()).collect();
        for method in methods.iter().filter(|method| method.generic.is_empty()) {
            if known.insert(format!("{name}__{}", method.name)) {
                jobs.push(MonoMethodJob {
                    mono_type: name.clone(),
                    method: method.clone(),
                    subst: subst.clone(),
                    def_mod: gdef.def_mod.clone(),
                    conversion: false,
                });
            }
        }
    }
    jobs
}

/// Where an instance reads its parameters' associated types from while
/// instances are being made, before any body is lowered (D237).
#[derive(Default)]
struct AssocTables {
    /// Declared types' associated types as written, with their module's
    /// type names.
    written: HashMap<String, (HashMap<String, String>, Vec<(String, TypeExpr)>)>,
    /// Those already worked out (`SyntheticRegistry::assoc`).
    resolved: HashMap<String, HashMap<String, Ty>>,
    /// The built-in types' (`Array`'s `Cursor`), written in std/core's names.
    built_in: crate::trait_desugar::BuiltInAssoc,
    core_names: HashMap<String, String>,
}

/// `owner`'s associated type `name` (D237): a declared type's, a generic
/// type's instance's (made or still to make), or a built-in type's. An
/// instance it names is queued, so the type is given by its name.
fn assoc_of(
    owner: &Ty,
    name: &str,
    types: &HashMap<String, TypeInfo>,
    generics: &HashMap<String, GenericDef>,
    tables: &AssocTables,
    pending: &mut Vec<(String, Vec<Ty>, HashMap<String, String>, Span)>,
    span: Span,
) -> Option<Ty> {
    let (written, names, subst): (TypeExpr, HashMap<String, String>, HashMap<String, Ty>) = match owner {
        Ty::User(mangled) => {
            if let Some(found) = tables.resolved.get(mangled).and_then(|each| each.get(name)) {
                return Some(found.clone());
            }
            if let Some((names, entries)) = tables.written.get(mangled) {
                let (_, te) = entries.iter().find(|(each, _)| each == name)?;
                (te.clone(), names.clone(), HashMap::new())
            } else {
                let (base, args) = types
                    .get(mangled)
                    .and_then(|info| info.instance.clone())
                    .or_else(|| {
                        pending
                            .iter()
                            .find(|(base, args, ..)| mono_type_name(base, args) == *mangled)
                            .map(|(base, args, ..)| (base.clone(), args.clone()))
                    })?;
                let gdef = generics.get(&base)?;
                let (_, te, _) = gdef.assoc.iter().find(|(each, ..)| each == name)?;
                let subst = gdef.type_params.iter().cloned().zip(args).collect();
                (te.clone(), gdef.name_map.clone(), subst)
            }
        }
        other => {
            let (base, args): (String, Vec<Ty>) = match other {
                Ty::Array { elem } => ("Array".into(), vec![elem.as_ref().clone()]),
                Ty::Map { key, val } => ("Map".into(), vec![key.as_ref().clone(), val.as_ref().clone()]),
                _ => (extension_key(other)?, Vec::new()),
            };
            let (params, _, te, _) = tables.built_in.get(&base)?.iter().find(|(_, each, ..)| each == name)?;
            let subst = params.iter().cloned().zip(args).collect();
            (te.clone(), tables.core_names.clone(), subst)
        }
    };
    let written = TypeExpr {
        span,
        ..subst_type_expr(&written, &subst)
    };
    enqueue_type_app_mono(&written, types, &names, generics, pending, span)
}

fn instantiate_union_mono(
    mono: String,
    gdef: &GenericDef,
    members: &[TypeExpr],
    subst: &HashMap<String, Ty>,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    name_map: &HashMap<String, String>,
    generics: &HashMap<String, GenericDef>,
    pending: &mut Vec<(String, Vec<Ty>, HashMap<String, String>, Span)>,
    span: Span,
) -> Result<()> {
    // Ensure nested product monos exist first.
    for mem in members {
        let te = subst_type_expr(mem, subst);
        if !te.args.is_empty() && !is_builtin_generic(&te.name) {
            let base = name_map
                .get(&te.name)
                .cloned()
                .unwrap_or_else(|| te.name.clone());
            if generics.contains_key(&base) {
                let mut arg_tys = Vec::new();
                for a in &te.args {
                    if let TypeArg::Type(inner) = a {
                        // strip mark for mono key
                        let mut bare = inner.clone();
                        bare.mark = None;
                        arg_tys.push(resolve_ty(&bare, types, name_map)?);
                    }
                }
                if arg_tys.len() == te.args.len() {
                    let nested_mono = mono_type_name(&base, &arg_tys);
                    if !types.contains_key(&nested_mono) {
                        pending.push((base, arg_tys, name_map.clone(), span));
                        // Re-queue ourselves after deps.
                        pending.push((
                            gdef.mangled_base.clone(),
                            gdef.type_params
                                .iter()
                                .map(|p| subst.get(p).unwrap().clone())
                                .collect(),
                            name_map.clone(),
                            span,
                        ));
                        return Ok(());
                    }
                }
            }
        }
    }

    let mut variants = Vec::new();
    let mut seen_names = HashSet::new();
    let mut residual_tag: Option<u32> = None;
    let mut extract_tag: Option<u32> = None;

    for mem in members {
        let te = subst_type_expr(mem, subst);
        let arg_extract = type_expr_has_arg_mark(&te, TypeMark::Extract);
        let arg_residual = type_expr_has_arg_mark(&te, TypeMark::Residual);
        let whole_extract = matches!(te.mark, Some(TypeMark::Extract));
        let whole_residual = matches!(te.mark, Some(TypeMark::Residual));
        let unwrap_payload = arg_extract;
        let mark = te.mark.or(if arg_extract {
            Some(TypeMark::Extract)
        } else if arg_residual {
            Some(TypeMark::Residual)
        } else {
            None
        });

        if (whole_residual || arg_residual) && residual_tag.is_some() {
            return Err(Error::at(
                "union may have at most one residual mark (`?`)",
                te.span,
            ));
        }
        if (whole_extract || arg_extract) && extract_tag.is_some() {
            return Err(Error::at(
                "union may have at most one extract mark (`!`)",
                te.span,
            ));
        }

        if let Some(lit) = &te.lit {
            let vname = match lit {
                TypeLit::Int(n) => n.to_string(),
                TypeLit::Bool(b) => b.to_string(),
                TypeLit::String(s) => format!("\"{s}\""),
            };
            if !seen_names.insert(vname.clone()) {
                return Err(Error::at(
                    format!("duplicate union member `{vname}`"),
                    te.span,
                ));
            }
            let tag = variants.len() as u32;
            if whole_residual || arg_residual {
                residual_tag = Some(tag);
            }
            if whole_extract || arg_extract {
                extract_tag = Some(tag);
            }
            variants.push(SumVariant {
                tag,
                name: vname,
                ty: Ty::Void,
                lit: Some(lit.clone()),
                mark,
                unwrap_payload: false,
            });
            continue;
        }

        // Resolve member type (marks on args already handled for mono).
        let mut te_res = te.clone();
        te_res.mark = None;
        for a in &mut te_res.args {
            if let TypeArg::Type(inner) = a {
                inner.mark = None;
            }
        }
        let ty = resolve_ty(&te_res, types, name_map)?;
        let Ty::User(ref member_name) = ty else {
            return Err(Error::at(
                format!("union member must be a named type or literal, got {ty:?}"),
                te.span,
            ));
        };
        // Flatten nested unions.
        if let Some(minfo) = types.get(member_name) {
            if let Some(inner_sum) = &minfo.sum {
                if whole_extract || whole_residual || arg_extract || arg_residual {
                    return Err(Error::at(
                        "cannot place extract marks on nested union member",
                        te.span,
                    ));
                }
                for iv in &inner_sum.variants {
                    if !seen_names.insert(iv.name.clone()) {
                        return Err(Error::at(
                            format!("duplicate union member `{}`", iv.name),
                            te.span,
                        ));
                    }
                    let tag = variants.len() as u32;
                    variants.push(SumVariant {
                        tag,
                        name: iv.name.clone(),
                        ty: iv.ty.clone(),
                        lit: iv.lit.clone(),
                        mark: None,
                        unwrap_payload: iv.unwrap_payload,
                    });
                }
                continue;
            }
        }
        let vname = if te.name.is_empty() {
            member_name.clone()
        } else {
            // Prefer source generic name for patterns: Box not Box__Int64
            let src = te.name.clone();
            if generics.values().any(|g| g.source_name == src) {
                src
            } else {
                source_name_from_map(name_map, member_name)
            }
        };
        if !seen_names.insert(vname.clone()) {
            return Err(Error::at(
                format!("duplicate union member `{vname}`"),
                te.span,
            ));
        }
        if let Some(minfo) = types.get(member_name) {
            for f in &minfo.fields {
                if !union_member_field_ok(&f.ty) {
                    return Err(Error::at(
                        format!(
                            "union member `{vname}` field `{}` must be scalar, String/Bytes, User, Array, or Map",
                            f.name
                        ),
                        te.span,
                    ));
                }
            }
            if unwrap_payload && minfo.fields.len() != 1 {
                return Err(Error::at(
                    format!("type-arg extract on `{vname}` requires a single-field product"),
                    te.span,
                ));
            }
        }
        let tag = variants.len() as u32;
        if whole_residual || arg_residual {
            residual_tag = Some(tag);
        }
        if whole_extract || arg_extract {
            extract_tag = Some(tag);
        }
        variants.push(SumVariant {
            tag,
            name: vname,
            ty,
            lit: None,
            mark,
            unwrap_payload,
        });
    }

    if variants.len() < 2 {
        return Err(Error::at(
            "union type requires at least two members after monomorphization",
            gdef.name_span,
        ));
    }

    let sum = SumInfo {
        variants: variants.clone(),
        residual_tag,
        extract_tag,
    };
    let sum_scalar = if variants.iter().all(|v| v.lit.is_some()) {
        Some(pure_lit_scalar_ty(variants.len()))
    } else {
        None
    };
    let info = TypeInfo {
        fields: Vec::new(),
        methods: HashMap::new(),
        zero_init: false,
        def_mod: gdef.def_mod.clone(),
        sum: Some(sum),
        refined: None,
        resource: false,
        traits: Vec::new(),
        derived: HashSet::new(),
        instance: Some(instance_of(gdef, subst)),
        templates: HashMap::new(),
    };
    let layout = TypeLayout {
        name: mono.clone(),
        fields: Vec::new(),
        sum: Some(
            variants
                .iter()
                .map(|v| SumVariantLayout {
                    tag: v.tag,
                    name: v.name.clone(),
                    payload_ty: v.ty.clone(),
                })
                .collect(),
        ),
        sum_scalar,
        has_drop: false,
        own_hash: false,
        own_equals: false,
    };
    types.insert(mono, info);
    layouts.push(layout);
    Ok(())
}

fn source_name_from_map(name_map: &HashMap<String, String>, mangled: &str) -> String {
    for (src, m) in name_map {
        if m == mangled {
            return src.clone();
        }
    }
    mangled.to_string()
}

/// True when any nested type-arg carries `mark` (`Hold<Int64!>` / `Fail<E?>`).
fn type_expr_has_arg_mark(t: &TypeExpr, mark: TypeMark) -> bool {
    for arg in &t.args {
        if let TypeArg::Type(inner) = arg {
            if inner.mark == Some(mark) || type_expr_has_arg_mark(inner, mark) {
                return true;
            }
        }
    }
    false
}

fn lower_function(
    f: &ast::Function,
    mangled: &str,
    is_entry: bool,
    fns: &HashMap<String, FnSig>,
    types: &HashMap<String, TypeInfo>,
    name_to_type: &HashMap<String, String>,
    name_to_fn: &HashMap<String, String>,
    name_to_global: &HashMap<String, GlobalInfo>,
    namespaces: &HashMap<String, String>,
    module_exports: &HashMap<String, ModuleExports>,
    package_maps: &HashMap<String, ModuleNameMaps>,
    module_id: &str,
    synthetics: &Rc<RefCell<SyntheticRegistry>>,
) -> Result<Function> {
    let is_main = is_entry && f.name == "main";
    // Prefer FnSig.ret (includes D129 inference); fall back to annotation defaults.
    let ret_ty = fns
        .get(mangled)
        .map(|s| s.ret.clone())
        .unwrap_or_else(|| resolve_return_ty(f, is_entry, types, name_to_type).unwrap_or(Ty::Void));
    let mut params = Vec::new();
    for p in &f.params {
        let ty = resolve_ty(&p.ty, types, name_to_type)?;
        params.push((sanitize_name(&p.name), ty));
    }

    let mut ctx = LowerCtx::new(
        fns.clone(),
        types.clone(),
        name_to_type.clone(),
        name_to_fn.clone(),
        name_to_global.clone(),
        namespaces.clone(),
        module_exports.clone(),
        package_maps.clone(),
        ret_ty.clone(),
        module_id.to_string(),
        is_entry,
        Rc::clone(synthetics),
    );
    for p in &f.params {
        let ty = resolve_ty(&p.ty, types, name_to_type)?;
        bind_param(&mut ctx, &p.name, ty, p.mutable);
    }
    ctx.route_clause = is_generated_route_handler(&f.name);
    // A built-in type's getter, `__extend_String_get_initial` (D228).
    if let Some((_, method)) = f.name.strip_prefix("__extend_").and_then(|rest| rest.rsplit_once("_get_")) {
        note_getter(&ctx, mangled, &getter_name(method), f.name_span);
    }

    let ret = lower_block_value(&mut ctx, &f.body, &ret_ty)?;
    ctx.stmts = fill_early_return_drops(ctx.stmts, &ctx.drop_places, &ret_ty);
    insert_function_drops(&mut ctx.stmts, &ret, &ret_ty, &ctx.drop_places);

    Ok(Function {
        name: mangled.to_string(),
        is_main,
        receiver: None,
        params,
        mut_params: f.params.iter().map(|p| p.mutable).collect(),
        body: ctx.stmts,
        ret,
        ret_ty,
    })
}

/// Lower a multi-clause free function as a first-match cascade on dispatch formals (D131).
fn lower_multi_free(
    f: &ast::Function,
    mangled: &str,
    clauses: &[MultiClause],
    is_entry: bool,
    fns: &HashMap<String, FnSig>,
    types: &HashMap<String, TypeInfo>,
    name_to_type: &HashMap<String, String>,
    name_to_fn: &HashMap<String, String>,
    name_to_global: &HashMap<String, GlobalInfo>,
    namespaces: &HashMap<String, String>,
    module_exports: &HashMap<String, ModuleExports>,
    package_maps: &HashMap<String, ModuleNameMaps>,
    module_id: &str,
    synthetics: &Rc<RefCell<SyntheticRegistry>>,
) -> Result<Function> {
    let is_main = is_entry && f.name == "main";
    let sig = fns.get(mangled).ok_or_else(|| {
        Error::at(
            format!("internal: missing FnSig for multi-clause `{mangled}`"),
            f.name_span,
        )
    })?;
    let ret_ty = sig.ret.clone();
    let domain_params = sig.params.clone();

    let mut ctx = LowerCtx::new(
        fns.clone(),
        types.clone(),
        name_to_type.clone(),
        name_to_fn.clone(),
        name_to_global.clone(),
        namespaces.clone(),
        module_exports.clone(),
        package_maps.clone(),
        ret_ty.clone(),
        module_id.to_string(),
        is_entry,
        Rc::clone(synthetics),
    );

    // Domain formals (merged names from first clause / FnSig).
    let mut domain_places: Vec<(String, Place, Ty)> = Vec::new();
    for (name, ty) in &domain_params {
        let place = Place {
            name: sanitize_name(name),
        };
        ctx.note_drop_place(&place, ty);
        ctx.define(name, ty.clone(), false, place.clone());
        domain_places.push((name.clone(), place, ty.clone()));
    }

    let ret = lower_multi_clause_cascade(&mut ctx, clauses, &domain_places, &ret_ty)?;
    ctx.stmts = fill_early_return_drops(ctx.stmts, &ctx.drop_places, &ret_ty);
    insert_function_drops(&mut ctx.stmts, &ret, &ret_ty, &ctx.drop_places);

    let params: Vec<(String, Ty)> = domain_params
        .iter()
        .map(|(n, t)| (sanitize_name(n), t.clone()))
        .collect();

    Ok(Function {
        name: mangled.to_string(),
        is_main,
        receiver: None,
        params,
        mut_params: Vec::new(),
        body: ctx.stmts,
        ret,
        ret_ty,
    })
}

/// Lower a multi-clause method (D131).
fn lower_multi_method(
    mangled_type: &str,
    method_name: &str,
    clauses: &[MultiClause],
    fns: &HashMap<String, FnSig>,
    types: &HashMap<String, TypeInfo>,
    name_to_type: &HashMap<String, String>,
    name_to_fn: &HashMap<String, String>,
    name_to_global: &HashMap<String, GlobalInfo>,
    namespaces: &HashMap<String, String>,
    module_exports: &HashMap<String, ModuleExports>,
    package_maps: &HashMap<String, ModuleNameMaps>,
    module_id: &str,
    is_entry: bool,
    synthetics: &Rc<RefCell<SyntheticRegistry>>,
) -> Result<Function> {
    let info = types.get(mangled_type).ok_or_else(|| {
        Error::new(format!(
            "internal: missing type `{mangled_type}` for multi-method"
        ))
    })?;
    let sig = info.methods.get(method_name).ok_or_else(|| {
        Error::new(format!(
            "internal: missing MethodSig for `{mangled_type}.{method_name}`"
        ))
    })?;
    let ret_ty = sig.ret.clone();
    let domain_params = sig.params.clone();
    let recv_ty = Ty::User(mangled_type.to_string());
    let recv_place = "_self".to_string();

    let mut ctx = LowerCtx::new(
        fns.clone(),
        types.clone(),
        name_to_type.clone(),
        name_to_fn.clone(),
        name_to_global.clone(),
        namespaces.clone(),
        module_exports.clone(),
        package_maps.clone(),
        ret_ty.clone(),
        module_id.to_string(),
        is_entry,
        Rc::clone(synthetics),
    );

    bind_method_fields(&mut ctx, mangled_type);

    let mut domain_places: Vec<(String, Place, Ty)> = Vec::new();
    for (name, ty) in &domain_params {
        let place = Place {
            name: sanitize_name(name),
        };
        ctx.note_drop_place(&place, ty);
        ctx.define(name, ty.clone(), false, place.clone());
        domain_places.push((name.clone(), place, ty.clone()));
    }

    let ret = lower_multi_clause_cascade(&mut ctx, clauses, &domain_places, &ret_ty)?;
    ctx.stmts = fill_early_return_drops(ctx.stmts, &ctx.drop_places, &ret_ty);
    insert_function_drops(&mut ctx.stmts, &ret, &ret_ty, &ctx.drop_places);

    let params: Vec<(String, Ty)> = domain_params
        .iter()
        .map(|(n, t)| (sanitize_name(n), t.clone()))
        .collect();

    Ok(Function {
        name: format!("{mangled_type}__{method_name}"),
        is_main: false,
        receiver: Some((recv_place, recv_ty)),
        params,
        mut_params: Vec::new(),
        body: ctx.stmts,
        ret,
        ret_ty,
    })
}

/// First-match cascade: for each clause, if domain formals match clause types, run body.
fn lower_multi_clause_cascade(
    ctx: &mut LowerCtx,
    clauses: &[MultiClause],
    domain_places: &[(String, Place, Ty)],
    ret_ty: &Ty,
) -> Result<Option<Place>> {
    let result_place = if *ret_ty != Ty::Void && !is_never(ret_ty) {
        let p = ctx.fresh("mcret");
        init_place(ctx, &p, ret_ty)?;
        Some(p)
    } else {
        None
    };

    // Build cascade from last clause to first (last is the final else).
    let mut acc: Vec<Stmt> = Vec::new();
    for (ci, clause) in clauses.iter().enumerate().rev() {
        let is_last = ci == clauses.len() - 1;
        let outer = std::mem::take(&mut ctx.stmts);
        ctx.push_scope();

        // Refine each formal to the clause type when needed.
        for (i, ((dom_name, dom_place, dom_ty), clause_ty)) in domain_places
            .iter()
            .zip(clause.param_tys.iter())
            .enumerate()
        {
            let clause_name = &clause.param_names[i];
            let refined = refine_formal_to_clause(
                ctx,
                dom_place.clone(),
                dom_ty,
                clause_ty,
                clause.name_span,
            )?;
            // Bind under this clause's formal name (may differ from domain name).
            ctx.define(clause_name, clause_ty.clone(), false, refined);
            let _ = dom_name;
        }

        // Local return expect for this clause.
        let local_expect = if let Some(rt) = &clause.return_type {
            resolve_ty(rt, &ctx.types, &ctx.name_to_type)?
        } else {
            ret_ty.clone()
        };

        let arm_val = lower_block_value(ctx, &clause.body, &local_expect)?;
        if let (Some(rp), Some(av)) = (&result_place, &arm_val) {
            // Coerce arm value into overall ret_ty when needed.
            let (av_place, av_ty) = {
                // lower_block_value returns Option<Place>; recover type from expect.
                (av.clone(), local_expect.clone())
            };
            let coerced = if ty_eq(&ctx.types, &av_ty, ret_ty) {
                av_place
            } else if let Some(p) = try_sum_coerce(
                ctx,
                av_place.clone(),
                &av_ty,
                ret_ty,
                None,
                clause.name_span,
            )? {
                p
            } else if can_coerce_ty(&ctx.types, &av_ty, ret_ty) {
                av_place
            } else {
                // Last resort: assign if equal after display / force for Void
                if *ret_ty == Ty::Void {
                    av_place
                } else {
                    return Err(Error::at(
                        format!(
                            "multi-clause body type {} is not assignable to {}",
                            display_ty(&av_ty),
                            display_ty(ret_ty)
                        ),
                        clause.name_span,
                    ));
                }
            };
            // `assign` of a heap `Use` already Dups (D152 leftover was a second Dup).
            ctx.assign(rp.clone(), ret_ty.clone(), RValue::Use(coerced));
        }
        let body = std::mem::take(&mut ctx.stmts);
        ctx.pop_scope();
        ctx.stmts = outer;

        if is_last {
            acc = body;
        } else {
            // Build condition: all dispatch formals match this clause.
            let mut cond_place: Option<Place> = None;
            let cond_outer = std::mem::take(&mut ctx.stmts);
            for ((_, dom_place, dom_ty), clause_ty) in
                domain_places.iter().zip(clause.param_tys.iter())
            {
                if ty_eq(&ctx.types, dom_ty, clause_ty) {
                    continue; // shared formal — always matches
                }
                let c = emit_clause_match_cond(
                    ctx,
                    dom_place.clone(),
                    dom_ty,
                    clause_ty,
                    clause.name_span,
                )?;
                cond_place = Some(match cond_place {
                    None => c,
                    Some(prev) => {
                        let both = ctx.fresh("mcand");
                        ctx.assign(
                            both.clone(),
                            Ty::Bool,
                            RValue::Binary {
                                op: BinaryOp::And,
                                left: prev,
                                right: c,
                            },
                        );
                        both
                    }
                });
            }
            let cond_setup = std::mem::take(&mut ctx.stmts);
            ctx.stmts = cond_outer;

            if let Some(cond) = cond_place {
                // Cond setup must run *before* the if (cond place is read by Stmt::If).
                let mut wrapped = cond_setup;
                wrapped.push(Stmt::If {
                    cond,
                    then_body: body,
                    else_body: acc,
                });
                acc = wrapped;
            } else {
                // All formals shared / equivalent — this clause always matches when reached.
                acc = body;
            }
        }
    }
    ctx.stmts.extend(acc);
    Ok(result_place)
}

/// Coerce / view a domain formal as the clause's refined type.
/// Called under a match gate that already established membership in `clause_ty`.
fn refine_formal_to_clause(
    ctx: &mut LowerCtx,
    place: Place,
    domain_ty: &Ty,
    clause_ty: &Ty,
    span: Span,
) -> Result<Place> {
    if ty_eq(&ctx.types, domain_ty, clause_ty) {
        return Ok(place);
    }
    // Equal member-set aliases (transparent remap).
    if let Some(p) = try_sum_coerce(ctx, place.clone(), domain_ty, clause_ty, None, span)? {
        return Ok(p);
    }

    // Clause is a pure-lit singleton: inject that literal (value known by match gate).
    if let Ty::User(cn) = clause_ty {
        let lit_opt = ctx
            .types
            .get(cn)
            .and_then(|i| i.sum.as_ref())
            .and_then(|s| {
                if s.variants.len() == 1 {
                    s.variants[0].lit.clone()
                } else {
                    None
                }
            });
        if let Some(lit) = lit_opt {
            if let Some(p) = try_sum_inject_lit(ctx, &lit, clause_ty, span)? {
                let _ = place;
                let _ = domain_ty;
                return Ok(p);
            }
        }
    }

    // Clause is an atom (product or builtin): extract matching payload from domain sum.
    if let Ty::User(dn) = domain_ty {
        if let Some(ds) = ctx.types.get(dn).and_then(|i| i.sum.clone()) {
            if let Some(v) = ds.variants.iter().find(|v| match (&v.ty, clause_ty) {
                (Ty::User(n), Ty::User(cn)) => n == cn,
                (vt, ct) => ty_eq(&ctx.types, vt, ct),
            }) {
                if v.ty != Ty::Void {
                    let pp = ctx.fresh("mcref");
                    ctx.assign_no_drop(
                        pp.clone(),
                        v.ty.clone(),
                        RValue::SumPayload {
                            base: place,
                            tag: v.tag,
                            payload_ty: v.ty.clone(),
                        },
                    );
                    if needs_drop(&v.ty) {
                        ctx.stmts.push(Stmt::Dup {
                            place: pp.clone(),
                            ty: v.ty.clone(),
                        });
                        ctx.note_drop_place(&pp, &v.ty);
                    }
                    return Ok(pp);
                }
            }
        }
    }

    // Clause is a smaller union (`Json` in a domain that also holds `Line`):
    // remap the members it has; the clause gate excluded the others.
    if let (Ty::User(dn), Ty::User(cn)) = (domain_ty, clause_ty) {
        let domain_sum = ctx.types.get(dn).and_then(|i| i.sum.clone());
        let clause_sum = ctx.types.get(cn).and_then(|i| i.sum.clone());
        if let (Some(ds), Some(cs)) = (domain_sum, clause_sum) {
            let domain_keys: HashSet<String> = ds.variants.iter().map(sum_variant_key).collect();
            if cs.variants.iter().all(|v| domain_keys.contains(&sum_variant_key(v))) {
                return emit_sum_remap(ctx, place, dn, cn, &ds, &cs, true);
            }
        }
    }

    // Last resort: alias domain place (typing uses clause_ty via define).
    let _ = span;
    Ok(place)
}

/// Bool place: does `place: domain` match `clause_ty`?
fn emit_clause_match_cond(
    ctx: &mut LowerCtx,
    place: Place,
    domain_ty: &Ty,
    clause_ty: &Ty,
    span: Span,
) -> Result<Place> {
    if ty_eq(&ctx.types, domain_ty, clause_ty) {
        let t = ctx.fresh("mctrue");
        ctx.assign(t.clone(), Ty::Bool, RValue::Bool(true));
        return Ok(t);
    }

    // Domain sum + clause sum/atom: tag is one of clause's members.
    if let Ty::User(dn) = domain_ty {
        if let Some(ds) = ctx.types.get(dn).and_then(|i| i.sum.clone()) {
            let clause_keys: HashSet<String> = if let Some(members) =
                collect_union_members(&ctx.types, clause_ty)
            {
                members
                    .iter()
                    .map(|(_n, ty, lit)| {
                        if let Some(l) = lit {
                            match l {
                                TypeLit::Int(v) => format!("liti_{v}"),
                                TypeLit::Bool(b) => format!("litb_{b}"),
                                TypeLit::String(s) => {
                                    let safe: String = s
                                        .chars()
                                        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                                        .collect();
                                    format!("lits_{safe}")
                                }
                            }
                        } else if let Ty::User(un) = ty {
                            format!("ty_{un}")
                        } else {
                            format!(
                                "other_{}",
                                format!("{ty:?}")
                                    .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
                            )
                        }
                    })
                    .collect()
            } else {
                HashSet::new()
            };

            let matching_tags: Vec<u32> = ds
                .variants
                .iter()
                .filter(|v| clause_keys.contains(&sum_variant_key(v)))
                .map(|v| v.tag)
                .collect();

            if !matching_tags.is_empty() {
                let tag_p = ctx.fresh("mctag");
                ctx.assign(
                    tag_p.clone(),
                    Ty::Int64,
                    RValue::SumTag {
                        place: place.clone(),
                        sum_type: dn.clone(),
                    },
                );
                let mut cond: Option<Place> = None;
                for tag in matching_tags {
                    let expected = ctx.fresh("mcex");
                    ctx.assign(expected.clone(), Ty::Int64, RValue::Int(tag as i64));
                    let eq = ctx.fresh("mceq");
                    ctx.assign(
                        eq.clone(),
                        Ty::Bool,
                        RValue::Binary {
                            op: BinaryOp::Eq,
                            left: tag_p.clone(),
                            right: expected,
                        },
                    );
                    cond = Some(match cond {
                        None => eq,
                        Some(prev) => {
                            let both = ctx.fresh("mcor");
                            ctx.assign(
                                both.clone(),
                                Ty::Bool,
                                RValue::Binary {
                                    op: BinaryOp::Or,
                                    left: prev,
                                    right: eq,
                                },
                            );
                            both
                        }
                    });
                }
                return Ok(cond.unwrap());
            }
        }
    }

    // Wide String/Int64/Bool domain vs lit singleton clause.
    if let Ty::User(cn) = clause_ty {
        let lit_opt = ctx
            .types
            .get(cn)
            .and_then(|i| i.sum.as_ref())
            .and_then(|s| {
                if s.variants.len() == 1 {
                    s.variants[0].lit.clone()
                } else {
                    None
                }
            });
        if let Some(lit) = lit_opt {
            return emit_lit_eq_cond(ctx, place, domain_ty, &lit, span);
        }
    }

    Err(Error::at(
        format!(
            "cannot discriminate multi-clause types {} vs {}",
            display_ty(domain_ty),
            display_ty(clause_ty)
        ),
        span,
    ))
}

fn emit_lit_eq_cond(
    ctx: &mut LowerCtx,
    place: Place,
    domain_ty: &Ty,
    lit: &TypeLit,
    span: Span,
) -> Result<Place> {
    match (domain_ty, lit) {
        (Ty::String, TypeLit::String(s)) => {
            let lit_p = ctx.fresh("mcsl");
            ctx.assign(lit_p.clone(), Ty::String, RValue::StringLit(s.clone()));
            let eq = ctx.fresh("mcseq");
            ctx.assign(
                eq.clone(),
                Ty::Bool,
                RValue::Binary {
                    op: BinaryOp::Eq,
                    left: place,
                    right: lit_p,
                },
            );
            Ok(eq)
        }
        (Ty::Int64, TypeLit::Int(n)) => {
            let lit_p = ctx.fresh("mcil");
            ctx.assign(lit_p.clone(), Ty::Int64, RValue::Int(*n));
            let eq = ctx.fresh("mcieq");
            ctx.assign(
                eq.clone(),
                Ty::Bool,
                RValue::Binary {
                    op: BinaryOp::Eq,
                    left: place,
                    right: lit_p,
                },
            );
            Ok(eq)
        }
        (Ty::Bool, TypeLit::Bool(b)) => {
            let lit_p = ctx.fresh("mcbl");
            ctx.assign(lit_p.clone(), Ty::Bool, RValue::Bool(*b));
            let eq = ctx.fresh("mcbeq");
            ctx.assign(
                eq.clone(),
                Ty::Bool,
                RValue::Binary {
                    op: BinaryOp::Eq,
                    left: place,
                    right: lit_p,
                },
            );
            Ok(eq)
        }
        _ => Err(Error::at(
            format!(
                "literal match condition unsupported for domain {}",
                display_ty(domain_ty)
            ),
            span,
        )),
    }
}

pub(crate) fn subst_method<S: TypeSubst + ?Sized>(m: &Method, subst: &S) -> Method {
    Method {
        exported: m.exported,
        name: m.name.clone(),
        name_span: m.name_span,
        params: m
            .params
            .iter()
            .map(|p| ast::Param {
                name: p.name.clone(),
                name_span: p.name_span,
                positional: p.positional,
                mutable: p.mutable,
                ty: subst_type_expr(&p.ty, subst),
                default: p.default.as_ref().map(|e| subst_expr(e, subst)),
            })
            .collect(),
        return_type: m.return_type.as_ref().map(|t| subst_type_expr(t, subst)),
        body: subst_block(&m.body, subst),
        type_params: m.type_params.clone(),
        provided: m.provided.clone(),
        scope_module: m.scope_module.clone(),
        requires: m.requires.clone(),
        generic: m.generic.clone(),
    }
}

/// A generic function's body for its type arguments (D228 §5).
fn subst_function<S: TypeSubst + ?Sized>(function: &ast::Function, subst: &S) -> ast::Function {
    ast::Function {
        exported: false,
        name: function.name.clone(),
        name_span: function.name_span,
        type_params: Vec::new(),
        params: function
            .params
            .iter()
            .map(|p| ast::Param {
                name: p.name.clone(),
                name_span: p.name_span,
                positional: p.positional,
                mutable: p.mutable,
                ty: subst_type_expr(&p.ty, subst),
                default: p.default.as_ref().map(|e| subst_expr(e, subst)),
            })
            .collect(),
        return_type: function.return_type.as_ref().map(|t| subst_type_expr(t, subst)),
        body: subst_block(&function.body, subst),
        generic: Vec::new(),
    }
}

fn subst_block<S: TypeSubst + ?Sized>(block: &Block, subst: &S) -> Block {
    Block {
        items: block
            .items
            .iter()
            .map(|it| subst_block_item(it, subst))
            .collect(),
        tail: block.tail.as_ref().map(|e| Box::new(subst_expr(e, subst))),
        span: block.span,
    }
}

fn subst_block_item<S: TypeSubst + ?Sized>(it: &BlockItem, subst: &S) -> BlockItem {
    match it {
        BlockItem::Let {
            name,
            name_span,
            mutable,
            ty,
            init,
        } => BlockItem::Let {
            name: name.clone(),
            name_span: *name_span,
            mutable: *mutable,
            ty: ty.as_ref().map(|t| subst_type_expr(t, subst)),
            init: subst_expr(init, subst),
        },
        BlockItem::Assign { target, value } => BlockItem::Assign {
            target: subst_assign_target(target, subst),
            value: subst_expr(value, subst),
        },
        BlockItem::Expr(e) => BlockItem::Expr(subst_expr(e, subst)),
    }
}

fn subst_assign_target<S: TypeSubst + ?Sized>(t: &AssignTarget, subst: &S) -> AssignTarget {
    match t {
        AssignTarget::Name { name, name_span } => AssignTarget::Name {
            name: name.clone(),
            name_span: *name_span,
        },
        AssignTarget::Field {
            base,
            base_span,
            field,
            field_span,
        } => AssignTarget::Field {
            base: base.clone(),
            base_span: *base_span,
            field: field.clone(),
            field_span: *field_span,
        },
        AssignTarget::Index {
            base,
            base_span,
            field,
            field_span,
            index,
        } => AssignTarget::Index {
            base: base.clone(),
            base_span: *base_span,
            field: field.clone(),
            field_span: *field_span,
            index: subst_expr(index, subst),
        },
        AssignTarget::DynIndex {
            name,
            name_span,
            index,
        } => AssignTarget::DynIndex {
            name: name.clone(),
            name_span: *name_span,
            index: subst_expr(index, subst),
        },
    }
}

fn subst_expr<S: TypeSubst + ?Sized>(e: &Expr, subst: &S) -> Expr {
    let kind = match &e.kind {
        ExprKind::Int(n) => ExprKind::Int(*n),
        ExprKind::BigInt(n) => ExprKind::BigInt(*n),
        ExprKind::Float(n) => ExprKind::Float(*n),
        ExprKind::Bool(b) => ExprKind::Bool(*b),
        ExprKind::String(s) => ExprKind::String(s.clone()),
        ExprKind::Interpolate(parts) => ExprKind::Interpolate(
            parts
                .iter()
                .map(|p| match p {
                    InterpPart::Lit(s) => InterpPart::Lit(s.clone()),
                    InterpPart::Expr(ex) => InterpPart::Expr(subst_expr(ex, subst)),
                })
                .collect(),
        ),
        ExprKind::TaggedInterp { tag, parts } => ExprKind::TaggedInterp {
            tag: tag.clone(),
            parts: parts
                .iter()
                .map(|p| match p {
                    InterpPart::Lit(s) => InterpPart::Lit(s.clone()),
                    InterpPart::Expr(ex) => InterpPart::Expr(subst_expr(ex, subst)),
                })
                .collect(),
        },
        ExprKind::Name(n) => ExprKind::Name(n.clone()),
        ExprKind::MutArg(n) => ExprKind::MutArg(n.clone()),
        ExprKind::Unary { op, expr } => ExprKind::Unary {
            op: *op,
            expr: Box::new(subst_expr(expr, subst)),
        },
        ExprKind::Binary { op, left, right } => ExprKind::Binary {
            op: *op,
            left: Box::new(subst_expr(left, subst)),
            right: Box::new(subst_expr(right, subst)),
        },
        ExprKind::Call { callee, args, type_args } => ExprKind::Call {
            callee: callee.clone(),
            args: args.iter().map(|a| subst_arg(a, subst)).collect(),
            type_args: type_args.iter().map(|ty| subst_type_expr(ty, subst)).collect(),
        },
        ExprKind::IndirectCall { callee, args } => ExprKind::IndirectCall {
            callee: Box::new(subst_expr(callee, subst)),
            args: args.iter().map(|a| subst_arg(a, subst)).collect(),
        },
        ExprKind::Start {
            callee,
            callee_span,
            args,
        } => ExprKind::Start {
            callee: callee.clone(),
            callee_span: *callee_span,
            args: args.iter().map(|a| subst_arg(a, subst)).collect(),
        },
        ExprKind::Field {
            base,
            name,
            name_span,
        } => ExprKind::Field {
            base: Box::new(subst_expr(base, subst)),
            name: name.clone(),
            name_span: *name_span,
        },
        ExprKind::MethodCall {
            receiver,
            method,
            method_span,
            args,
            type_args,
        } => ExprKind::MethodCall {
            receiver: Box::new(subst_expr(receiver, subst)),
            method: method.clone(),
            method_span: *method_span,
            args: args.iter().map(|a| subst_arg(a, subst)).collect(),
            type_args: type_args.iter().map(|ty| subst_type_expr(ty, subst)).collect(),
        },
        ExprKind::Index { base, index } => ExprKind::Index {
            base: Box::new(subst_expr(base, subst)),
            index: Box::new(subst_expr(index, subst)),
        },
        ExprKind::ArrayLit(elems) => {
            ExprKind::ArrayLit(elems.iter().map(|x| subst_expr(x, subst)).collect())
        }
        ExprKind::MapLit(entries) => ExprKind::MapLit(
            entries
                .iter()
                .map(|entry| match entry {
                    MapEntry::Pair(k, v) => MapEntry::Pair(subst_expr(k, subst), subst_expr(v, subst)),
                    MapEntry::Spread(v) => MapEntry::Spread(subst_expr(v, subst)),
                })
                .collect(),
        ),
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => ExprKind::If {
            cond: Box::new(subst_expr(cond, subst)),
            then_branch: subst_block(then_branch, subst),
            else_branch: else_branch.as_ref().map(|b| subst_block(b, subst)),
        },
        ExprKind::While { cond, body } => ExprKind::While {
            cond: Box::new(subst_expr(cond, subst)),
            body: subst_block(body, subst),
        },
        ExprKind::Loop { body } => ExprKind::Loop {
            body: subst_block(body, subst),
        },
        ExprKind::For {
            name,
            name_span,
            value,
            start,
            end,
            inclusive,
            body,
        } => ExprKind::For {
            name: name.clone(),
            name_span: *name_span,
            value: value.clone(),
            start: Box::new(subst_expr(start, subst)),
            end: end.as_ref().map(|x| Box::new(subst_expr(x, subst))),
            inclusive: *inclusive,
            body: subst_block(body, subst),
        },
        ExprKind::Break => ExprKind::Break,
        ExprKind::Continue => ExprKind::Continue,
        ExprKind::Return { value } => ExprKind::Return {
            value: value.as_ref().map(|x| Box::new(subst_expr(x, subst))),
        },
        ExprKind::Block(b) => ExprKind::Block(subst_block(b, subst)),
        ExprKind::Try(inner) => ExprKind::Try(Box::new(subst_expr(inner, subst))),
        ExprKind::Cast {
            expr: inner,
            ty,
            propagate,
        } => ExprKind::Cast {
            expr: Box::new(subst_expr(inner, subst)),
            ty: subst_type_expr(ty, subst),
            propagate: *propagate,
        },
        ExprKind::Match { scrutinee, arms } => ExprKind::Match {
            scrutinee: Box::new(subst_expr(scrutinee, subst)),
            arms: arms
                .iter()
                .map(|a| MatchArm {
                    pattern: a.pattern.clone(),
                    guard: a.guard.as_ref().map(|g| subst_expr(g, subst)),
                    body: subst_block(&a.body, subst),
                })
                .collect(),
        },
    };
    Expr { kind, span: e.span }
}

fn subst_arg<S: TypeSubst + ?Sized>(a: &Arg, subst: &S) -> Arg {
    match a {
        Arg::Pos(e) => Arg::Pos(subst_expr(e, subst)),
        Arg::Named {
            name,
            name_span,
            value,
        } => Arg::Named {
            name: name.clone(),
            name_span: *name_span,
            value: subst_expr(value, subst),
        },
        Arg::Spread { value, span } => Arg::Spread {
            value: subst_expr(value, subst),
            span: *span,
        },
    }
}

/// Bind a method's fields and parameters in `ctx`, for its body.
///
/// Fields are bare names read from the receiver (no `self` in Thoxa). Heap
/// fields must Dup: the local is dropped at method end while `_self` is a
/// param (not dropped here). Without Dup, drop(field) would free the caller's ref.
///
/// D107 write-back: ctor-param fields and C-layout `mut` fields are mutable
/// locals; reassignment also stores through `_self`.
fn define_method_scope(ctx: &mut LowerCtx, mangled_type: &str, m: &Method) -> Result<()> {
    ctx.name_to_type.insert("Self".to_string(), mangled_type.to_string());
    ctx.trait_scope = m
        .provided
        .as_ref()
        .map(|provided| provided.members.iter().cloned().collect());
    ctx.current_method = Some(m.name.clone());
    bind_method_fields(ctx, mangled_type);
    // Heap params owned (D154). `_self` stays borrowed (pointer, D107).
    for p in &m.params {
        let ty = resolve_ty(&p.ty, &ctx.types, &ctx.name_to_type)?;
        bind_param(ctx, &p.name, ty, p.mutable);
    }
    Ok(())
}

/// A method's fields (single or multi-clause, D220): each a local read from
/// the receiver, writable with write-back through `_self` (D107). Its
/// sibling calls resolve as calls on the receiver (D208).
fn bind_method_fields(ctx: &mut LowerCtx, mangled_type: &str) {
    let info = ctx.types.get(mangled_type).unwrap().clone();
    ctx.method_type = Some(mangled_type.to_string());
    // A trait's body sees the trait, not the conforming type's fields (D228).
    if ctx.trait_scope.is_some() {
        return;
    }
    let recv = Place {
        name: "_self".to_string(),
    };
    for f in &info.fields {
        if matches!(f.ty, Ty::FixedArray { .. }) {
            continue;
        }
        let p = load_method_field(ctx, f);
        // Ctor-param types (!zero_init): fields are writable in methods.
        // C-layout (zero_init): only `mut` body fields.
        let writable = if info.zero_init { f.mutable } else { true };
        let writeback = if writable {
            Some((recv.clone(), sanitize_field(&f.name)))
        } else {
            None
        };
        // In its own accessor, a property's stored field is `field` (D228).
        let own_accessor = ctx
            .current_method
            .as_deref()
            .and_then(accessor_of)
            .is_some_and(|(_, property)| property == f.name);
        if own_accessor {
            ctx.define_full("field", f.ty.clone(), writable, p, writeback);
            continue;
        }
        ctx.define_full(&f.name, f.ty.clone(), writable, p, writeback);
        if let Some(binding) = ctx.lookup(&f.name) {
            ctx.method_fields.insert(f.name.clone(), binding);
        }
    }
}

/// The accessor a property name reaches in the body being lowered (D228): the
/// receiver type's `get_name` / `set_name`, when visible, unless the body is
/// that accessor, and unless `found` is a local that hides the property.
fn property_accessor(
    ctx: &LowerCtx,
    property: &str,
    kind: AccessorKind,
    found: Option<&Binding>,
) -> Option<(String, String)> {
    let type_name = ctx.method_type.clone()?;
    let method = match kind {
        AccessorKind::Get => getter_name(property),
        AccessorKind::Set => setter_name(property),
    };
    if ctx.current_method.as_deref() == Some(method.as_str())
        || !sibling_visible(ctx, &type_name, &method)
    {
        return None;
    }
    let is_field = |binding: &Binding| {
        ctx.method_fields
            .get(property)
            .is_some_and(|field| field.place == binding.place)
    };
    if found.is_some_and(|binding| !is_field(binding)) {
        return None;
    }
    Some((type_name, method))
}

/// A property named inside its own accessor, where it would call itself.
fn own_accessor_error(ctx: &LowerCtx, property: &str, span: Span) -> Option<Error> {
    let (kind, own) = accessor_of(ctx.current_method.as_deref()?)?;
    if own != property {
        return None;
    }
    let which = match kind {
        AccessorKind::Get => "getter",
        AccessorKind::Set => "setter",
    };
    let stored = ctx.lookup("field").is_some();
    Some(
        Error::at(
            format!("`{property}` inside its own {which} would call the {which} again"),
            span,
        )
        .with_help(if stored {
            "the stored value is `field` here".to_string()
        } else {
            format!("`{property}` has no stored field; use the fields it is computed from")
        }),
    )
}

/// `self`, the receiver as a value (D228): a copy of what `_self` points to
/// (a `Resource` is its box, shared).
fn lower_self_value(ctx: &mut LowerCtx, type_name: &str) -> (Place, Ty) {
    let ty = Ty::User(type_name.to_string());
    let boxed = ctx.types.get(type_name).is_some_and(|info| info.resource);
    let source = Place {
        name: if boxed { "_self" } else { "(*_self)" }.to_string(),
    };
    let place = ctx.fresh("self");
    ctx.assign(place.clone(), ty.clone(), RValue::Use(source));
    (place, ty)
}

/// The local a write through `receiver` changes in place, with what it is
/// for errors: a binding by name, or, in a type's methods, `self.name` for a
/// stored field, which is that field's local, as a bare `name` is (D228).
fn named_receiver(ctx: &LowerCtx, receiver: &Expr) -> Option<(String, Binding)> {
    match &receiver.kind {
        ExprKind::Name(name) => ctx.lookup(name).map(|binding| (format!("binding `{name}`"), binding)),
        ExprKind::Field { base, name, .. } => {
            self_field(ctx, base, name).map(|binding| (format!("field `{name}`"), binding))
        }
        _ => None,
    }
}

/// `self.name`'s local in a type's methods when `name` is a stored field,
/// not a property read through its getter.
fn self_field(ctx: &LowerCtx, base: &Expr, name: &str) -> Option<Binding> {
    if !matches!(&base.kind, ExprKind::Name(base) if base == "self") || self_receiver(ctx).is_none() {
        return None;
    }
    if property_accessor(ctx, name, AccessorKind::Get, None).is_some() {
        return None;
    }
    ctx.method_fields.get(name).cloned()
}

/// The receiver's type when `self` names it: inside a method, unless a local
/// is called `self` (an `extend` method's receiver parameter is).
fn self_receiver(ctx: &LowerCtx) -> Option<String> {
    if ctx.lookup("self").is_some() {
        return None;
    }
    ctx.method_type.clone()
}

/// Whether `method` is a method of the receiver of the built-in type's method
/// being lowered (D196, D228): `self` is a parameter there, not a type's
/// receiver, and its type has the method from an `extend` block or a
/// conformance.
fn extension_sibling(ctx: &LowerCtx, method: &str, span: Span) -> bool {
    if ctx.method_type.is_some() {
        return false;
    }
    let Some(receiver) = ctx.lookup("self") else {
        return false;
    };
    // A private one is still its sibling; the call reports it.
    !matches!(extension_method(ctx, &receiver.ty, method, span), Ok(None))
        || !matches!(generic_extension(ctx, &receiver.ty, method, span), Ok(None))
}

/// A method of `type_name` callable by bare name or through `self` in the body
/// being lowered: any of its methods, or, in a body a trait provides, only the
/// trait's members (D208, D228).
fn sibling_visible(ctx: &LowerCtx, type_name: &str, method: &str) -> bool {
    let declared = ctx
        .types
        .get(type_name)
        .is_some_and(|info| info.methods.contains_key(method) || info.templates.contains_key(method));
    declared
        && ctx
            .trait_scope
            .as_ref()
            .is_none_or(|members| members.contains(method))
}

/// Load a field from `_self` into its local in a method body (a heap field
/// takes its own reference: the local drops at method end, `_self` does not).
fn load_method_field(ctx: &mut LowerCtx, field: &FieldInfo) -> Place {
    let place = Place {
        name: sanitize_name(&field.name),
    };
    ctx.assign(
        place.clone(),
        field.ty.clone(),
        RValue::Field {
            base: Place {
                name: "_self".to_string(),
            },
            field: sanitize_field(&field.name),
        },
    );
    if needs_drop(&field.ty) {
        ctx.stmts.push(Stmt::Dup {
            place: place.clone(),
            ty: field.ty.clone(),
        });
    }
    place
}

/// A bare call to a sibling method, `double()` inside `quad()`: a call on
/// the receiver. The callee may write fields through `_self`, so the
/// caller's field locals are loaded again after it.
fn lower_sibling_call(
    ctx: &mut LowerCtx,
    type_name: &str,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    let instance: String;
    let method = if ctx.types[type_name].methods.contains_key(method) {
        method
    } else {
        instance = generic::instantiate_method(ctx, type_name, method, args, span, &[])?;
        instance.as_str()
    };
    let info = ctx.types.get(type_name).unwrap().clone();
    let sig = info.methods.get(method).unwrap().clone();
    check_requires(ctx, type_name, method, &sig, span)?;
    ctx.infer_check_callee(&format!("{type_name}__{method}"), method, span)?;
    let arg_places = resolve_call_args_full(
        ctx,
        &sig.params,
        &sig.defaults,
        &sig.mutable,
        &sig.positional,
        args,
        span,
        &sig.def_mod,
    )?;
    let mut all_args = vec![Place {
        name: "_self".to_string(),
    }];
    all_args.extend(arg_places);
    let result = emit_call_result(
        ctx,
        Callee::Method {
            type_name: type_name.to_string(),
            method: method.to_string(),
        },
        all_args,
        sig.ret.clone(),
    )?;
    for field in &info.fields {
        if !matches!(field.ty, Ty::FixedArray { .. }) {
            load_method_field(ctx, field);
        }
    }
    Ok(result)
}

/// Bind a parameter in a body. A heap parameter is owned: the caller moved a
/// copy in (D154), and it drops at the end. A `mut` parameter is the
/// caller's binding, passed by address (D199): writes reach the caller, and
/// the callee does not drop it.
fn bind_param(ctx: &mut LowerCtx, name: &str, ty: Ty, mutable: bool) {
    let c_name = sanitize_name(name);
    if mutable {
        let place = Place {
            name: format!("(*{c_name})"),
        };
        ctx.define(name, ty, true, place);
    } else {
        let place = Place { name: c_name };
        ctx.note_drop_place(&place, &ty);
        ctx.define(name, ty, false, place);
    }
}

fn lower_method(
    mangled_type: &str,
    m: &Method,
    fns: &HashMap<String, FnSig>,
    types: &HashMap<String, TypeInfo>,
    name_to_type: &HashMap<String, String>,
    name_to_fn: &HashMap<String, String>,
    name_to_global: &HashMap<String, GlobalInfo>,
    namespaces: &HashMap<String, String>,
    module_exports: &HashMap<String, ModuleExports>,
    package_maps: &HashMap<String, ModuleNameMaps>,
    module_id: &str,
    is_entry: bool,
    synthetics: &Rc<RefCell<SyntheticRegistry>>,
) -> Result<Function> {
    let name_to_type = &with_self_type(name_to_type.clone(), mangled_type);
    // A late instance's signature may name instances made while lowering.
    let mut seeded = types.clone();
    seed_synthetics(&mut seeded, &synthetics.borrow());
    let types = &seeded;
    // Unannotated: the type inference gave the signature (D174), else Void.
    let ret_ty = match m.return_type.as_ref() {
        Some(t) => resolve_ty(t, types, name_to_type)?,
        None => types
            .get(mangled_type)
            .and_then(|info| info.methods.get(&m.name))
            .map_or(Ty::Void, |sig| sig.ret.clone()),
    };
    let recv_ty = Ty::User(mangled_type.to_string());
    let recv_place = "_self".to_string();

    let mut params = Vec::new();
    for p in &m.params {
        let ty = resolve_ty(&p.ty, types, name_to_type)?;
        params.push((sanitize_name(&p.name), ty));
    }

    let mut ctx = LowerCtx::new(
        fns.clone(),
        types.clone(),
        name_to_type.clone(),
        name_to_fn.clone(),
        name_to_global.clone(),
        namespaces.clone(),
        module_exports.clone(),
        package_maps.clone(),
        ret_ty.clone(),
        module_id.to_string(),
        is_entry,
        Rc::clone(synthetics),
    );

    define_method_scope(&mut ctx, mangled_type, m)?;
    note_getter(&ctx, &format!("{mangled_type}__{}", m.name), &m.name, m.name_span);

    let ret = lower_block_value(&mut ctx, &m.body, &ret_ty)?;
    ctx.stmts = fill_early_return_drops(ctx.stmts, &ctx.drop_places, &ret_ty);
    insert_function_drops(&mut ctx.stmts, &ret, &ret_ty, &ctx.drop_places);

    Ok(Function {
        name: format!("{mangled_type}__{}", m.name),
        is_main: false,
        receiver: Some((recv_place, recv_ty)),
        params,
        mut_params: m.params.iter().map(|p| p.mutable).collect(),
        body: ctx.stmts,
        ret,
        ret_ty,
    })
}

/// Note `function` for the getter check when `method` is a getter.
fn note_getter(ctx: &LowerCtx, function: &str, method: &str, span: Span) {
    if let Some((AccessorKind::Get, property)) = accessor_of(method) {
        ctx.synthetics.borrow_mut().getter_checks.push(GetterCheck {
            function: function.to_string(),
            property: property.to_string(),
            span,
            module_id: ctx.module_id.clone(),
        });
    }
}

/// Drop every noted heap local at function end except the returned place (moved out).
/// Uses `drop_places` (from `assign` / `note_drop_place`), not every Assign — so
/// `assign_no_drop` sum-payload intermediates are not double-freed (D118 Fallible).
fn insert_function_drops(
    stmts: &mut Vec<Stmt>,
    ret: &Option<Place>,
    ret_ty: &Ty,
    drop_places: &[(String, Ty)],
) {
    let places: Vec<(String, Ty)> = drop_places.to_vec();
    let skip = if needs_drop(ret_ty) {
        ret.as_ref().map(|p| p.name.clone())
    } else {
        None
    };
    let mut seen = std::collections::HashSet::new();
    for (name, ty) in places {
        if skip.as_ref() == Some(&name) {
            continue;
        }
        if seen.insert(name.clone()) {
            stmts.push(Stmt::Drop {
                place: Place { name },
                ty,
            });
        }
    }
}

#[allow(dead_code)] // kept for diagnostics / possible end-of-fn collect restore
fn collect_drop_places(stmts: &[Stmt], out: &mut Vec<(String, Ty)>) {
    for s in stmts {
        match s {
            Stmt::Assign { place, ty, .. } if needs_drop(ty) => {
                out.push((place.name.clone(), ty.clone()));
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_drop_places(then_body, out);
                collect_drop_places(else_body, out);
            }
            Stmt::Loop { body } | Stmt::StoreBack { body, .. } => collect_drop_places(body, out),
            Stmt::ForRange { body, .. } => collect_drop_places(body, out),
            _ => {}
        }
    }
}

fn lower_block_value(ctx: &mut LowerCtx, block: &Block, expect: &Ty) -> Result<Option<Place>> {
    ctx.push_scope();
    let mut errors: Vec<Error> = Vec::new();
    let mut failed_lets = Vec::new();
    if let Err(e) = lower_block_items_tracked(ctx, &block.items, &mut failed_lets) {
        errors.extend(e.into_diagnostics());
    }

    let result = if let Some(tail) = &block.tail {
        // Empty `[]` as block tail: take elem type from expected Array<T> (e.g. fn return).
        let tail_res = (|| -> Result<Option<Place>> {
            let (place, ty) = if is_empty_array_lit(tail) {
                if let Ty::Array { elem } = expect {
                    lower_empty_array_typed(ctx, elem.as_ref(), tail.span)?
                } else {
                    lower_expr_expect(ctx, tail, Some(expect))?
                }
            } else {
                let exp = if *expect != Ty::Void {
                    Some(expect)
                } else {
                    None
                };
                lower_expr_expect(ctx, tail, exp)?
            };
            // D129: record non-Never tail as a value exit during inference probe.
            if ctx.inferring && !is_never(&ty) {
                ctx.infer_values.push(ty.clone());
            }
            // Diverging tail (e.g. infinite `loop`) satisfies any expected type (D128).
            if is_never(&ty) {
                return Ok(None);
            }
            let place = if *expect != Ty::Void && &ty != expect {
                match try_sum_coerce(
                    ctx,
                    place.clone(),
                    &ty,
                    expect,
                    expr_as_type_lit(tail).as_ref(),
                    tail.span,
                )? {
                    Some(p) => p,
                    None if can_numeric_cast(&ty, expect) => {
                        coerce_place(ctx, place, &ty, expect, tail.span)?
                    }
                    None => match convert::encode_where_json_expected(ctx, place, &ty, expect)? {
                        Some(p) => p,
                        None => {
                            return Err(Error::at(ctx.type_mismatch_msg(expect, &ty), tail.span));
                        }
                    },
                }
            } else {
                place
            };
            Ok(Some(place))
        })();
        match tail_res {
            Ok(r) => r,
            Err(e) => {
                errors.extend(without_failed_let_uses(e, &failed_lets));
                None
            }
        }
    } else if *expect == Ty::Void {
        None
    } else {
        errors.push(Error::at("missing block value", block.span));
        None
    };
    ctx.pop_scope();
    if !errors.is_empty() {
        return Err(Error::multi(errors));
    }
    Ok(result)
}

fn is_empty_array_lit(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::ArrayLit(elems) if elems.is_empty())
}

/// Typed empty array literal (when context supplies `Array<T>`).
fn lower_empty_array_typed(ctx: &mut LowerCtx, elem: &Ty, span: Span) -> Result<(Place, Ty)> {
    if !is_array_elem_ty(elem) {
        return Err(Error::at(
            format!("Array element type not yet supported: {elem:?}"),
            span,
        ));
    }
    if array_elem_is_user(elem) && !is_array_user_elem(elem, &ctx.types) {
        return Err(Error::at(
            format!(
                "Array element User type must be constructor-param with Int64/Bool/String/Bytes/Node/Array/Map/User fields: {elem:?}"
            ),
            span,
        ));
    }
    let arr_ty = Ty::Array {
        elem: Box::new(elem.clone()),
    };
    let p = ctx.fresh("aempty");
    ctx.assign(
        p.clone(),
        arr_ty.clone(),
        RValue::ArrayLit {
            elems: vec![],
            elem_ty: elem.clone(),
        },
    );
    Ok((p, arr_ty))
}

fn lower_block_items(ctx: &mut LowerCtx, items: &[BlockItem]) -> Result<()> {
    lower_block_items_tracked(ctx, items, &mut Vec::new())
}

/// Lower a block's statements, collecting independent errors so one bad
/// name does not hide the next. `failed_lets` gathers each unannotated
/// `let` that failed: later "undefined name" errors for it are dropped, here
/// and in the caller's block tail, since they only repeat the first error.
fn lower_block_items_tracked(
    ctx: &mut LowerCtx,
    items: &[BlockItem],
    failed_lets: &mut Vec<String>,
) -> Result<()> {
    let mut errors: Vec<Error> = Vec::new();
    for item in items {
        if let Err(e) = lower_block_item(ctx, item) {
            errors.extend(without_failed_let_uses(e, failed_lets));
            if let BlockItem::Let {
                name,
                mutable,
                ty: ann,
                ..
            } = item
            {
                // An annotated `let` stays in scope at its declared type, so
                // later lines are still checked against it. Nothing is
                // emitted: the function already has an error.
                let declared = ann
                    .as_ref()
                    .and_then(|ann| resolve_ty(ann, &ctx.types, &ctx.name_to_type).ok());
                match declared {
                    Some(ty) => {
                        let place = ctx.local_place(name);
                        ctx.define(name, ty, *mutable, place);
                    }
                    None => failed_lets.push(name.clone()),
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::multi(errors))
    }
}

/// `error`'s diagnostics minus "undefined name" for a `let` that already failed.
fn without_failed_let_uses(error: Error, failed_lets: &[String]) -> Vec<Error> {
    error
        .into_diagnostics()
        .into_iter()
        .filter(|diagnostic| {
            !failed_lets
                .iter()
                .any(|name| diagnostic.message == format!("undefined name `{name}`"))
        })
        .collect()
}

fn lower_block_item(ctx: &mut LowerCtx, item: &BlockItem) -> Result<()> {
    match item {
        BlockItem::Let {
            name,
            name_span,
            mutable,
            ty: ann,
            init,
        } => {
            // Annotated empty `[]` — emit only the typed empty lit (skip Void intermediate).
            if let Some(ann) = ann {
                if is_empty_array_lit(init) {
                    let expect = resolve_ty(ann, &ctx.types, &ctx.name_to_type)?;
                    let Ty::Array { elem } = &expect else {
                        return Err(Error::at(
                            format!("empty array `[]` needs Array<T> annotation, got {expect:?}"),
                            *name_span,
                        ));
                    };
                    if !is_array_elem_ty(elem.as_ref()) {
                        return Err(Error::at(
                            format!("Array element type not yet supported: {:?}", elem),
                            *name_span,
                        ));
                    }
                    if array_elem_is_user(elem.as_ref())
                        && !is_array_user_elem(elem.as_ref(), &ctx.types)
                    {
                        return Err(Error::at(
                            format!(
                                "Array element User type must be constructor-param with Int64/Bool/String/Bytes/Node/Array/Map/User fields: {:?}",
                                elem
                            ),
                            *name_span,
                        ));
                    }
                    let p = ctx.fresh("aempty");
                    ctx.assign(
                        p.clone(),
                        expect.clone(),
                        RValue::ArrayLit {
                            elems: vec![],
                            elem_ty: elem.as_ref().clone(),
                        },
                    );
                    ctx.bind_name(name, expect, *mutable, p);
                    return Ok(());
                }
            }

            // Annotated fixed Array<T,N> from array literal (D117).
            if let Some(ann) = ann {
                let expect = resolve_ty(ann, &ctx.types, &ctx.name_to_type)?;
                if let Ty::FixedArray { elem, len } = &expect {
                    if let ExprKind::ArrayLit(elems) = &init.kind {
                        if elems.len() as u64 != *len {
                            return Err(Error::at(
                                format!(
                                    "fixed Array length mismatch: expected {len}, found {}",
                                    elems.len()
                                ),
                                init.span,
                            ));
                        }
                        if !is_abi_scalar(elem) {
                            return Err(Error::at(
                                format!("fixed Array element must be an ABI scalar, got {elem:?}"),
                                *name_span,
                            ));
                        }
                        let mut places = Vec::new();
                        for e in elems {
                            let (p, ty) = lower_expr(ctx, e)?;
                            let p = coerce_to_field(ctx, p, &ty, elem, e.span)?;
                            places.push(p);
                        }
                        // Bind directly (C arrays are not assignable via `Use` copy).
                        let bound = ctx.local_place(name);
                        ctx.assign(
                            bound.clone(),
                            expect.clone(),
                            RValue::FixedArrayLit {
                                elems: places,
                                elem_ty: elem.as_ref().clone(),
                                len: *len,
                            },
                        );
                        ctx.define(name, expect, *mutable, bound);
                        return Ok(());
                    }
                }
            }

            let (place, ty) = if let Some(ann) = ann {
                let expect = resolve_ty(ann, &ctx.types, &ctx.name_to_type)?;
                let (place, ty) = lower_expr_expect(ctx, init, Some(&expect))?;
                let place = coerce_annotated(ctx, place, &ty, &expect, init, name, *name_span)?;
                (place, expect)
            } else {
                let (place, ty) = lower_expr(ctx, init)?;
                if matches!(&ty, Ty::Array { elem } if matches!(elem.as_ref(), Ty::Void)) {
                    return Err(Error::at(
                        "empty array `[]` requires a type annotation (e.g. `Array<Int64>`)",
                        init.span,
                    ));
                }
                (place, ty)
            };
            // A Void value holds nothing: the name refers to the unit place
            // (copying it emitted C for an undeclared local, D186).
            if ty == Ty::Void {
                let (unit, _) = void_place(ctx);
                ctx.define(name, Ty::Void, *mutable, unit);
                return Ok(());
            }
            ctx.bind_name(name, ty, *mutable, place);
            Ok(())
        }
        BlockItem::Assign { target, value } => lower_assign(ctx, target, value),
        BlockItem::Expr(e) => {
            let _ = lower_expr(ctx, e)?;
            Ok(())
        }
    }
}

/// A value where `expect` is expected (a `return`, an assignment): the
/// conversions an annotated `let` makes — a union member into its union, a
/// literal into a literal or refined type, a numeric conversion. `None` when
/// none applies (D192).
fn coerce_to_expected(
    ctx: &mut LowerCtx,
    place: Place,
    ty: &Ty,
    expect: &Ty,
    value: &Expr,
) -> Result<Option<Place>> {
    if ty == expect {
        return Ok(Some(place));
    }
    let literal = expr_as_type_lit(value);
    if let Some(p) = try_sum_coerce(ctx, place.clone(), ty, expect, literal.as_ref(), value.span)? {
        return Ok(Some(p));
    }
    if can_numeric_cast(ty, expect) {
        return Ok(Some(coerce_place(ctx, place, ty, expect, value.span)?));
    }
    if let Some(p) = try_refine_coerce(ctx, place.clone(), ty, expect, literal.as_ref(), value.span)? {
        return Ok(Some(p));
    }
    convert::encode_where_json_expected(ctx, place, ty, expect)
}

/// A value bound under an annotation (`let x: T = …`, in a block or at module
/// level) takes `T`: a sum member, a numeric conversion, or a refinement.
fn coerce_annotated(
    ctx: &mut LowerCtx,
    place: Place,
    ty: &Ty,
    expect: &Ty,
    init: &Expr,
    name: &str,
    name_span: Span,
) -> Result<Place> {
    if ty == expect {
        return Ok(place);
    }
    let literal = expr_as_type_lit(init);
    if let Some(p) = try_sum_coerce(ctx, place.clone(), ty, expect, literal.as_ref(), name_span)? {
        return Ok(p);
    }
    if can_numeric_cast(ty, expect) {
        return coerce_place(ctx, place, ty, expect, init.span);
    }
    if let Some(p) = try_refine_coerce(ctx, place.clone(), ty, expect, literal.as_ref(), name_span)? {
        return Ok(p);
    }
    if let Some(p) = convert::encode_where_json_expected(ctx, place, ty, expect)? {
        return Ok(p);
    }
    let error = Error::at(
        format!("type annotation mismatch for `{name}`: expected {expect:?}, found {ty:?}"),
        name_span,
    );
    Err(match convert::decode_hint(ctx, ty, expect) {
        Some(hint) => error.with_help(hint),
        None => error,
    })
}

fn lower_assign(ctx: &mut LowerCtx, target: &AssignTarget, value: &Expr) -> Result<()> {
    match target {
        AssignTarget::Name { name, name_span } => ctx.reassign_name(name, *name_span, value),
        AssignTarget::Field {
            base,
            field,
            field_span,
            ..
        } if base == "self" && self_receiver(ctx).is_some() => {
            if let Some((type_name, setter)) = property_accessor(ctx, field, AccessorKind::Set, None) {
                let args = [Arg::Pos(value.clone())];
                lower_sibling_call(ctx, &type_name, &setter, &args, *field_span)?;
                return Ok(());
            }
            if let Some(error) = own_accessor_error(ctx, field, *field_span) {
                return Err(error);
            }
            // `self.field = …` writes the field, even where a local shadows it (D228).
            match ctx.method_fields.get(field).cloned() {
                Some(binding) if binding.mutable => ctx.reassign_binding(binding, field, value),
                Some(_) => Err(Error::at(
                    format!("cannot assign to immutable field `{field}`"),
                    *field_span,
                )),
                None => Err(Error::at(format!("no field `{field}` on `self`"), *field_span)),
            }
        }
        AssignTarget::Field {
            base,
            base_span,
            field,
            field_span,
        } => {
            let binding = ctx
                .lookup(base)
                .ok_or_else(|| Error::at(format!("undefined name `{base}`"), *base_span))?;
            // A property with a setter is written through it, from anywhere (D228).
            if let Ty::User(type_name) = &binding.ty {
                let setter = setter_name(field);
                if ctx
                    .types
                    .get(type_name)
                    .is_some_and(|info| info.methods.contains_key(&setter))
                {
                    let receiver = Expr {
                        kind: ExprKind::Name(base.clone()),
                        span: *base_span,
                    };
                    lower_method_call(ctx, &receiver, &setter, *field_span, &[Arg::Pos(value.clone())])?;
                    return Ok(());
                }
            }
            if !binding.mutable {
                return Err(Error::at(
                    format!("cannot assign to field of immutable binding `{base}`"),
                    *base_span,
                ));
            }
            let type_name = match &binding.ty {
                Ty::User(n) => n.clone(),
                other => {
                    return Err(Error::at(
                        format!("field assign on non-struct type {other:?}"),
                        *base_span,
                    ));
                }
            };
            let info = ctx
                .types
                .get(&type_name)
                .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), *base_span))?
                .clone();
            let finfo = info
                .fields
                .iter()
                .find(|f| f.name == *field)
                .ok_or_else(|| {
                    Error::at(format!("no field `{field}` on `{type_name}`"), *field_span)
                })?;
            if !finfo.mutable {
                return Err(Error::at(
                    format!("cannot assign to immutable field `{field}`"),
                    *field_span,
                ));
            }
            // Whole fixed-array field assign is allowed (element-wise copy in cgen).
            let field_ty = finfo.ty.clone();
            let (v, vty) = lower_expr_expect(ctx, value, Some(&field_ty))?;
            let v = coerce_to_field(ctx, v, &vty, &field_ty, value.span)?;
            ctx.stmts.push(Stmt::FieldAssign {
                base: binding.place,
                field: sanitize_field(field),
                ty: field_ty,
                value: v,
            });
            Ok(())
        }
        // `self.items[i] = …` in a type's methods writes the field (D228).
        AssignTarget::Index {
            base,
            field,
            field_span,
            index,
            ..
        } if base == "self" && ctx.method_fields.contains_key(field) => {
            let receiver = crate::ast_build::name("self");
            match self_field(ctx, &receiver, field) {
                Some(binding) => {
                    assign_element(ctx, binding, &format!("field `{field}`"), *field_span, index, value)
                }
                None => Err(Error::at(
                    format!("`{field}` is a property: write it whole, `self.{field} = …`"),
                    *field_span,
                )),
            }
        }
        AssignTarget::Index {
            base,
            base_span,
            field,
            field_span,
            index,
        } => {
            // A fixed-array field of `self` is stored through the receiver.
            let binding = match self_receiver(ctx).filter(|_| base == "self") {
                Some(type_name) => Binding {
                    ty: Ty::User(type_name),
                    mutable: true,
                    place: Place { name: "_self".into() },
                    writeback: None,
                },
                None => ctx
                    .lookup(base)
                    .ok_or_else(|| Error::at(format!("undefined name `{base}`"), *base_span))?,
            };
            if !binding.mutable {
                return Err(Error::at(
                    format!("cannot assign to field of immutable binding `{base}`"),
                    *base_span,
                ));
            }
            let type_name = match &binding.ty {
                Ty::User(n) => n.clone(),
                other => {
                    return Err(Error::at(
                        format!("index assign on non-struct type {other:?}"),
                        *base_span,
                    ));
                }
            };
            let info = ctx
                .types
                .get(&type_name)
                .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), *base_span))?
                .clone();
            let finfo = info
                .fields
                .iter()
                .find(|f| f.name == *field)
                .ok_or_else(|| {
                    Error::at(format!("no field `{field}` on `{type_name}`"), *field_span)
                })?;
            if !finfo.mutable {
                return Err(Error::at(
                    format!("cannot assign to immutable field `{field}`"),
                    *field_span,
                ));
            }
            let (elem_ty, len) = match &finfo.ty {
                Ty::FixedArray { elem, len } => (elem.as_ref().clone(), *len),
                other => {
                    return Err(Error::at(
                        format!("cannot index non-array field type {other:?}"),
                        *field_span,
                    ));
                }
            };
            let (idx_p, idx_ty) = lower_expr(ctx, index)?;
            if !is_int(&idx_ty) {
                return Err(Error::at("array index must be an integer", index.span));
            }
            let idx_p = if idx_ty == Ty::Int64 {
                idx_p
            } else {
                coerce_place(ctx, idx_p, &idx_ty, &Ty::Int64, index.span)?
            };
            // Runtime bounds check left to C; const-fold check when index is Int literal.
            if let ExprKind::Int(n) = &index.kind {
                if *n < 0 || (*n as u64) >= len {
                    return Err(Error::at(
                        format!("index {n} out of bounds for Array of length {len}"),
                        index.span,
                    ));
                }
            }
            let (v, vty) = lower_expr(ctx, value)?;
            let v = coerce_to_field(ctx, v, &vty, &elem_ty, value.span)?;
            ctx.stmts.push(Stmt::IndexAssign {
                base: binding.place,
                field: sanitize_field(field),
                index: idx_p,
                ty: elem_ty,
                value: v,
                len,
            });
            Ok(())
        }
        AssignTarget::DynIndex {
            name,
            name_span,
            index,
        } => {
            let binding = ctx
                .lookup(name)
                .ok_or_else(|| Error::at(format!("undefined name `{name}`"), *name_span))?;
            assign_element(ctx, binding, &format!("binding `{name}`"), *name_span, index, value)
        }
    }
}

/// `items[index] = value` on a binding: an Array element or a Map entry,
/// stored through the receiver for a method's field (D107).
fn assign_element(
    ctx: &mut LowerCtx,
    binding: Binding,
    described: &str,
    span: Span,
    index: &Expr,
    value: &Expr,
) -> Result<()> {
    if !binding.mutable {
        return Err(Error::at(
            format!("cannot assign through immutable {described}"),
            span,
        ));
    }
    match &binding.ty {
        Ty::Array { elem } => {
            let elem_ty = elem.as_ref().clone();
            let (idx_p, idx_ty) = lower_expr(ctx, index)?;
            if !is_int(&idx_ty) {
                return Err(Error::at("array index must be an integer", index.span));
            }
            let idx_p = if idx_ty == Ty::Int64 {
                idx_p
            } else {
                coerce_place(ctx, idx_p, &idx_ty, &Ty::Int64, index.span)?
            };
            let (v, vty) = lower_expr(ctx, value)?;
            if vty != elem_ty {
                return Err(Error::at(
                    format!(
                        "array element type mismatch: expected {elem_ty:?}, found {vty:?}"
                    ),
                    value.span,
                ));
            }
            ctx.stmts.push(Stmt::DynIndexAssign {
                base: binding.place.clone(),
                index: idx_p,
                elem_ty,
                value: v,
            });
            write_back_field(ctx, &binding);
            Ok(())
        }
        Ty::Map { key, val } => {
            let key_ty = key.as_ref().clone();
            let val_ty = val.as_ref().clone();
            let (kp, kt) = lower_expr(ctx, index)?;
            if kt != key_ty {
                return Err(Error::at(
                    format!("Map key type mismatch: expected {key_ty:?}, found {kt:?}"),
                    index.span,
                ));
            }
            let (v, vty) = lower_expr(ctx, value)?;
            if vty != val_ty {
                return Err(Error::at(
                    format!("Map value type mismatch: expected {val_ty:?}, found {vty:?}"),
                    value.span,
                ));
            }
            let kp = coerce_map_key(ctx, kp, &key_ty);
            let map_ty = binding.ty.clone();
            ctx.assign(
                binding.place.clone(),
                map_ty,
                RValue::MapSet {
                    map: binding.place.clone(),
                    key: kp,
                    val: v,
                    key_ty,
                    val_ty,
                },
            );
            write_back_field(ctx, &binding);
            Ok(())
        }
        Ty::FixedArray { elem, len } => {
            let elem_ty = elem.as_ref().clone();
            let len = *len;
            let (idx_p, idx_ty) = lower_expr(ctx, index)?;
            if !is_int(&idx_ty) {
                return Err(Error::at("array index must be an integer", index.span));
            }
            let idx_p = if idx_ty == Ty::Int64 {
                idx_p
            } else {
                coerce_place(ctx, idx_p, &idx_ty, &Ty::Int64, index.span)?
            };
            if let ExprKind::Int(n) = &index.kind {
                if *n < 0 || (*n as u64) >= len {
                    return Err(Error::at(
                        format!("index {n} out of bounds for Array of length {len}"),
                        index.span,
                    ));
                }
            }
            let (v, vty) = lower_expr(ctx, value)?;
            let v = coerce_to_field(ctx, v, &vty, &elem_ty, value.span)?;
            ctx.stmts.push(Stmt::FixedIndexAssign {
                base: binding.place,
                index: idx_p,
                ty: elem_ty,
                value: v,
                len,
            });
            Ok(())
        }
        other => Err(Error::at(
            format!("index assign on non-Array type {other:?}"),
            span,
        )),
    }
}

/// Coerce/truncate a value for store into a typed field (width truncation at boundary).
fn coerce_to_field(
    ctx: &mut LowerCtx,
    place: Place,
    from: &Ty,
    to: &Ty,
    span: Span,
) -> Result<Place> {
    if from == to {
        return Ok(place);
    }
    if can_numeric_cast(from, to) {
        return coerce_place(ctx, place, from, to, span);
    }
    Err(Error::at(
        format!("cannot store {from:?} into field of type {to:?}"),
        span,
    ))
}

fn lower_expr(ctx: &mut LowerCtx, expr: &Expr) -> Result<(Place, Ty)> {
    lower_expr_expect(ctx, expr, None)
}

fn lower_expr_expect(ctx: &mut LowerCtx, expr: &Expr, expect: Option<&Ty>) -> Result<(Place, Ty)> {
    // Thread expect into `if` so branches can inject into an annotated union (D119).
    if let ExprKind::If {
        cond,
        then_branch,
        else_branch,
    } = &expr.kind
    {
        return lower_if(
            ctx,
            cond,
            then_branch,
            else_branch.as_ref(),
            expr.span,
            expect,
        );
    }
    // Empty `[]` as a call/return/let value: take elem type from expect.
    if is_empty_array_lit(expr) {
        if let Some(Ty::Array { elem }) = expect {
            return lower_empty_array_typed(ctx, elem.as_ref(), expr.span);
        }
    }
    // `{ ok: true }` / `[1, 2]` where a Json is expected (D182 §4).
    if let Some(json) = expect.filter(|_| codec::is_json_literal(ctx, expr, expect)) {
        return codec::lower_json_literal(ctx, expr, json);
    }
    // A map literal takes an expected key and value type (D213).
    if let (ExprKind::MapLit(entries), Some(Ty::Map { key, val })) = (&expr.kind, expect) {
        return lower_map_lit(ctx, entries, expr.span, Some((key.as_ref(), val.as_ref())));
    }
    // Fixed Array<T,N> from array literal when expected type is known (let/assign).
    if let Some(Ty::FixedArray { elem, len }) = expect {
        if let ExprKind::ArrayLit(elems) = &expr.kind {
            return lower_fixed_array_lit(ctx, elems, elem.as_ref(), *len, expr.span);
        }
    }
    match &expr.kind {
        ExprKind::Int(n) => {
            let p = ctx.fresh("i");
            ctx.assign(p.clone(), Ty::Int64, RValue::Int(*n));
            inhabit_literal(
                ctx,
                p,
                Ty::Int64,
                crate::ast::WhereLit::Int(*n),
                expect,
                expr.span,
            )
        }
        // Above the largest `Int64`: a `UInt64`, its bit pattern in the place
        // (read back unsigned by `literal_int_value`).
        ExprKind::BigInt(n) => {
            let p = ctx.fresh("big");
            ctx.assign(p.clone(), Ty::UInt64, RValue::Int(*n as i64));
            Ok((p, Ty::UInt64))
        }
        ExprKind::Float(n) => {
            let p = ctx.fresh("f");
            ctx.assign(p.clone(), Ty::Float64, RValue::Float(*n));
            inhabit_literal(
                ctx,
                p,
                Ty::Float64,
                crate::ast::WhereLit::Float(*n),
                expect,
                expr.span,
            )
        }
        ExprKind::Bool(b) => {
            let p = ctx.fresh("b");
            ctx.assign(p.clone(), Ty::Bool, RValue::Bool(*b));
            Ok((p, Ty::Bool))
        }
        ExprKind::String(s) => {
            let p = ctx.fresh("s");
            ctx.assign(p.clone(), Ty::String, RValue::StringLit(s.clone()));
            inhabit_literal(
                ctx,
                p,
                Ty::String,
                crate::ast::WhereLit::String(s.clone()),
                expect,
                expr.span,
            )
        }
        ExprKind::Interpolate(parts) => lower_interpolate(ctx, parts, expr.span),
        ExprKind::TaggedInterp { tag, parts } => lower_tagged_interp(ctx, tag, parts, expr.span),
        ExprKind::MutArg(name) => Err(Error::at(
            format!("`mut {name}` passes a binding to a `mut` parameter; it is only an argument"),
            expr.span,
        )),
        ExprKind::Name(name) => {
            // Unit value `Void` (type name used as value).
            if name == "Void" && ctx.lookup(name).is_none() {
                let p = ctx.fresh("void");
                return Ok((p, Ty::Void));
            }
            let found = ctx.lookup(name);
            // A property with a getter reads through it (D228).
            if let Some((type_name, getter)) =
                property_accessor(ctx, name, AccessorKind::Get, found.as_ref())
            {
                return lower_sibling_call(ctx, &type_name, &getter, &[], expr.span);
            }
            if found.is_none() {
                if let Some(error) = own_accessor_error(ctx, name, expr.span) {
                    return Err(error);
                }
                // A property of a built-in type's receiver, in its methods.
                if extension_sibling(ctx, &getter_name(name), expr.span) {
                    let property = Expr {
                        kind: ExprKind::Field {
                            base: Box::new(crate::ast_build::name("self")),
                            name: name.clone(),
                            name_span: expr.span,
                        },
                        span: expr.span,
                    };
                    return lower_expr(ctx, &property);
                }
            }
            if let Some(binding) = found {
                let ty = binding.ty.clone();
                let src = binding.place;
                // Simple RC: reading a heap / Trial-with-heap produces a new owner (`assign` dups).
                if needs_drop(&ty) {
                    let p = ctx.fresh("dup");
                    ctx.assign(p.clone(), ty.clone(), RValue::Use(src));
                    Ok((p, ty))
                } else {
                    Ok((src, ty))
                }
            } else if let Some(type_name) = (name == "self").then(|| self_receiver(ctx)).flatten() {
                Ok(lower_self_value(ctx, &type_name))
            } else if name == "self" {
                Err(Error::at("`self` is the receiver; it is only inside a method", expr.span))
            } else if ctx.name_to_fn.contains_key(name) {
                form_free_fn(ctx, name, expr.span)
            } else {
                Err(Error::at(format!("undefined name `{name}`"), expr.span))
            }
        }
        ExprKind::Unary { op, expr: inner } => {
            let (arg, arg_ty) = lower_expr(ctx, inner)?;
            match op {
                UnaryOp::Neg => {
                    if is_float(&arg_ty) {
                        let p = ctx.fresh("neg");
                        ctx.assign(
                            p.clone(),
                            Ty::Float64,
                            RValue::Unary {
                                op: UnaryOp::Neg,
                                arg,
                            },
                        );
                        return Ok((p, Ty::Float64));
                    }
                    if !is_int(&arg_ty) {
                        return Err(Error::at(
                            "unary `-` expects an integer or Float64",
                            expr.span,
                        ));
                    }
                    if arg_ty == Ty::UInt64 {
                        return Err(Error::at("a UInt64 cannot be negated", expr.span)
                            .with_help("`0 - x` wraps around, as UInt64 arithmetic does (D219)"));
                    }
                    let arg = if arg_ty == Ty::Int64 {
                        arg
                    } else {
                        coerce_place(ctx, arg, &arg_ty, &Ty::Int64, expr.span)?
                    };
                    let p = ctx.fresh("neg");
                    ctx.assign(
                        p.clone(),
                        Ty::Int64,
                        RValue::Unary {
                            op: UnaryOp::Neg,
                            arg,
                        },
                    );
                    Ok((p, Ty::Int64))
                }
                UnaryOp::Not => {
                    if arg_ty != Ty::Bool {
                        return Err(Error::at("`not` expects Bool", expr.span));
                    }
                    let p = ctx.fresh("not");
                    ctx.assign(p.clone(), Ty::Bool, RValue::Unary { op: *op, arg });
                    Ok((p, Ty::Bool))
                }
                UnaryOp::BitNot => {
                    if !is_int(&arg_ty) {
                        return Err(Error::at("bitwise `~` expects an integer", expr.span));
                    }
                    // A UInt64 keeps its 64 bits (D219).
                    if arg_ty == Ty::UInt64 {
                        let p = ctx.fresh("bnot");
                        ctx.assign(
                            p.clone(),
                            Ty::UInt64,
                            RValue::Unary {
                                op: UnaryOp::BitNot,
                                arg,
                            },
                        );
                        return Ok((p, Ty::UInt64));
                    }
                    // Compute in Int64 so width truncation happens on field store.
                    let arg = if arg_ty == Ty::Int64 {
                        arg
                    } else {
                        coerce_place(ctx, arg, &arg_ty, &Ty::Int64, expr.span)?
                    };
                    let p = ctx.fresh("bnot");
                    ctx.assign(
                        p.clone(),
                        Ty::Int64,
                        RValue::Unary {
                            op: UnaryOp::BitNot,
                            arg,
                        },
                    );
                    Ok((p, Ty::Int64))
                }
            }
        }
        ExprKind::Binary { op, left, right } => lower_binary(ctx, *op, left, right, expr.span),
        ExprKind::Call { callee, args, type_args } => lower_call(ctx, callee, args, expr.span, expect, type_args),
        ExprKind::IndirectCall { callee, args } => {
            let (fun, fun_ty) = lower_expr(ctx, callee)?;
            lower_indirect_call(ctx, fun, fun_ty, args, expr.span)
        }
        ExprKind::Start { callee, args, .. } => lower_start(ctx, callee, args, expr.span),
        ExprKind::Field {
            base,
            name,
            name_span,
        } => lower_field(ctx, base, name, *name_span),
        ExprKind::Index { base, index } => lower_index(ctx, base, index, expr.span),
        ExprKind::ArrayLit(elems) => lower_array_lit(ctx, elems, expr.span, expect),
        ExprKind::MapLit(entries) => lower_map_lit(ctx, entries, expr.span, None),
        ExprKind::MethodCall {
            receiver,
            method,
            method_span,
            args,
            type_args,
        } => lower_method_call_typed(ctx, receiver, method, *method_span, args, type_args),
        ExprKind::If { .. } => unreachable!("If handled in lower_expr_expect"),
        ExprKind::While { cond, body } => lower_while(ctx, cond, body, expr.span),
        ExprKind::Loop { body } => lower_loop(ctx, body, expr.span),
        ExprKind::For {
            name,
            name_span,
            value,
            start,
            end,
            inclusive,
            body,
        } => lower_for(
            ctx,
            name,
            *name_span,
            value.as_ref(),
            start,
            end.as_deref(),
            *inclusive,
            body,
            expr.span,
        ),
        ExprKind::Break => lower_break(ctx, expr.span),
        ExprKind::Continue => lower_continue(ctx, expr.span),
        ExprKind::Return { value } => lower_return(ctx, value.as_deref(), expr.span),
        ExprKind::Block(b) => {
            if b.tail.is_none() {
                lower_block_value(ctx, b, &Ty::Void)?;
                let p = ctx.fresh("void");
                ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
                return Ok((p, Ty::Void));
            }
            ctx.push_scope();
            lower_block_items(ctx, &b.items)?;
            let (place, ty) = lower_expr(ctx, b.tail.as_ref().unwrap())?;
            ctx.pop_scope();
            Ok((place, ty))
        }
        ExprKind::Try(inner) => lower_try(ctx, inner, expr.span),
        ExprKind::Cast {
            expr: inner,
            ty,
            propagate,
        } => convert::lower_cast(ctx, inner, ty, *propagate, expr.span),
        ExprKind::Match { scrutinee, arms } => lower_match(ctx, scrutinee, arms, expr.span, expect),
    }
}

fn void_place(ctx: &mut LowerCtx) -> (Place, Ty) {
    let p = ctx.fresh("u");
    ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
    (p, Ty::Void)
}

fn lower_break(ctx: &mut LowerCtx, span: Span) -> Result<(Place, Ty)> {
    if ctx.loop_depth == 0 {
        return Err(Error::at("`break` outside of a loop", span));
    }
    ctx.stmts.push(Stmt::Break);
    Ok(void_place(ctx))
}

fn lower_continue(ctx: &mut LowerCtx, span: Span) -> Result<(Place, Ty)> {
    if ctx.loop_depth == 0 {
        return Err(Error::at("`continue` outside of a loop", span));
    }
    ctx.stmts.push(Stmt::Continue);
    Ok(void_place(ctx))
}

/// `return` / `return expr` — type-check against `fn_ret`, drop live heap locals, emit C return.
/// The expression types as `fn_ret` so it unifies with sibling if/match arms.
fn lower_return(ctx: &mut LowerCtx, value: Option<&Expr>, span: Span) -> Result<(Place, Ty)> {
    // D129 probe: collect exit types only.
    if ctx.inferring {
        match value {
            None => ctx.infer_values.push(Ty::Void),
            Some(e) => {
                let (_, ty) = lower_expr(ctx, e)?;
                if !is_never(&ty) {
                    ctx.infer_values.push(ty);
                }
            }
        }
        let (p, _) = void_place(ctx);
        return Ok((p, Ty::Never));
    }

    let fn_ret = ctx.fn_ret.clone();
    let ret_place = match value {
        None => {
            if fn_ret != Ty::Void {
                return Err(Error::at(
                    format!("`return` without a value in function returning {fn_ret:?}"),
                    span,
                ));
            }
            None
        }
        Some(e) => {
            // The return type is the expected type, as for a block's value
            // (`return []`, `return Circle(…)` into a union, D192).
            let expect = (fn_ret != Ty::Void).then(|| fn_ret.clone());
            let (place, ty) = lower_expr_expect(ctx, e, expect.as_ref())?;
            if ty != fn_ret {
                if let Some(place) = coerce_to_expected(ctx, place.clone(), &ty, &fn_ret, e)? {
                    Some(place)
                } else if fn_ret == Ty::Void && ty == Ty::Void {
                    None
                } else {
                    return Err(Error::at(
                        if ctx.route_clause {
                            ctx.type_mismatch_msg(&fn_ret, &ty)
                        } else {
                            format!("return type mismatch: expected {fn_ret:?}, found {ty:?}")
                        },
                        e.span,
                    ));
                }
            } else if fn_ret == Ty::Void {
                None
            } else {
                Some(place)
            }
        }
    };

    push_early_return(ctx, ret_place.clone(), &fn_ret);

    // `return` diverges: Never fits any `if` / `match` arm, so
    // `if c { return x } else { y }` takes `y`'s type, whatever it is.
    let (p, _) = void_place(ctx);
    Ok((p, Ty::Never))
}

/// Emit `Return` only. Drops are filled after the whole function is lowered
/// (`fill_early_return_drops`) so loop temps assigned later still get dropped
/// (JSON `return Pass(freeze())` after an escape iteration).
fn push_early_return(ctx: &mut LowerCtx, ret: Option<Place>, _ret_ty: &Ty) {
    ctx.stmts.push(Stmt::Return { value: ret });
}

fn append_return_with_drops(
    stmts: &mut Vec<Stmt>,
    _drop_places: &[(String, Ty)],
    ret: Option<Place>,
    _ret_ty: &Ty,
) {
    stmts.push(Stmt::Return { value: ret });
}

/// Before every `Return`, drop the final `drop_places` set (NULL locals are
/// no-ops). Snapshots taken while lowering missed temps assigned later in a loop.
fn fill_early_return_drops(
    stmts: Vec<Stmt>,
    drop_places: &[(String, Ty)],
    ret_ty: &Ty,
) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        match stmt {
            Stmt::Return { value } => {
                let skip = if needs_drop(ret_ty) {
                    value.as_ref().map(|p| p.name.clone())
                } else {
                    None
                };
                let mut seen = std::collections::HashSet::new();
                for (name, ty) in drop_places {
                    if skip.as_ref() == Some(name) {
                        continue;
                    }
                    if seen.insert(name.clone()) {
                        out.push(Stmt::Drop {
                            place: Place { name: name.clone() },
                            ty: ty.clone(),
                        });
                    }
                }
                out.push(Stmt::Return { value });
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => out.push(Stmt::If {
                cond,
                then_body: fill_early_return_drops(then_body, drop_places, ret_ty),
                else_body: fill_early_return_drops(else_body, drop_places, ret_ty),
            }),
            Stmt::Loop { body } => out.push(Stmt::Loop {
                body: fill_early_return_drops(body, drop_places, ret_ty),
            }),
            Stmt::ForRange {
                index,
                end,
                inclusive,
                body,
            } => out.push(Stmt::ForRange {
                index,
                end,
                inclusive,
                body: fill_early_return_drops(body, drop_places, ret_ty),
            }),
            Stmt::StoreBack { callees, body } => out.push(Stmt::StoreBack {
                callees,
                body: fill_early_return_drops(body, drop_places, ret_ty),
            }),
            other => out.push(other),
        }
    }
    out
}

fn lower_loop(ctx: &mut LowerCtx, body: &Block, _span: Span) -> Result<(Place, Ty)> {
    let outer = std::mem::take(&mut ctx.stmts);
    ctx.loop_depth += 1;
    ctx.push_scope();
    lower_block_into(ctx, body, &Ty::Void)?;
    ctx.pop_scope();
    ctx.loop_depth -= 1;
    let body_stmts = std::mem::take(&mut ctx.stmts);
    ctx.stmts = outer;
    ctx.stmts.push(Stmt::Loop { body: body_stmts });
    // No `break` that can leave this loop ⇒ diverges (D128). Nested loops' breaks
    // only exit the nested loop, so they do not count for the outer.
    if block_has_break(body) {
        Ok(void_place(ctx))
    } else {
        let (p, _) = void_place(ctx);
        Ok((p, Ty::Never))
    }
}

/// True if `break` can exit this block's enclosing `loop` (depth 0), not only a nested loop.
fn block_has_break(block: &Block) -> bool {
    block_items_have_break(&block.items, 0)
        || block
            .tail
            .as_ref()
            .map(|e| expr_has_break(e, 0))
            .unwrap_or(false)
}

fn block_items_have_break(items: &[BlockItem], depth: u32) -> bool {
    for item in items {
        match item {
            BlockItem::Expr(e) | BlockItem::Let { init: e, .. } => {
                if expr_has_break(e, depth) {
                    return true;
                }
            }
            BlockItem::Assign { value, .. } => {
                if expr_has_break(value, depth) {
                    return true;
                }
            }
        }
    }
    false
}

fn expr_has_break(expr: &Expr, depth: u32) -> bool {
    use crate::ast::{Arg, InterpPart};
    match &expr.kind {
        ExprKind::Break => depth == 0,
        ExprKind::Loop { body } => block_has_break_at(body, depth + 1),
        ExprKind::While { cond, body } => {
            expr_has_break(cond, depth) || block_has_break_at(body, depth + 1)
        }
        ExprKind::For {
            start, end, body, ..
        } => {
            expr_has_break(start, depth)
                || end
                    .as_ref()
                    .map(|e| expr_has_break(e, depth))
                    .unwrap_or(false)
                || block_has_break_at(body, depth + 1)
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_break(cond, depth)
                || block_has_break_at(then_branch, depth)
                || else_branch
                    .as_ref()
                    .map(|b| block_has_break_at(b, depth))
                    .unwrap_or(false)
        }
        ExprKind::Match { scrutinee, arms } => {
            expr_has_break(scrutinee, depth)
                || arms.iter().any(|a| block_has_break_at(&a.body, depth))
        }
        ExprKind::Block(b) => block_has_break_at(b, depth),
        ExprKind::Unary { expr, .. } => expr_has_break(expr, depth),
        ExprKind::Binary { left, right, .. } => {
            expr_has_break(left, depth) || expr_has_break(right, depth)
        }
        ExprKind::Call { args, .. } | ExprKind::Start { args, .. } => {
            args.iter().any(|a| match a {
                Arg::Pos(e) | Arg::Named { value: e, .. } | Arg::Spread { value: e, .. } => {
                    expr_has_break(e, depth)
                }
            })
        }
        ExprKind::IndirectCall { callee, args } => {
            expr_has_break(callee, depth)
                || args.iter().any(|a| match a {
                    Arg::Pos(e) | Arg::Named { value: e, .. } | Arg::Spread { value: e, .. } => {
                        expr_has_break(e, depth)
                    }
                })
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            expr_has_break(receiver, depth)
                || args.iter().any(|a| match a {
                    Arg::Pos(e) | Arg::Named { value: e, .. } | Arg::Spread { value: e, .. } => {
                        expr_has_break(e, depth)
                    }
                })
        }
        ExprKind::Field { base, .. } => expr_has_break(base, depth),
        ExprKind::Index { base, index } => {
            expr_has_break(base, depth) || expr_has_break(index, depth)
        }
        ExprKind::Return { value } => value
            .as_ref()
            .map(|e| expr_has_break(e, depth))
            .unwrap_or(false),
        ExprKind::Try(e) | ExprKind::Cast { expr: e, .. } => expr_has_break(e, depth),
        ExprKind::ArrayLit(elems) => elems.iter().any(|e| expr_has_break(e, depth)),
        ExprKind::MapLit(entries) => entries
            .iter()
            .flat_map(|entry| entry.exprs())
            .any(|e| expr_has_break(e, depth)),
        ExprKind::Interpolate(parts) | ExprKind::TaggedInterp { parts, .. } => {
            parts.iter().any(|p| match p {
                InterpPart::Expr(e) => expr_has_break(e, depth),
                InterpPart::Lit(_) => false,
            })
        }
        ExprKind::Continue
        | ExprKind::Name(_)
        | ExprKind::MutArg(_)
        | ExprKind::Int(_) | ExprKind::BigInt(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_) => false,
    }
}

fn block_has_break_at(block: &Block, depth: u32) -> bool {
    block_items_have_break(&block.items, depth)
        || block
            .tail
            .as_ref()
            .map(|e| expr_has_break(e, depth))
            .unwrap_or(false)
}

fn lower_while(ctx: &mut LowerCtx, cond: &Expr, body: &Block, _span: Span) -> Result<(Place, Ty)> {
    // Desugar: loop { if not cond { break }; body }
    let outer = std::mem::take(&mut ctx.stmts);
    ctx.loop_depth += 1;

    let (cond_p, cond_ty) = lower_expr(ctx, cond)?;
    if cond_ty != Ty::Bool {
        ctx.loop_depth -= 1;
        ctx.stmts = outer;
        return Err(Error::at("`while` condition must be Bool", cond.span));
    }
    let not_p = ctx.fresh("ncond");
    ctx.assign(
        not_p.clone(),
        Ty::Bool,
        RValue::Unary {
            op: UnaryOp::Not,
            arg: cond_p,
        },
    );
    ctx.stmts.push(Stmt::If {
        cond: not_p,
        then_body: vec![Stmt::Break],
        else_body: Vec::new(),
    });

    ctx.push_scope();
    lower_block_into(ctx, body, &Ty::Void)?;
    ctx.pop_scope();

    ctx.loop_depth -= 1;
    let body_stmts = std::mem::take(&mut ctx.stmts);
    ctx.stmts = outer;
    ctx.stmts.push(Stmt::Loop { body: body_stmts });
    Ok(void_place(ctx))
}

fn lower_for(
    ctx: &mut LowerCtx,
    name: &str,
    name_span: Span,
    value: Option<&(String, Span)>,
    start: &Expr,
    end: Option<&Expr>,
    inclusive: bool,
    body: &Block,
    _span: Span,
) -> Result<(Place, Ty)> {
    // Collection iteration: `for x in xs` / `for k, v in m`
    if end.is_none() {
        if let Some((vname, vspan)) = value {
            return lower_for_map(ctx, name, name_span, vname, *vspan, start, body);
        }
        return lower_for_array(ctx, name, name_span, start, body);
    }
    if value.is_some() {
        return Err(Error::at(
            "`for k, v` is only for Map iteration (not ranges)",
            name_span,
        ));
    }
    let end = end.unwrap();

    let (start_p, start_ty) = lower_expr(ctx, start)?;
    if !is_int(&start_ty) {
        return Err(Error::at("for-range start must be an integer", start.span));
    }
    let start_p = if start_ty == Ty::Int64 {
        start_p
    } else {
        coerce_place(ctx, start_p, &start_ty, &Ty::Int64, start.span)?
    };

    let (end_p, end_ty) = lower_expr(ctx, end)?;
    if !is_int(&end_ty) {
        return Err(Error::at("for-range end must be an integer", end.span));
    }
    let end_p = if end_ty == Ty::Int64 {
        end_p
    } else {
        coerce_place(ctx, end_p, &end_ty, &Ty::Int64, end.span)?
    };

    let idx = ctx.local_place(name);
    // Init index before the C for-loop (step/continue-safe).
    ctx.assign(idx.clone(), Ty::Int64, RValue::Use(start_p));

    let outer = std::mem::take(&mut ctx.stmts);
    ctx.loop_depth += 1;
    ctx.push_scope();
    ctx.define(name, Ty::Int64, false, idx.clone());
    // Silence unused if name shadows; name_span kept for future errors.
    let _ = name_span;
    lower_block_into(ctx, body, &Ty::Void)?;
    ctx.pop_scope();
    ctx.loop_depth -= 1;
    let body_stmts = std::mem::take(&mut ctx.stmts);
    ctx.stmts = outer;
    ctx.stmts.push(Stmt::ForRange {
        index: idx,
        end: end_p,
        inclusive,
        body: body_stmts,
    });
    Ok(void_place(ctx))
}

/// `for x in arr` — index 0..count, bind element each iteration.
fn lower_for_array(
    ctx: &mut LowerCtx,
    name: &str,
    name_span: Span,
    collection: &Expr,
    body: &Block,
) -> Result<(Place, Ty)> {
    let (arr_p, arr_ty) = lower_expr(ctx, collection)?;
    // An Array is a `Sequence` too, but read directly; a Map with one name
    // gives its entries, and a String its scalars (D236).
    let built_in = matches!(arr_ty, Ty::Array { .. });
    if !built_in && (ctx.conforms(&arr_ty, "Iterator") || ctx.conforms(&arr_ty, "Sequence")) {
        return lower_for_cursor(ctx, name, name_span, arr_p, arr_ty, collection.span, body);
    }
    let elem_ty = match &arr_ty {
        Ty::Array { elem } => elem.as_ref().clone(),
        Ty::Map { .. } => {
            return Err(Error::at(
                "`for x in` on a Map is not allowed (use `for k, v in` or `.keys()` / `.values()`)",
                collection.span,
            ));
        }
        other => {
            return Err(Error::at(
                format!(
                    "`for … in` takes an Array, a Map, or a value that conforms to `Iterator` or `Sequence`, and {} does not",
                    display_ty(other)
                ),
                collection.span,
            ));
        }
    };
    let _ = name_span;

    let count_p = ctx.fresh("acnt");
    ctx.assign(
        count_p.clone(),
        Ty::Int64,
        RValue::Call {
            callee: Callee::Builtin("array_count".into()),
            args: vec![arr_p.clone()],
        },
    );

    let idx = ctx.fresh("ai");
    ctx.assign(idx.clone(), Ty::Int64, RValue::Int(0));

    // Loop variable lives across iterations; end-of-fn drops the last value.
    // Fresh C name so a nested `for x` does not NULL an outer `let x` (jsx).
    let elem_p = ctx.fresh(&format!("fv_{}", sanitize_name(name)));
    init_place(ctx, &elem_p, &elem_ty)?;

    let outer = std::mem::take(&mut ctx.stmts);
    ctx.loop_depth += 1;
    ctx.push_scope();
    ctx.define(name, elem_ty.clone(), false, elem_p.clone());

    read_array_elem(ctx, &arr_p, &idx, &elem_p, &elem_ty);
    lower_block_into(ctx, body, &Ty::Void)?;
    ctx.pop_scope();
    ctx.loop_depth -= 1;
    let body_stmts = std::mem::take(&mut ctx.stmts);
    ctx.stmts = outer;
    ctx.stmts.push(Stmt::ForRange {
        index: idx,
        end: count_p,
        inclusive: false,
        body: body_stmts,
    });
    Ok(void_place(ctx))
}

/// `for item in value` over a type that conforms to `Iterator` or `Sequence`
/// (D228 §7): a copy of the iterator (or the cursor a sequence's `iterate()`
/// makes) gives `next()` until it is `Not`.
///
/// ```text
/// mut cursor = value            // value.iterate() for a Sequence
/// loop { match cursor.next() { Has(item) => body  Not => break } }
/// ```
fn lower_for_cursor(
    ctx: &mut LowerCtx,
    name: &str,
    name_span: Span,
    value: Place,
    value_ty: Ty,
    span: Span,
    body: &Block,
) -> Result<(Place, Ty)> {
    let iterator = ctx.conforms(&value_ty, "Iterator");
    ctx.push_scope();
    ctx.define("__for_value", value_ty, false, value);
    let named = |text: &str| Expr {
        kind: ExprKind::Name(text.to_string()),
        span,
    };
    let method = |receiver: Expr, method: &str| Expr {
        kind: ExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: method.to_string(),
            method_span: span,
            args: Vec::new(),
            type_args: Vec::new(),
        },
        span,
    };
    let init = if iterator {
        named("__for_value")
    } else {
        method(named("__for_value"), "iterate")
    };
    let arms = vec![
        MatchArm {
            pattern: Pattern::Variant {
                name: "Has".into(),
                type_args: Vec::new(),
                binds: vec![(name != "_").then(|| name.to_string())],
                bind_span: name_span,
                span: name_span,
            },
            guard: None,
            body: body.clone(),
        },
        MatchArm {
            pattern: Pattern::Variant {
                name: "Not".into(),
                type_args: Vec::new(),
                binds: Vec::new(),
                bind_span: span,
                span,
            },
            guard: None,
            body: tail_block(Expr {
                kind: ExprKind::Break,
                span,
            }),
        },
    ];
    let lowered = Block {
        items: vec![BlockItem::Let {
            name: "__for_cursor".into(),
            name_span: span,
            mutable: true,
            ty: None,
            init,
        }],
        tail: Some(Box::new(Expr {
            kind: ExprKind::Loop {
                body: tail_block(Expr {
                    kind: ExprKind::Match {
                        scrutinee: Box::new(method(named("__for_cursor"), "next")),
                        arms,
                    },
                    span,
                }),
            },
            span,
        })),
        span,
    };
    let result = lower_expr(
        ctx,
        &Expr {
            kind: ExprKind::Block(lowered),
            span,
        },
    );
    ctx.pop_scope();
    result?;
    Ok(void_place(ctx))
}

/// Load `arr[index]` into `elem`, which owns it (`for x in xs`, D178 `==`).
/// `elem` must have been set up with `init_place`: the load drops what it
/// held before.
fn read_array_elem(ctx: &mut LowerCtx, arr: &Place, index: &Place, elem: &Place, elem_ty: &Ty) {
    if array_elem_is_user(elem_ty) {
        // ArrayGet (cgen) field-drops prior ownership of `elem`, then memcpy + field-dup.
        // No separate Drop here — would double-free with the get pre-drop.
        ctx.stmts.push(Stmt::Assign {
            place: elem.clone(),
            ty: elem_ty.clone(),
            value: RValue::ArrayGet {
                arr: arr.clone(),
                index: index.clone(),
                elem_ty: elem_ty.clone(),
            },
        });
    } else if array_elem_is_heap(elem_ty) {
        // Drop previous iteration's ownership (no-op when NULL on first entry).
        ctx.stmts.push(Stmt::Drop {
            place: elem.clone(),
            ty: elem_ty.clone(),
        });
        ctx.stmts.push(Stmt::Assign {
            place: elem.clone(),
            ty: elem_ty.clone(),
            value: RValue::Call {
                callee: Callee::Builtin("array_get_ptr".into()),
                args: vec![arr.clone(), index.clone()],
            },
        });
        // get_ptr does not dup; own a ref for the loop variable lifetime.
        ctx.stmts.push(Stmt::Dup {
            place: elem.clone(),
            ty: elem_ty.clone(),
        });
    } else if *elem_ty == Ty::Bool {
        let raw = ctx.fresh("araw");
        ctx.stmts.push(Stmt::Assign {
            place: raw.clone(),
            ty: Ty::Int64,
            value: RValue::Call {
                callee: Callee::Builtin("array_get_i64".into()),
                args: vec![arr.clone(), index.clone()],
            },
        });
        ctx.stmts.push(Stmt::Assign {
            place: elem.clone(),
            ty: Ty::Bool,
            value: RValue::Cast {
                value: raw,
                from: Ty::Int64,
                to: Ty::Bool,
            },
        });
    } else if *elem_ty == Ty::Float64 {
        let raw = ctx.fresh("araw");
        ctx.stmts.push(Stmt::Assign {
            place: raw.clone(),
            ty: Ty::Int64,
            value: RValue::Call {
                callee: Callee::Builtin("array_get_i64".into()),
                args: vec![arr.clone(), index.clone()],
            },
        });
        ctx.stmts.push(Stmt::Assign {
            place: elem.clone(),
            ty: Ty::Float64,
            value: RValue::Call {
                callee: Callee::Builtin("f64_from_bits".into()),
                args: vec![raw],
            },
        });
    } else {
        ctx.stmts.push(Stmt::Assign {
            place: elem.clone(),
            ty: elem_ty.clone(),
            value: RValue::Call {
                callee: Callee::Builtin("array_get_i64".into()),
                args: vec![arr.clone(), index.clone()],
            },
        });
    }
}

fn lower_fixed_array_lit(
    ctx: &mut LowerCtx,
    elems: &[Expr],
    elem: &Ty,
    len: u64,
    span: Span,
) -> Result<(Place, Ty)> {
    if elems.len() as u64 != len {
        return Err(Error::at(
            format!(
                "fixed Array length mismatch: expected {len}, found {}",
                elems.len()
            ),
            span,
        ));
    }
    if !is_abi_scalar(elem) {
        return Err(Error::at(
            format!("fixed Array element must be an ABI scalar, got {elem:?}"),
            span,
        ));
    }
    let mut places = Vec::new();
    for e in elems {
        let (p, ty) = lower_expr(ctx, e)?;
        let p = coerce_to_field(ctx, p, &ty, elem, e.span)?;
        places.push(p);
    }
    let arr_ty = Ty::FixedArray {
        elem: Box::new(elem.clone()),
        len,
    };
    let p = ctx.fresh("falit");
    ctx.assign(
        p.clone(),
        arr_ty.clone(),
        RValue::FixedArrayLit {
            elems: places,
            elem_ty: elem.clone(),
            len,
        },
    );
    Ok((p, arr_ty))
}

fn lower_array_lit(
    ctx: &mut LowerCtx,
    elems: &[Expr],
    span: Span,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
    if elems.is_empty() {
        // Untyped empty — bind site must supply Array<T> annotation.
        let p = ctx.fresh("aempty");
        ctx.assign(
            p.clone(),
            Ty::Array {
                elem: Box::new(Ty::Void),
            },
            RValue::ArrayLit {
                elems: vec![],
                elem_ty: Ty::Void,
            },
        );
        return Ok((
            p,
            Ty::Array {
                elem: Box::new(Ty::Void),
            },
        ));
    }
    // An expected element type (an annotation, a parameter, a return) types
    // every element: a union member widens to the union, an integer to
    // Float64, and a nested `[]` takes the element type (D184).
    let wanted = match expect {
        Some(Ty::Array { elem }) if **elem != Ty::Void => Some(elem.as_ref().clone()),
        _ => None,
    };
    let constants = match expect {
        None => true,
        Some(Ty::Array { elem }) => matches!(**elem, Ty::Int64 | Ty::Void),
        Some(_) => false,
    };
    if constants {
        if let Some(values) = elems.iter().map(int_constant).collect::<Option<Vec<i64>>>() {
            let ty = Ty::Array {
                elem: Box::new(Ty::Int64),
            };
            let p = ctx.fresh("alit");
            ctx.assign(p.clone(), ty.clone(), RValue::IntArray(values));
            return Ok((p, ty));
        }
    }
    let mut places = Vec::new();
    let mut elem_ty: Option<Ty> = None;
    for e in elems {
        let (p, ty) = match &wanted {
            Some(want) => {
                let (p, ty) = lower_expr_expect(ctx, e, Some(want))?;
                coerce_element(ctx, p, &ty, want, e)?
            }
            None => lower_expr(ctx, e)?,
        };
        match &elem_ty {
            None => {
                if !is_array_elem_ty(&ty) {
                    return Err(Error::at(
                        format!("array element type not supported: {ty:?}"),
                        e.span,
                    ));
                }
                if array_elem_is_user(&ty) && !is_array_user_elem(&ty, &ctx.types) {
                    return Err(Error::at(
                        format!(
                            "array element User type must be constructor-param with Int64/Bool/String/Bytes/Node/Array/Map/User fields: {ty:?}"
                        ),
                        e.span,
                    ));
                }
                elem_ty = Some(ty.clone());
            }
            Some(expect) if *expect != ty => {
                return Err(Error::at(
                    format!("heterogeneous array: expected {expect:?}, found {ty:?}"),
                    e.span,
                ));
            }
            Some(_) => {}
        }
        places.push(p);
    }
    let elem_ty = elem_ty.unwrap();
    let arr_ty = Ty::Array {
        elem: Box::new(elem_ty.clone()),
    };
    let p = ctx.fresh("alit");
    ctx.assign(
        p.clone(),
        arr_ty.clone(),
        RValue::ArrayLit {
            elems: places,
            elem_ty,
        },
    );
    let _ = span;
    Ok((p, arr_ty))
}

/// An integer literal, negated or not.
fn int_constant(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::Int(value) => Some(*value),
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => match expr.kind {
            ExprKind::Int(value) => value.checked_neg(),
            _ => None,
        },
        _ => None,
    }
}

/// An array element as the expected element type when it converts (as an
/// annotated `let` would); otherwise unchanged, and the heterogeneous-array
/// check reports it.
fn coerce_element(
    ctx: &mut LowerCtx,
    place: Place,
    ty: &Ty,
    want: &Ty,
    element: &Expr,
) -> Result<(Place, Ty)> {
    if ty == want {
        return Ok((place, ty.clone()));
    }
    let literal = expr_as_type_lit(element);
    if let Some(p) = try_sum_coerce(ctx, place.clone(), ty, want, literal.as_ref(), element.span)? {
        return Ok((p, want.clone()));
    }
    if can_numeric_cast(ty, want) {
        return Ok((coerce_place(ctx, place, ty, want, element.span)?, want.clone()));
    }
    if let Some(p) = try_refine_coerce(ctx, place.clone(), ty, want, literal.as_ref(), element.span)? {
        return Ok((p, want.clone()));
    }
    if let Some(p) = convert::encode_where_json_expected(ctx, place.clone(), ty, want)? {
        return Ok((p, want.clone()));
    }
    Ok((place, ty.clone()))
}

fn coerce_map_key(ctx: &mut LowerCtx, key: Place, key_ty: &Ty) -> Place {
    // A user key travels as its address; the map copies it only to store it.
    if let Ty::User(_) = key_ty {
        let p = ctx.fresh("mkey");
        ctx.assign(
            p.clone(),
            Ty::Address(Box::new(key_ty.clone())),
            RValue::AddressOf(key),
        );
        return p;
    }
    // A Float64 key travels as SameValueZero bits: every NaN alike, -0.0 as
    // 0.0 (D214).
    if *key_ty == Ty::Float64 {
        let p = ctx.fresh("mkey");
        ctx.assign(
            p.clone(),
            Ty::Int64,
            RValue::Call {
                callee: Callee::Builtin("f64_key_bits".into()),
                args: vec![key],
            },
        );
        return p;
    }
    if *key_ty == Ty::Bool {
        let p = ctx.fresh("mkey");
        ctx.assign(
            p.clone(),
            Ty::Int64,
            RValue::Cast {
                value: key,
                from: Ty::Bool,
                to: Ty::Int64,
            },
        );
        p
    } else {
        key
    }
}

fn lower_map_new(
    ctx: &mut LowerCtx,
    args: &[Arg],
    span: Span,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
    if !args.is_empty() {
        return Err(Error::at("`Map()` takes no arguments", span));
    }
    let Some(Ty::Map { key, val }) = expect else {
        return Err(Error::at(
            "`Map()` needs a type annotation (e.g. `let m: Map<String, Int64> = Map()`)",
            span,
        ));
    };
    check_map_key(ctx, key, span)?;
    let ty = Ty::Map {
        key: key.clone(),
        val: val.clone(),
    };
    let p = ctx.fresh("mnew");
    ctx.assign(
        p.clone(),
        ty.clone(),
        RValue::MapNew {
            key_ty: key.as_ref().clone(),
            val_ty: val.as_ref().clone(),
        },
    );
    Ok((p, ty))
}

/// `{k: v, …}`. An expected `Map<K, V>` (an annotation, a parameter, a field)
/// types every key and value, as an expected element type does an array's
/// (D184): a member widens to its union, an integer to Float64, and `[]` or
/// a nested `{…}` takes the type (D213).
fn lower_map_lit(
    ctx: &mut LowerCtx,
    entries: &[MapEntry],
    span: Span,
    expect: Option<(&Ty, &Ty)>,
) -> Result<(Place, Ty)> {
    if entries.is_empty() {
        return Err(Error::at(
            "empty `{}` is a block; use `Map()` for an empty map",
            span,
        ));
    }
    let mut pairs_in: Vec<(&Expr, &Expr)> = Vec::new();
    for entry in entries {
        match entry {
            MapEntry::Pair(k, v) => pairs_in.push((k, v)),
            MapEntry::Spread(v) => {
                return Err(Error::at(
                    "`...` spreads entries into a Json literal; a Map literal lists its entries",
                    v.span,
                ))
            }
        }
    }
    let mut key_ty: Option<Ty> = None;
    let mut val_ty: Option<Ty> = None;
    let mut seen_keys: Vec<(Ty, String)> = Vec::new();
    let mut pairs: Vec<(Place, Place)> = Vec::new();
    for (k, v) in pairs_in {
        let (kp, kt) = match expect {
            Some((want, _)) => {
                let (kp, kt) = lower_expr_expect(ctx, k, Some(want))?;
                coerce_element(ctx, kp, &kt, want, k)?
            }
            None => lower_expr(ctx, k)?,
        };
        if !is_map_key_ty(&kt) {
            return Err(Error::at(
                format!("Map key type not supported: {kt:?}"),
                k.span,
            ));
        }
        match &key_ty {
            None => key_ty = Some(kt.clone()),
            Some(expect) if *expect != kt => {
                return Err(Error::at(
                    format!("heterogeneous map keys: expected {expect:?}, found {kt:?}"),
                    k.span,
                ));
            }
            Some(_) => {}
        }
        let key_id = match &k.kind {
            ExprKind::String(s) => format!("s:{s}"),
            ExprKind::Int(n) => format!("i:{n}"),
            ExprKind::Bool(b) => format!("b:{b}"),
            _ => String::new(),
        };
        if !key_id.is_empty() {
            if seen_keys.iter().any(|(_, id)| id == &key_id) {
                return Err(Error::at("duplicate key in map literal", k.span));
            }
            seen_keys.push((kt.clone(), key_id));
        }
        let (vp, vt) = match expect {
            Some((_, want)) => {
                let (vp, vt) = lower_expr_expect(ctx, v, Some(want))?;
                coerce_element(ctx, vp, &vt, want, v)?
            }
            None => lower_expr(ctx, v)?,
        };
        if !is_map_val_ty(&vt) {
            return Err(Error::at(
                format!("Map value type not supported: {vt:?}"),
                v.span,
            ));
        }
        match &val_ty {
            None => val_ty = Some(vt.clone()),
            Some(expect) if *expect != vt => {
                return Err(Error::at(
                    format!("heterogeneous map values: expected {expect:?}, found {vt:?}"),
                    v.span,
                ));
            }
            Some(_) => {}
        }
        pairs.push((kp, vp));
    }
    let key_ty = key_ty.unwrap();
    let val_ty = val_ty.unwrap();
    check_map_key(ctx, &key_ty, span)?;
    let map_ty = Ty::Map {
        key: Box::new(key_ty.clone()),
        val: Box::new(val_ty.clone()),
    };
    let p = ctx.fresh("mlit");
    ctx.assign(
        p.clone(),
        map_ty.clone(),
        RValue::MapNew {
            key_ty: key_ty.clone(),
            val_ty: val_ty.clone(),
        },
    );
    for (k, v) in pairs {
        let k = coerce_map_key(ctx, k, &key_ty);
        ctx.assign(
            p.clone(),
            map_ty.clone(),
            RValue::MapSet {
                map: p.clone(),
                key: k,
                val: v,
                key_ty: key_ty.clone(),
                val_ty: val_ty.clone(),
            },
        );
    }
    Ok((p, map_ty))
}

fn lower_map_get(
    ctx: &mut LowerCtx,
    map: Place,
    key: Place,
    key_ty: &Ty,
    val_ty: &Ty,
    span: Span,
) -> Result<(Place, Ty)> {
    let key = coerce_map_key(ctx, key, key_ty);
    let out = ctx.fresh("mgv");
    init_place(ctx, &out, val_ty)?;
    let found = ctx.fresh("mgf");
    ctx.assign(
        found.clone(),
        Ty::Int64,
        RValue::MapGet {
            map,
            key,
            out: out.clone(),
            val_ty: val_ty.clone(),
        },
    );
    let may_ty = fallible_mono(ctx, "May", std::slice::from_ref(val_ty), span)?;
    let result = ctx.fresh("mmay");
    init_place(ctx, &result, &may_ty)?;
    let zero = ctx.fresh("mz");
    ctx.assign(zero.clone(), Ty::Int64, RValue::Int(0));
    let missing = ctx.fresh("mmiss");
    ctx.assign(
        missing.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Eq,
            left: found,
            right: zero,
        },
    );
    let not_m = match union_arm_payload(&ctx.types, &may_ty, "Not") {
        Some(Ty::User(n)) => n,
        _ => return Err(Error::at("May is not in scope (import std/fallible)", span)),
    };
    let has_m = match union_arm_payload(&ctx.types, &may_ty, "Has") {
        Some(Ty::User(n)) => n,
        _ => return Err(Error::at("May is not in scope (import std/fallible)", span)),
    };
    let outer = std::mem::take(&mut ctx.stmts);
    let mut then_body = Vec::new();
    {
        let saved = std::mem::take(&mut ctx.stmts);
        ctx.stmts = then_body;
        let (not_p, not_ty) = construct_product(ctx, &not_m, &[], span)?;
        let (inj, _) = inject_into_union(ctx, not_p, &not_ty, &may_ty, span)?;
        ctx.assign(result.clone(), may_ty.clone(), RValue::Use(inj));
        then_body = std::mem::take(&mut ctx.stmts);
        ctx.stmts = saved;
    }
    let mut else_body = Vec::new();
    {
        let saved = std::mem::take(&mut ctx.stmts);
        ctx.stmts = else_body;
        // MapGet is a borrow of the slot (User memcpy / heap-ptr load). The
        // found path then copies that value into Has, the May union, and the
        // result place; end-of-fn drops every copy. Two Dups match those extra
        // owners (ArrayGet of heap elems Dups once because it returns the elem
        // place directly, not a May). User values need the same retain —
        // dropping Json/Header walks inner heaps.
        if needs_drop(val_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: out.clone(),
                ty: val_ty.clone(),
            });
            ctx.stmts.push(Stmt::Dup {
                place: out.clone(),
                ty: val_ty.clone(),
            });
        }
        let hp = ctx.fresh("has");
        let hty = Ty::User(has_m.clone());
        ctx.assign(
            hp.clone(),
            hty.clone(),
            RValue::StructLit {
                type_name: has_m,
                fields: vec![out],
            },
        );
        let (inj, _) = inject_into_union(ctx, hp, &hty, &may_ty, span)?;
        ctx.assign(result.clone(), may_ty.clone(), RValue::Use(inj));
        else_body = std::mem::take(&mut ctx.stmts);
        ctx.stmts = saved;
    }
    ctx.stmts = outer;
    ctx.stmts.push(Stmt::If {
        cond: missing,
        then_body,
        else_body,
    });
    Ok((result, may_ty))
}

fn lower_for_map(
    ctx: &mut LowerCtx,
    kname: &str,
    kspan: Span,
    vname: &str,
    _vspan: Span,
    collection: &Expr,
    body: &Block,
) -> Result<(Place, Ty)> {
    let (map_p, map_ty) = lower_expr(ctx, collection)?;
    let (key_ty, val_ty) = match &map_ty {
        Ty::Map { key, val } => (key.as_ref().clone(), val.as_ref().clone()),
        Ty::Array { .. } => {
            return Err(Error::at(
                "`for k, v in` requires a Map (use `for x in` for Array)",
                collection.span,
            ));
        }
        other => {
            return Err(Error::at(
                format!("`for k, v in` collection must be Map, got {other:?}"),
                collection.span,
            ));
        }
    };
    let _ = kspan;
    let count_p = ctx.fresh("mcnt");
    ctx.assign(
        count_p.clone(),
        Ty::Int64,
        RValue::Call {
            callee: Callee::Builtin("map_count".into()),
            args: vec![map_p.clone()],
        },
    );
    let idx = ctx.fresh("mi");
    ctx.assign(idx.clone(), Ty::Int64, RValue::Int(0));
    let key_p = ctx.fresh(&format!("fk_{}", sanitize_name(kname)));
    let val_p = ctx.fresh(&format!("fv_{}", sanitize_name(vname)));
    init_place(ctx, &key_p, &key_ty)?;
    init_place(ctx, &val_p, &val_ty)?;
    let outer = std::mem::take(&mut ctx.stmts);
    ctx.loop_depth += 1;
    ctx.push_scope();
    ctx.define(kname, key_ty.clone(), false, key_p.clone());
    ctx.define(vname, val_ty.clone(), false, val_p.clone());

    let end_c = ctx.fresh("mend");
    ctx.assign(
        end_c.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Ge,
            left: idx.clone(),
            right: count_p,
        },
    );
    ctx.stmts.push(Stmt::If {
        cond: end_c,
        then_body: vec![Stmt::Break],
        else_body: Vec::new(),
    });

    if needs_drop(&key_ty) {
        ctx.stmts.push(Stmt::Drop {
            place: key_p.clone(),
            ty: key_ty.clone(),
        });
    }
    ctx.assign(
        key_p.clone(),
        key_ty.clone(),
        RValue::MapKeyAt {
            map: map_p.clone(),
            index: idx.clone(),
        },
    );
    if needs_drop(&key_ty) {
        ctx.stmts.push(Stmt::Dup {
            place: key_p.clone(),
            ty: key_ty.clone(),
        });
    }
    if array_elem_is_user(&val_ty) {
        ctx.assign(
            val_p.clone(),
            val_ty.clone(),
            RValue::MapValAt {
                map: map_p.clone(),
                index: idx.clone(),
                val_ty: val_ty.clone(),
            },
        );
    } else {
        if needs_drop(&val_ty) {
            ctx.stmts.push(Stmt::Drop {
                place: val_p.clone(),
                ty: val_ty.clone(),
            });
        }
        ctx.assign(
            val_p.clone(),
            val_ty.clone(),
            RValue::MapValAt {
                map: map_p.clone(),
                index: idx.clone(),
                val_ty: val_ty.clone(),
            },
        );
        if needs_drop(&val_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: val_p.clone(),
                ty: val_ty.clone(),
            });
        }
    }

    lower_block_into(ctx, body, &Ty::Void)?;
    let one = ctx.fresh("mone");
    ctx.assign(one.clone(), Ty::Int64, RValue::Int(1));
    ctx.assign(
        idx.clone(),
        Ty::Int64,
        RValue::Binary {
            op: BinaryOp::Add,
            left: idx,
            right: one,
        },
    );
    ctx.pop_scope();
    ctx.loop_depth -= 1;
    let body_stmts = std::mem::take(&mut ctx.stmts);
    ctx.stmts = outer;
    ctx.stmts.push(Stmt::Loop { body: body_stmts });
    Ok(void_place(ctx))
}

fn lower_interpolate(ctx: &mut LowerCtx, parts: &[InterpPart], span: Span) -> Result<(Place, Ty)> {
    if parts.is_empty() {
        let p = ctx.fresh("s");
        ctx.assign(p.clone(), Ty::String, RValue::StringLit(String::new()));
        return Ok((p, Ty::String));
    }

    let mut acc: Option<Place> = None;
    for part in parts {
        let piece = match part {
            InterpPart::Lit(s) => {
                let p = ctx.fresh("sl");
                ctx.assign(p.clone(), Ty::String, RValue::StringLit(s.clone()));
                p
            }
            InterpPart::Expr(e) => {
                let (v, ty) = lower_expr(ctx, e)?;
                let (v, ty) = match maybe_widen_lit_sum(ctx, v.clone(), &ty)? {
                    Some((p, t)) => (p, t),
                    None => (v, ty),
                };
                match ty {
                    Ty::String => v,
                    Ty::Int64 | Ty::Float64 | Ty::Bool => {
                        let p = ctx.fresh("ts");
                        ctx.assign(
                            p.clone(),
                            Ty::String,
                            RValue::ToString { value: v, from: ty },
                        );
                        p
                    }
                    Ty::UInt64 => {
                        // Not through Int64: values above Int64 max would print negative.
                        let p = ctx.fresh("ts");
                        ctx.assign(
                            p.clone(),
                            Ty::String,
                            RValue::ToString {
                                value: v,
                                from: Ty::UInt64,
                            },
                        );
                        p
                    }
                    ref t if is_int(t) => {
                        let widened = coerce_place(ctx, v, t, &Ty::Int64, span)?;
                        let p = ctx.fresh("ts");
                        ctx.assign(
                            p.clone(),
                            Ty::String,
                            RValue::ToString {
                                value: widened,
                                from: Ty::Int64,
                            },
                        );
                        p
                    }
                    Ty::Array { ref elem }
                        if matches!(elem.as_ref(), Ty::Int64 | Ty::Bool | Ty::String) =>
                    {
                        let p = ctx.fresh("ats");
                        ctx.assign(
                            p.clone(),
                            Ty::String,
                            RValue::ToString {
                                value: v,
                                from: Ty::Array { elem: elem.clone() },
                            },
                        );
                        p
                    }
                    ref array @ Ty::Array { .. } => order::show_text(ctx, v, array, span)?,
                    Ty::Map { ref key, ref val }
                        if matches!(key.as_ref(), Ty::String | Ty::Int64 | Ty::Bool)
                            && matches!(val.as_ref(), Ty::Int64 | Ty::Bool | Ty::String) =>
                    {
                        let p = ctx.fresh("mts");
                        ctx.assign(
                            p.clone(),
                            Ty::String,
                            RValue::ToString {
                                value: v,
                                from: Ty::Map {
                                    key: key.clone(),
                                    val: val.clone(),
                                },
                            },
                        );
                        p
                    }
                    Ty::Map { .. } => {
                        return Err(Error::at(
                            "cannot interpolate this Map into String (supported values: Int64, Bool, String)",
                            span,
                        ));
                    }
                    other if refined_info(&ctx.types, &other).is_some() => {
                        let (inner, ity) = unwrap_refined(ctx, v, &other)?;
                        match &ity {
                            Ty::String => inner,
                            Ty::Int64 | Ty::Float64 | Ty::Bool => {
                                let p = ctx.fresh("rts");
                                ctx.assign(
                                    p.clone(),
                                    Ty::String,
                                    RValue::ToString {
                                        value: inner,
                                        from: ity,
                                    },
                                );
                                p
                            }
                            _ => {
                                return Err(Error::at(
                                    format!("cannot interpolate {other:?} into String"),
                                    span,
                                ));
                            }
                        }
                    }
                    other => order::show_text(ctx, v, &other, span)?,
                }
            }
        };
        acc = Some(match acc {
            None => piece,
            Some(left) => {
                let p = ctx.fresh("cat");
                ctx.assign(
                    p.clone(),
                    Ty::String,
                    RValue::StringConcat { left, right: piece },
                );
                p
            }
        });
    }
    Ok((acc.unwrap(), Ty::String))
}

fn lower_tagged_interp(
    ctx: &mut LowerCtx,
    tag: &str,
    parts: &[InterpPart],
    span: Span,
) -> Result<(Place, Ty)> {
    if tag != "sql" {
        return Err(Error::at(
            format!("unknown string interpolator `{tag}` (only sql\"…\" is shipped)"),
            span,
        ));
    }
    let text = lower_sql_interpolate(ctx, parts, span)?;
    let Some(sql_ty) = find_sql_type(ctx) else {
        return Err(Error::at(
            "sql\"…\" requires type `Sql` from std/sqlite (import the module)",
            span,
        ));
    };
    let p = ctx.fresh("sql");
    let ty = Ty::User(sql_ty.clone());
    ctx.assign(
        p.clone(),
        ty.clone(),
        RValue::StructLit {
            type_name: sql_ty,
            fields: vec![text],
        },
    );
    if needs_drop(&ty) {
        ctx.stmts.push(Stmt::Dup {
            place: p.clone(),
            ty: ty.clone(),
        });
    }
    Ok((p, ty))
}

fn find_sql_type(ctx: &LowerCtx) -> Option<String> {
    ctx.types.iter().find_map(|(name, info)| {
        if info.sum.is_some() || info.zero_init {
            return None;
        }
        if info.fields.len() == 1
            && info.fields[0].name == "text"
            && matches!(info.fields[0].ty, Ty::String)
            && (name == "std_sqlite_Sql" || name.ends_with("_Sql"))
        {
            Some(name.clone())
        } else {
            None
        }
    })
}

fn is_sql_type(ctx: &LowerCtx, ty: &Ty) -> bool {
    match ty {
        Ty::User(name) => {
            find_sql_type(ctx).as_deref() == Some(name.as_str())
                || name == "std_sqlite_Sql"
                || name.ends_with("_Sql")
        }
        _ => false,
    }
}

fn sql_embed_piece(ctx: &mut LowerCtx, v: Place, ty: Ty, span: Span) -> Result<Place> {
    match ty {
        Ty::String => {
            let p = ctx.fresh("sqlq");
            ctx.assign(
                p.clone(),
                Ty::String,
                RValue::Call {
                    callee: Callee::Builtin("sql_quote".into()),
                    args: vec![v],
                },
            );
            Ok(p)
        }
        Ty::Int64 | Ty::Float64 => {
            let p = ctx.fresh("sqln");
            ctx.assign(
                p.clone(),
                Ty::String,
                RValue::ToString { value: v, from: ty },
            );
            Ok(p)
        }
        Ty::UInt64 => {
            let p = ctx.fresh("sqln");
            ctx.assign(
                p.clone(),
                Ty::String,
                RValue::ToString {
                    value: v,
                    from: Ty::UInt64,
                },
            );
            Ok(p)
        }
        ref t if is_int(t) => {
            let widened = coerce_place(ctx, v, t, &Ty::Int64, span)?;
            let p = ctx.fresh("sqln");
            ctx.assign(
                p.clone(),
                Ty::String,
                RValue::ToString {
                    value: widened,
                    from: Ty::Int64,
                },
            );
            Ok(p)
        }
        Ty::Bool => {
            let p = ctx.fresh("sqlb");
            init_place(ctx, &p, &Ty::String)?;
            let mut then_body = Vec::new();
            then_body.push(Stmt::Assign {
                place: p.clone(),
                ty: Ty::String,
                value: RValue::StringLit("1".into()),
            });
            let mut else_body = Vec::new();
            else_body.push(Stmt::Assign {
                place: p.clone(),
                ty: Ty::String,
                value: RValue::StringLit("0".into()),
            });
            ctx.stmts.push(Stmt::If {
                cond: v,
                then_body,
                else_body,
            });
            Ok(p)
        }
        ref t if is_sql_type(ctx, t) => {
            let p = ctx.fresh("sqlf");
            ctx.assign(
                p.clone(),
                Ty::String,
                RValue::Field {
                    base: v,
                    field: sanitize_field("text"),
                },
            );
            if needs_drop(&Ty::String) {
                ctx.stmts.push(Stmt::Dup {
                    place: p.clone(),
                    ty: Ty::String,
                });
            }
            Ok(p)
        }
        other if refined_info(&ctx.types, &other).is_some() => {
            let (inner, ity) = unwrap_refined(ctx, v, &other)?;
            sql_embed_piece(ctx, inner, ity, span)
        }
        other => Err(Error::at(
            format!(
                "cannot interpolate {} into sql\"…\" (supported: String, Int64, Float64, Bool, Sql)",
                display_ty(&other)
            ),
            span,
        )),
    }
}

fn lower_sql_interpolate(ctx: &mut LowerCtx, parts: &[InterpPart], span: Span) -> Result<Place> {
    if parts.is_empty() {
        let p = ctx.fresh("s");
        ctx.assign(p.clone(), Ty::String, RValue::StringLit(String::new()));
        return Ok(p);
    }
    let mut acc: Option<Place> = None;
    for part in parts {
        let piece = match part {
            InterpPart::Lit(s) => {
                let p = ctx.fresh("sl");
                ctx.assign(p.clone(), Ty::String, RValue::StringLit(s.clone()));
                p
            }
            InterpPart::Expr(e) => {
                let (v, ty) = lower_expr(ctx, e)?;
                let (v, ty) = match maybe_widen_lit_sum(ctx, v.clone(), &ty)? {
                    Some((p, t)) => (p, t),
                    None => (v, ty),
                };
                sql_embed_piece(ctx, v, ty, span)?
            }
        };
        acc = Some(match acc {
            None => piece,
            Some(left) => {
                let p = ctx.fresh("cat");
                ctx.assign(
                    p.clone(),
                    Ty::String,
                    RValue::StringConcat { left, right: piece },
                );
                p
            }
        });
    }
    Ok(acc.unwrap())
}

fn lower_field(
    ctx: &mut LowerCtx,
    base: &Expr,
    name: &str,
    name_span: Span,
) -> Result<(Place, Ty)> {
    // `self.field` is the field, even where a local shadows it (D228).
    if matches!(&base.kind, ExprKind::Name(name) if name == "self") && self_receiver(ctx).is_some() {
        if let Some((type_name, getter)) = property_accessor(ctx, name, AccessorKind::Get, None) {
            return lower_sibling_call(ctx, &type_name, &getter, &[], name_span);
        }
        if let Some(error) = own_accessor_error(ctx, name, name_span) {
            return Err(error);
        }
        if let Some(binding) = ctx.method_fields.get(name).cloned() {
            let ty = binding.ty.clone();
            if needs_drop(&ty) {
                let place = ctx.fresh("dup");
                ctx.assign(place.clone(), ty.clone(), RValue::Use(binding.place));
                return Ok((place, ty));
            }
            return Ok((binding.place, ty));
        }
        if ctx.trait_scope.is_some() {
            return Err(Error::at(
                format!("the trait has no member `{name}`"),
                name_span,
            ));
        }
    }
    // Namespace member: `geometry.origin` (module-level let) or `routes.handle` (fn).
    if let ExprKind::Name(ns) = &base.kind {
        if let Some(mod_id) = ctx.namespaces.get(ns).cloned() {
            let exp = ctx.module_exports.get(&mod_id).cloned().unwrap_or_default();
            if let Some(info) = exp.globals.get(name) {
                return Ok((info.place.clone(), info.ty.clone()));
            }
            if let Some(mangled) = exp.fns.get(name) {
                let sig = ctx
                    .fns
                    .get(mangled)
                    .ok_or_else(|| Error::at(format!("unknown function `{name}`"), name_span))?
                    .clone();
                refuse_fn_value(
                    sig.multi_clause,
                    sig.extern_symbol.is_some(),
                    sig.mutable.contains(&true),
                    name,
                    name_span,
                )?;
                let ty = fn_ty_of_sig(&sig.params, &sig.ret);
                return Ok(make_fn_value(
                    ctx,
                    None,
                    None,
                    FnTarget::Free {
                        mangled: sig.mangled.clone(),
                    },
                    ty,
                ));
            }
            return Err(Error::at(
                format!("no exported value `{name}` on namespace `{ns}`"),
                name_span,
            ));
        }
        // Unbound method: `Crema.handle` (type qualifier, not a value).
        if ctx.lookup(ns).is_none() {
            if let Some(mangled_ty) = ctx.resolve_type_name(ns) {
                return form_unbound_method(ctx, &mangled_ty, name, name_span);
            }
        }
    }

    let (base_p, base_ty) = lower_expr(ctx, base)?;
    // A refined String has String's `.count` / `.byteCount` (D195).
    let (base_p, base_ty) = if matches!(name, "count" | "byteCount")
        && refined_info(&ctx.types, &base_ty).is_some_and(|info| info.base == Ty::String)
    {
        unwrap_refined(ctx, base_p, &base_ty)?
    } else {
        (base_p, base_ty)
    };
    // A getter std declares on a built-in type with `extend` (D228).
    if !matches!(base_ty, Ty::User(_)) {
        if let Some(sig) = extension_method(ctx, &base_ty, &getter_name(name), name_span)? {
            return lower_extension_call(ctx, Some(base), base_p, &base_ty, sig, name, &[], name_span);
        }
    }
    // A Float64's IEEE bits (D228: std/core hashes it with them).
    if base_ty == Ty::Float64 && name == "bits" {
        let p = ctx.fresh("bits");
        ctx.assign(
            p.clone(),
            Ty::UInt64,
            RValue::Call {
                callee: Callee::Builtin("f64_to_bits".into()),
                args: vec![base_p],
            },
        );
        return Ok((p, Ty::UInt64));
    }
    match &base_ty {
        Ty::String => {
            // `.count` is Unicode scalars, `.byteCount` bytes (D195 §4).
            let builtin = match name {
                "count" => "string_count",
                "byteCount" => "string_len",
                _ => {
                    if string_method_names(ctx).iter().any(|method| method == name) {
                        return Err(Error::at(
                            format!("`{name}` on a String is a method: write `.{name}()`"),
                            name_span,
                        ));
                    }
                    return Err(Error::at(
                        format!("no field `{name}` on String (use .count or .byteCount)"),
                        name_span,
                    ));
                }
            };
            let p = ctx.fresh("scnt");
            ctx.assign(
                p.clone(),
                Ty::Int64,
                RValue::Call {
                    callee: Callee::Builtin(builtin.into()),
                    args: vec![base_p],
                },
            );
            Ok((p, Ty::Int64))
        }
        Ty::Bytes => {
            if name != "count" {
                return Err(Error::at(
                    format!(
                        "no field `{name}` on Bytes (use .count, .get, .set, .slice, .toString)"
                    ),
                    name_span,
                ));
            }
            let p = ctx.fresh("cnt");
            ctx.assign(
                p.clone(),
                Ty::Int64,
                RValue::Call {
                    callee: Callee::Builtin("bytes_count".into()),
                    args: vec![base_p],
                },
            );
            Ok((p, Ty::Int64))
        }
        Ty::Map { .. } => {
            if name != "count" {
                return Err(Error::at(
                    format!(
                        "no field `{name}` on Map (use .count, .has, .get, .set, .remove, .keys, .values, [k])"
                    ),
                    name_span,
                ));
            }
            let p = ctx.fresh("mcnt");
            ctx.assign(
                p.clone(),
                Ty::Int64,
                RValue::Call {
                    callee: Callee::Builtin("map_count".into()),
                    args: vec![base_p],
                },
            );
            Ok((p, Ty::Int64))
        }
        Ty::Array { .. } => {
            if name != "count" {
                return Err(Error::at(
                    format!(
                        "no field `{name}` on Array (use .count, .append, .dropLast, .insert, .removeAt, [i])"
                    ),
                    name_span,
                ));
            }
            let p = ctx.fresh("acnt");
            ctx.assign(
                p.clone(),
                Ty::Int64,
                RValue::Call {
                    callee: Callee::Builtin("array_count".into()),
                    args: vec![base_p],
                },
            );
            Ok((p, Ty::Int64))
        }
        Ty::Buffer => {
            let builtin = match name {
                "count" => "buffer_count",
                "capacity" => "buffer_capacity",
                other => {
                    return Err(Error::at(
                        format!(
                            "no field `{other}` on Buffer (use .count, .capacity, .append, .appendBytes, .freeze)"
                        ),
                        name_span,
                    ));
                }
            };
            let p = ctx.fresh("bc");
            ctx.assign(
                p.clone(),
                Ty::Int64,
                RValue::Call {
                    callee: Callee::Builtin(builtin.into()),
                    args: vec![base_p],
                },
            );
            Ok((p, Ty::Int64))
        }
        Ty::User(type_name) => {
            let info = ctx
                .types
                .get(type_name)
                .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), name_span))?;
            // A `mixed T`'s property, through its member type (D228 §6).
            if is_mixed(type_name) {
                let type_name = type_name.clone();
                let read = ReceiverWrite::ReadOnly("a property read".into());
                return lower_mixed_call(ctx, base_p, read, &type_name, &getter_name(name), &[], name_span);
            }
            // A property with a getter reads through it (D228).
            let getter = getter_name(name);
            if let Some(sig) = info.methods.get(&getter).cloned() {
                ctx.infer_check_callee(&format!("{type_name}__{getter}"), name, name_span)?;
                return emit_call_result(
                    ctx,
                    Callee::Method {
                        type_name: type_name.clone(),
                        method: getter,
                    },
                    vec![base_p],
                    sig.ret,
                );
            }
            if info.methods.contains_key(name) && info.fields.iter().all(|f| f.name != name) {
                return form_bound_method(ctx, base_p, base_ty, name, name_span);
            }
            let field_ty = info
                .fields
                .iter()
                .find(|f| f.name == name)
                .map(|f| f.ty.clone())
                .ok_or_else(|| Error::at(no_member_message(type_name, name, "field"), name_span))?;
            // Whole fixed-array field load: copy into a local (C arrays not first-class).
            if let Ty::FixedArray { elem, len } = &field_ty {
                let mut places = Vec::new();
                let field_c = sanitize_field(name);
                for i in 0..*len {
                    let ip = ctx.fresh("fai");
                    ctx.assign(ip.clone(), Ty::Int64, RValue::Int(i as i64));
                    let ep = ctx.fresh("fae");
                    ctx.assign(
                        ep.clone(),
                        elem.as_ref().clone(),
                        RValue::Index {
                            base: base_p.clone(),
                            field: field_c.clone(),
                            index: ip,
                            len: *len,
                        },
                    );
                    places.push(ep);
                }
                let p = ctx.fresh("fa");
                ctx.assign(
                    p.clone(),
                    field_ty.clone(),
                    RValue::FixedArrayLit {
                        elems: places,
                        elem_ty: elem.as_ref().clone(),
                        len: *len,
                    },
                );
                return Ok((p, field_ty));
            }
            let p = ctx.fresh("fld");
            ctx.assign(
                p.clone(),
                field_ty.clone(),
                RValue::Field {
                    base: base_p,
                    field: sanitize_field(name),
                },
            );
            // Field load aliases the struct's pointer; own a separate ref when heap.
            if needs_drop(&field_ty) {
                ctx.stmts.push(Stmt::Dup {
                    place: p.clone(),
                    ty: field_ty.clone(),
                });
            }
            Ok((p, field_ty))
        }
        other => Err(Error::at(
            format!("field access on non-struct type {other:?}"),
            name_span,
        )),
    }
}

fn lower_index(ctx: &mut LowerCtx, base: &Expr, index: &Expr, span: Span) -> Result<(Place, Ty)> {
    // `struct.field[i]`: FixedArray peels the field; dynamic Array loads then indexes.
    if let ExprKind::Field {
        base: s,
        name,
        name_span,
    } = &base.kind
    {
        let (struct_p, struct_ty) = lower_expr(ctx, s)?;
        let type_name = match &struct_ty {
            Ty::User(n) => n.clone(),
            other => {
                return Err(Error::at(
                    format!("cannot index field on non-struct type {other:?}"),
                    *name_span,
                ));
            }
        };
        let info = ctx
            .types
            .get(&type_name)
            .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), *name_span))?
            .clone();
        let finfo = info
            .fields
            .iter()
            .find(|f| f.name == *name)
            .ok_or_else(|| Error::at(format!("no field `{name}` on `{type_name}`"), *name_span))?;
        match &finfo.ty {
            Ty::FixedArray { elem, len } => {
                let elem_ty = elem.as_ref().clone();
                let len = *len;
                let (idx_p, idx_ty) = lower_expr(ctx, index)?;
                if !is_int(&idx_ty) {
                    return Err(Error::at("array index must be an integer", index.span));
                }
                let idx_p = if idx_ty == Ty::Int64 {
                    idx_p
                } else {
                    coerce_place(ctx, idx_p, &idx_ty, &Ty::Int64, index.span)?
                };
                if let ExprKind::Int(n) = &index.kind {
                    if *n < 0 || (*n as u64) >= len {
                        return Err(Error::at(
                            format!("index {n} out of bounds for Array of length {len}"),
                            index.span,
                        ));
                    }
                }
                let p = ctx.fresh("idx");
                ctx.assign(
                    p.clone(),
                    elem_ty.clone(),
                    RValue::Index {
                        base: struct_p,
                        field: sanitize_field(name),
                        index: idx_p,
                        len,
                    },
                );
                return Ok((p, elem_ty));
            }
            _ => {
                // Load the field (an Array, a Map, …), then index it like a local.
                let field_p = ctx.fresh("afld");
                let field_ty = finfo.ty.clone();
                ctx.assign(
                    field_p.clone(),
                    field_ty.clone(),
                    RValue::Field {
                        base: struct_p,
                        field: sanitize_field(name),
                    },
                );
                if needs_drop(&field_ty) {
                    ctx.stmts.push(Stmt::Dup {
                        place: field_p.clone(),
                        ty: field_ty.clone(),
                    });
                }
                return lower_index_value(ctx, field_p, &field_ty, index, span);
            }
        }
    }

    // Local Array / fixed Array: `arr[i]`
    let (base_p, base_ty) = lower_expr(ctx, base)?;
    if let Ty::FixedArray { elem, len } = &base_ty {
        let elem_ty = elem.as_ref().clone();
        let len = *len;
        let (idx_p, idx_ty) = lower_expr(ctx, index)?;
        if !is_int(&idx_ty) {
            return Err(Error::at("array index must be an integer", index.span));
        }
        let idx_p = if idx_ty == Ty::Int64 {
            idx_p
        } else {
            coerce_place(ctx, idx_p, &idx_ty, &Ty::Int64, index.span)?
        };
        if let ExprKind::Int(n) = &index.kind {
            if *n < 0 || (*n as u64) >= len {
                return Err(Error::at(
                    format!("index {n} out of bounds for Array of length {len}"),
                    index.span,
                ));
            }
        }
        let p = ctx.fresh("fidx");
        ctx.assign(
            p.clone(),
            elem_ty.clone(),
            RValue::FixedIndex {
                base: base_p,
                index: idx_p,
                len,
            },
        );
        return Ok((p, elem_ty));
    }
    lower_index_value(ctx, base_p, &base_ty, index, span)
}

/// Index a lowered dynamic `Array` (by position) or `Map` (by key, a `May`).
fn lower_index_value(
    ctx: &mut LowerCtx,
    base_p: Place,
    base_ty: &Ty,
    index: &Expr,
    span: Span,
) -> Result<(Place, Ty)> {
    match base_ty {
        Ty::Array { elem } => lower_array_index(ctx, base_p, elem.as_ref().clone(), index),
        Ty::Map { key, val } => {
            let (kp, kt) = lower_expr(ctx, index)?;
            if kt != **key {
                return Err(Error::at(
                    format!("Map key type mismatch: expected {key:?}, found {kt:?}"),
                    index.span,
                ));
            }
            lower_map_get(ctx, base_p, kp, key, val, span)
        }
        other => Err(Error::at(
            format!("cannot index non-Array type {other:?}"),
            span,
        )),
    }
}

/// Index into a lowered dynamic `Array` place.
fn lower_array_index(
    ctx: &mut LowerCtx,
    base_p: Place,
    elem_ty: Ty,
    index: &Expr,
) -> Result<(Place, Ty)> {
    let (idx_p, idx_ty) = lower_expr(ctx, index)?;
    if !is_int(&idx_ty) {
        return Err(Error::at("array index must be an integer", index.span));
    }
    let idx_p = if idx_ty == Ty::Int64 {
        idx_p
    } else {
        coerce_place(ctx, idx_p, &idx_ty, &Ty::Int64, index.span)?
    };
    let p = ctx.fresh("aget");
    if array_elem_is_user(&elem_ty) {
        ctx.assign(
            p.clone(),
            elem_ty.clone(),
            RValue::ArrayGet {
                arr: base_p,
                index: idx_p,
                elem_ty: elem_ty.clone(),
            },
        );
    } else if array_elem_is_heap(&elem_ty) {
        ctx.assign(
            p.clone(),
            elem_ty.clone(),
            RValue::Call {
                callee: Callee::Builtin("array_get_ptr".into()),
                args: vec![base_p, idx_p],
            },
        );
        ctx.stmts.push(Stmt::Dup {
            place: p.clone(),
            ty: elem_ty.clone(),
        });
    } else if elem_ty == Ty::Bool {
        let raw = ctx.fresh("araw");
        ctx.assign(
            raw.clone(),
            Ty::Int64,
            RValue::Call {
                callee: Callee::Builtin("array_get_i64".into()),
                args: vec![base_p, idx_p],
            },
        );
        ctx.assign(
            p.clone(),
            Ty::Bool,
            RValue::Cast {
                value: raw,
                from: Ty::Int64,
                to: Ty::Bool,
            },
        );
    } else if elem_ty == Ty::Float64 {
        let raw = ctx.fresh("araw");
        ctx.assign(
            raw.clone(),
            Ty::Int64,
            RValue::Call {
                callee: Callee::Builtin("array_get_i64".into()),
                args: vec![base_p, idx_p],
            },
        );
        ctx.assign(
            p.clone(),
            Ty::Float64,
            RValue::Call {
                callee: Callee::Builtin("f64_from_bits".into()),
                args: vec![raw],
            },
        );
    } else {
        ctx.assign(
            p.clone(),
            elem_ty.clone(),
            RValue::Call {
                callee: Callee::Builtin("array_get_i64".into()),
                args: vec![base_p, idx_p],
            },
        );
    }
    Ok((p, elem_ty))
}

fn lower_method_call(
    ctx: &mut LowerCtx,
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[Arg],
) -> Result<(Place, Ty)> {
    lower_method_call_typed(ctx, receiver, method, method_span, args, &[])
}

/// `value.method<Int64>(…)`: a method call with its type arguments written
/// out (D230), or none.
fn lower_method_call_typed(
    ctx: &mut LowerCtx,
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[Arg],
    type_args: &[TypeExpr],
) -> Result<(Place, Ty)> {
    let explicit = resolve_type_args(ctx, type_args)?;
    // `<Card …>…</Card>` calls `view` on a `Component` (D228).
    if method == "__view" {
        let ty = infer_block_ty(ctx, &tail_block(receiver.clone()))?;
        if !ctx.conforms(&ty, "Component") {
            let shown = display_ty(&ty);
            return Err(Error::at(
                format!("`<{shown}>` needs `{shown}` to be a `Component`"),
                method_span,
            )
            .with_help(format!(
                "declare `conform {shown} to Component {{ fn view(children: Array<Node>) -> Node {{ … }} }}`"
            )));
        }
        return lower_method_call(ctx, receiver, "view", method_span, args);
    }
    // `self.method(…)` is a sibling call on the receiver (D228).
    if matches!(&receiver.kind, ExprKind::Name(name) if name == "self") {
        if let Some(type_name) = self_receiver(ctx) {
            if sibling_visible(ctx, &type_name, method) {
                return lower_sibling_call(ctx, &type_name, method, args, method_span);
            }
            let shown = display_ty(&Ty::User(type_name.clone()));
            return Err(Error::at(
                if ctx.trait_scope.is_some() {
                    format!("the trait has no member `{method}`")
                } else {
                    format!("`{shown}` has no method `{method}`")
                },
                method_span,
            ));
        }
    }
    // Static methods: `Bytes.fromString(…)`, `Type.read` / `Type.fromJson`.
    if let ExprKind::Name(type_name) = &receiver.kind {
        if type_name == "Bytes" && ctx.lookup(type_name).is_none() {
            return lower_bytes_static(ctx, method, args, method_span);
        }
        // `T.from(x)` and `T.fromJson(json)` became `x as T` (D191 §6).
        if matches!(method, "from" | "fromJson")
            && ctx.lookup(type_name).is_none()
            && ctx.resolve_type_name(type_name).is_some()
        {
            let operand = if method == "from" { "value" } else { "json" };
            return Err(Error::at(
                format!("`{type_name}.{method}({operand})` is written `{operand} as {type_name}`"),
                method_span,
            )
            .with_help("`as` gives a Trial<T, Array<Issue>> when the conversion can fail"));
        }
        // Generated readers call `T.__from` / `T.__fromJson`, names user code
        // cannot write.
        let internal = match method {
            "__from" => Some("from"),
            "__fromJson" => Some("fromJson"),
            _ => None,
        };
        if (internal.is_some()
            || matches!(method, "read" | "readForm" | "fromForm" | "encode" | "toJson"))
            && ctx.lookup(type_name).is_none()
        {
            if let Some(mangled_ty) = ctx.resolve_type_name(type_name) {
                let method = internal.unwrap_or(method);
                return lower_type_read_static(ctx, &mangled_ty, method, args, method_span);
            }
        }
        // Namespace: `mu.add(…)`, `geometry.Point(…)`
        if let Some(mod_id) = ctx.namespaces.get(type_name).cloned() {
            let exp = ctx.module_exports.get(&mod_id).cloned().unwrap_or_default();
            // Type constructor
            if let Some(mangled_ty) = exp.types.get(method) {
                let info = ctx.types.get(mangled_ty).unwrap().clone();
                let p = ctx.fresh("obj");
                let ty = Ty::User(mangled_ty.clone());
                if info.zero_init {
                    if !args.is_empty() {
                        return Err(Error::at(
                            format!("`{method}()` takes no arguments (zero-init constructor)"),
                            method_span,
                        ));
                    }
                    ctx.assign(
                        p.clone(),
                        ty.clone(),
                        RValue::StructZero {
                            type_name: mangled_ty.clone(),
                        },
                    );
                } else {
                    let formal: Vec<(String, Ty)> = info
                        .fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect();
                    let defaults: Vec<Option<Expr>> =
                        info.fields.iter().map(|f| f.default.clone()).collect();
                    let marked: Vec<bool> = info.fields.iter().map(|f| f.positional).collect();
                    let field_places = resolve_call_args_full(
                        ctx,
                        &formal,
                        &defaults,
                        &[],
                        &marked,
                        args,
                        method_span,
                        &info.def_mod,
                    )?;
                    ctx.assign(
                        p.clone(),
                        ty.clone(),
                        RValue::StructLit {
                            type_name: mangled_ty.clone(),
                            fields: field_places.clone(),
                        },
                    );
                    // StructLit copies field pointers; take our own refs so arg temps can drop.
                    // Drop types are already a unique box (D155); duping the box leaks.
                    if needs_drop(&ty) && !type_has_drop(&ctx.types, &ty) {
                        ctx.stmts.push(Stmt::Dup {
                            place: p.clone(),
                            ty: ty.clone(),
                        });
                    }
                    run_constructor_setters(ctx, &info, &ty, &p, &field_places)?;
                }
                return Ok((p, ty));
            }
            // Free function
            if let Some(mangled_fn) = exp.fns.get(method) {
                let sig = ctx.fns.get(mangled_fn).unwrap().clone();
                let arg_places = resolve_call_args_full(
                    ctx,
                    &sig.params,
                    &sig.defaults,
                    &sig.mutable,
                    &sig.positional,
                    args,
                    method_span,
                    &sig.def_mod,
                )?;
                let callee = if let Some(sym) = &sig.extern_symbol {
                    Callee::Extern {
                        c_symbol: sym.clone(),
                        arg_tys: sig.params.iter().map(|(_, t)| t.clone()).collect(),
                        ret_ty: sig.ret.clone(),
                    }
                } else {
                    Callee::User(sig.mangled.clone())
                };
                return emit_call_result(ctx, callee, arg_places, sig.ret);
            }
            return Err(Error::at(
                format!("no exported member `{method}` on namespace `{type_name}`"),
                method_span,
            ));
        }
    }
    // `api.CreateOrder.read(s)` — type qualifier under a namespace.
    if matches!(method, "read" | "readForm" | "fromForm") {
        if let ExprKind::Field {
            base,
            name: ty_name,
            ..
        } = &receiver.kind
        {
            if let ExprKind::Name(ns) = &base.kind {
                if let Some(mod_id) = ctx.namespaces.get(ns).cloned() {
                    let exp = ctx.module_exports.get(&mod_id).cloned().unwrap_or_default();
                    if let Some(mangled_ty) = exp.types.get(ty_name) {
                        return lower_type_read_static(ctx, mangled_ty, method, args, method_span);
                    }
                }
            }
        }
    }

    // Buffer / Array / Map mutating methods must use the binding place without Dup
    // and write back the returned pointer (COW may allocate a new unique object).
    if matches!(
        method,
        "append"
            | "appendBytes"
            | "freeze"
            | "dropLast"
            | "insert"
            | "removeAt"
            | "set"
            | "remove"
    ) {
        if let Some((described, binding)) = named_receiver(ctx, receiver) {
            if binding.ty == Ty::Buffer {
                let result = lower_buffer_method(
                    ctx,
                    binding.place.clone(),
                    method,
                    args,
                    method_span,
                    /*write_back*/ method != "freeze",
                )?;
                if method != "freeze" {
                    write_back_field(ctx, &binding);
                }
                return Ok(result);
            }
            if matches!(&binding.ty, Ty::Array { .. } | Ty::Map { .. }) {
                let mutating = !matches!(method, "freeze");
                if mutating && !binding.mutable {
                    return Err(Error::at(
                        format!("cannot mutate immutable {described}"),
                        method_span,
                    ));
                }
                let recv_p = binding.place.clone();
                let recv_ty = binding.ty.clone();
                let result =
                    lower_collection_method(ctx, recv_p, &recv_ty, method, args, method_span)?;
                if mutating {
                    write_back_field(ctx, &binding);
                }
                return Ok(result);
            }
        }
        // `grid[i].append(x)` on a `mut` Array: change a copy of the element,
        // then store it back, as `items[i].method()` does (D163).
        if let ExprKind::Index { base, index } = &receiver.kind {
            if let Some((described, binding)) = named_receiver(ctx, base) {
                let elem_ty = match &binding.ty {
                    Ty::Array { elem } => elem.as_ref().clone(),
                    _ => Ty::Void,
                };
                let collection = matches!(elem_ty, Ty::Array { .. } | Ty::Map { .. });
                if collection {
                    return lower_element_collection_write(
                        ctx,
                        &described,
                        binding,
                        &elem_ty,
                        index,
                        method,
                        args,
                        method_span,
                    );
                }
            }
        }
    }

    // User method receivers: use the binding place when the receiver is a name so
    // D107 field write-back mutates the caller's object (not a needs_drop temp copy).
    // `items[i].method()` on a `mut` Array stores the element back (D163).
    let in_place = receiver_in_place(ctx, receiver, |ty| matches!(ty, Ty::User(_)))?;
    let (recv_p, recv_ty, receiver_write) = match in_place {
        Some(in_place) => in_place,
        None => {
            let receiver_write = ReceiverWrite::ReadOnly(match &receiver.kind {
                ExprKind::Field { name, .. } => {
                    format!("field `{name}` (a field changes only inside its own type's methods)")
                }
                _ => "a temporary value".into(),
            });
            let (place, ty) = lower_expr(ctx, receiver)?;
            (place, ty, receiver_write)
        }
    };
    let collection_write = matches!(
        method,
        "append" | "appendBytes" | "dropLast" | "insert" | "removeAt" | "set" | "remove"
    ) && matches!(&recv_ty, Ty::Array { .. } | Ty::Map { .. } | Ty::Buffer);
    if collection_write {
        if let Some(target) = unwritable_receiver(ctx, receiver) {
            let error = Error::at(format!("cannot mutate {target}"), method_span);
            return Err(error.with_help(
                "a collection write needs a `mut` binding: a field changes only inside its \
                 own type's methods, and a temporary would drop the change",
            ));
        }
    }
    // A `mixed T`'s member, through the value it holds (D228 §6, D235);
    // `encode` is its `toJson`'s text.
    if let Ty::User(type_name) = &recv_ty {
        if is_mixed(type_name) {
            let type_name = type_name.clone();
            if method == "encode" && args.is_empty() {
                let (json, json_ty) =
                    lower_mixed_call(ctx, recv_p, receiver_write, &type_name, "toJson", &[], method_span)?;
                let bound = codec::bind_fresh(ctx, "__encoded", &json_ty, json);
                return lower_method_call(ctx, &crate::ast_build::name(&bound), "encode", method_span, &[]);
            }
            return lower_mixed_call(ctx, recv_p, receiver_write, &type_name, method, args, method_span);
        }
    }
    if matches!(method, "toJson" | "encode") && args.is_empty() {
        if let Some(result) =
            codec::lower_structural_encode(ctx, recv_p.clone(), &recv_ty, method, method_span)?
        {
            return Ok(result);
        }
    }
    // A refined String takes String's methods unless it declares its own (D195);
    // its generated `toJson` / `encode` stay with the type.
    let string_receiver = match &recv_ty {
        Ty::String => true,
        Ty::User(name) => {
            refined_info(&ctx.types, &recv_ty).is_some_and(|info| info.base == Ty::String)
                && !matches!(method, "toJson" | "encode")
                && !ctx
                    .types
                    .get(name)
                    .is_some_and(|info| info.methods.contains_key(method))
        }
        _ => false,
    };
    let (recv_p, recv_ty) = if string_receiver {
        unwrap_refined(ctx, recv_p, &recv_ty)?
    } else {
        (recv_p, recv_ty)
    };
    // Methods std writes in Thoxa with `extend` (D196), then the runtime's.
    if let Some(sig) = extension_method(ctx, &recv_ty, method, method_span)? {
        no_type_args(&explicit, method, method_span)?;
        return lower_extension_call(ctx, Some(receiver), recv_p, &recv_ty, sig, method, args, method_span);
    }
    // A generic `extend` block's (`extend Array<T: Equatable>`, D228 §5).
    if let Some(template) = generic_extension(ctx, &recv_ty, method, method_span)? {
        let (receiver_arg, receiver_write, place) = extension_receiver(ctx, Some(receiver), recv_p, &recv_ty)?;
        let mut all_args = vec![receiver_arg];
        all_args.extend(args.iter().cloned());
        let instance = generic::instantiate_call(ctx, &template, method, &all_args, method_span, None, &explicit)?;
        let sig = ctx.fns[&instance].clone();
        let places = resolve_call_args_full(
            ctx,
            &sig.params,
            &sig.defaults,
            &sig.mutable,
            &sig.positional,
            &all_args,
            method_span,
            &sig.def_mod,
        )?;
        let result = emit_call_result(ctx, Callee::User(instance.clone()), places, sig.ret)?;
        store_receiver(ctx, receiver_write, place, &[instance], method, method_span);
        return Ok(result);
    }
    if !matches!(recv_ty, Ty::User(_)) {
        no_type_args(&explicit, method, method_span)?;
    }
    if string_receiver {
        return lower_string_method(ctx, recv_p, method, args, method_span);
    }
    match &recv_ty {
        Ty::Bytes => lower_bytes_method(ctx, recv_p, method, args, method_span),
        Ty::Buffer => lower_buffer_method(ctx, recv_p, method, args, method_span, false),
        Ty::Node => lower_html_method(ctx, recv_p, method, args, method_span),
        Ty::Array { elem } => {
            lower_array_method(ctx, recv_p, elem.as_ref(), method, args, method_span)
        }
        Ty::Map { key, val } => lower_map_method(
            ctx,
            recv_p,
            key.as_ref(),
            val.as_ref(),
            method,
            args,
            method_span,
        ),
        Ty::User(type_name) => {
            let type_name = type_name.clone();
            if matches!(method, "encode" | "toJson") {
                if let Some(info) = ctx.types.get(&type_name).cloned() {
                    if let Some(mangled_fn) =
                        lookup_generated_read_fn(ctx, &info, &type_name, method)
                    {
                        if !args.is_empty() {
                            return Err(Error::at(
                                format!("`{method}` takes no arguments"),
                                method_span,
                            ));
                        }
                        let sig = ctx.fns.get(&mangled_fn).unwrap().clone();
                        // By-value generated fn; Dup so the method-like call
                        // does not consume the receiver (D154).
                        let pass = if needs_drop(&recv_ty) {
                            let copy = ctx.fresh("enc");
                            ctx.assign(copy.clone(), recv_ty.clone(), RValue::Use(recv_p));
                            copy
                        } else {
                            recv_p
                        };
                        return emit_call_result(
                            ctx,
                            Callee::User(sig.mangled.clone()),
                            vec![pass],
                            sig.ret,
                        );
                    }
                    if !info.methods.contains_key(method) && !ctx.json_declares(&recv_ty, "Encodable") {
                        return Err(not_codec(&recv_ty, "Encodable", method_span));
                    }
                    if !info.methods.contains_key(method) {
                        if let Some((part, part_ty)) = undeclared_part(ctx, &type_name, "Encodable") {
                            return Err(Error::at(
                                format!(
                                    "`{}` cannot be encoded: `{part}` is {}, which is not `Encodable`",
                                    display_ty(&recv_ty),
                                    display_ty(&part_ty)
                                ),
                                method_span,
                            ));
                        }
                    }
                    if !info.methods.contains_key(method) {
                        return Err(Error::at(
                            format!(
                                "{} has no JSON encoding",
                                display_ty(&recv_ty)
                            ),
                            method_span,
                        )
                        .with_help(
                            "a product or union encodes when its fields are String, integers, \
                             Float64, Bool, Array, Map<String, …>, May, Json, or types that \
                             encode; std/json must be in the program",
                        ));
                    }
                }
            }
            let info = ctx
                .types
                .get(&type_name)
                .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), method_span))?
                .clone();
            // `equals` / `compare` of a type without the method (a union, a
            // generic type's instance): the structural default (D228).
            if !info.methods.contains_key(method) && method == "show" && args.is_empty() {
                let text = order::show_text(ctx, recv_p, &recv_ty, method_span)?;
                return Ok((text, Ty::String));
            }
            if !info.methods.contains_key(method) && method == "hash" {
                if let [Arg::Pos(Expr {
                    kind: ExprKind::MutArg(hasher),
                    ..
                })] = args
                {
                    if !ctx.conforms(&recv_ty, "Hashable") {
                        return Err(Error::at(
                            format!("`{}` is not `Hashable`", display_ty(&recv_ty)),
                            method_span,
                        ));
                    }
                    order::lower_structural_hash(ctx, recv_p, &recv_ty, hasher, method_span)?;
                    let unit = ctx.fresh("u");
                    ctx.assign(unit.clone(), Ty::Int64, RValue::Int(0));
                    return Ok((unit, Ty::Void));
                }
            }
            if !info.methods.contains_key(method) && matches!(method, "equals" | "compare") {
                if let [Arg::Pos(other)] = args {
                    let (other_p, _) = lower_expr_expect(ctx, other, Some(&recv_ty))?;
                    if method == "equals" {
                        let equal = lower_user_eq(ctx, recv_p, other_p, &type_name, method_span)?;
                        return Ok((equal, Ty::Bool));
                    }
                    let sign = order::lower_compare(ctx, recv_p, other_p, &recv_ty, method_span)?;
                    return order::int_to_order(ctx, sign, method_span);
                }
            }
            // A method with type parameters: the one for this call's (D228 §5).
            let instance: String;
            let (info, method) = if !info.methods.contains_key(method) && info.templates.contains_key(method) {
                instance = generic::instantiate_method(ctx, &type_name, method, args, method_span, &explicit)?;
                (ctx.types[&type_name].clone(), instance.as_str())
            } else {
                no_type_args(&explicit, method, method_span)?;
                (info, method)
            };
            let sig = info.methods.get(method).ok_or_else(|| {
                Error::at(
                    no_member_message(&type_name, method, "method"),
                    method_span,
                )
            })?;
            check_requires(ctx, &type_name, method, sig, method_span)?;
            ctx.infer_check_callee(&format!("{type_name}__{method}"), method, method_span)?;
            if !sig.exported && ctx.method_type.as_deref() != Some(type_name.as_str()) {
                return Err(Error::at(
                    format!("method `{method}` is private"),
                    method_span,
                ));
            }
            let arg_places = resolve_call_args_full(
                ctx,
                &sig.params,
                &sig.defaults,
                &sig.mutable,
                &sig.positional,
                args,
                method_span,
                &sig.def_mod,
            )?;
            let mut all_args = vec![recv_p.clone()];
            all_args.extend(arg_places);
            let result = emit_call_result(
                ctx,
                Callee::Method {
                    type_name: type_name.clone(),
                    method: method.to_string(),
                },
                all_args,
                sig.ret.clone(),
            )?;
            // A `drop` type is a shared box (D155): every copy sees the write.
            if !type_has_drop(&ctx.types, &recv_ty) {
                let functions = [format!("{type_name}__{method}")];
                store_receiver(ctx, receiver_write, recv_p, &functions, method, method_span);
            }
            Ok(result)
        }
        other => Err(Error::at(
            format!("method call on non-struct type {other:?}"),
            method_span,
        )),
    }
}

/// The functions that write their receiver (D163, D233): a type's methods,
/// which write through `_self`, and built-in types' members, which write
/// `*self`. One writes it by storing to it, by passing it to a `mut`
/// parameter, by calling a sibling that writes it, or by storing it back
/// after a call that writes its own (`Stmt::StoreBack`).
fn mutating_functions(functions: &[Function]) -> HashSet<String> {
    let members: HashSet<&str> = functions
        .iter()
        .filter(|function| is_member_of_built_in(function))
        .map(|function| function.name.as_str())
        .collect();
    let mut mutating = HashSet::new();
    let mut edges: Vec<(String, Vec<String>)> = Vec::new();
    for function in functions {
        let receiver = if function.receiver.is_some() {
            "_self".to_string()
        } else if is_member_of_built_in(function) {
            format!("(*{})", sanitize_name("self"))
        } else {
            continue;
        };
        let mut effects = ReceiverEffects {
            receiver,
            members: &members,
            writes: false,
            calls: Vec::new(),
        };
        effects.scan(&function.body);
        if effects.writes {
            mutating.insert(function.name.clone());
        } else if !effects.calls.is_empty() {
            edges.push((function.name.clone(), effects.calls));
        }
    }
    loop {
        let before = mutating.len();
        for (name, calls) in &edges {
            if calls.iter().any(|call| mutating.contains(call)) {
                mutating.insert(name.clone());
            }
        }
        if mutating.len() == before {
            return mutating;
        }
    }
}

/// A built-in type's member: a function whose first parameter is `self`,
/// by address.
fn is_member_of_built_in(function: &Function) -> bool {
    function.receiver.is_none()
        && function.mut_params.first() == Some(&true)
        && function.params.first().is_some_and(|(name, _)| *name == sanitize_name("self"))
}

/// What a body does to its receiver (`receiver`, the C place): whether it
/// writes it outright, and the functions it writes it through if they
/// write theirs.
struct ReceiverEffects<'a> {
    receiver: String,
    members: &'a HashSet<&'a str>,
    writes: bool,
    calls: Vec<String>,
}

impl ReceiverEffects<'_> {
    fn scan(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Assign { place, value, .. } => {
                    self.writes |= place.name == self.receiver;
                    if let RValue::Call { callee, args } = value {
                        self.call(callee, args);
                    }
                }
                Stmt::Eval(RValue::Call { callee, args }) => self.call(callee, args),
                Stmt::FieldAssign { base, .. }
                | Stmt::IndexAssign { base, .. }
                | Stmt::DynIndexAssign { base, .. }
                | Stmt::FixedIndexAssign { base, .. } => self.writes |= base.name == self.receiver,
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    self.scan(then_body);
                    self.scan(else_body);
                }
                Stmt::Loop { body } | Stmt::ForRange { body, .. } => self.scan(body),
                Stmt::StoreBack { callees, body } => {
                    let mut inner = ReceiverEffects {
                        receiver: self.receiver.clone(),
                        members: self.members,
                        writes: false,
                        calls: Vec::new(),
                    };
                    inner.scan(body);
                    if inner.writes {
                        self.calls.extend(callees.iter().cloned());
                    }
                }
                _ => {}
            }
        }
    }

    /// A call that takes the receiver, or a part of it (a `mixed` value's
    /// payload, `(*self).u.v0`): as the receiver of a method or a member,
    /// it writes it if that does; passed by address anywhere else, it does.
    fn call(&mut self, callee: &Callee, args: &[Place]) {
        for (index, arg) in args.iter().enumerate() {
            let (address, name) = match arg.name.strip_prefix('&') {
                Some(rest) => (true, rest),
                None => (false, arg.name.as_str()),
            };
            let part = name == self.receiver
                || name.strip_prefix(self.receiver.as_str()).is_some_and(|rest| rest.starts_with('.'));
            if !part {
                continue;
            }
            let sibling = match callee {
                Callee::Method { type_name, method } if index == 0 => Some(format!("{type_name}__{method}")),
                Callee::User(name) if index == 0 && self.members.contains(name.as_str()) => Some(name.clone()),
                _ => None,
            };
            match sibling {
                Some(sibling) => self.calls.push(sibling),
                None => self.writes |= address,
            }
        }
    }
}

/// D163: a method that writes its receiver needs a writable one. Returns
/// each error with the id of the module it is in.
fn receiver_errors(
    functions: &[Function],
    checks: &[ReceiverCheck],
    getters: &[GetterCheck],
) -> Vec<(Error, String)> {
    if checks.is_empty() && getters.is_empty() {
        return Vec::new();
    }
    let mutating = mutating_functions(functions);
    // Return inference (D129) lowers a body twice: report each once.
    let mut seen = HashSet::new();
    let returns: HashMap<&str, &Ty> = functions
        .iter()
        .map(|function| (function.name.as_str(), &function.ret_ty))
        .collect();
    let getter_errors = getters
        .iter()
        .filter(|getter| mutating.contains(&getter.function))
        .filter(|getter| seen.insert((getter.module_id.clone(), getter.span.start, getter.span.end)))
        .map(|getter| {
            let shown = returns
                .get(getter.function.as_str())
                .map_or_else(|| "…".to_string(), |ty| display_ty(ty));
            let error = Error::at(
                format!(
                    "the getter `{}` writes its value: reading a property never changes it",
                    getter.property
                ),
                getter.span,
            )
            .with_help(format!(
                "make it a method, `fn {}() -> {shown} {{ … }}`, which may write fields",
                getter.property
            ));
            (error, getter.module_id.clone())
        })
        .collect::<Vec<_>>();
    getter_errors.into_iter().chain(checks
        .iter()
        .filter(|check| mutating.contains(&check.function))
        .filter(|check| seen.insert((check.module_id.clone(), check.span.start, check.span.end)))
        .map(|check| {
            let error = Error::at(
                format!(
                    "cannot call mutating method `{}` on {}",
                    check.method, check.receiver
                ),
                check.span,
            )
            .with_help(format!(
                "`{}` writes its receiver: call it on a `mut` binding",
                check.method
            ));
            (error, check.module_id.clone())
        }))
        .collect()
}

/// After a call that may have changed `recv` in place: store it back where
/// it came from, or, when it cannot be written, check once every body is
/// lowered that none of `functions` (the methods the call may run) writes
/// its receiver (D163).
fn store_receiver(
    ctx: &mut LowerCtx,
    receiver_write: ReceiverWrite,
    recv: Place,
    functions: &[String],
    method: &str,
    span: Span,
) {
    let outside = std::mem::take(&mut ctx.stmts);
    match receiver_write {
        ReceiverWrite::Binding(binding) => write_back_field(ctx, &binding),
        ReceiverWrite::Element {
            binding,
            index,
            elem_ty,
        } => {
            ctx.stmts.push(Stmt::DynIndexAssign {
                base: binding.place.clone(),
                index,
                elem_ty,
                value: recv,
            });
            write_back_field(ctx, &binding);
        }
        ReceiverWrite::ReadOnly(receiver) => {
            for function in functions {
                ctx.synthetics.borrow_mut().receiver_checks.push(ReceiverCheck {
                    function: function.clone(),
                    method: method.to_string(),
                    receiver: receiver.clone(),
                    span,
                    module_id: ctx.module_id.clone(),
                });
            }
        }
    }
    let body = std::mem::replace(&mut ctx.stmts, outside);
    if !body.is_empty() {
        ctx.stmts.push(Stmt::StoreBack {
            callees: functions.to_vec(),
            body,
        });
    }
}

/// How a user method call writes its receiver back (D163).
enum ReceiverWrite {
    /// A `mut` binding, passed in place; a method's field local also stores
    /// through `_self` (D107).
    Binding(Binding),
    /// `items[i].method()` on a `mut` Array: the element is stored back.
    Element {
        binding: Binding,
        index: Place,
        elem_ty: Ty,
    },
    /// Not writable; the description names the receiver in the error.
    ReadOnly(String),
}

fn type_source_name(info: &TypeInfo, mangled_ty: &str) -> String {
    let prefix = format!("{}_", info.def_mod);
    if !info.def_mod.is_empty() && mangled_ty.starts_with(&prefix) {
        mangled_ty[prefix.len()..].to_string()
    } else {
        mangled_ty.to_string()
    }
}

fn lookup_generated_read_fn(
    ctx: &LowerCtx,
    info: &TypeInfo,
    mangled_ty: &str,
    method: &str,
) -> Option<String> {
    let src = type_source_name(info, mangled_ty);
    let short = format!("__{method}_{src}");
    if ctx.fns.contains_key(&short) {
        return Some(short);
    }
    if !info.def_mod.is_empty() {
        let long = format!("{}_{short}", info.def_mod);
        if ctx.fns.contains_key(&long) {
            return Some(long);
        }
    }
    None
}

fn lower_type_read_static(
    ctx: &mut LowerCtx,
    mangled_ty: &str,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    let info = ctx
        .types
        .get(mangled_ty)
        .ok_or_else(|| Error::at(format!("unknown type `{mangled_ty}`"), span))?
        .clone();
    if info.sum.is_some() && lookup_generated_read_fn(ctx, &info, mangled_ty, method).is_none() {
        let ty = Ty::User(mangled_ty.to_string());
        if !ctx.json_declares(&ty, "Decodable") {
            return Err(not_codec(&ty, "Decodable", span));
        }
        if let Some((part, part_ty)) = undeclared_part(ctx, mangled_ty, "Decodable") {
            return Err(Error::at(
                format!(
                    "`{}` cannot be read: `{part}` is {}, which is not `Decodable`",
                    display_ty(&ty),
                    display_ty(&part_ty)
                ),
                span,
            ));
        }
        return Err(Error::at(
            format!("cannot `{method}` this union: a union reads when its members are products whose fields read"),
            span,
        ));
    }
    if info.zero_init {
        return Err(Error::at(
            format!("cannot `{method}` a C-layout / body-field type"),
            span,
        ));
    }
    let Some(mangled_fn) = lookup_generated_read_fn(ctx, &info, mangled_ty, method) else {
        let ty = Ty::User(mangled_ty.to_string());
        if !ctx.json_declares(&ty, "Decodable") {
            return Err(not_codec(&ty, "Decodable", span));
        }
        if let Some((part, part_ty)) = undeclared_part(ctx, mangled_ty, "Decodable") {
            return Err(Error::at(
                format!(
                    "`{}` cannot be read: `{part}` is {}, which is not `Decodable`",
                    display_ty(&ty),
                    display_ty(&part_ty)
                ),
                span,
            ));
        }
        if matches!(method, "readForm" | "fromForm") {
            return Err(codec::form_unreadable(ctx, mangled_ty, span));
        }
        return Err(Error::at(
            format!(
                "type `{}` cannot be read — its fields must be String, an integer type, Float64, \
                 Bool, refined or literal types, Array, May, Map<String, …>, Json, or \
                 types that read (a type with its own toJson has no generated reader)",
                type_source_name(&info, mangled_ty)
            ),
            span,
        ));
    };
    let sig = ctx
        .fns
        .get(&mangled_fn)
        .ok_or_else(|| Error::at(format!("internal: missing `{mangled_fn}`"), span))?
        .clone();
    let arg_places = resolve_call_args_full(
        ctx,
        &sig.params,
        &sig.defaults,
        &[],
        &sig.positional,
        args,
        span,
        &sig.def_mod,
    )?;
    emit_call_result(ctx, Callee::User(sig.mangled.clone()), arg_places, sig.ret)
}

fn lower_bytes_static(
    ctx: &mut LowerCtx,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    match method {
        "fromString" => {
            let formal = vec![("text".into(), Ty::String)];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            emit_call_result(
                ctx,
                Callee::Builtin("bytes_from_string".into()),
                places,
                Ty::Bytes,
            )
        }
        other => Err(Error::at(
            format!("unknown static method `Bytes.{other}`"),
            span,
        )),
    }
}

/// `Bytes(n)` — zero-filled buffer of length `n` (R9 / D18).
fn lower_bytes_new(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    let formal = vec![("count".into(), Ty::Int64)];
    let places = resolve_call_args(ctx, &formal, args, span)?;
    emit_call_result(ctx, Callee::Builtin("bytes_new".into()), places, Ty::Bytes)
}

fn lower_buffer_new(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    match args.len() {
        0 => emit_call_result(
            ctx,
            Callee::Builtin("buffer_new".into()),
            vec![],
            Ty::Buffer,
        ),
        1 => {
            let formal = vec![("capacity".into(), Ty::Int64)];
            let places = resolve_call_args_full(ctx, &formal, &[], &[], &[false], args, span, "")?;
            emit_call_result(
                ctx,
                Callee::Builtin("buffer_with_capacity".into()),
                places,
                Ty::Buffer,
            )
        }
        _ => Err(Error::at(
            "`Buffer()` / `Buffer(capacity: n)` expects 0 or 1 argument",
            span,
        )),
    }
}

fn lower_html_attr(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    if args.len() != 1 {
        return Err(Error::at("JSX attribute takes one argument", span));
    }
    let expr = match &args[0] {
        Arg::Pos(e) | Arg::Named { value: e, .. } | Arg::Spread { value: e, .. } => e,
    };
    let (p, ty) = lower_expr(ctx, expr)?;
    match &ty {
        Ty::String => Ok((p, Ty::String)),
        Ty::Int64 | Ty::Float64 | Ty::Bool => {
            let s = stringify_place(ctx, p, ty, expr.span)?;
            Ok((s, Ty::String))
        }
        other if is_int(other) => {
            let s = stringify_place(ctx, p, ty, expr.span)?;
            Ok((s, Ty::String))
        }
        other => Err(Error::at(
            format!(
                "JSX attribute must be String, Int64, Float64, or Bool, got {}",
                display_ty(other)
            ),
            expr.span,
        )),
    }
}

/// `Element(tag: "p", attributes: [], children: [])`; the lists default to
/// empty, so `Element(tag: "br")` is an element with neither.
fn lower_html_el(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    let formal = vec![
        ("tag".into(), Ty::String),
        ("attributes".into(), attr_array_ty()),
        ("children".into(), node_array_ty()),
    ];
    let empty = Some(crate::ast_build::empty_array());
    let defaults = vec![None, empty.clone(), empty];
    let places =
        resolve_call_args_full(ctx, &formal, &defaults, &[], &[false; 3], args, span, "")?;
    emit_call_result(ctx, Callee::Builtin("html_el".into()), places, Ty::Node)
}

fn lower_html_element_ctor(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    lower_html_el(ctx, args, span)
}

fn lower_html_text_ctor(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    let formal = vec![("text".into(), Ty::String)];
    let places = resolve_call_args(ctx, &formal, args, span)?;
    emit_call_result(ctx, Callee::Builtin("html_text".into()), places, Ty::Node)
}

fn lower_html_frag_ctor(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    let formal = vec![("children".into(), node_array_ty())];
    let places = resolve_call_args(ctx, &formal, args, span)?;
    emit_call_result(ctx, Callee::Builtin("html_frag".into()), places, Ty::Node)
}

fn lower_html_child(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    if args.len() != 1 {
        return Err(Error::at("JSX child takes one argument", span));
    }
    let expr = match &args[0] {
        Arg::Pos(e) | Arg::Named { value: e, .. } | Arg::Spread { value: e, .. } => e,
    };
    // `<>…</>` desugars to `__jsx_child([…])`. Empty `[]` is Array<Void>
    // unless we thread Array<Node> as the expected type.
    let node_arr = Ty::Array {
        elem: Box::new(Ty::Node),
    };
    let child_expect = match &expr.kind {
        ExprKind::ArrayLit(_) => Some(&node_arr),
        _ => None,
    };
    let (p, ty) = lower_expr_expect(ctx, expr, child_expect)?;
    match &ty {
        Ty::String => emit_call_result(ctx, Callee::Builtin("html_text".into()), vec![p], Ty::Node),
        Ty::Node => Ok((p, Ty::Node)),
        Ty::Array { elem } if matches!(elem.as_ref(), Ty::Node) => {
            emit_call_result(ctx, Callee::Builtin("html_frag".into()), vec![p], Ty::Node)
        }
        // Same scalars string interp accepts — `{n}` is a text node, not a wrap.
        Ty::Int64 | Ty::Float64 | Ty::Bool => {
            let s = stringify_place(ctx, p, ty, expr.span)?;
            emit_call_result(ctx, Callee::Builtin("html_text".into()), vec![s], Ty::Node)
        }
        other if is_int(other) => {
            let s = stringify_place(ctx, p, ty, expr.span)?;
            emit_call_result(ctx, Callee::Builtin("html_text".into()), vec![s], Ty::Node)
        }
        other => Err(Error::at(
            format!(
                "JSX embed must be String, Int64, Float64, Bool, Node, or Array<Node>, got {}",
                display_ty(other)
            ),
            expr.span,
        )),
    }
}

/// Int64 / Bool / other ints → owned String (shared with interpolation).
fn stringify_place(ctx: &mut LowerCtx, value: Place, ty: Ty, span: Span) -> Result<Place> {
    match &ty {
        Ty::Int64 | Ty::Float64 | Ty::Bool => {
            let p = ctx.fresh("ts");
            ctx.assign(p.clone(), Ty::String, RValue::ToString { value, from: ty });
            Ok(p)
        }
        Ty::UInt64 => {
            let p = ctx.fresh("ts");
            ctx.assign(
                p.clone(),
                Ty::String,
                RValue::ToString {
                    value,
                    from: Ty::UInt64,
                },
            );
            Ok(p)
        }
        t if is_int(t) => {
            let widened = coerce_place(ctx, value, t, &Ty::Int64, span)?;
            let p = ctx.fresh("ts");
            ctx.assign(
                p.clone(),
                Ty::String,
                RValue::ToString {
                    value: widened,
                    from: Ty::Int64,
                },
            );
            Ok(p)
        }
        other => Err(Error::at(
            format!("cannot stringify {other:?} for JSX embed"),
            span,
        )),
    }
}

fn lower_html_method(
    ctx: &mut LowerCtx,
    recv: Place,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    match method {
        "html" => {
            if !args.is_empty() {
                return Err(Error::at("Node.html() takes no arguments", span));
            }
            emit_call_result(
                ctx,
                Callee::Builtin("html_render".into()),
                vec![recv],
                Ty::String,
            )
        }
        other => Err(Error::at(
            format!("no method `{other}` on Node (use .html())"),
            span,
        )),
    }
}

/// What a collection write on `receiver` could not reach (it would change a
/// copy and be lost), or `None` for a module value, whose place is written.
/// A plain local binding never gets here.
fn unwritable_receiver(ctx: &LowerCtx, receiver: &Expr) -> Option<String> {
    match &receiver.kind {
        ExprKind::Name(_) => None,
        // `module.items` is that module's own value.
        ExprKind::Field { base, .. }
            if matches!(&base.kind, ExprKind::Name(n)
                if ctx.lookup(n).is_none() && ctx.namespaces.contains_key(n)) =>
        {
            None
        }
        // From outside its type a field is read-only, as `board.label = …`
        // is (D163).
        ExprKind::Field { name, .. } => Some(format!("immutable field `{name}`")),
        ExprKind::Index { base, .. } => match unwritable_receiver(ctx, base) {
            Some(target) => Some(format!("an element of {target}")),
            None => match &base.kind {
                ExprKind::Name(name) => Some(format!("an element of `{name}`")),
                _ => Some("an element of a temporary value".into()),
            },
        },
        _ => Some("a temporary value".into()),
    }
}

/// D163 for collections: `grid[i].append(x)` loads the element, changes it,
/// and stores it back into the `mut` Array.
fn lower_element_collection_write(
    ctx: &mut LowerCtx,
    described: &str,
    binding: Binding,
    elem_ty: &Ty,
    index: &Expr,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    if !binding.mutable {
        return Err(Error::at(
            format!("cannot mutate an element of immutable {described}"),
            span,
        ));
    }
    let (index_p, index_ty) = lower_expr(ctx, index)?;
    if !is_int(&index_ty) {
        return Err(Error::at("array index must be an integer", index.span));
    }
    let index_p = if index_ty == Ty::Int64 {
        index_p
    } else {
        coerce_place(ctx, index_p, &index_ty, &Ty::Int64, index.span)?
    };
    let elem = ctx.fresh("elemw");
    init_place(ctx, &elem, elem_ty)?;
    read_array_elem(ctx, &binding.place, &index_p, &elem, elem_ty);
    let result = lower_collection_method(ctx, elem.clone(), elem_ty, method, args, span)?;
    ctx.stmts.push(Stmt::DynIndexAssign {
        base: binding.place.clone(),
        index: index_p,
        elem_ty: elem_ty.clone(),
        value: elem,
    });
    write_back_field(ctx, &binding);
    Ok(result)
}

fn lower_collection_method(
    ctx: &mut LowerCtx,
    recv: Place,
    recv_ty: &Ty,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    match recv_ty {
        Ty::Array { elem } => lower_array_method(ctx, recv, elem.as_ref(), method, args, span),
        Ty::Map { key, val } => {
            lower_map_method(ctx, recv, key.as_ref(), val.as_ref(), method, args, span)
        }
        _ => Err(Error::at(
            format!("internal: not a collection type {recv_ty:?}"),
            span,
        )),
    }
}

fn lower_map_method(
    ctx: &mut LowerCtx,
    recv: Place,
    key_ty: &Ty,
    val_ty: &Ty,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    let map_ty = Ty::Map {
        key: Box::new(key_ty.clone()),
        val: Box::new(val_ty.clone()),
    };
    match method {
        "has" => {
            let formal = vec![("key".into(), key_ty.clone())];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let key = coerce_map_key(ctx, places.into_iter().next().unwrap(), key_ty);
            let raw = ctx.fresh("mhas");
            ctx.assign(
                raw.clone(),
                Ty::Int64,
                RValue::Call {
                    callee: Callee::Builtin("map_has".into()),
                    args: vec![recv, key],
                },
            );
            let p = ctx.fresh("mhasb");
            ctx.assign(
                p.clone(),
                Ty::Bool,
                RValue::Cast {
                    value: raw,
                    from: Ty::Int64,
                    to: Ty::Bool,
                },
            );
            Ok((p, Ty::Bool))
        }
        "get" => {
            let formal = vec![("key".into(), key_ty.clone())];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let key = places.into_iter().next().unwrap();
            lower_map_get(ctx, recv, key, key_ty, val_ty, span)
        }
        "set" => {
            let formal = vec![
                ("key".into(), key_ty.clone()),
                ("value".into(), val_ty.clone()),
            ];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let mut it = places.into_iter();
            let key = coerce_map_key(ctx, it.next().unwrap(), key_ty);
            let val = it.next().unwrap();
            ctx.assign(
                recv.clone(),
                map_ty,
                RValue::MapSet {
                    map: recv,
                    key,
                    val,
                    key_ty: key_ty.clone(),
                    val_ty: val_ty.clone(),
                },
            );
            Ok(void_place(ctx))
        }
        "remove" => {
            let formal = vec![("key".into(), key_ty.clone())];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let key = coerce_map_key(ctx, places.into_iter().next().unwrap(), key_ty);
            ctx.assign(
                recv.clone(),
                map_ty,
                RValue::MapRemove {
                    map: recv,
                    key,
                    key_ty: key_ty.clone(),
                    val_ty: val_ty.clone(),
                },
            );
            Ok(void_place(ctx))
        }
        "keys" => {
            if !args.is_empty() {
                return Err(Error::at("Map.keys() takes no arguments", span));
            }
            let p = ctx.fresh("mkeys");
            let arr_ty = Ty::Array {
                elem: Box::new(key_ty.clone()),
            };
            ctx.assign(
                p.clone(),
                arr_ty.clone(),
                RValue::MapKeys {
                    map: recv,
                    key_ty: key_ty.clone(),
                },
            );
            Ok((p, arr_ty))
        }
        "values" => {
            if !args.is_empty() {
                return Err(Error::at("Map.values() takes no arguments", span));
            }
            let p = ctx.fresh("mvals");
            let arr_ty = Ty::Array {
                elem: Box::new(val_ty.clone()),
            };
            ctx.assign(
                p.clone(),
                arr_ty.clone(),
                RValue::MapValues {
                    map: recv,
                    val_ty: val_ty.clone(),
                },
            );
            Ok((p, arr_ty))
        }
        other => Err(Error::at(
            format!(
                "no method `{other}` on Map (use .has, .get, .set, .remove, .keys, .values, .count)"
            ),
            span,
        )),
    }
}

/// Array methods — write-back Void on mutating ops (D133).
fn lower_array_method(
    ctx: &mut LowerCtx,
    recv: Place,
    elem: &Ty,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    let arr_ty = Ty::Array {
        elem: Box::new(elem.clone()),
    };
    match method {
        "append" => {
            let formal = vec![("item".into(), elem.clone())];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let item = places.into_iter().next().unwrap();
            if array_elem_is_user(elem) {
                ctx.assign(
                    recv.clone(),
                    arr_ty,
                    RValue::ArrayAppend {
                        arr: recv,
                        item,
                        elem_ty: elem.clone(),
                    },
                );
                return Ok(void_place(ctx));
            }
            let item = coerce_array_scalar_item(ctx, elem, item);
            let all = vec![recv.clone(), item];
            let builtin = if array_elem_is_heap(elem) {
                "array_append_ptr"
            } else {
                "array_append_i64"
            };
            ctx.assign(
                recv,
                arr_ty,
                RValue::Call {
                    callee: Callee::Builtin(builtin.into()),
                    args: all,
                },
            );
            Ok(void_place(ctx))
        }
        "dropLast" => {
            if !args.is_empty() {
                return Err(Error::at("Array.dropLast() takes no arguments", span));
            }
            if array_elem_is_user(elem) {
                ctx.assign(
                    recv.clone(),
                    arr_ty,
                    RValue::ArrayDropLast {
                        arr: recv,
                        elem_ty: elem.clone(),
                    },
                );
                return Ok(void_place(ctx));
            }
            ctx.assign(
                recv.clone(),
                arr_ty,
                RValue::Call {
                    callee: Callee::Builtin("array_drop_last".into()),
                    args: vec![recv],
                },
            );
            Ok(void_place(ctx))
        }
        "insert" => {
            // `insert(item, at: index)` (D205): the item is the subject.
            let formal = vec![("item".into(), elem.clone()), ("at".into(), Ty::Int64)];
            let places =
                resolve_call_args_full(ctx, &formal, &[], &[], &[true, false], args, span, "")?;
            let mut it = places.into_iter();
            let item = it.next().unwrap();
            let at = it.next().unwrap();
            if array_elem_is_user(elem) {
                ctx.assign(
                    recv.clone(),
                    arr_ty,
                    RValue::ArrayInsertAt {
                        arr: recv,
                        at,
                        item,
                        elem_ty: elem.clone(),
                    },
                );
                return Ok(void_place(ctx));
            }
            let item = coerce_array_scalar_item(ctx, elem, item);
            let all = vec![recv.clone(), at, item];
            let builtin = if array_elem_is_heap(elem) {
                "array_insert_at_ptr"
            } else {
                "array_insert_at_i64"
            };
            ctx.assign(
                recv,
                arr_ty,
                RValue::Call {
                    callee: Callee::Builtin(builtin.into()),
                    args: all,
                },
            );
            Ok(void_place(ctx))
        }
        "removeAt" => {
            let formal = vec![("at".into(), Ty::Int64)];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let at = places.into_iter().next().unwrap();
            if array_elem_is_user(elem) {
                ctx.assign(
                    recv.clone(),
                    arr_ty,
                    RValue::ArrayRemoveAt {
                        arr: recv,
                        at,
                        elem_ty: elem.clone(),
                    },
                );
                return Ok(void_place(ctx));
            }
            ctx.assign(
                recv.clone(),
                arr_ty,
                RValue::Call {
                    callee: Callee::Builtin("array_remove_at".into()),
                    args: vec![recv, at],
                },
            );
            Ok(void_place(ctx))
        }
        "count" => Err(Error::at(
            "`count` on an Array is a property, not a method: write `.count`",
            span,
        )),
        other => Err(Error::at(
            format!("no method `{other}` on Array (use .append, .dropLast, .insert(item, at:), .removeAt)"),
            span,
        )),
    }
}

/// The name the `extend` methods of a built-in type carry (D196): `String`,
/// `Array_String`, `Map_String_Int64`. None for a declared type, whose
/// methods are in its body.
fn extension_key(ty: &Ty) -> Option<String> {
    match ty {
        Ty::String => Some("String".into()),
        Ty::Bytes => Some("Bytes".into()),
        Ty::Buffer => Some("Buffer".into()),
        Ty::Int64 => Some("Int64".into()),
        Ty::Int32 => Some("Int32".into()),
        Ty::UInt64 => Some("UInt64".into()),
        Ty::UInt32 => Some("UInt32".into()),
        Ty::Byte => Some("Byte".into()),
        Ty::Float64 => Some("Float64".into()),
        Ty::Bool => Some("Bool".into()),
        Ty::Array { elem } => Some(format!("Array_{}", extension_key(elem)?)),
        Ty::Map { key, val } => Some(format!(
            "Map_{}_{}",
            extension_key(key)?,
            extension_key(val)?
        )),
        _ => None,
    }
}

/// The function an `extend` block in a std module declares for `method` on
/// `ty` (D196), if any.
fn extension_method(ctx: &LowerCtx, ty: &Ty, method: &str, span: Span) -> Result<Option<FnSig>> {
    let Some(key) = extension_key(ty) else {
        return Ok(None);
    };
    let short = crate::extend_desugar::extension_fn_name(&key, method);
    for (module, exports) in &ctx.module_exports {
        // A conformance in the entry module adds them there, named bare.
        let mangled = exports.fns.get(&short).cloned().unwrap_or_else(|| format!("{module}_{short}"));
        let Some(sig) = ctx.fns.get(&mangled) else {
            continue;
        };
        if *module != ctx.module_id && !exports.fns.contains_key(&short) {
            return Err(Error::at(
                format!("method `{method}` on {} is private", display_ty(ty)),
                span,
            ));
        }
        return Ok(Some(sig.clone()));
    }
    Ok(None)
}

/// The template a generic `extend` block in a std module declares for
/// `method` on instances of `ty`'s type (`Array`, `Map`), if any (D228 §5).
fn generic_extension(ctx: &LowerCtx, ty: &Ty, method: &str, span: Span) -> Result<Option<String>> {
    let base = match ty {
        Ty::Array { .. } => "Array",
        Ty::Map { .. } => "Map",
        _ => return Ok(None),
    };
    let short = crate::extend_desugar::extension_fn_name(base, method);
    for (module, exports) in &ctx.module_exports {
        let template = exports.fns.get(&short).cloned().unwrap_or_else(|| format!("{module}_{short}"));
        if !ctx.synthetics.borrow().generic_fns.contains_key(&template) {
            continue;
        }
        if *module != ctx.module_id && !exports.fns.contains_key(&short) {
            return Err(Error::at(
                format!("method `{method}` on {} is private", display_ty(ty)),
                span,
            ));
        }
        return Ok(Some(template));
    }
    Ok(None)
}

/// `value.method(args)` for a built-in type's member (an `extend` method or
/// a conformance's): the function's first argument is the receiver, by
/// address, so a member may write `self` (D228). `receiver` is the written
/// receiver, when there is one: a binding or an element of one is passed
/// in place.
#[allow(clippy::too_many_arguments)]
fn lower_extension_call(
    ctx: &mut LowerCtx,
    receiver: Option<&Expr>,
    recv: Place,
    recv_ty: &Ty,
    sig: FnSig,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    ctx.infer_check_callee(&sig.mangled, method, span)?;
    let (receiver_arg, receiver_write, place) = extension_receiver(ctx, receiver, recv, recv_ty)?;
    let mut all_args = vec![receiver_arg];
    all_args.extend(args.iter().cloned());
    let places = resolve_call_args_full(
        ctx,
        &sig.params,
        &sig.defaults,
        &sig.mutable,
        &sig.positional,
        &all_args,
        span,
        &sig.def_mod,
    )?;
    let result = emit_call_result(ctx, Callee::User(sig.mangled.clone()), places, sig.ret)?;
    store_receiver(ctx, receiver_write, place, &[sig.mangled.clone()], method, span);
    Ok(result)
}

/// The receiver argument of a built-in type's member: `mut` an alias of the
/// place the member may change (D228), with how that place is stored back
/// after the call (`store_receiver`). A binding or an element of one is
/// that place; anything else is the temporary `recv`.
fn extension_receiver(
    ctx: &mut LowerCtx,
    receiver: Option<&Expr>,
    recv: Place,
    recv_ty: &Ty,
) -> Result<(Arg, ReceiverWrite, Place)> {
    let in_place = match receiver {
        Some(receiver) => receiver_in_place(ctx, receiver, |ty| ty == recv_ty)?,
        None => None,
    };
    let (place, _, receiver_write) = in_place
        .unwrap_or_else(|| (recv, recv_ty.clone(), ReceiverWrite::ReadOnly("a temporary value".into())));
    let alias = format!("__receiver{}", ctx.temps);
    ctx.temps += 1;
    ctx.define_full(&alias, recv_ty.clone(), true, place.clone(), None);
    let span = receiver.map_or_else(Span::dummy, |receiver| receiver.span);
    let arg = Arg::Pos(Expr {
        kind: ExprKind::MutArg(alias),
        span,
    });
    Ok((arg, receiver_write, place))
}

/// The place a method call on `receiver` changes in place, with its type,
/// when it is a binding (or `self.field`) or an element of an Array
/// binding whose type `fits`, and how it is stored back (D163): a `mut`
/// binding is its own place, an element is loaded and stored back into its
/// `mut` Array, and an immutable one is checked once every body is
/// lowered.
fn receiver_in_place(
    ctx: &mut LowerCtx,
    receiver: &Expr,
    fits: impl Fn(&Ty) -> bool,
) -> Result<Option<(Place, Ty, ReceiverWrite)>> {
    if let Some((described, binding)) = named_receiver(ctx, receiver) {
        if !fits(&binding.ty) {
            return Ok(None);
        }
        let (place, ty) = (binding.place.clone(), binding.ty.clone());
        let receiver_write = if binding.mutable {
            ReceiverWrite::Binding(binding)
        } else {
            ReceiverWrite::ReadOnly(format!("immutable {described}"))
        };
        return Ok(Some((place, ty, receiver_write)));
    }
    let ExprKind::Index { base, index } = &receiver.kind else {
        return Ok(None);
    };
    let Some((described, binding)) = named_receiver(ctx, base) else {
        return Ok(None);
    };
    let elem_ty = match &binding.ty {
        Ty::Array { elem } if fits(elem) => elem.as_ref().clone(),
        _ => return Ok(None),
    };
    let (index_p, index_ty) = lower_expr(ctx, index)?;
    if !is_int(&index_ty) {
        return Err(Error::at("array index must be an integer", index.span));
    }
    let index_p = if index_ty == Ty::Int64 {
        index_p
    } else {
        coerce_place(ctx, index_p, &index_ty, &Ty::Int64, index.span)?
    };
    let element = ctx.fresh("aget");
    init_place(ctx, &element, &elem_ty)?;
    read_array_elem(ctx, &binding.place, &index_p, &element, &elem_ty);
    let receiver_write = if binding.mutable {
        ReceiverWrite::Element {
            binding,
            index: index_p,
            elem_ty: elem_ty.clone(),
        }
    } else {
        ReceiverWrite::ReadOnly(format!("an element of immutable {described}"))
    };
    Ok(Some((element, elem_ty, receiver_write)))
}

/// String's primitives (D196): `name → (runtime builtin, parameters, result)`.
/// Offsets are bytes. Every other String method is written in Thoxa in
/// std/string, on top of these.
const STRING_PRIMITIVES: &[(&str, &str, &[(&str, StringParam)], StringResult)] = &[
    ("byteAt", "string_byte_at", &[("index", StringParam::Int)], StringResult::Int),
    ("byteSlice", "string_byte_slice", &[("start", StringParam::Int), ("end", StringParam::Int)], StringResult::Text),
    ("byteIndexOf", "string_byte_index_of", &[("needle", StringParam::Text), ("from", StringParam::From)], StringResult::Index),
];

#[derive(Clone, Copy)]
enum StringParam {
    Text,
    Int,
    /// `from: Int64 = 0`, labelled
    From,
}

#[derive(Clone, Copy)]
enum StringResult {
    Int,
    Text,
    /// `May<Int64>` from a runtime index, `-1` when missing
    Index,
}

/// The names of String's methods: the primitives and std's `extend` methods.
fn string_method_names(ctx: &LowerCtx) -> Vec<String> {
    let prefix = crate::extend_desugar::extension_fn_name("String", "");
    let mut names: Vec<String> = STRING_PRIMITIVES.iter().map(|(name, ..)| name.to_string()).collect();
    for module in ctx.module_exports.keys() {
        let long = format!("{module}_{prefix}");
        names.extend(ctx.fns.keys().filter_map(|key| key.strip_prefix(&long).map(str::to_string)));
    }
    // The entry module's, from a conformance there, are named bare.
    names.extend(ctx.fns.keys().filter_map(|key| key.strip_prefix(&prefix).map(str::to_string)));
    names.sort();
    names.dedup();
    names
}

fn lower_string_method(
    ctx: &mut LowerCtx,
    recv: Place,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    let Some((_, builtin, params, result)) =
        STRING_PRIMITIVES.iter().find(|(name, ..)| *name == method)
    else {
        if method == "count" || method == "byteCount" {
            return Err(Error::at(
                format!("`{method}` on a String is a property, not a method: write `.{method}`"),
                span,
            ));
        }
        return Err(Error::at(format!("no method `{method}` on String"), span)
            .with_help(format!("String's methods: {}", string_method_names(ctx).join(", "))));
    };
    let mut formal = Vec::new();
    let mut defaults = Vec::new();
    let mut marked = Vec::new();
    for (name, param) in params.iter() {
        let (ty, default) = match param {
            StringParam::Text => (Ty::String, None),
            StringParam::Int => (Ty::Int64, None),
            StringParam::From => (Ty::Int64, Some(crate::ast_build::int_lit(0))),
        };
        formal.push((name.to_string(), ty));
        defaults.push(default);
        marked.push(!matches!(param, StringParam::From));
    }
    let places = resolve_call_args_full(ctx, &formal, &defaults, &[], &marked, args, span, "")?;
    let ret = match result {
        StringResult::Int | StringResult::Index => Ty::Int64,
        StringResult::Text => Ty::String,
    };
    let mut all = vec![recv];
    all.extend(places);
    let (value, ty) = emit_call_result(ctx, Callee::Builtin(builtin.to_string()), all, ret)?;
    match result {
        StringResult::Index => index_to_may(ctx, value, span),
        _ => Ok((value, ty)),
    }
}

/// Bool array slots are int64 at runtime; cast when needed.
fn coerce_array_scalar_item(ctx: &mut LowerCtx, elem: &Ty, item: Place) -> Place {
    if *elem == Ty::Bool {
        let p = ctx.fresh("abool");
        ctx.assign(
            p.clone(),
            Ty::Int64,
            RValue::Cast {
                value: item,
                from: Ty::Bool,
                to: Ty::Int64,
            },
        );
        p
    } else if *elem == Ty::Float64 {
        let p = ctx.fresh("fbits");
        ctx.assign(
            p.clone(),
            Ty::Int64,
            RValue::Call {
                callee: Callee::Builtin("f64_to_bits".into()),
                args: vec![item],
            },
        );
        p
    } else {
        item
    }
}

/// Buffer methods. Mutating ops return a (possibly new) buffer pointer.
/// When `write_back` is true, assign that pointer onto `recv` (COW place update).
fn lower_buffer_method(
    ctx: &mut LowerCtx,
    recv: Place,
    method: &str,
    args: &[Arg],
    span: Span,
    write_back: bool,
) -> Result<(Place, Ty)> {
    match method {
        "append" => {
            let formal = vec![("byte".into(), Ty::Int64)];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let mut all = vec![recv.clone()];
            all.extend(places);
            // Ownership transfer: overwrite recv with returned unique buffer.
            ctx.stmts.push(Stmt::Assign {
                place: recv.clone(),
                ty: Ty::Buffer,
                value: RValue::Call {
                    callee: Callee::Builtin("buffer_append".into()),
                    args: all,
                },
            });
            if !write_back {
                // Receiver was a temporary; still ok — place holds unique buffer.
            }
            Ok(void_place(ctx))
        }
        "appendBytes" => {
            let formal = vec![("bytes".into(), Ty::Bytes)];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let mut all = vec![recv.clone()];
            all.extend(places);
            ctx.stmts.push(Stmt::Assign {
                place: recv.clone(),
                ty: Ty::Buffer,
                value: RValue::Call {
                    callee: Callee::Builtin("buffer_append_bytes".into()),
                    args: all,
                },
            });
            Ok(void_place(ctx))
        }
        "freeze" => {
            if !args.is_empty() {
                return Err(Error::at("Buffer.freeze() takes no arguments", span));
            }
            emit_call_result(
                ctx,
                Callee::Builtin("buffer_freeze".into()),
                vec![recv],
                Ty::Bytes,
            )
        }
        other => Err(Error::at(format!("no method `{other}` on Buffer"), span)),
    }
}

fn lower_bytes_method(
    ctx: &mut LowerCtx,
    recv: Place,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    match method {
        "get" => {
            let formal = vec![("index".into(), Ty::Int64)];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let mut all = vec![recv];
            all.extend(places);
            emit_call_result(ctx, Callee::Builtin("bytes_get".into()), all, Ty::Int64)
        }
        "set" => {
            let formal = vec![("index".into(), Ty::Int64), ("byte".into(), Ty::Int64)];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let mut all = vec![recv];
            all.extend(places);
            emit_call_result(ctx, Callee::Builtin("bytes_set".into()), all, Ty::Void)
        }
        "slice" => {
            let formal = vec![("start".into(), Ty::Int64), ("end".into(), Ty::Int64)];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let mut all = vec![recv];
            all.extend(places);
            emit_call_result(ctx, Callee::Builtin("bytes_slice".into()), all, Ty::Bytes)
        }
        "find" => {
            // `find(needle, from: 0) -> May<Int64>`: Has(offset) or Not (never -1 at surface).
            let formal = vec![("needle".into(), Ty::Bytes), ("from".into(), Ty::Int64)];
            let defaults = vec![None, Some(crate::ast_build::int_lit(0))];
            let places =
                resolve_call_args_full(ctx, &formal, &defaults, &[], &[true, false], args, span, "")?;
            let mut all = vec![recv];
            all.extend(places);
            let idx = ctx.fresh("findi");
            ctx.assign(
                idx.clone(),
                Ty::Int64,
                RValue::Call {
                    callee: Callee::Builtin("bytes_find".into()),
                    args: all,
                },
            );
            index_to_may(ctx, idx, span)
        }
        "toString" => {
            if !args.is_empty() {
                return Err(Error::at("Bytes.toString() takes no arguments", span));
            }
            emit_call_result(
                ctx,
                Callee::Builtin("bytes_to_string".into()),
                vec![recv],
                Ty::String,
            )
        }
        "equals" => {
            let formal = vec![("other".into(), Ty::Bytes)];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let mut all = vec![recv];
            all.extend(places);
            // Runtime returns int (0/1); cast to Bool.
            let raw = ctx.fresh("beq");
            ctx.assign(
                raw.clone(),
                Ty::Int64,
                RValue::Call {
                    callee: Callee::Builtin("bytes_equals".into()),
                    args: all,
                },
            );
            let p = ctx.fresh("beqb");
            ctx.assign(
                p.clone(),
                Ty::Bool,
                RValue::Cast {
                    value: raw,
                    from: Ty::Int64,
                    to: Ty::Bool,
                },
            );
            Ok((p, Ty::Bool))
        }
        "insert" => {
            // `insert(byte, at: index)` (D205), a new Bytes.
            let formal = vec![("byte".into(), Ty::Int64), ("at".into(), Ty::Int64)];
            let places =
                resolve_call_args_full(ctx, &formal, &[], &[], &[true, false], args, span, "")?;
            let mut it = places.into_iter();
            let byte = it.next().unwrap();
            let at = it.next().unwrap();
            let all = vec![recv, at, byte];
            emit_call_result(
                ctx,
                Callee::Builtin("bytes_insert_at".into()),
                all,
                Ty::Bytes,
            )
        }
        "removeAt" => {
            let formal = vec![("at".into(), Ty::Int64)];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let mut all = vec![recv];
            all.extend(places);
            emit_call_result(
                ctx,
                Callee::Builtin("bytes_remove_at".into()),
                all,
                Ty::Bytes,
            )
        }
        other => Err(Error::at(
            format!(
                "no method `{other}` on Bytes (use .get, .set, .slice, .find, .toString, .equals, .insert(byte, at:), .removeAt)"
            ),
            span,
        )),
    }
}

/// Callee already owns the object; poison the caller local so a later
/// overwrite-drop is a no-op (loop temps passed into `info`, etc.).
fn clear_moved_place(ctx: &mut LowerCtx, place: &Place, ty: &Ty) {
    match ty {
        Ty::String | Ty::Bytes | Ty::Buffer | Ty::Node | Ty::Array { .. } | Ty::Map { .. } => {
            ctx.assign_no_drop(place.clone(), ty.clone(), RValue::Null);
        }
        Ty::User(name) => {
            ctx.assign_no_drop(
                place.clone(),
                ty.clone(),
                RValue::StructZero {
                    type_name: name.clone(),
                },
            );
        }
        Ty::Fn { .. } => {
            ctx.assign_no_drop(
                place.clone(),
                ty.clone(),
                RValue::MakeFn {
                    recv: None,
                    recv_ty: None,
                    target: FnTarget::Free {
                        mangled: String::new(),
                    },
                },
            );
        }
        _ => {}
    }
}

/// Emit a call that either assigns a result or is evaluated for side effects (Void).
/// A runtime index, `-1` when missing, as `May<Int64>`: `Has(index)` or `Not`
/// (`Bytes.find`, `String.indexOf`).
fn index_to_may(ctx: &mut LowerCtx, idx: Place, span: Span) -> Result<(Place, Ty)> {
    let may_ty = fallible_mono(ctx, "May", &[Ty::Int64], span)?;
    let out = ctx.fresh("findm");
    init_place(ctx, &out, &may_ty)?;
    let zero = ctx.fresh("fz");
    ctx.assign(zero.clone(), Ty::Int64, RValue::Int(0));
    let missing = ctx.fresh("fmiss");
    ctx.assign(
        missing.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Lt,
            left: idx.clone(),
            right: zero,
        },
    );
    let outer = std::mem::take(&mut ctx.stmts);
    let not_m = match union_arm_payload(&ctx.types, &may_ty, "Not") {
        Some(Ty::User(n)) => n,
        _ => {
            return Err(Error::at("internal: May missing Not arm", span));
        }
    };
    let has_m = match union_arm_payload(&ctx.types, &may_ty, "Has") {
        Some(Ty::User(n)) => n,
        _ => {
            return Err(Error::at("internal: May missing Has arm", span));
        }
    };
    // then: Not arm
    let mut then_body = Vec::new();
    {
        let saved = std::mem::take(&mut ctx.stmts);
        ctx.stmts = then_body;
        let (not_p, not_ty) = construct_product(ctx, &not_m, &[], span)?;
        let (inj, _) = inject_into_union(ctx, not_p, &not_ty, &may_ty, span)?;
        ctx.assign(out.clone(), may_ty.clone(), RValue::Use(inj));
        then_body = std::mem::take(&mut ctx.stmts);
        ctx.stmts = saved;
    }
    // else: Has(idx)
    let mut else_body = Vec::new();
    {
        let saved = std::mem::take(&mut ctx.stmts);
        ctx.stmts = else_body;
        let hp = ctx.fresh("has");
        let hty = Ty::User(has_m.clone());
        ctx.assign(
            hp.clone(),
            hty.clone(),
            RValue::StructLit {
                type_name: has_m,
                fields: vec![idx],
            },
        );
        let (inj, _) = inject_into_union(ctx, hp, &hty, &may_ty, span)?;
        ctx.assign(out.clone(), may_ty.clone(), RValue::Use(inj));
        else_body = std::mem::take(&mut ctx.stmts);
        ctx.stmts = saved;
    }
    ctx.stmts = outer;
    ctx.stmts.push(Stmt::If {
        cond: missing,
        then_body,
        else_body,
    });
    Ok((out, may_ty))
}

fn emit_call_result(
    ctx: &mut LowerCtx,
    callee: Callee,
    args: Vec<Place>,
    ret: Ty,
) -> Result<(Place, Ty)> {
    // D154: the callee owns moved heap args. After the call each is reset to
    // NULL / zero (`clear_moved_place`), so it stays in the drop set: dropping
    // an emptied place is a no-op, and an early `return` / `?` taken *before*
    // the call (in a later argument) still drops the copy it owns.
    let moved = heap_args_to_move(&callee, &args);
    let mut cleared: Vec<(Place, Ty)> = Vec::new();
    for arg in &moved {
        if let Some(ty) = ctx.drop_place_ty(arg) {
            cleared.push((arg.clone(), ty));
        }
    }
    let result = if ret == Ty::Void {
        ctx.stmts.push(Stmt::Eval(RValue::Call { callee, args }));
        let p = ctx.fresh("u");
        ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
        (p, Ty::Void)
    } else {
        let p = ctx.fresh("mcall");
        ctx.assign(p.clone(), ret.clone(), RValue::Call { callee, args });
        (p, ret)
    };
    for (place, ty) in cleared {
        clear_moved_place(ctx, &place, &ty);
    }
    for field in std::mem::take(&mut ctx.mut_field_args) {
        write_back_field(ctx, &field);
    }
    Ok(result)
}

fn heap_args_to_move(callee: &Callee, args: &[Place]) -> Vec<Place> {
    match callee {
        Callee::User(_) | Callee::Indirect { .. } => args.to_vec(),
        Callee::Method { .. } => args.iter().skip(1).cloned().collect(),
        Callee::Builtin(_) | Callee::Extern { .. } => Vec::new(),
    }
}

/// Arguments for a built-in whose parameters are all positional, as if
/// declared `!` (D200, D203): `items.append(item)`, `map.set(key, value)`.
fn resolve_call_args(
    ctx: &mut LowerCtx,
    formal: &[(String, Ty)],
    args: &[Arg],
    span: Span,
) -> Result<Vec<Place>> {
    let marked = vec![true; formal.len()];
    resolve_call_args_full(ctx, formal, &[], &[], &marked, args, span, "")
}

/// The places for a call's arguments, in `formal`'s order. `defaults[i]`
/// fills formal `i` when omitted (`defaults` may be shorter than `formal`);
/// default expressions resolve free names in `def_mod` (defining module,
/// D116). `mutable[i]` marks a `mut` parameter (D199), whose argument is
/// `mut binding`, passed by address, and `marked[i]` a `!` parameter: those go
/// by position, the rest by label (D200; built-ins too, D203).
#[allow(clippy::too_many_arguments)]
fn resolve_call_args_full(
    ctx: &mut LowerCtx,
    formal: &[(String, Ty)],
    defaults: &[Option<Expr>],
    mutable: &[bool],
    marked: &[bool],
    args: &[Arg],
    span: Span,
    def_mod: &str,
) -> Result<Vec<Place>> {
    let is_mut = |i: usize| mutable.get(i).copied().unwrap_or(false);
    if args.is_empty() && formal.is_empty() {
        return Ok(Vec::new());
    }

    let default_at = |i: usize| -> Option<&Expr> { defaults.get(i).and_then(|d| d.as_ref()) };

    let punned = punned_arguments(formal, marked, args);
    let labelled;
    let args: &[Arg] = if punned.iter().any(|pun| pun.is_some()) {
        labelled = args
            .iter()
            .zip(&punned)
            .map(|(arg, pun)| match (arg, pun) {
                (Arg::Pos(value), Some(name)) => Arg::Named {
                    name: name.clone(),
                    name_span: value.span,
                    value: value.clone(),
                },
                _ => arg.clone(),
            })
            .collect::<Vec<_>>();
        &labelled
    } else {
        args
    };
    let first_pun = punned.iter().flatten().next();

    let has_spread = args.iter().any(|a| matches!(a, Arg::Spread { .. }));
    if has_spread {
        return resolve_call_args_with_spreads(ctx, formal, defaults, args, span, def_mod);
    }

    // Positional first, then named (D194): `format("hi", width: 40)`.
    let positional = args.iter().take_while(|a| matches!(a, Arg::Pos(_))).count();
    if args[positional..].iter().any(|a| matches!(a, Arg::Pos(_))) {
        if let (Some(name), Arg::Named { name_span, .. }) = (&punned[positional], &args[positional]) {
            return Err(Error::at(
                format!("`{name}` fills the labelled parameter `{name}`, so it goes after the positional arguments"),
                *name_span,
            ));
        }
        return Err(Error::at(
            "a positional argument cannot follow a named one",
            span,
        ));
    }
    let all_pos = positional == args.len();
    check_argument_labels(formal, marked, args, positional)?;

    if all_pos {
        if args.len() > formal.len() {
            return Err(Error::at(
                format!(
                    "expected at most {} argument(s), found {}",
                    formal.len(),
                    args.len()
                ),
                span,
            ));
        }
        // Trailing formals may be defaulted when fewer positional args are given.
        for i in args.len()..formal.len() {
            if default_at(i).is_none() {
                return Err(Error::at(
                    format!(
                        "expected {} argument(s), found {} (missing `{}`)",
                        formal.len(),
                        args.len(),
                        formal[i].0
                    ),
                    span,
                ));
            }
        }
        let mut places = Vec::new();
        let mut fields = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            let expr = match arg {
                Arg::Pos(e) => e,
                Arg::Named { .. } | Arg::Spread { .. } => unreachable!(),
            };
            let (p, field) = lower_call_arg(ctx, expr, &formal[i].1, &formal[i].0, i, is_mut(i))?;
            places.push(p);
            fields.extend(field);
        }
        for i in args.len()..formal.len() {
            let expr = default_at(i).expect("checked above");
            let p = ctx.with_def_mod_maps(def_mod, |ctx| {
                lower_arg_for_formal(ctx, expr, &formal[i].1, &formal[i].0, i)
            })?;
            places.push(p);
        }
        ctx.mut_field_args = fields;
        return Ok(places);
    }

    // Named: match by name; omitted formals use defaults; declaration order for result.
    // Leading positional args fill the first formals.
    if positional > formal.len() {
        return Err(Error::at(
            format!(
                "expected at most {} argument(s), found {}",
                formal.len(),
                args.len()
            ),
            span,
        ));
    }
    let mut by_name: HashMap<String, &Expr> = HashMap::new();
    for arg in &args[positional..] {
        match arg {
            Arg::Named {
                name,
                name_span,
                value,
            } => {
                if by_name.contains_key(name) {
                    return Err(Error::at(
                        format!("duplicate argument `{name}`"),
                        *name_span,
                    ));
                }
                if formal[..positional].iter().any(|(fname, _)| fname == name) {
                    return Err(Error::at(
                        format!("argument `{name}` is already given by position"),
                        *name_span,
                    ));
                }
                if !formal.iter().any(|(fname, _)| fname == name) {
                    let names: Vec<&str> = formal.iter().map(|(fname, _)| fname.as_str()).collect();
                    return Err(Error::at(format!("unknown argument `{name}`"), *name_span)
                        .with_help(format!("the parameters are {}", names.join(", "))));
                }
                by_name.insert(name.clone(), value);
            }
            Arg::Pos(_) | Arg::Spread { .. } => unreachable!(),
        }
    }
    let mut places = Vec::new();
    let mut fields = Vec::new();
    for (i, (fname, fty)) in formal.iter().enumerate() {
        if i < positional {
            let Arg::Pos(expr) = &args[i] else {
                unreachable!()
            };
            let (p, field) = lower_call_arg(ctx, expr, fty, fname, i, is_mut(i))?;
            places.push(p);
            fields.extend(field);
            continue;
        }
        let (expr, is_default) = if let Some(e) = by_name.remove(fname) {
            (e, false)
        } else if let Some(d) = default_at(i) {
            (d, true)
        } else {
            let error = Error::at(format!("missing argument `{fname}`"), span);
            return Err(match first_pun {
                Some(name) => error.with_help(format!(
                    "a bare `{name}` fills the labelled parameter `{name}`; to pass it by position, label `{name}: …` too"
                )),
                None => error,
            });
        };
        let p = if is_default {
            ctx.with_def_mod_maps(def_mod, |ctx| {
                lower_arg_for_formal(ctx, expr, fty, fname, i)
            })?
        } else {
            let (p, field) = lower_call_arg(ctx, expr, fty, fname, i, is_mut(i))?;
            fields.extend(field);
            p
        };
        places.push(p);
    }
    if let Some((extra, _)) = by_name.iter().next() {
        return Err(Error::at(format!("unknown argument `{extra}`"), span));
    }
    ctx.mut_field_args = fields;
    Ok(places)
}

/// Named args + `{...product}` spreads. Later entries win (React order).
/// Spread value must be a constructor-param user type whose fields cover the
/// names being copied (same type as the callee product, typically).
fn resolve_call_args_with_spreads(
    ctx: &mut LowerCtx,
    formal: &[(String, Ty)],
    defaults: &[Option<Expr>],
    args: &[Arg],
    span: Span,
    def_mod: &str,
) -> Result<Vec<Place>> {
    if args.iter().any(|a| matches!(a, Arg::Pos(_))) {
        return Err(Error::at(
            "cannot mix positional arguments with `{...}` spreads",
            span,
        ));
    }
    let default_at = |i: usize| -> Option<&Expr> { defaults.get(i).and_then(|d| d.as_ref()) };
    let mut slots: Vec<Option<Place>> = vec![None; formal.len()];
    let idx_of = |name: &str| formal.iter().position(|(n, _)| n == name);

    for arg in args {
        match arg {
            Arg::Named {
                name,
                name_span,
                value,
            } => {
                let Some(i) = idx_of(name) else {
                    return Err(Error::at(format!("unknown argument `{name}`"), *name_span));
                };
                let p = lower_arg_for_formal(ctx, value, &formal[i].1, name, i)?;
                slots[i] = Some(p);
            }
            Arg::Spread {
                value,
                span: sp_span,
            } => {
                let (src, src_ty) = lower_expr(ctx, value)?;
                let Ty::User(type_name) = &src_ty else {
                    return Err(Error::at(
                        format!(
                            "component spread must be a user type, got {}",
                            display_ty(&src_ty)
                        ),
                        *sp_span,
                    ));
                };
                let info = ctx
                    .types
                    .get(type_name)
                    .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), *sp_span))?
                    .clone();
                if info.zero_init || info.sum.is_some() {
                    return Err(Error::at(
                        "component spread must be a constructor-param product",
                        *sp_span,
                    ));
                }
                for f in &info.fields {
                    let Some(i) = idx_of(&f.name) else {
                        continue;
                    };
                    if !ty_eq(&ctx.types, &f.ty, &formal[i].1) {
                        return Err(Error::at(
                            format!(
                                "spread field `{}` has type {}, expected {}",
                                f.name,
                                display_ty(&f.ty),
                                display_ty(&formal[i].1)
                            ),
                            *sp_span,
                        ));
                    }
                    let fp = ctx.fresh("sp");
                    ctx.assign(
                        fp.clone(),
                        f.ty.clone(),
                        RValue::Field {
                            base: src.clone(),
                            field: sanitize_field(&f.name),
                        },
                    );
                    if needs_drop(&f.ty) {
                        ctx.stmts.push(Stmt::Dup {
                            place: fp.clone(),
                            ty: f.ty.clone(),
                        });
                    }
                    slots[i] = Some(fp);
                }
            }
            Arg::Pos(_) => unreachable!(),
        }
    }

    let mut places = Vec::new();
    for (i, (fname, fty)) in formal.iter().enumerate() {
        if let Some(p) = slots[i].take() {
            places.push(p);
            continue;
        }
        if let Some(d) = default_at(i) {
            let p =
                ctx.with_def_mod_maps(def_mod, |ctx| lower_arg_for_formal(ctx, d, fty, fname, i))?;
            places.push(p);
            continue;
        }
        return Err(Error::at(format!("missing argument `{fname}`"), span));
    }
    Ok(places)
}

/// The labelled parameter each argument fills by its name (D202): a bare
/// `deadline` (or `mut deadline`) where the callee has a labelled
/// `deadline` is `deadline: deadline`, so `connect(host, deadline)` needs no
/// `deadline: deadline`. A call that also labels `deadline:` keeps the bare
/// one positional.
fn punned_arguments(formal: &[(String, Ty)], marked: &[bool], args: &[Arg]) -> Vec<Option<String>> {
    let labelled_in_call = |name: &str| {
        args.iter()
            .any(|arg| matches!(arg, Arg::Named { name: label, .. } if label == name))
    };
    args.iter()
        .map(|arg| {
            let Arg::Pos(value) = arg else {
                return None;
            };
            let (ExprKind::Name(name) | ExprKind::MutArg(name)) = &value.kind else {
                return None;
            };
            let index = formal.iter().position(|(formal_name, _)| formal_name == name)?;
            let is_labelled = !marked.get(index).copied().unwrap_or(true);
            (is_labelled && !labelled_in_call(name)).then(|| name.clone())
        })
        .collect()
}

/// `!` parameters are passed by position and the rest by label (D200):
/// `contains(needle!: String, ignoreCase: Bool = false)` is called
/// `s.contains(x, ignoreCase: true)`, never `s.contains(x, true)`.
fn check_argument_labels(
    formal: &[(String, Ty)],
    marked: &[bool],
    args: &[Arg],
    positional: usize,
) -> Result<()> {
    for (index, arg) in args[..positional].iter().enumerate() {
        let (Some((name, _)), Arg::Pos(value)) = (formal.get(index), arg) else {
            continue;
        };
        if !marked.get(index).copied().unwrap_or(true) {
            return Err(Error::at(format!("`{name}` is passed by label: write `{name}: …`"), value.span)
                .with_help("parameters marked `!` go by position; the others go by label"));
        }
    }
    for arg in &args[positional..] {
        let Arg::Named { name, name_span, .. } = arg else {
            continue;
        };
        let index = formal.iter().position(|(formal_name, _)| formal_name == name);
        if index.is_some_and(|index| marked.get(index).copied().unwrap_or(false)) {
            return Err(Error::at(
                format!("`{name}` is positional (`!`): pass it without `{name}:`"),
                *name_span,
            ));
        }
    }
    Ok(())
}

/// An argument for parameter `name`. A `mut` parameter takes `mut binding`:
/// a `mut` binding of exactly its type, passed by address (D199). A method's
/// field passes its local, and comes back as the binding to store through
/// `_self` after the call (D206).
fn lower_call_arg(
    ctx: &mut LowerCtx,
    expr: &Expr,
    ty: &Ty,
    name: &str,
    index: usize,
    mutable: bool,
) -> Result<(Place, Option<Binding>)> {
    match (&expr.kind, mutable) {
        (ExprKind::MutArg(binding), true) => {
            let found = ctx
                .lookup(binding)
                .ok_or_else(|| Error::at(format!("undefined name `{binding}`"), expr.span))?;
            if !found.mutable {
                return Err(Error::at(
                    format!("`{binding}` is not `mut`, so it cannot be passed to `mut {name}`"),
                    expr.span,
                )
                .with_help(format!("declare it `mut {binding} = …`")));
            }
            if found.ty != *ty {
                return Err(Error::at(
                    format!(
                        "`mut {name}` is {}, but `{binding}` is {}",
                        display_ty(ty),
                        display_ty(&found.ty)
                    ),
                    expr.span,
                ));
            }
            let place = Place {
                name: format!("&{}", found.place.name),
            };
            let field = found.writeback.is_some().then_some(found);
            Ok((place, field))
        }
        (ExprKind::MutArg(binding), false) => Err(Error::at(
            format!("parameter `{name}` is not `mut`; pass `{binding}` without `mut`"),
            expr.span,
        )),
        (ExprKind::Name(binding), true) => Err(Error::at(
            format!("parameter `{name}` is `mut`: write `mut {binding}`"),
            expr.span,
        )),
        (_, true) => Err(Error::at(
            format!("parameter `{name}` is `mut`: pass a `mut` binding, as `mut total`"),
            expr.span,
        )),
        (_, false) => Ok((lower_arg_for_formal(ctx, expr, ty, name, index)?, None)),
    }
}

/// Lower one call argument against a formal type (int coerce, Address implicit, sum inject).
fn lower_arg_for_formal(
    ctx: &mut LowerCtx,
    expr: &Expr,
    formal_ty: &Ty,
    formal_name: &str,
    index: usize,
) -> Result<Place> {
    // Implicit address: mut struct binding → Address<T>
    if let Ty::Address(pointee) = formal_ty {
        // Already an address (an `Address<T>` parameter): pass it on.
        if let ExprKind::Name(name) = &expr.kind {
            if let Some(binding) = ctx.lookup(name) {
                if &binding.ty == formal_ty {
                    return Ok(binding.place);
                }
            }
        }
        // A `drop` type is a shared box (D155): any value of it passes its box
        // pointer, so C sees the one object every copy shares (D168).
        if type_has_drop(&ctx.types, pointee) {
            let (place, ty) = lower_expr(ctx, expr)?;
            if &ty != pointee.as_ref() {
                return Err(Error::at(
                    format!(
                        "argument `{formal_name}`: expected {}, found {}",
                        display_ty(pointee),
                        display_ty(&ty)
                    ),
                    expr.span,
                ));
            }
            let address = ctx.fresh("addr");
            ctx.assign(address.clone(), formal_ty.clone(), RValue::AddressOf(place));
            return Ok(address);
        }
        let name = match &expr.kind {
            ExprKind::Name(n) => n.as_str(),
            _ => {
                return Err(Error::at(
                    format!(
                        "argument {} (`{formal_name}`): Address parameter requires a mut local",
                        index + 1
                    ),
                    expr.span,
                ));
            }
        };
        let binding = ctx
            .lookup(name)
            .ok_or_else(|| Error::at(format!("undefined name `{name}`"), expr.span))?;
        if !binding.mutable {
            return Err(Error::at(
                format!("argument `{formal_name}`: coercing to Address requires a `mut` binding"),
                expr.span,
            ));
        }
        match (&binding.ty, pointee.as_ref()) {
            (Ty::User(got), Ty::User(want)) if got == want => {
                // An explicit pointer, so extern and Thoxa callees both get one.
                let address = ctx.fresh("addr");
                ctx.assign(address.clone(), formal_ty.clone(), RValue::AddressOf(binding.place));
                return Ok(address);
            }
            (got, want) => {
                return Err(Error::at(
                    format!("argument `{formal_name}`: expected Address<{want:?}>, found {got:?}"),
                    expr.span,
                ));
            }
        }
    }

    let (p, ty) = lower_expr_expect(ctx, expr, Some(formal_ty))?;
    if &ty == formal_ty {
        Ok(p)
    } else if can_numeric_cast(&ty, formal_ty) {
        coerce_place(ctx, p, &ty, formal_ty, expr.span)
    } else if let Some(rp) = try_refine_coerce(
        ctx,
        p.clone(),
        &ty,
        formal_ty,
        expr_as_type_lit(expr).as_ref(),
        expr.span,
    )? {
        Ok(rp)
    } else if let Some(p) = try_sum_coerce(
        ctx,
        p.clone(),
        &ty,
        formal_ty,
        expr_as_type_lit(expr).as_ref(),
        expr.span,
    )? {
        Ok(p)
    } else if let Some(p) = convert::encode_where_json_expected(ctx, p, &ty, formal_ty)? {
        Ok(p)
    } else {
        let error = Error::at(
            format!(
                "argument {} (`{formal_name}`): expected {formal_ty:?}, found {ty:?}",
                index + 1
            ),
            expr.span,
        );
        Err(match convert::decode_hint(ctx, &ty, formal_ty) {
            Some(hint) => error.with_help(hint),
            None => error,
        })
    }
}

fn lower_try(ctx: &mut LowerCtx, inner: &Expr, span: Span) -> Result<(Place, Ty)> {
    let (scrut_p, scrut_ty) = lower_expr(ctx, inner)?;
    // May/Trial are residual-marked user unions (std/fallible); no builtin Ty::May/Trial.
    if let Ty::User(ref n) = &scrut_ty {
        if let Some(sum) = ctx.types.get(n).and_then(|i| i.sum.clone()) {
            return lower_try_sum(ctx, scrut_p, scrut_ty, sum, span);
        }
    }
    Err(Error::at(
        "`?` expects a Trial, May, or residual-marked union value",
        span,
    ))
}

/// `?` on a user union with a residual mark (D118).
fn lower_try_sum(
    ctx: &mut LowerCtx,
    scrut_p: Place,
    scrut_ty: Ty,
    sum: SumInfo,
    span: Span,
) -> Result<(Place, Ty)> {
    let residual_tag = sum.residual_tag.ok_or_else(|| {
        Error::at(
            "`?` requires a residual mark (`?`) on one union member",
            span,
        )
    })?;

    // D129 probe: record collapsed residual exit; yield success type for continuation.
    if ctx.inferring {
        let mut layouts_tmp: Vec<TypeLayout> = Vec::new();
        let collapsed = collapse_residual_exit(
            &mut ctx.types,
            &ctx.generics,
            &mut layouts_tmp,
            &scrut_ty,
            span,
        )?;
        ctx.infer_residuals.push(collapsed);
        let success_ty = sum_success_extract_ty(ctx, &sum, span)?;
        let _ = (scrut_p, residual_tag);
        let p = ctx.fresh("isok");
        init_place(ctx, &p, &success_ty)?;
        return Ok((p, success_ty));
    }

    let fn_ret = ctx.fn_ret.clone();
    let same_ret = fn_ret == scrut_ty;
    let rewrap = !same_ret && is_residual_union(&ctx.types, &fn_ret);
    if !same_ret && !rewrap {
        return Err(Error::at(
            "`?` on a union requires the enclosing function to return the same union type (or a residual-compatible Trial/May)",
            span,
        ));
    }

    // Success extract type.
    let success_ty = sum_success_extract_ty(ctx, &sum, span)?;

    let sum_type_name = match &scrut_ty {
        Ty::User(n) => n.clone(),
        _ => {
            return Err(Error::at("internal: sum try on non-User", span));
        }
    };
    let residual_arm = sum
        .variants
        .iter()
        .find(|v| v.tag == residual_tag)
        .cloned()
        .ok_or_else(|| Error::at("internal: bad residual tag", span))?;

    let tag = ctx.fresh("stag");
    ctx.assign(
        tag.clone(),
        Ty::Int64,
        RValue::SumTag {
            place: scrut_p.clone(),
            sum_type: sum_type_name,
        },
    );
    let expected = ctx.fresh("sre");
    ctx.assign(
        expected.clone(),
        Ty::Int64,
        RValue::Int(residual_tag as i64),
    );
    let is_res = ctx.fresh("sres");
    ctx.assign(
        is_res.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Eq,
            left: tag,
            right: expected,
        },
    );

    let outer = std::mem::take(&mut ctx.stmts);
    // then: residual → return (same type or rewrap into fn_ret)
    let mut then_body = Vec::new();
    if same_ret {
        // Move scrut out as the return value; drop other locals first.
        append_return_with_drops(
            &mut then_body,
            &ctx.drop_places,
            Some(scrut_p.clone()),
            &scrut_ty,
        );
    } else {
        // Rewrap residual arm into enclosing Trial/May monomorph (historical Trial ?).
        // Cross-module monomorphs of Fail/Not may differ by name; match residual by arm
        // name and rebuild the target residual product from extracted field(s).
        let saved = std::mem::take(&mut ctx.stmts);
        ctx.stmts = then_body;
        let inj = rewrap_residual_into(ctx, &scrut_p, residual_tag, &residual_arm, &fn_ret, span)?;
        then_body = std::mem::take(&mut ctx.stmts);
        ctx.stmts = saved;
        append_return_with_drops(&mut then_body, &ctx.drop_places, Some(inj), &fn_ret);
    }

    // else: extract success
    let mut else_body = Vec::new();
    let success_place = ctx.fresh("sok");
    // Temporarily use else_body via ctx.stmts
    ctx.stmts = std::mem::take(&mut else_body);
    let ok_p = emit_sum_success_extract(ctx, &scrut_p, &sum, &success_ty, span)?;
    // Void has no C local type — store as Int64 dummy like historical Trial Void.
    let place_ty = if success_ty == Ty::Void {
        Ty::Int64
    } else {
        success_ty.clone()
    };
    ctx.assign(success_place.clone(), place_ty, RValue::Use(ok_p));
    else_body = std::mem::take(&mut ctx.stmts);

    ctx.stmts = outer;
    ctx.stmts.push(Stmt::If {
        cond: is_res,
        then_body,
        else_body,
    });
    Ok((success_place, success_ty))
}

fn sum_success_extract_ty(ctx: &LowerCtx, sum: &SumInfo, span: Span) -> Result<Ty> {
    if let Some(et) = sum.extract_tag {
        let v = sum
            .variants
            .iter()
            .find(|v| v.tag == et)
            .ok_or_else(|| Error::at("internal: bad extract tag", span))?;
        // Type-arg extract (`Hold<Int64!>`): unwrap sole field → Int64.
        // Whole-arm extract (`Hold!`): yield the arm type Hold.
        if v.unwrap_payload {
            return sum_variant_payload_ty(ctx, v, span);
        }
        return Ok(v.ty.clone());
    }
    // W2: no `!` mark — single non-residual arm; one-field → field type; nullary → Void.
    let non_res: Vec<_> = sum
        .variants
        .iter()
        .filter(|v| Some(v.tag) != sum.residual_tag)
        .collect();
    if non_res.len() != 1 {
        return Err(Error::at(
            "`?` success extract is ambiguous (mark success arm with `!` or use a single success member)",
            span,
        ));
    }
    let v = non_res[0];
    match &v.ty {
        Ty::User(n) => {
            let fields = ctx.types.get(n).map(|i| i.fields.as_slice()).unwrap_or(&[]);
            if fields.is_empty() {
                Ok(Ty::Void)
            } else if fields.len() == 1 {
                Ok(fields[0].ty.clone())
            } else {
                Ok(v.ty.clone())
            }
        }
        Ty::Void => Ok(Ty::Void),
        other => Ok(other.clone()),
    }
}

/// Sole field type of a product arm (for type-arg / W2 unwrap).
fn sum_variant_payload_ty(ctx: &LowerCtx, v: &SumVariant, span: Span) -> Result<Ty> {
    match &v.ty {
        Ty::User(n) => {
            let fields = ctx.types.get(n).map(|i| i.fields.as_slice()).unwrap_or(&[]);
            if fields.len() == 1 {
                Ok(fields[0].ty.clone())
            } else if fields.is_empty() {
                Ok(Ty::Void)
            } else {
                Err(Error::at(
                    "type-arg extract requires a single-field product arm",
                    span,
                ))
            }
        }
        Ty::Void => Ok(Ty::Void),
        other => Ok(other.clone()),
    }
}

fn emit_sum_success_extract(
    ctx: &mut LowerCtx,
    scrut_p: &Place,
    sum: &SumInfo,
    success_ty: &Ty,
    span: Span,
) -> Result<Place> {
    let (tag, arm_ty, unwrap) = if let Some(et) = sum.extract_tag {
        let v = sum
            .variants
            .iter()
            .find(|v| v.tag == et)
            .ok_or_else(|| Error::at("internal: bad extract tag", span))?;
        (v.tag, v.ty.clone(), v.unwrap_payload)
    } else {
        let v = sum
            .variants
            .iter()
            .find(|v| Some(v.tag) != sum.residual_tag)
            .ok_or_else(|| Error::at("internal: no success arm", span))?;
        // W2 auto-unwrap when success_ty is the field type.
        (v.tag, v.ty.clone(), true)
    };

    if arm_ty == Ty::Void || success_ty == &Ty::Void {
        let p = ctx.fresh("svoid");
        ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
        return Ok(p);
    }

    // Intermediate product aliases the sum arm — no drop registration.
    let payload = ctx.fresh("spay");
    ctx.assign_no_drop(
        payload.clone(),
        arm_ty.clone(),
        RValue::SumPayload {
            base: scrut_p.clone(),
            tag,
            payload_ty: arm_ty.clone(),
        },
    );

    // Unwrap sole field when type-arg `!` / W2 demands the payload type.
    if unwrap {
        if success_ty == &Ty::Void {
            let p = ctx.fresh("svoid");
            ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
            return Ok(p);
        }
        if let Ty::User(n) = &arm_ty {
            let field_name = ctx.types.get(n).and_then(|info| {
                if info.fields.len() == 1
                    && info.fields[0].ty == *success_ty
                    && info.fields[0].ty != Ty::Void
                {
                    Some(info.fields[0].name.clone())
                } else {
                    None
                }
            });
            if let Some(fname) = field_name {
                let fp = ctx.fresh("sf");
                ctx.assign(
                    fp.clone(),
                    success_ty.clone(),
                    RValue::Field {
                        base: payload,
                        field: sanitize_field(&fname),
                    },
                );
                if needs_drop(success_ty) {
                    ctx.stmts.push(Stmt::Dup {
                        place: fp.clone(),
                        ty: success_ty.clone(),
                    });
                }
                return Ok(fp);
            }
        }
    }
    // Whole-arm extract: re-home into a dropped place and Dup so sum + result both own.
    let owned = ctx.fresh("sown");
    ctx.assign(owned.clone(), arm_ty.clone(), RValue::Use(payload));
    let _ = span;
    Ok(owned)
}

/// Narrowest unsigned scalar that can hold `n` distinct tags (Byte-up chain).
/// (`Ty` has no UInt16 — mid-range uses UInt32.)
fn pure_lit_scalar_ty(n_arms: usize) -> Ty {
    match n_arms {
        0..=256 => Ty::Byte,
        257..=65536 => Ty::UInt32,
        _ => Ty::UInt64,
    }
}

fn display_type_lit(lit: &TypeLit) -> String {
    match lit {
        TypeLit::Int(n) => n.to_string(),
        TypeLit::Bool(b) => b.to_string(),
        TypeLit::String(s) => format!("\"{s}\""),
    }
}

/// Stable mangled name for a bare singleton literal type (D131).
fn lit_type_name(lit: &TypeLit) -> String {
    match lit {
        TypeLit::Int(n) => format!("$lit$liti_{n}"),
        TypeLit::Bool(b) => format!("$lit$litb_{b}"),
        TypeLit::String(s) => {
            let safe: String = s
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            format!("$lit$lits_{safe}")
        }
    }
}

/// Ensure a single-member pure-literal sum type exists; return its `Ty::User`.
fn ensure_lit_type(
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    lit: &TypeLit,
    def_mod: &str,
) -> Ty {
    let name = lit_type_name(lit);
    if types.contains_key(&name) {
        return Ty::User(name);
    }
    let vname = display_type_lit(lit);
    let sum = SumInfo {
        variants: vec![SumVariant {
            tag: 0,
            name: vname.clone(),
            ty: Ty::Void,
            lit: Some(lit.clone()),
            mark: None,
            unwrap_payload: false,
        }],
        residual_tag: None,
        extract_tag: None,
    };
    let info = TypeInfo {
        fields: Vec::new(),
        methods: HashMap::new(),
        zero_init: false,
        def_mod: def_mod.to_string(),
        sum: Some(sum),
        refined: None,
        resource: false,
        traits: Vec::new(),
        derived: HashSet::new(),
        instance: None,
        templates: HashMap::new(),
    };
    let layout = TypeLayout {
        name: name.clone(),
        fields: Vec::new(),
        sum: Some(vec![SumVariantLayout {
            tag: 0,
            name: vname,
            payload_ty: Ty::Void,
        }]),
        sum_scalar: Some(pure_lit_scalar_ty(1)),
        has_drop: false,
        own_hash: false,
        own_equals: false,
    };
    types.insert(name.clone(), info);
    layouts.push(layout);
    Ty::User(name)
}

/// Canonical synthetic name for a member-set union of already-resolved member tys.
fn member_set_type_name(types: &HashMap<String, TypeInfo>, mem_tys: &[Ty]) -> String {
    let mut keys: Vec<String> = Vec::new();
    for ty in mem_tys {
        if let Some(members) = collect_union_members(types, ty) {
            for (name, mty, lit) in members {
                let key = if let Some(ref l) = lit {
                    match l {
                        TypeLit::Int(n) => format!("liti_{n}"),
                        TypeLit::Bool(b) => format!("litb_{b}"),
                        TypeLit::String(s) => {
                            let safe: String = s
                                .chars()
                                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                                .collect();
                            format!("lits_{safe}")
                        }
                    }
                } else if let Ty::User(n) = &mty {
                    format!("ty_{n}")
                } else {
                    format!(
                        "other_{}",
                        format!("{mty:?}").replace(|c: char| !c.is_ascii_alphanumeric(), "_")
                    )
                };
                let _ = name;
                keys.push(key);
            }
        } else {
            keys.push(format!(
                "atom_{}",
                format!("{ty:?}").replace(|c: char| !c.is_ascii_alphanumeric(), "_")
            ));
        }
    }
    keys.sort();
    keys.dedup();
    format!("$union${}", keys.join("$"))
}

/// Ensure a member-set union type for `mem_tys` exists (flattens nested sums).
fn ensure_member_set_type(
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    mem_tys: &[Ty],
    span: Span,
    def_mod: &str,
) -> Result<Ty> {
    // Expand to atoms/lits.
    let mut flat: Vec<(String, Ty, Option<TypeLit>)> = Vec::new();
    let mut seen = HashSet::new();
    for ty in mem_tys {
        if let Some(members) = collect_union_members(types, ty) {
            for (name, mty, lit) in members {
                let key = if let Some(ref l) = lit {
                    lit_type_name(l)
                } else if let Ty::User(n) = &mty {
                    format!("ty_{n}")
                } else {
                    format!("{mty:?}")
                };
                if seen.insert(key) {
                    flat.push((name, mty, lit));
                }
            }
        } else {
            return Err(Error::at(
                format!("cannot form union member from {ty:?}"),
                span,
            ));
        }
    }
    if flat.is_empty() {
        return Err(Error::at("empty union type", span));
    }
    if flat.len() == 1 {
        let (name, ty, lit) = &flat[0];
        if let Some(lit) = lit {
            return Ok(ensure_lit_type(types, layouts, lit, def_mod));
        }
        let _ = name;
        return Ok(ty.clone());
    }

    let name = member_set_type_name(types, mem_tys);
    if types.contains_key(&name) {
        return Ok(Ty::User(name));
    }

    let mut variants = Vec::new();
    let mut layout_vars = Vec::new();
    for (i, (vname, ty, lit)) in flat.iter().enumerate() {
        let tag = i as u32;
        let payload_ty = if lit.is_some() { Ty::Void } else { ty.clone() };
        variants.push(SumVariant {
            tag,
            name: vname.clone(),
            ty: payload_ty.clone(),
            lit: lit.clone(),
            mark: None,
            unwrap_payload: false,
        });
        layout_vars.push(SumVariantLayout {
            tag,
            name: vname.clone(),
            payload_ty,
        });
    }
    let pure_lit = variants.iter().all(|v| v.lit.is_some());
    let sum = SumInfo {
        variants,
        residual_tag: None,
        extract_tag: None,
    };
    let info = TypeInfo {
        fields: Vec::new(),
        methods: HashMap::new(),
        zero_init: false,
        def_mod: def_mod.to_string(),
        sum: Some(sum),
        refined: None,
        resource: false,
        traits: Vec::new(),
        derived: HashSet::new(),
        instance: None,
        templates: HashMap::new(),
    };
    let layout = TypeLayout {
        name: name.clone(),
        fields: Vec::new(),
        sum: Some(layout_vars),
        sum_scalar: if pure_lit {
            Some(pure_lit_scalar_ty(flat.len()))
        } else {
            None
        },
        has_drop: false,
        own_hash: false,
        own_equals: false,
    };
    types.insert(name.clone(), info);
    layouts.push(layout);
    Ok(Ty::User(name))
}

/// Walk package TypeExprs and materialize bare lit / inline union types (D131).
fn materialize_package_typespace(
    package: &Package,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    per_mod_types: &HashMap<String, HashMap<String, String>>,
) -> Result<()> {
    for m in &package.modules {
        let name_map = per_mod_types.get(&m.id).cloned().unwrap_or_default();
        for item in &m.ast.items {
            materialize_item_typespace(item, types, layouts, &name_map, &m.id)?;
        }
    }
    Ok(())
}

fn materialize_item_typespace(
    item: &Item,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    name_map: &HashMap<String, String>,
    def_mod: &str,
) -> Result<()> {
    match item {
        Item::Function(f) => {
            for p in &f.params {
                materialize_type_expr(&p.ty, types, layouts, name_map, def_mod)?;
            }
            if let Some(rt) = &f.return_type {
                materialize_type_expr(rt, types, layouts, name_map, def_mod)?;
            }
            materialize_block_typespace(&f.body, types, layouts, name_map, def_mod)?;
        }
        Item::Type(td) => {
            for p in &td.params {
                materialize_type_expr(&p.ty, types, layouts, name_map, def_mod)?;
            }
            for f in &td.fields {
                materialize_type_expr(&f.ty, types, layouts, name_map, def_mod)?;
            }
            if let Some(members) = &td.union_members {
                for mem in members {
                    materialize_type_expr(mem, types, layouts, name_map, def_mod)?;
                }
            }
            for method in td.methods.iter().filter(|method| method.generic.is_empty()) {
                for p in &method.params {
                    materialize_type_expr(&p.ty, types, layouts, name_map, def_mod)?;
                }
                if let Some(rt) = &method.return_type {
                    materialize_type_expr(rt, types, layouts, name_map, def_mod)?;
                }
                materialize_block_typespace(&method.body, types, layouts, name_map, def_mod)?;
            }
        }
        Item::Extern(e) => {
            for p in &e.params {
                materialize_type_expr(&p.ty, types, layouts, name_map, def_mod)?;
            }
            if let Some(rt) = &e.return_type {
                materialize_type_expr(rt, types, layouts, name_map, def_mod)?;
            }
        }
        Item::Global(g) => {
            if let Some(ann) = &g.ty {
                materialize_type_expr(ann, types, layouts, name_map, def_mod)?;
            }
            materialize_expr_typespace(&g.init, types, layouts, name_map, def_mod)?;
        }
        Item::Import(_) | Item::Extend(_) | Item::Trait(_) | Item::Conform(_) => {}
    }
    Ok(())
}

fn materialize_block_typespace(
    block: &Block,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    name_map: &HashMap<String, String>,
    def_mod: &str,
) -> Result<()> {
    for it in &block.items {
        materialize_block_item_typespace(it, types, layouts, name_map, def_mod)?;
    }
    if let Some(t) = &block.tail {
        materialize_expr_typespace(t, types, layouts, name_map, def_mod)?;
    }
    Ok(())
}

fn materialize_block_item_typespace(
    it: &BlockItem,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    name_map: &HashMap<String, String>,
    def_mod: &str,
) -> Result<()> {
    match it {
        BlockItem::Let { ty, init, .. } => {
            if let Some(ann) = ty {
                materialize_type_expr(ann, types, layouts, name_map, def_mod)?;
            }
            materialize_expr_typespace(init, types, layouts, name_map, def_mod)?;
        }
        BlockItem::Assign { value, .. } => {
            materialize_expr_typespace(value, types, layouts, name_map, def_mod)?;
        }
        BlockItem::Expr(e) => materialize_expr_typespace(e, types, layouts, name_map, def_mod)?,
    }
    Ok(())
}

fn materialize_expr_typespace(
    expr: &Expr,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    name_map: &HashMap<String, String>,
    def_mod: &str,
) -> Result<()> {
    match &expr.kind {
        ExprKind::Int(_) | ExprKind::BigInt(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Name(_)
        | ExprKind::MutArg(_)
        | ExprKind::Break
        | ExprKind::Continue => {}
        ExprKind::Interpolate(parts) | ExprKind::TaggedInterp { parts, .. } => {
            for p in parts {
                if let InterpPart::Expr(e) = p {
                    materialize_expr_typespace(e, types, layouts, name_map, def_mod)?;
                }
            }
        }
        ExprKind::Unary { expr: inner, .. } => {
            materialize_expr_typespace(inner, types, layouts, name_map, def_mod)?;
        }
        ExprKind::Binary { left, right, .. } => {
            materialize_expr_typespace(left, types, layouts, name_map, def_mod)?;
            materialize_expr_typespace(right, types, layouts, name_map, def_mod)?;
        }
        ExprKind::Call { args, .. } | ExprKind::Start { args, .. } => {
            for a in args {
                materialize_arg_typespace(a, types, layouts, name_map, def_mod)?;
            }
        }
        ExprKind::IndirectCall { callee, args } => {
            materialize_expr_typespace(callee, types, layouts, name_map, def_mod)?;
            for a in args {
                materialize_arg_typespace(a, types, layouts, name_map, def_mod)?;
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            materialize_expr_typespace(receiver, types, layouts, name_map, def_mod)?;
            for a in args {
                materialize_arg_typespace(a, types, layouts, name_map, def_mod)?;
            }
        }
        ExprKind::Field { base, .. } => {
            materialize_expr_typespace(base, types, layouts, name_map, def_mod)?;
        }
        ExprKind::Index { base, index } => {
            materialize_expr_typespace(base, types, layouts, name_map, def_mod)?;
            materialize_expr_typespace(index, types, layouts, name_map, def_mod)?;
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            materialize_expr_typespace(cond, types, layouts, name_map, def_mod)?;
            materialize_block_typespace(then_branch, types, layouts, name_map, def_mod)?;
            if let Some(eb) = else_branch {
                materialize_block_typespace(eb, types, layouts, name_map, def_mod)?;
            }
        }
        ExprKind::While { cond, body } => {
            materialize_expr_typespace(cond, types, layouts, name_map, def_mod)?;
            materialize_block_typespace(body, types, layouts, name_map, def_mod)?;
        }
        ExprKind::Loop { body } => {
            materialize_block_typespace(body, types, layouts, name_map, def_mod)?;
        }
        ExprKind::For {
            start, end, body, ..
        } => {
            materialize_expr_typespace(start, types, layouts, name_map, def_mod)?;
            if let Some(e) = end {
                materialize_expr_typespace(e, types, layouts, name_map, def_mod)?;
            }
            materialize_block_typespace(body, types, layouts, name_map, def_mod)?;
        }
        ExprKind::Match { scrutinee, arms } => {
            materialize_expr_typespace(scrutinee, types, layouts, name_map, def_mod)?;
            for arm in arms {
                if let Some(g) = &arm.guard {
                    materialize_expr_typespace(g, types, layouts, name_map, def_mod)?;
                }
                materialize_block_typespace(&arm.body, types, layouts, name_map, def_mod)?;
            }
        }
        ExprKind::Return { value } => {
            if let Some(v) = value {
                materialize_expr_typespace(v, types, layouts, name_map, def_mod)?;
            }
        }
        ExprKind::Try(inner) => {
            materialize_expr_typespace(inner, types, layouts, name_map, def_mod)?;
        }
        ExprKind::Cast { expr: inner, ty, .. } => {
            materialize_type_expr(ty, types, layouts, name_map, def_mod)?;
            materialize_expr_typespace(inner, types, layouts, name_map, def_mod)?;
        }
        ExprKind::ArrayLit(elems) => {
            for e in elems {
                materialize_expr_typespace(e, types, layouts, name_map, def_mod)?;
            }
        }
        ExprKind::MapLit(entries) => {
            for e in entries.iter().flat_map(|entry| entry.exprs()) {
                materialize_expr_typespace(e, types, layouts, name_map, def_mod)?;
            }
        }
        ExprKind::Block(b) => {
            materialize_block_typespace(b, types, layouts, name_map, def_mod)?;
        }
    }
    Ok(())
}

fn materialize_arg_typespace(
    arg: &ast::Arg,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    name_map: &HashMap<String, String>,
    def_mod: &str,
) -> Result<()> {
    match arg {
        ast::Arg::Pos(e) | ast::Arg::Named { value: e, .. } | ast::Arg::Spread { value: e, .. } => {
            materialize_expr_typespace(e, types, layouts, name_map, def_mod)
        }
    }
}

fn materialize_type_expr(
    te: &TypeExpr,
    types: &mut HashMap<String, TypeInfo>,
    layouts: &mut Vec<TypeLayout>,
    name_map: &HashMap<String, String>,
    def_mod: &str,
) -> Result<()> {
    if let Some(lit) = &te.lit {
        ensure_lit_type(types, layouts, lit, def_mod);
        return Ok(());
    }
    if let Some(members) = &te.union_members {
        for m in members {
            materialize_type_expr(m, types, layouts, name_map, def_mod)?;
        }
        if members.len() >= 2 {
            let mut mem_tys = Vec::new();
            for m in members {
                mem_tys.push(resolve_ty(m, types, name_map)?);
            }
            ensure_member_set_type(types, layouts, &mem_tys, te.span, def_mod)?;
        } else if members.len() == 1 {
            materialize_type_expr(&members[0], types, layouts, name_map, def_mod)?;
        }
        return Ok(());
    }
    for a in &te.args {
        if let TypeArg::Type(inner) = a {
            materialize_type_expr(inner, types, layouts, name_map, def_mod)?;
        }
    }
    Ok(())
}

/// Stable key for a union arm (D119 member-set equality).
/// Keys must be C-identifier-safe when embedded in synthetic type names.
fn sum_variant_key(v: &SumVariant) -> String {
    if let Some(lit) = &v.lit {
        match lit {
            TypeLit::Int(n) => format!("liti_{n}"),
            TypeLit::Bool(b) => format!("litb_{b}"),
            TypeLit::String(s) => {
                let safe: String = s
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect();
                format!("lits_{safe}")
            }
        }
    } else if let Ty::User(n) = &v.ty {
        format!("ty_{n}")
    } else {
        format!(
            "other_{}",
            format!("{:?}", v.ty).replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        )
    }
}

fn sum_member_set(sum: &SumInfo) -> HashSet<String> {
    sum.variants.iter().map(sum_variant_key).collect()
}

fn member_sets_eq(a: &SumInfo, b: &SumInfo) -> bool {
    sum_member_set(a) == sum_member_set(b)
}

fn sum_info_of<'a>(types: &'a HashMap<String, TypeInfo>, ty: &Ty) -> Option<&'a SumInfo> {
    match ty {
        Ty::User(n) => types.get(n).and_then(|i| i.sum.as_ref()),
        _ => None,
    }
}

fn lit_sum_widens_to(types: &HashMap<String, TypeInfo>, from: &Ty, to: &Ty) -> bool {
    match to {
        Ty::String => is_pure_lit_sum_of_kind(types, from, LitKind::String),
        Ty::Int64 => is_pure_lit_sum_of_kind(types, from, LitKind::Int),
        Ty::Bool => is_pure_lit_sum_of_kind(types, from, LitKind::Bool),
        _ => false,
    }
}

/// Materialize a pure-literal union as its base (`"r"|"w"` → String, `0|1` → Int64).
fn emit_lit_widen(ctx: &mut LowerCtx, place: Place, from: &Ty, to: &Ty) -> Result<Option<Place>> {
    if !lit_sum_widens_to(&ctx.types, from, to) {
        return Ok(None);
    }
    let Ty::User(from_n) = from else {
        return Ok(None);
    };
    let sum = ctx
        .types
        .get(from_n)
        .and_then(|i| i.sum.clone())
        .ok_or_else(|| Error::new("internal: lit widen on non-sum"))?;
    let tag = ctx.fresh("wtag");
    ctx.assign(
        tag.clone(),
        Ty::Int64,
        RValue::SumTag {
            place,
            sum_type: from_n.clone(),
        },
    );
    let out = ctx.fresh("wwide");
    init_place(ctx, &out, to)?;

    let mut acc: Vec<Stmt> = Vec::new();
    for v in sum.variants.iter().rev() {
        let Some(lit) = &v.lit else {
            continue;
        };
        let want = ctx.fresh("wte");
        ctx.assign(want.clone(), Ty::Int64, RValue::Int(v.tag as i64));
        let cond = ctx.fresh("wc");
        ctx.assign(
            cond.clone(),
            Ty::Bool,
            RValue::Binary {
                op: BinaryOp::Eq,
                left: tag.clone(),
                right: want,
            },
        );
        let saved = std::mem::take(&mut ctx.stmts);
        match lit {
            TypeLit::String(s) => {
                ctx.assign(out.clone(), Ty::String, RValue::StringLit(s.clone()));
            }
            TypeLit::Int(n) => {
                ctx.assign(out.clone(), Ty::Int64, RValue::Int(*n));
            }
            TypeLit::Bool(b) => {
                ctx.assign(out.clone(), Ty::Bool, RValue::Bool(*b));
            }
        }
        let then_body = std::mem::take(&mut ctx.stmts);
        ctx.stmts = saved;
        acc = vec![Stmt::If {
            cond,
            then_body,
            else_body: acc,
        }];
    }
    ctx.stmts.extend(acc);
    Ok(Some(out))
}

fn maybe_widen_lit_sum(ctx: &mut LowerCtx, place: Place, ty: &Ty) -> Result<Option<(Place, Ty)>> {
    let to = if is_pure_lit_sum_of_kind(&ctx.types, ty, LitKind::String) {
        Ty::String
    } else if is_pure_lit_sum_of_kind(&ctx.types, ty, LitKind::Int) {
        Ty::Int64
    } else if is_pure_lit_sum_of_kind(&ctx.types, ty, LitKind::Bool) {
        Ty::Bool
    } else {
        return Ok(None);
    };
    match emit_lit_widen(ctx, place, ty, &to)? {
        Some(p) => Ok(Some((p, to))),
        None => Ok(None),
    }
}

/// Type equality including transparent union aliases (D119).
fn ty_eq(types: &HashMap<String, TypeInfo>, a: &Ty, b: &Ty) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (Ty::User(_), Ty::User(_)) => match (sum_info_of(types, a), sum_info_of(types, b)) {
            (Some(sa), Some(sb)) => member_sets_eq(sa, sb),
            _ => false,
        },
        (Ty::Array { elem: ae }, Ty::Array { elem: be }) => ty_eq(types, ae, be),
        (Ty::Map { key: ak, val: av }, Ty::Map { key: bk, val: bv }) => {
            ty_eq(types, ak, bk) && ty_eq(types, av, bv)
        }
        (Ty::FixedArray { elem: ae, len: al }, Ty::FixedArray { elem: be, len: bl }) => {
            al == bl && ty_eq(types, ae, be)
        }
        (Ty::Address(ae), Ty::Address(be)) => ty_eq(types, ae, be),
        _ => false,
    }
}

/// Whether `from` can be coerced to `to` (equality, inject, or transparent remap).
/// [`can_coerce_ty`], or a value with a JSON encoding where a `Json` is
/// expected (D191 §4). Only for an expected type: dispatch and unification
/// never encode.
/// A branch of an `if` where `expect` is expected: its type fits, or its
/// value is a literal `expect` takes (`"less"` where an `Order` is), or an
/// `if` whose branches are.
fn branch_fits(ctx: &LowerCtx, branch: &Block, ty: &Ty, expect: &Ty) -> bool {
    if fits_expected(&ctx.types, ty, expect) {
        return true;
    }
    let Some(tail) = &branch.tail else {
        return false;
    };
    if let Some(lit) = expr_as_type_lit(tail) {
        return sum_info_of(&ctx.types, expect)
            .is_some_and(|sum| sum.variants.iter().any(|variant| variant.lit.as_ref() == Some(&lit)));
    }
    match &tail.kind {
        ExprKind::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => {
            let then_ty = infer_block_ty(ctx, then_branch).unwrap_or(Ty::Void);
            let else_ty = infer_block_ty(ctx, else_branch).unwrap_or(Ty::Void);
            branch_fits(ctx, then_branch, &then_ty, expect)
                && branch_fits(ctx, else_branch, &else_ty, expect)
        }
        ExprKind::Block(inner) => branch_fits(ctx, inner, ty, expect),
        _ => false,
    }
}

fn fits_expected(types: &HashMap<String, TypeInfo>, from: &Ty, to: &Ty) -> bool {
    can_coerce_ty(types, from, to)
        || (matches!(to, Ty::User(n) if n == "std_json_Json")
            && !matches!(
                from,
                Ty::Void | Ty::Fn { .. } | Ty::Bytes | Ty::Buffer | Ty::Node | Ty::Address(_)
            ))
}

fn can_coerce_ty(types: &HashMap<String, TypeInfo>, from: &Ty, to: &Ty) -> bool {
    // Bottom type flows into any expectation (diverging `loop`, D128).
    if is_never(from) {
        return true;
    }
    if ty_eq(types, from, to) {
        return true;
    }
    // `"espresso" | "mocha"` <: String (and 0|1 <: Int64, true|false <: Bool).
    if lit_sum_widens_to(types, from, to) {
        return true;
    }
    if let Some(rf) = refined_info(types, from) {
        if &rf.base == to {
            return true;
        }
    }
    let Ty::User(to_n) = to else {
        return false;
    };
    let Some(to_sum) = types.get(to_n).and_then(|i| i.sum.as_ref()) else {
        return false;
    };
    // Atom inject into union (User or builtin payload).
    if let Ty::User(from_n) = from {
        if to_sum.variants.iter().any(|v| match &v.ty {
            Ty::User(n) => n == from_n,
            _ => false,
        }) {
            return true;
        }
    } else if to_sum.variants.iter().any(|v| ty_eq(types, &v.ty, from)) {
        return true;
    }
    let Ty::User(from_n) = from else {
        return false;
    };
    // Nested / smaller sum inject into larger (after flatten, members are atoms).
    if let Some(from_sum) = types.get(from_n).and_then(|i| i.sum.as_ref()) {
        let to_set = sum_member_set(to_sum);
        return sum_member_set(from_sum).iter().all(|k| to_set.contains(k));
    }
    false
}

/// Remap a sum value to another union with the same member set (D119).
/// `place` of union `from_n` as union `to_n`, member by member. With
/// `narrowing`, members `to_n` lacks are skipped: the caller has checked the
/// value is not one of them (a multi-clause arm for a smaller union).
fn emit_sum_remap(
    ctx: &mut LowerCtx,
    place: Place,
    from_n: &str,
    to_n: &str,
    from_sum: &SumInfo,
    to_sum: &SumInfo,
    narrowing: bool,
) -> Result<Place> {
    let mut to_by_key: HashMap<String, (u32, Ty)> = HashMap::new();
    for v in &to_sum.variants {
        to_by_key.insert(sum_variant_key(v), (v.tag, v.ty.clone()));
    }

    let out_ty = Ty::User(to_n.to_string());
    let out = ctx.fresh("srm");
    init_place(ctx, &out, &out_ty)?;

    let tag_p = ctx.fresh("srt");
    ctx.assign(
        tag_p.clone(),
        Ty::Int64,
        RValue::SumTag {
            place: place.clone(),
            sum_type: from_n.to_string(),
        },
    );

    let mut acc: Vec<Stmt> = Vec::new();
    for fv in from_sum.variants.iter().rev() {
        let key = sum_variant_key(fv);
        let Some(&(to_tag, ref to_payload_ty)) = to_by_key.get(&key) else {
            if narrowing {
                continue;
            }
            return Err(Error::new(format!(
                "internal: cannot remap union member `{key}` from `{from_n}` to `{to_n}`"
            )));
        };
        let to_payload_ty = to_payload_ty.clone();

        let outer = std::mem::take(&mut ctx.stmts);
        let payload = if fv.ty != Ty::Void {
            let pp = ctx.fresh("srp");
            // Alias source arm; no drop on intermediate product.
            ctx.assign_no_drop(
                pp.clone(),
                fv.ty.clone(),
                RValue::SumPayload {
                    base: place.clone(),
                    tag: fv.tag,
                    payload_ty: fv.ty.clone(),
                },
            );
            // Own a ref for the inject (source sum still owns its arm).
            if needs_drop(&fv.ty) {
                ctx.stmts.push(Stmt::Dup {
                    place: pp.clone(),
                    ty: fv.ty.clone(),
                });
            }
            pp
        } else {
            let dummy = ctx.fresh("srz");
            ctx.assign(dummy.clone(), Ty::Int64, RValue::Int(0));
            dummy
        };
        let injected = ctx.fresh("sri");
        ctx.assign(
            injected.clone(),
            out_ty.clone(),
            RValue::SumInject {
                sum_type: to_n.to_string(),
                tag: to_tag,
                payload,
                payload_ty: to_payload_ty,
            },
        );
        // Share into `out` (Use dups when sum needs_drop).
        ctx.assign(out.clone(), out_ty.clone(), RValue::Use(injected));
        let body = std::mem::take(&mut ctx.stmts);
        ctx.stmts = outer;

        let expected = ctx.fresh("sre");
        ctx.assign(expected.clone(), Ty::Int64, RValue::Int(fv.tag as i64));
        let cond = ctx.fresh("src");
        ctx.assign(
            cond.clone(),
            Ty::Bool,
            RValue::Binary {
                op: BinaryOp::Eq,
                left: tag_p.clone(),
                right: expected,
            },
        );
        acc = vec![Stmt::If {
            cond,
            then_body: body,
            else_body: acc,
        }];
    }
    ctx.stmts.extend(acc);
    Ok(out)
}

/// Coerce `place: from` to `to` when possible (inject / transparent remap / lit inject).
fn try_sum_coerce(
    ctx: &mut LowerCtx,
    place: Place,
    from: &Ty,
    to: &Ty,
    lit: Option<&TypeLit>,
    span: Span,
) -> Result<Option<Place>> {
    if from == to {
        return Ok(Some(place));
    }
    if let Some(p) = emit_lit_widen(ctx, place.clone(), from, to)? {
        return Ok(Some(p));
    }
    // Transparent aliases / nested flatten: remap when from members ⊆ to (D119).
    if let (Ty::User(from_n), Ty::User(to_n)) = (from, to) {
        if let (Some(fs), Some(ts)) = (
            ctx.types.get(from_n).and_then(|i| i.sum.clone()),
            ctx.types.get(to_n).and_then(|i| i.sum.clone()),
        ) {
            let to_set = sum_member_set(&ts);
            let from_set = sum_member_set(&fs);
            if !from_set.is_empty() && from_set.iter().all(|k| to_set.contains(k)) {
                return Ok(Some(emit_sum_remap(ctx, place, from_n, to_n, &fs, &ts, false)?));
            }
        }
    }
    if let Some(inj) = try_sum_inject(ctx, place, from, to, span)? {
        return Ok(Some(inj));
    }
    if let Some(lit) = lit {
        if let Some(inj) = try_sum_inject_lit(ctx, lit, to, span)? {
            return Ok(Some(inj));
        }
    }
    Ok(None)
}

/// If `from` is a member of union type `to`, inject into a sum value (D118).
fn try_sum_inject(
    ctx: &mut LowerCtx,
    place: Place,
    from: &Ty,
    to: &Ty,
    span: Span,
) -> Result<Option<Place>> {
    let Ty::User(to_n) = to else {
        return Ok(None);
    };
    let Some(info) = ctx.types.get(to_n) else {
        return Ok(None);
    };
    let Some(sum) = info.sum.as_ref() else {
        return Ok(None);
    };
    let found = sum.variants.iter().find(|v| match (&v.ty, from) {
        (Ty::User(n), Ty::User(from_n)) => n == from_n,
        (vt, ft) => ty_eq(&ctx.types, vt, ft),
    });
    // A `mixed T` holds any type that conforms (D235).
    let v = match found {
        Some(found) => found.clone(),
        None if is_mixed(to_n) => {
            let to_n = to_n.clone();
            match mixed_variant(ctx, &to_n, from) {
                Some(variant) => variant,
                None if matches!(from, Ty::Void) || is_never(from) => return Ok(None),
                None => return Err(not_mixed_member(ctx, &to_n, from, span)),
            }
        }
        None => return Ok(None),
    };
    let tag = v.tag;
    let payload_ty = v.ty.clone();
    // Heap-bearing payload: Dup so inject's shallow copy and the source both own.
    if needs_drop(&payload_ty) {
        ctx.stmts.push(Stmt::Dup {
            place: place.clone(),
            ty: payload_ty.clone(),
        });
    }
    let p = ctx.fresh("sum");
    ctx.assign(
        p.clone(),
        to.clone(),
        RValue::SumInject {
            sum_type: to_n.clone(),
            tag,
            payload: place,
            payload_ty,
        },
    );
    Ok(Some(p))
}

/// The member of `mixed` that holds a `ty` (D235): one already in it, or,
/// when `ty` conforms to its trait, a new one, which every body lowered
/// after this one sees too. A type only generic code's checks use (an
/// archetype) is added for this body alone: it is never a program's.
fn mixed_variant(ctx: &mut LowerCtx, mixed: &str, ty: &Ty) -> Option<SumVariant> {
    {
        let synthetics = ctx.synthetics.borrow();
        if let Some(added) = synthetics.mixed_added.get(mixed) {
            add_mixed_variants(&mut ctx.types, mixed, added);
        }
    }
    let sum = ctx.types.get(mixed)?.sum.clone()?;
    if let Some(found) = sum.variants.iter().find(|variant| ty_eq(&ctx.types, &variant.ty, ty)) {
        return Some(found.clone());
    }
    let trait_ref = ctx.synthetics.borrow().mixed_members.get(mixed)?.0.clone();
    // A `Void` is no value to hold.
    if matches!(ty, Ty::Void) || is_never(ty) {
        return None;
    }
    if !ty_conforms(&ctx.types, &ctx.synthetics.borrow().conditional, ty, &trait_ref) {
        return None;
    }
    let variant = SumVariant {
        tag: sum.variants.iter().map(|variant| variant.tag + 1).max().unwrap_or(0),
        name: display_ty(ty),
        ty: ty.clone(),
        lit: None,
        mark: None,
        unwrap_payload: false,
    };
    if !mentions_archetype(ty) {
        ctx.synthetics
            .borrow_mut()
            .mixed_added
            .entry(mixed.to_string())
            .or_default()
            .push(variant.clone());
    }
    add_mixed_variants(&mut ctx.types, mixed, std::slice::from_ref(&variant));
    Some(variant)
}

/// "`Box<Secret>` is not `Describe`, so it does not convert to `mixed
/// Describe`", naming the bounds of a generic type's conformance (D235).
fn not_mixed_member(ctx: &LowerCtx, mixed: &str, from: &Ty, span: Span) -> Error {
    let synthetics = ctx.synthetics.borrow();
    let trait_name = synthetics
        .mixed_members
        .get(mixed)
        .map_or_else(|| display_ty(&Ty::User(mixed.to_string())), |(trait_ref, _)| trait_ref.name.clone());
    let error = Error::at(
        format!(
            "{} is not `{trait_name}`, so it does not convert to `{}`",
            display_ty(from),
            display_ty(&Ty::User(mixed.to_string()))
        ),
        span,
    );
    let template = match from {
        Ty::User(name) => ctx.types.get(name).and_then(|info| info.instance.as_ref()).map(|(base, _)| base.clone()),
        _ => None,
    };
    let bounds = template.and_then(|base| {
        let gdef = synthetics.generics.get(&base)?;
        let conformance = synthetics
            .conditional
            .get(&base)?
            .iter()
            .find(|each| each.trait_ref.name == trait_name)?;
        let parts: Vec<String> = gdef
            .type_params
            .iter()
            .zip(&conformance.bounds)
            .filter(|(_, bounds)| !bounds.is_empty())
            .map(|(param, bounds)| {
                let names: Vec<&str> = bounds.iter().map(|bound| bound.name.as_str()).collect();
                format!("`{param}` is `{}`", names.join(" & "))
            })
            .collect();
        (!parts.is_empty()).then(|| format!("`{}` is `{trait_name}` when {}", gdef.source_name, parts.join(" and ")))
    });
    match bounds {
        Some(help) => error.with_help(help),
        None => error,
    }
}

/// Whether `ty` is, or holds, a type only generic code's checks use.
fn mentions_archetype(ty: &Ty) -> bool {
    match ty {
        Ty::User(name) => is_param_archetype(ty) || name.starts_with("Self_") || name.contains("_Self_"),
        Ty::Array { elem } => mentions_archetype(elem),
        Ty::Map { key, val } => mentions_archetype(key) || mentions_archetype(val),
        _ => false,
    }
}

/// Inject a literal expression into a pure-literal (or lit-arm) union (D118).
fn try_sum_inject_lit(
    ctx: &mut LowerCtx,
    lit: &TypeLit,
    to: &Ty,
    span: Span,
) -> Result<Option<Place>> {
    let Ty::User(to_n) = to else {
        return Ok(None);
    };
    let Some(info) = ctx.types.get(to_n) else {
        return Ok(None);
    };
    let Some(sum) = info.sum.as_ref() else {
        return Ok(None);
    };
    let Some(v) = sum.variants.iter().find(|v| v.lit.as_ref() == Some(lit)) else {
        return Ok(None);
    };
    let tag = v.tag;
    // Dummy payload for void lit arms.
    let dummy = ctx.fresh("slit");
    ctx.assign(dummy.clone(), Ty::Int64, RValue::Int(0));
    let p = ctx.fresh("sum");
    ctx.assign(
        p.clone(),
        to.clone(),
        RValue::SumInject {
            sum_type: to_n.clone(),
            tag,
            payload: dummy,
            payload_ty: Ty::Void,
        },
    );
    let _ = span;
    Ok(Some(p))
}

fn expr_as_type_lit(expr: &Expr) -> Option<TypeLit> {
    match &expr.kind {
        ExprKind::Int(n) => Some(TypeLit::Int(*n)),
        ExprKind::Bool(b) => Some(TypeLit::Bool(*b)),
        ExprKind::String(s) => Some(TypeLit::String(s.clone())),
        _ => None,
    }
}

fn lower_match(
    ctx: &mut LowerCtx,
    scrutinee: &Expr,
    arms: &[MatchArm],
    span: Span,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
    // With an expected type every arm's value converts to it, as a block's
    // last value does: arms may be different members of an expected union,
    // or values that encode into an expected Json (D191 §4).
    let expect = expect.filter(|ty| **ty != Ty::Void);
    if arms.is_empty() {
        return Err(Error::at("match requires at least one arm", span));
    }

    let (scrut_p, scrut_ty) = lower_expr(ctx, scrutinee)?;
    // May/Trial are residual-marked user unions — use general sum match.
    if matches!(scrut_ty, Ty::String | Ty::Int64 | Ty::Bool) {
        return lower_match_value(ctx, scrut_p, scrut_ty, arms, span, expect);
    }
    if scrut_ty == Ty::Node {
        return lower_match_node(ctx, scrut_p, arms, span, expect);
    }
    if let Ty::User(ref n) = &scrut_ty {
        if ctx.types.get(n).and_then(|i| i.sum.as_ref()).is_some() {
            return lower_match_sum(ctx, scrut_p, scrut_ty, arms, span, expect);
        }
    }
    Err(Error::at(
        "match supports Trial, May, String, Int64, Bool, Node, or union types",
        span,
    ))
}

/// A match arm's value: lowered expecting the match's expected type and
/// converted to it when it differs (a member into the expected union, a value
/// into an expected Json). Without one, or when nothing converts, it is left
/// as is and the arm-type check reports a mismatch.
fn lower_arm_value(ctx: &mut LowerCtx, tail: &Expr, expect: Option<&Ty>) -> Result<(Place, Ty)> {
    let (place, ty) = lower_expr_expect(ctx, tail, expect)?;
    let Some(want) = expect else {
        return Ok((place, ty));
    };
    if ty == *want || ty == Ty::Void || is_never(&ty) {
        return Ok((place, ty));
    }
    match coerce_to_expected(ctx, place.clone(), &ty, want, tail)? {
        Some(converted) => Ok((converted, want.clone())),
        None => Ok((place, ty)),
    }
}

fn finish_match_arm_ty(
    ctx: &mut LowerCtx,
    result_place: &Place,
    result_ty: &mut Option<Ty>,
    arm_val: Option<(Place, Ty)>,
    body: &mut Vec<Stmt>,
    span: Span,
) -> Result<Ty> {
    let _ = ctx;
    match arm_val {
        // A diverging arm (`return`, `loop`) has no value and fits any type.
        Some((_, ty)) if is_never(&ty) => Ok(ty),
        Some((p, ty)) => {
            if let Some(rt) = result_ty {
                if *rt != ty {
                    return Err(Error::at(
                        format!("match arms have different types: {rt:?} vs {ty:?}"),
                        span,
                    ));
                }
            } else {
                *result_ty = Some(ty.clone());
            }
            if ty != Ty::Void {
                // Own a ref in the shared result place. Overwrite-drop the previous
                // occupant so a match inside a loop does not leak (stdlib split).
                assign_result_place(body, result_place, p, &ty);
            }
            Ok(ty)
        }
        None => {
            if let Some(rt) = result_ty {
                if *rt != Ty::Void {
                    return Err(Error::at(
                        format!("match arms have different types: {rt:?} vs Void"),
                        span,
                    ));
                }
            } else {
                *result_ty = Some(Ty::Void);
            }
            Ok(Ty::Void)
        }
    }
}

fn node_field_builtin(variant: &str, field: &str) -> Option<&'static str> {
    match (variant, field) {
        ("Element", "tag") => Some("html_tag"),
        ("Element", "attributes") => Some("html_attrs"),
        ("Element", "children") => Some("html_kids"),
        ("Text", "text") => Some("html_get_text"),
        ("Fragment", "children") => Some("html_kids"),
        _ => None,
    }
}

/// Match on builtin `Node` — same arms as `Element | Text | Fragment`.
fn lower_match_node(
    ctx: &mut LowerCtx,
    scrut_p: Place,
    arms: &[MatchArm],
    span: Span,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
    let sum = node_sum_info();
    let (tag_p, _) = emit_call_result(
        ctx,
        Callee::Builtin("html_kind".into()),
        vec![scrut_p.clone()],
        Ty::Int64,
    )?;
    let result_place = ctx.fresh("mres");
    let mut result_ty: Option<Ty> = None;
    let mut covered: HashSet<u32> = HashSet::new();
    let mut has_else = false;
    let mut catch_all_done = false;

    struct ArmCode {
        tag: Option<u32>,
        prefix: Vec<Stmt>,
        guard: Option<Place>,
        body: Vec<Stmt>,
    }
    let mut codes: Vec<ArmCode> = Vec::new();

    for arm in arms {
        if catch_all_done {
            return Err(Error::at(
                "unreachable match arm after `else` or catch-all binding",
                pattern_span_mir(&arm.pattern),
            ));
        }
        match &arm.pattern {
            Pattern::Else { .. } => {
                if arm.guard.is_some() {
                    return Err(Error::at(
                        "match guards are not allowed on catch-all arms (`else` or bare bind)",
                        span,
                    ));
                }
                has_else = true;
                catch_all_done = true;
                let outer = std::mem::take(&mut ctx.stmts);
                lower_block_items(ctx, &arm.body.items)?;
                let arm_val = match &arm.body.tail {
                    Some(t) => Some(lower_arm_value(ctx, t, expect)?),
                    None => None,
                };
                let mut body = std::mem::take(&mut ctx.stmts);
                ctx.stmts = outer;
                finish_match_arm_ty(ctx, &result_place, &mut result_ty, arm_val, &mut body, span)?;
                codes.push(ArmCode {
                    tag: None,
                    prefix: Vec::new(),
                    guard: None,
                    body,
                });
            }
            Pattern::Bind { name, span: bspan } => {
                if let Some(variant) = sum.variants.iter().find(|v| v.name == *name).cloned() {
                    let guarded = arm.guard.is_some();
                    if !guarded && !covered.insert(variant.tag) {
                        return Err(Error::at(
                            format!("duplicate match arm for `{name}`"),
                            *bspan,
                        ));
                    }
                    let (prefix, guard, body) = lower_union_arm_body(
                        ctx,
                        arm,
                        &result_place,
                        &mut result_ty,
                        span,
                        Vec::new(),
                        expect,
                    )?;
                    codes.push(ArmCode {
                        tag: Some(variant.tag),
                        prefix,
                        guard,
                        body,
                    });
                } else {
                    if arm.guard.is_some() {
                        return Err(Error::at(
                            "match guards are not allowed on catch-all arms (`else` or bare bind)",
                            *bspan,
                        ));
                    }
                    has_else = true;
                    catch_all_done = true;
                    let outer = std::mem::take(&mut ctx.stmts);
                    let bound = ctx.fresh("sbind");
                    // `assign` of a heap `Use` already Dups (same as union catch-all / D152).
                    ctx.assign(bound.clone(), Ty::Node, RValue::Use(scrut_p.clone()));
                    // The bind is visible in this arm only, not after the match.
                    ctx.push_scope();
                    ctx.define(name, Ty::Node, false, bound);
                    lower_block_items(ctx, &arm.body.items)?;
                    let arm_val = match &arm.body.tail {
                        Some(t) => Some(lower_arm_value(ctx, t, expect)?),
                        None => None,
                    };
                    ctx.pop_scope();
                    let mut body = std::mem::take(&mut ctx.stmts);
                    ctx.stmts = outer;
                    finish_match_arm_ty(
                        ctx,
                        &result_place,
                        &mut result_ty,
                        arm_val,
                        &mut body,
                        *bspan,
                    )?;
                    codes.push(ArmCode {
                        tag: None,
                        prefix: Vec::new(),
                        guard: None,
                        body,
                    });
                }
            }
            Pattern::Variant {
                name,
                binds,
                span: pspan,
                ..
            } => {
                let variant = sum
                    .variants
                    .iter()
                    .find(|v| v.name == *name)
                    .ok_or_else(|| Error::at(format!("`{name}` is not a member of Node"), *pspan))?
                    .clone();
                let guarded = arm.guard.is_some();
                if !guarded && !covered.insert(variant.tag) {
                    return Err(Error::at(
                        format!("duplicate match arm for `{name}`"),
                        *pspan,
                    ));
                }
                let member_fields = ctx
                    .types
                    .get(name)
                    .map(|i| i.fields.clone())
                    .unwrap_or_default();
                let outer = std::mem::take(&mut ctx.stmts);
                // Payload binds are visible in this arm only, not after the match.
                ctx.push_scope();
                if !binds.is_empty() {
                    if binds.len() != member_fields.len() {
                        return Err(Error::at(
                            format!(
                                "`{name}` pattern expects {} field(s), found {}",
                                member_fields.len(),
                                binds.len()
                            ),
                            *pspan,
                        ));
                    }
                    for (i, bopt) in binds.iter().enumerate() {
                        if let Some(bname) = bopt {
                            let fty = member_fields[i].ty.clone();
                            let Some(builtin) = node_field_builtin(name, &member_fields[i].name)
                            else {
                                return Err(Error::at(
                                    format!(
                                        "internal: no Node field `{}.{}`",
                                        name, member_fields[i].name
                                    ),
                                    *pspan,
                                ));
                            };
                            let (fp, _) = emit_call_result(
                                ctx,
                                Callee::Builtin(builtin.into()),
                                vec![scrut_p.clone()],
                                fty.clone(),
                            )?;
                            ctx.define(bname, fty, false, fp);
                        }
                    }
                }
                let prefix = std::mem::take(&mut ctx.stmts);
                ctx.stmts = outer;
                let lowered =
                    lower_union_arm_body(ctx, arm, &result_place, &mut result_ty, span, prefix, expect);
                ctx.pop_scope();
                let (prefix, guard, body) = lowered?;
                codes.push(ArmCode {
                    tag: Some(variant.tag),
                    prefix,
                    guard,
                    body,
                });
            }
            _ => {
                return Err(Error::at("unsupported pattern in Node match", span));
            }
        }
    }

    let all_tags: HashSet<u32> = sum.variants.iter().map(|v| v.tag).collect();
    if !has_else && covered != all_tags {
        let mut missing: Vec<&str> = sum
            .variants
            .iter()
            .filter(|v| !covered.contains(&v.tag))
            .map(|v| v.name.as_str())
            .collect();
        missing.sort();
        return Err(Error::at(
            format!(
                "non-exhaustive Node match: missing {} (or add `else`)",
                missing.join(", ")
            ),
            span,
        ));
    }

    let mut acc: Vec<Stmt> = Vec::new();
    for code in codes.into_iter().rev() {
        match code.tag {
            None => acc = code.body,
            Some(tag) => {
                let expected = ctx.fresh("ste");
                ctx.assign(expected.clone(), Ty::Int64, RValue::Int(tag as i64));
                let tag_cond = ctx.fresh("sc");
                ctx.assign(
                    tag_cond.clone(),
                    Ty::Bool,
                    RValue::Binary {
                        op: BinaryOp::Eq,
                        left: tag_p.clone(),
                        right: expected,
                    },
                );
                let then_body = if let Some(g) = code.guard {
                    let mut tb = code.prefix;
                    tb.push(Stmt::If {
                        cond: g,
                        then_body: code.body,
                        else_body: acc.clone(),
                    });
                    tb
                } else {
                    let mut tb = code.prefix;
                    tb.extend(code.body);
                    tb
                };
                acc = vec![Stmt::If {
                    cond: tag_cond,
                    then_body,
                    else_body: acc,
                }];
            }
        }
    }
    ctx.stmts.extend(acc);
    // No arm gave a type: every arm diverges, so the match does too.
    let ty = result_ty.unwrap_or(Ty::Never);
    if needs_drop(&ty) {
        ctx.note_drop_place(&result_place, &ty);
    }
    Ok((result_place, ty))
}

/// Match on a user union (D118): constructor arms + optional `else`.
/// The member a `match` arm on a `mixed` value names (D235): the arm's type,
/// `Circle`, `Int64`, or an instance written out, `Box<Int64>`, which joins
/// the union when it conforms.
fn mixed_arm(ctx: &mut LowerCtx, mixed: &str, name: &str, type_args: &[TypeExpr], span: Span) -> Result<SumVariant> {
    let written = TypeExpr {
        name: name.to_string(),
        args: type_args.iter().cloned().map(TypeArg::Type).collect(),
        span,
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    };
    let name_map = ctx.name_to_type.clone();
    if type_args.is_empty() && generic_template(ctx, name).is_some() {
        return Err(Error::at(
            format!("`{name}` is generic: name the instance, `{name}<Int64>(…)`"),
            span,
        ));
    }
    generic::ensure_instances(ctx, &written, &name_map)?;
    let ty = resolve_ty(&written, &ctx.types, &name_map)?;
    let shown = display_ty(&Ty::User(mixed.to_string()));
    mixed_variant(ctx, mixed, &ty).ok_or_else(|| {
        Error::at(
            format!("{} does not conform, so a {shown} never holds one", display_ty(&ty)),
            span,
        )
    })
}

/// The mangled template `name` is when it names a generic type.
fn generic_template(ctx: &LowerCtx, name: &str) -> Option<String> {
    let mangled = ctx.name_to_type.get(name).cloned().unwrap_or_else(|| name.to_string());
    ctx.synthetics.borrow().generics.contains_key(&mangled).then_some(mangled)
}

fn lower_match_sum(
    ctx: &mut LowerCtx,
    scrut_p: Place,
    scrut_ty: Ty,
    arms: &[MatchArm],
    span: Span,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
    let Ty::User(sum_name) = &scrut_ty else {
        return Err(Error::at("internal: sum match on non-User", span));
    };
    let sum = ctx
        .types
        .get(sum_name)
        .and_then(|i| i.sum.clone())
        .ok_or_else(|| Error::at("internal: missing sum info", span))?;

    let tag_p = ctx.fresh("stag");
    ctx.assign(
        tag_p.clone(),
        Ty::Int64,
        RValue::SumTag {
            place: scrut_p.clone(),
            sum_type: sum_name.clone(),
        },
    );

    let result_place = ctx.fresh("mres");
    let mut result_ty: Option<Ty> = None;
    let mut covered: HashSet<u32> = HashSet::new();
    let mut has_else = false;
    // True after a catch-all (`else` or whole-sum bind); further arms are unreachable.
    let mut catch_all_done = false;

    // Forward pass: tag arms may carry `if guard` (D120). Guarded arms do not
    // exhaust a tag; bind setup + guard run only after the tag matches.
    struct ArmCode {
        /// None = else / catch-all bind
        tag: Option<u32>,
        /// Payload binds (and nothing else); empty for nullary/literal.
        prefix: Vec<Stmt>,
        /// `Some` → require Bool after prefix; fail falls through to next arm.
        guard: Option<Place>,
        body: Vec<Stmt>,
    }
    let mut codes: Vec<ArmCode> = Vec::new();

    for arm in arms {
        if catch_all_done {
            return Err(Error::at(
                "unreachable match arm after `else` or catch-all binding",
                pattern_span_mir(&arm.pattern),
            ));
        }
        match &arm.pattern {
            Pattern::Else { .. } => {
                if arm.guard.is_some() {
                    return Err(Error::at(
                        "match guards are not allowed on catch-all arms (`else` or bare bind)",
                        span,
                    ));
                }
                has_else = true;
                catch_all_done = true;
                let outer = std::mem::take(&mut ctx.stmts);
                lower_block_items(ctx, &arm.body.items)?;
                let arm_val = match &arm.body.tail {
                    Some(t) => Some(lower_arm_value(ctx, t, expect)?),
                    None => None,
                };
                let mut body = std::mem::take(&mut ctx.stmts);
                ctx.stmts = outer;
                finish_match_arm_ty(ctx, &result_place, &mut result_ty, arm_val, &mut body, span)?;
                codes.push(ArmCode {
                    tag: None,
                    prefix: Vec::new(),
                    guard: None,
                    body,
                });
            }
            Pattern::Bind { name, span: bspan } => {
                // Bare name that is a union member: tag-only (nullary or discard payload, D130).
                // Otherwise catch-all bind of the whole sum value (D110).
                if let Some(variant) = sum.variants.iter().find(|v| v.name == *name).cloned() {
                    let guarded = arm.guard.is_some();
                    if !guarded && !covered.insert(variant.tag) {
                        return Err(Error::at(
                            format!("duplicate match arm for `{name}`"),
                            *bspan,
                        ));
                    }
                    // Payload stays in the scrutinee; no field binds (≡ all `_` discards).
                    let (prefix, guard, body) = lower_union_arm_body(
                        ctx,
                        arm,
                        &result_place,
                        &mut result_ty,
                        span,
                        Vec::new(),
                        expect,
                    )?;
                    codes.push(ArmCode {
                        tag: Some(variant.tag),
                        prefix,
                        guard,
                        body,
                    });
                } else {
                    if arm.guard.is_some() {
                        return Err(Error::at(
                            "match guards are not allowed on catch-all arms (`else` or bare bind)",
                            *bspan,
                        ));
                    }
                    has_else = true;
                    catch_all_done = true;
                    let outer = std::mem::take(&mut ctx.stmts);
                    let bound = ctx.fresh("sbind");
                    ctx.assign(
                        bound.clone(),
                        scrut_ty.clone(),
                        RValue::Use(scrut_p.clone()),
                    );
                    // The bind is visible in this arm only, not after the match.
                    ctx.push_scope();
                    ctx.define(name, scrut_ty.clone(), false, bound);
                    lower_block_items(ctx, &arm.body.items)?;
                    let arm_val = match &arm.body.tail {
                        Some(t) => Some(lower_arm_value(ctx, t, expect)?),
                        None => None,
                    };
                    ctx.pop_scope();
                    let mut body = std::mem::take(&mut ctx.stmts);
                    ctx.stmts = outer;
                    finish_match_arm_ty(
                        ctx,
                        &result_place,
                        &mut result_ty,
                        arm_val,
                        &mut body,
                        *bspan,
                    )?;
                    codes.push(ArmCode {
                        tag: None,
                        prefix: Vec::new(),
                        guard: None,
                        body,
                    });
                }
            }
            Pattern::Variant {
                name,
                type_args,
                binds,
                span: pspan,
                ..
            } => {
                let variant = if is_mixed(sum_name) {
                    mixed_arm(ctx, sum_name, name, type_args, *pspan)?
                } else {
                    if !type_args.is_empty() {
                        return Err(Error::at(
                            format!("`{name}` is a member by its name alone: write `{name}(…)`"),
                            *pspan,
                        ));
                    }
                    sum.variants
                        .iter()
                        .find(|v| v.name == *name)
                        .ok_or_else(|| {
                            Error::at(format!("`{name}` is not a member of this union"), *pspan)
                        })?
                        .clone()
                };
                let guarded = arm.guard.is_some();
                if !guarded && !covered.insert(variant.tag) {
                    return Err(Error::at(
                        format!("duplicate match arm for `{name}`"),
                        *pspan,
                    ));
                }
                let member_fields = match &variant.ty {
                    Ty::User(mn) => ctx
                        .types
                        .get(mn)
                        .map(|i| i.fields.clone())
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                let outer = std::mem::take(&mut ctx.stmts);
                // Payload binds are visible in this arm only, not after the match.
                ctx.push_scope();
                if binds.is_empty() {
                    // Tag-only: nullary member, or payload member with no destructure (D130).
                    // Heap payloads remain owned by the scrutinee sum (dropped with it).
                } else if !matches!(variant.ty, Ty::User(_)) {
                    // A built-in type held by a `mixed` value binds the value (D235).
                    if binds.len() != 1 {
                        return Err(Error::at(
                            format!("`{name}(…)` binds the value: one name"),
                            *pspan,
                        ));
                    }
                    if let Some(bname) = &binds[0] {
                        let held = ctx.fresh("spay");
                        ctx.assign_no_drop(
                            held.clone(),
                            variant.ty.clone(),
                            RValue::SumPayload {
                                base: scrut_p.clone(),
                                tag: variant.tag,
                                payload_ty: variant.ty.clone(),
                            },
                        );
                        if needs_drop(&variant.ty) {
                            ctx.stmts.push(Stmt::Dup {
                                place: held.clone(),
                                ty: variant.ty.clone(),
                            });
                            ctx.note_drop_place(&held, &variant.ty);
                        }
                        ctx.define(bname, variant.ty.clone(), false, held);
                    }
                } else {
                    if member_fields.is_empty() {
                        return Err(Error::at(
                            format!("`{name}` has no payload; use bare `{name}` pattern"),
                            *pspan,
                        ));
                    }
                    if binds.len() != member_fields.len() {
                        return Err(Error::at(
                            format!(
                                "`{name}` pattern expects {} field(s), found {}",
                                member_fields.len(),
                                binds.len()
                            ),
                            *pspan,
                        ));
                    }
                    // Intermediate product: no drop (aliases scrutinee; fields Dup below).
                    let payload = ctx.fresh("spay");
                    ctx.assign_no_drop(
                        payload.clone(),
                        variant.ty.clone(),
                        RValue::SumPayload {
                            base: scrut_p.clone(),
                            tag: variant.tag,
                            payload_ty: variant.ty.clone(),
                        },
                    );
                    for (i, bopt) in binds.iter().enumerate() {
                        if let Some(bname) = bopt {
                            let fty = member_fields[i].ty.clone();
                            let fp = ctx.fresh("sf");
                            if fty == Ty::Void {
                                // Void field has no C storage (Pass(Void)).
                                ctx.assign(fp.clone(), Ty::Int64, RValue::Int(0));
                                ctx.define(bname, Ty::Void, false, fp);
                            } else {
                                ctx.assign(
                                    fp.clone(),
                                    fty.clone(),
                                    RValue::Field {
                                        base: payload.clone(),
                                        field: sanitize_field(&member_fields[i].name),
                                    },
                                );
                                // Field load aliases the payload; own a separate ref when heap.
                                if needs_drop(&fty) {
                                    ctx.stmts.push(Stmt::Dup {
                                        place: fp.clone(),
                                        ty: fty.clone(),
                                    });
                                }
                                ctx.define(bname, fty, false, fp);
                            }
                        }
                    }
                }
                let prefix = std::mem::take(&mut ctx.stmts);
                let lowered =
                    lower_union_arm_body(ctx, arm, &result_place, &mut result_ty, span, prefix, expect);
                ctx.pop_scope();
                let (prefix, guard, body) = lowered?;
                ctx.stmts = outer;
                codes.push(ArmCode {
                    tag: Some(variant.tag),
                    prefix,
                    guard,
                    body,
                });
            }
            Pattern::IntLit { .. } | Pattern::BoolLit { .. } | Pattern::StringLit { .. } => {
                let (lit, pspan) = match &arm.pattern {
                    Pattern::IntLit { value, span } => (TypeLit::Int(*value), *span),
                    Pattern::BoolLit { value, span } => (TypeLit::Bool(*value), *span),
                    Pattern::StringLit { value, span } => (TypeLit::String(value.clone()), *span),
                    _ => unreachable!(),
                };
                let variant = sum
                    .variants
                    .iter()
                    .find(|v| v.lit.as_ref() == Some(&lit))
                    .ok_or_else(|| Error::at("literal is not a member of this union", pspan))?
                    .clone();
                let guarded = arm.guard.is_some();
                if !guarded && !covered.insert(variant.tag) {
                    return Err(Error::at("duplicate match arm for literal", pspan));
                }
                let (prefix, guard, body) = lower_union_arm_body(
                    ctx,
                    arm,
                    &result_place,
                    &mut result_ty,
                    span,
                    Vec::new(),
                    expect,
                )?;
                codes.push(ArmCode {
                    tag: Some(variant.tag),
                    prefix,
                    guard,
                    body,
                });
            }
            other => {
                return Err(Error::at(
                    format!("unsupported pattern in union match: {other:?}"),
                    span,
                ));
            }
        }
    }

    // More types can conform to a trait: a `mixed` value's match ends in
    // `else` (D228 §6).
    if is_mixed(sum_name) && !has_else {
        return Err(Error::at(
            format!(
                "a `match` on {} needs an `else` arm: more types can conform",
                display_ty(&scrut_ty)
            ),
            span,
        ));
    }
    let all_tags: HashSet<u32> = sum.variants.iter().map(|v| v.tag).collect();
    if !has_else && covered != all_tags {
        let mut missing: Vec<&str> = sum
            .variants
            .iter()
            .filter(|v| !covered.contains(&v.tag))
            .map(|v| v.name.as_str())
            .collect();
        missing.sort();
        let list = missing.join(", ");
        return Err(Error::at(
            format!("non-exhaustive union match: missing {list} (or add `else`)"),
            span,
        ));
    }

    // Nest ifs from the end. Guarded arms fall through to later arms on guard fail.
    let mut acc: Vec<Stmt> = Vec::new();
    for code in codes.into_iter().rev() {
        match code.tag {
            None => {
                acc = code.body;
            }
            Some(tag) => {
                let expected = ctx.fresh("ste");
                ctx.assign(expected.clone(), Ty::Int64, RValue::Int(tag as i64));
                let tag_cond = ctx.fresh("sc");
                ctx.assign(
                    tag_cond.clone(),
                    Ty::Bool,
                    RValue::Binary {
                        op: BinaryOp::Eq,
                        left: tag_p.clone(),
                        right: expected,
                    },
                );
                let then_body = if let Some(g) = code.guard {
                    let mut tb = code.prefix;
                    tb.push(Stmt::If {
                        cond: g,
                        then_body: code.body,
                        else_body: acc.clone(),
                    });
                    tb
                } else {
                    let mut tb = code.prefix;
                    tb.extend(code.body);
                    tb
                };
                acc = vec![Stmt::If {
                    cond: tag_cond,
                    then_body,
                    else_body: acc,
                }];
            }
        }
    }
    ctx.stmts.extend(acc);

    // Result place is declared zeroed with other locals; arms assign into it.
    // No arm gave a type: every arm diverges, so the match does too.
    let ty = result_ty.unwrap_or(Ty::Never);
    if needs_drop(&ty) {
        ctx.note_drop_place(&result_place, &ty);
    }
    Ok((result_place, ty))
}

/// Lower a tagged union arm body: optional `if guard` after binds, then arm body.
/// `prefix` holds payload bind setup (may be empty). Returns (prefix+guard_setup, guard, body).
fn lower_union_arm_body(
    ctx: &mut LowerCtx,
    arm: &MatchArm,
    result_place: &Place,
    result_ty: &mut Option<Ty>,
    span: Span,
    mut prefix: Vec<Stmt>,
    expect: Option<&Ty>,
) -> Result<(Vec<Stmt>, Option<Place>, Vec<Stmt>)> {
    let outer = std::mem::take(&mut ctx.stmts);
    let guard = if let Some(gexpr) = &arm.guard {
        let (gp, gty) = lower_expr(ctx, gexpr)?;
        if gty != Ty::Bool {
            return Err(Error::at(
                format!("match guard must be Bool, got {gty:?}"),
                gexpr.span,
            ));
        }
        prefix.extend(std::mem::take(&mut ctx.stmts));
        Some(gp)
    } else {
        None
    };
    lower_block_items(ctx, &arm.body.items)?;
    let arm_val = match &arm.body.tail {
        Some(t) => Some(lower_arm_value(ctx, t, expect)?),
        None => None,
    };
    let mut body = std::mem::take(&mut ctx.stmts);
    ctx.stmts = outer;
    finish_match_arm_ty(ctx, result_place, result_ty, arm_val, &mut body, span)?;
    Ok((prefix, guard, body))
}

/// Value match: literal arms + required final `else` (D87).
/// Lowers to a cascade of `if scrut == lit { arm } else { … }`.
fn lower_match_value(
    ctx: &mut LowerCtx,
    scrut_p: Place,
    scrut_ty: Ty,
    arms: &[MatchArm],
    span: Span,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
    let mut lit_arms: Vec<&MatchArm> = Vec::new();
    let mut catch_arm: Option<&MatchArm> = None; // else or bare bind (D110)

    for arm in arms {
        match &arm.pattern {
            Pattern::Else { span: pspan } | Pattern::Bind { span: pspan, .. } => {
                if catch_arm.is_some() {
                    return Err(Error::at(
                        "value match already has a catch-all arm (`else` or bare bind)",
                        *pspan,
                    ));
                }
                if arm.guard.is_some() {
                    return Err(Error::at(
                        "match guards are not allowed on catch-all arms (`else` or bare bind)",
                        *pspan,
                    ));
                }
                catch_arm = Some(arm);
            }
            Pattern::StringLit { span: pspan, .. }
            | Pattern::IntLit { span: pspan, .. }
            | Pattern::BoolLit { span: pspan, .. }
            | Pattern::Or { span: pspan, .. } => {
                if catch_arm.is_some() {
                    return Err(Error::at(
                        "match arms after catch-all are unreachable",
                        *pspan,
                    ));
                }
                check_value_pattern_ty(&arm.pattern, &scrut_ty)?;
                lit_arms.push(arm);
            }
            Pattern::Variant {
                name, span: pspan, ..
            } => {
                return Err(Error::at(
                    format!(
                        "variant pattern `{name}` is not valid for {scrut_ty:?} match (use literals + else or a bare bind)"
                    ),
                    *pspan,
                ));
            }
        }
    }

    let catch_arm = catch_arm.ok_or_else(|| {
        Error::at(
            "value match requires a final `else` arm or bare name bind",
            span,
        )
    })?;

    // Duplicate literal detection for unguarded arms only (guards may re-use lits — D111).
    {
        let mut seen_s: Vec<&str> = Vec::new();
        let mut seen_i: Vec<i64> = Vec::new();
        let mut seen_b: Vec<bool> = Vec::new();
        for arm in &lit_arms {
            if arm.guard.is_some() {
                continue;
            }
            for leaf in value_pattern_leaves(&arm.pattern) {
                match leaf {
                    Pattern::StringLit { value, span: ps } => {
                        if seen_s.iter().any(|s| *s == value.as_str()) {
                            return Err(Error::at(
                                format!("duplicate match arm for \"{value}\""),
                                *ps,
                            ));
                        }
                        seen_s.push(value.as_str());
                    }
                    Pattern::IntLit { value, span: ps } => {
                        if seen_i.contains(value) {
                            return Err(Error::at(format!("duplicate match arm for {value}"), *ps));
                        }
                        seen_i.push(*value);
                    }
                    Pattern::BoolLit { value, span: ps } => {
                        if seen_b.contains(value) {
                            return Err(Error::at(format!("duplicate match arm for {value}"), *ps));
                        }
                        seen_b.push(*value);
                    }
                    _ => {}
                }
            }
        }
    }

    // Unify arm body types; an expected type is the result type.
    let mut result_ty: Option<Ty> = expect.cloned();
    for arm in lit_arms
        .iter()
        .chain(std::iter::once(&catch_arm))
        .filter(|_| expect.is_none())
    {
        ctx.push_scope();
        if let Pattern::Bind { name, .. } = &arm.pattern {
            // Scrutinee binding for type inference of the arm body.
            let p = ctx.local_place(name);
            ctx.define(name, scrut_ty.clone(), false, p);
        }
        let ty = infer_block_ty(ctx, &arm.body)?;
        ctx.pop_scope();
        result_ty = Some(match result_ty {
            None => ty,
            Some(prev) => unify_match_arm_tys(&prev, &ty, span)?,
        });
    }
    let result_ty = result_ty.unwrap_or(Ty::Void);

    let out = if result_ty == Ty::Void || is_never(&result_ty) {
        None
    } else {
        Some(ctx.fresh("m"))
    };

    // Build cascade from the inside out: catch-all body, then each lit arm wrapping it.
    let outer = std::mem::take(&mut ctx.stmts);

    ctx.push_scope();
    if let Pattern::Bind { name, .. } = &catch_arm.pattern {
        let p = ctx.local_place(name);
        // Bind to scrutinee. `assign` of a heap `Use` already Dups so the arm can
        // drop `s` without freeing the caller / match scrutinee.
        ctx.assign(p.clone(), scrut_ty.clone(), RValue::Use(scrut_p.clone()));
        ctx.define(name, scrut_ty.clone(), false, p);
    }
    let catch_val = lower_block_into(ctx, &catch_arm.body, &result_ty)?;
    ctx.pop_scope();
    let mut nest = std::mem::take(&mut ctx.stmts);
    assign_match_result(&mut nest, &out, catch_val, &result_ty);

    for arm in lit_arms.iter().rev() {
        // Eq setup lands on ctx.stmts; prefix it before the If.
        let mut cond = lower_value_pattern_eq(ctx, &scrut_p, &scrut_ty, &arm.pattern, span)?;
        // Optional `if guard` (D111): pattern match AND guard.
        if let Some(gexpr) = &arm.guard {
            let (gp, gty) = lower_expr(ctx, gexpr)?;
            if gty != Ty::Bool {
                return Err(Error::at(
                    format!("match guard must be Bool, got {gty:?}"),
                    gexpr.span,
                ));
            }
            let and_p = ctx.fresh("mg");
            ctx.assign(
                and_p.clone(),
                Ty::Bool,
                RValue::Binary {
                    op: BinaryOp::And,
                    left: cond,
                    right: gp,
                },
            );
            cond = and_p;
        }
        let mut pre = std::mem::take(&mut ctx.stmts);

        ctx.push_scope();
        let arm_val = lower_block_into(ctx, &arm.body, &result_ty)?;
        ctx.pop_scope();
        let mut then_body = std::mem::take(&mut ctx.stmts);
        assign_match_result(&mut then_body, &out, arm_val, &result_ty);

        pre.push(Stmt::If {
            cond,
            then_body,
            else_body: nest,
        });
        nest = pre;
    }

    ctx.stmts = outer;
    if let Some(ref out_p) = out {
        init_place(ctx, out_p, &result_ty)?;
    }
    ctx.stmts.extend(nest);

    match_result_place(ctx, out, result_ty)
}

fn check_value_pattern_ty(pattern: &Pattern, scrut_ty: &Ty) -> Result<()> {
    match pattern {
        Pattern::StringLit { .. } if matches!(scrut_ty, Ty::String) => Ok(()),
        Pattern::IntLit { .. } if matches!(scrut_ty, Ty::Int64) => Ok(()),
        Pattern::BoolLit { .. } if matches!(scrut_ty, Ty::Bool) => Ok(()),
        Pattern::StringLit { span: ps, .. } => Err(Error::at(
            format!("string pattern requires String scrutinee, got {scrut_ty:?}"),
            *ps,
        )),
        Pattern::IntLit { span: ps, .. } => Err(Error::at(
            format!("integer pattern requires Int64 scrutinee, got {scrut_ty:?}"),
            *ps,
        )),
        Pattern::BoolLit { span: ps, .. } => Err(Error::at(
            format!("bool pattern requires Bool scrutinee, got {scrut_ty:?}"),
            *ps,
        )),
        Pattern::Or { alts, span } => {
            if alts.is_empty() {
                return Err(Error::at("empty or-pattern", *span));
            }
            for alt in alts {
                if matches!(
                    alt,
                    Pattern::Variant { .. }
                        | Pattern::Else { .. }
                        | Pattern::Or { .. }
                        | Pattern::Bind { .. }
                ) {
                    return Err(Error::at(
                        "or-pattern alternatives must be string, int, or bool literals",
                        pattern_span_mir(alt),
                    ));
                }
                check_value_pattern_ty(alt, scrut_ty)?;
            }
            Ok(())
        }
        Pattern::Variant { name, span, .. } => Err(Error::at(
            format!(
                "variant pattern `{name}` is not valid for {scrut_ty:?} match (use literals + else or a bare bind)"
            ),
            *span,
        )),
        Pattern::Bind { span, .. } => Err(Error::at(
            "bare bind is only valid as a catch-all value-match arm",
            *span,
        )),
        Pattern::Else { span } => Err(Error::at("`else` is not a value pattern here", *span)),
    }
}

fn value_pattern_leaves(pattern: &Pattern) -> Vec<&Pattern> {
    match pattern {
        Pattern::Or { alts, .. } => {
            let mut out = Vec::new();
            for a in alts {
                out.extend(value_pattern_leaves(a));
            }
            out
        }
        other => vec![other],
    }
}

fn pattern_span_mir(p: &Pattern) -> Span {
    match p {
        Pattern::Variant { span, .. }
        | Pattern::StringLit { span, .. }
        | Pattern::IntLit { span, .. }
        | Pattern::BoolLit { span, .. }
        | Pattern::Or { span, .. }
        | Pattern::Bind { span, .. }
        | Pattern::Else { span } => *span,
    }
}

/// Emit `scrut == pattern_lit` → Bool place (string via `string_eq`).
/// Or-patterns lower to a chain of `||` (D108).
fn lower_value_pattern_eq(
    ctx: &mut LowerCtx,
    scrut_p: &Place,
    _scrut_ty: &Ty,
    pattern: &Pattern,
    span: Span,
) -> Result<Place> {
    match pattern {
        Pattern::Or { alts, .. } => {
            let mut acc: Option<Place> = None;
            for alt in alts {
                let eq = lower_value_pattern_eq(ctx, scrut_p, _scrut_ty, alt, span)?;
                acc = Some(match acc {
                    None => eq,
                    Some(prev) => {
                        let or_p = ctx.fresh("mor");
                        ctx.assign(
                            or_p.clone(),
                            Ty::Bool,
                            RValue::Binary {
                                op: BinaryOp::Or,
                                left: prev,
                                right: eq,
                            },
                        );
                        or_p
                    }
                });
            }
            acc.ok_or_else(|| Error::at("empty or-pattern", span))
        }
        Pattern::StringLit { value, .. } => {
            let lit = ctx.fresh("mlit");
            ctx.assign(lit.clone(), Ty::String, RValue::StringLit(value.clone()));
            let eq_p = ctx.fresh("meq");
            ctx.assign(
                eq_p.clone(),
                Ty::Bool,
                RValue::Call {
                    callee: Callee::Builtin("string_eq".into()),
                    args: vec![scrut_p.clone(), lit],
                },
            );
            Ok(eq_p)
        }
        Pattern::IntLit { value, .. } => {
            let lit = ctx.fresh("mlit");
            ctx.assign(lit.clone(), Ty::Int64, RValue::Int(*value));
            let eq_p = ctx.fresh("meq");
            ctx.assign(
                eq_p.clone(),
                Ty::Bool,
                RValue::Binary {
                    op: BinaryOp::Eq,
                    left: scrut_p.clone(),
                    right: lit,
                },
            );
            Ok(eq_p)
        }
        Pattern::BoolLit { value, .. } => {
            let lit = ctx.fresh("mlit");
            ctx.assign(lit.clone(), Ty::Bool, RValue::Bool(*value));
            let eq_p = ctx.fresh("meq");
            ctx.assign(
                eq_p.clone(),
                Ty::Bool,
                RValue::Binary {
                    op: BinaryOp::Eq,
                    left: scrut_p.clone(),
                    right: lit,
                },
            );
            Ok(eq_p)
        }
        _ => Err(Error::at("internal: not a value pattern", span)),
    }
}

fn unify_match_arm_tys(a: &Ty, b: &Ty, span: Span) -> Result<Ty> {
    match (a, b) {
        (Ty::Never, x) | (x, Ty::Never) => Ok(x.clone()),
        (Ty::Void, Ty::Void) => Ok(Ty::Void),
        (x, y) if x == y => Ok(x.clone()),
        (x, y) => Err(Error::at(
            format!("match arms have different types ({x:?} vs {y:?})"),
            span,
        )),
    }
}

/// Assign a heap if/match result into a reused out place: Dup the source, drop
/// the previous occupant, then store. Raw `Assign` leaked the prior string when
/// `split` / `replaceAll` reused `_if` across loop iterations.
fn assign_result_place(body: &mut Vec<Stmt>, dest: &Place, src: Place, ty: &Ty) {
    if *ty == Ty::Void {
        return;
    }
    if &src == dest {
        return;
    }
    if needs_drop(ty) {
        body.push(Stmt::Dup {
            place: src.clone(),
            ty: ty.clone(),
        });
        body.push(Stmt::Drop {
            place: dest.clone(),
            ty: ty.clone(),
        });
    }
    body.push(Stmt::Assign {
        place: dest.clone(),
        ty: ty.clone(),
        value: RValue::Use(src),
    });
}

fn assign_match_result(
    body: &mut Vec<Stmt>,
    out: &Option<Place>,
    val: Option<Place>,
    result_ty: &Ty,
) {
    if let (Some(out_p), Some(v)) = (out, val) {
        assign_result_place(body, out_p, v, result_ty);
    }
}

fn match_result_place(
    ctx: &mut LowerCtx,
    out: Option<Place>,
    result_ty: Ty,
) -> Result<(Place, Ty)> {
    match out {
        Some(p) => Ok((p, result_ty)),
        None => {
            let p = ctx.fresh("u");
            ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
            Ok((p, Ty::Void))
        }
    }
}

fn init_place(ctx: &mut LowerCtx, place: &Place, ty: &Ty) -> Result<()> {
    match ty {
        Ty::Int32 | Ty::Int64 | Ty::UInt32 | Ty::UInt64 | Ty::Byte => {
            ctx.assign(place.clone(), ty.clone(), RValue::Int(0))
        }
        Ty::Float64 => ctx.assign(place.clone(), ty.clone(), RValue::Float(0.0)),
        Ty::Bool => ctx.assign(place.clone(), ty.clone(), RValue::Bool(false)),
        Ty::String | Ty::Bytes | Ty::Buffer | Ty::Node | Ty::Array { .. } | Ty::Map { .. } => {
            // `assign` of Null overwrite-drops: a loop that re-inits `_if` / `_m`
            // must free the previous occupant, not just NULL the C local.
            ctx.assign(place.clone(), ty.clone(), RValue::Null);
        }
        Ty::Void | Ty::Never => {}
        Ty::Fn { .. } => {
            ctx.note_drop_place(place, ty);
            ctx.assign(
                place.clone(),
                ty.clone(),
                RValue::MakeFn {
                    recv: None,
                    recv_ty: None,
                    target: FnTarget::Free {
                        mangled: String::new(),
                    },
                },
            );
        }
        Ty::FixedArray { .. } | Ty::Address(_) => {
            return Err(Error::new(format!("cannot allocate local of type {ty:?}")));
        }
        Ty::User(name) => {
            let info = ctx
                .types
                .get(name)
                .ok_or_else(|| Error::new(format!("unknown type `{name}`")))?
                .clone();
            // Pure-literal sum: scalar zero (tag 0), not a struct compound (D118 §6).
            if let Some(sum) = &info.sum {
                if !sum.variants.is_empty() && sum.variants.iter().all(|v| v.lit.is_some()) {
                    ctx.assign(place.clone(), ty.clone(), RValue::Int(0));
                    return Ok(());
                }
            }
            // A `drop` type's placeholder is a NULL box: building a real one
            // would run its `drop()` (closing fd 0) when the placeholder goes.
            if info.resource {
                ctx.assign(
                    place.clone(),
                    ty.clone(),
                    RValue::StructZero {
                        type_name: name.clone(),
                    },
                );
                return Ok(());
            }
            if info.zero_init {
                ctx.assign(
                    place.clone(),
                    ty.clone(),
                    RValue::StructZero {
                        type_name: name.clone(),
                    },
                );
            } else if info.sum.is_some() {
                // Tagged sum: zero all bits (tag 0, NULL heap payloads).
                ctx.assign(
                    place.clone(),
                    ty.clone(),
                    RValue::StructZero {
                        type_name: name.clone(),
                    },
                );
            } else {
                // Zero-ish placeholder so branch-skipped drops are safe (NULL heap fields).
                let mut fields = Vec::new();
                for f in &info.fields {
                    let fp = ctx.fresh("zf");
                    match &f.ty {
                        Ty::Int64 => ctx.assign(fp.clone(), Ty::Int64, RValue::Int(0)),
                        Ty::Float64 => ctx.assign(fp.clone(), Ty::Float64, RValue::Float(0.0)),
                        Ty::Bool => ctx.assign(fp.clone(), Ty::Bool, RValue::Bool(false)),
                        Ty::String
                        | Ty::Bytes
                        | Ty::Buffer
                        | Ty::Node
                        | Ty::Array { .. }
                        | Ty::Map { .. } => {
                            ctx.assign(fp.clone(), f.ty.clone(), RValue::Null);
                        }
                        Ty::User(_) => {
                            init_place(ctx, &fp, &f.ty)?;
                        }
                        other => {
                            return Err(Error::new(format!(
                                "cannot zero-init field type {other:?}"
                            )));
                        }
                    }
                    fields.push(fp);
                }
                ctx.assign(
                    place.clone(),
                    ty.clone(),
                    RValue::StructLit {
                        type_name: name.clone(),
                        fields,
                    },
                );
            }
        }
    }
    Ok(())
}

fn lower_binary(
    ctx: &mut LowerCtx,
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
    span: Span,
) -> Result<(Place, Ty)> {
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        return lower_logical(ctx, op, left, right, span);
    }
    let (l, lt) = lower_expr(ctx, left)?;
    let (r, rt) = lower_expr(ctx, right)?;
    let (l, lt) = unwrap_refined(ctx, l, &lt)?;
    let (r, rt) = unwrap_refined(ctx, r, &rt)?;
    // `<` and the rest are `Ordered`'s (D228). A union of literals orders by
    // declaration, so this comes before it widens to its literals' type.
    if matches!(op, BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge)
        && matches!(&lt, Ty::User(_) | Ty::Array { .. })
        && lt == rt
    {
        let sign = order::lower_compare(ctx, l, r, &lt, span)?;
        return Ok(compare_sign(ctx, op, sign));
    }
    let (l, lt) = match maybe_widen_lit_sum(ctx, l.clone(), &lt)? {
        Some(w) => w,
        None => (l, lt),
    };
    let (r, rt) = match maybe_widen_lit_sum(ctx, r.clone(), &rt)? {
        Some(w) => w,
        None => (r, rt),
    };
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            // A type parameter has only its bounds' members (D228 §5), and no
            // trait gives arithmetic.
            if let Some(param) = [&lt, &rt].into_iter().find(|ty| is_param_archetype(ty)) {
                let shown = display_ty(param);
                let symbol = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    _ => "%",
                };
                return Err(Error::at(
                    format!("`{shown}` has no `{symbol}` (generic code sees only its type parameters' bounds)"),
                    span,
                )
                .with_help(format!(
                    "bound `{shown}` by a trait with a method for it (`fn plus(other!: Self) -> Self`) and call that"
                )));
            }
            if is_float(&lt) || is_float(&rt) {
                if matches!(op, BinaryOp::Rem) {
                    return Err(Error::at("`%` is not defined for Float64", span));
                }
                if !(is_float(&lt) || is_int(&lt)) || !(is_float(&rt) || is_int(&rt)) {
                    return Err(Error::at(
                        format!("operator {op:?} expects numeric operands"),
                        span,
                    ));
                }
                let left = if lt == Ty::Float64 {
                    l
                } else {
                    coerce_place(ctx, l, &lt, &Ty::Float64, span)?
                };
                let right = if rt == Ty::Float64 {
                    r
                } else {
                    coerce_place(ctx, r, &rt, &Ty::Float64, span)?
                };
                let p = ctx.fresh("fbin");
                ctx.assign(p.clone(), Ty::Float64, RValue::Binary { op, left, right });
                return Ok((p, Ty::Float64));
            }
            if !is_int(&lt) || !is_int(&rt) {
                return Err(Error::at(
                    format!("operator {op:?} expects integer operands"),
                    span,
                ));
            }
            if lt == Ty::UInt64 || rt == Ty::UInt64 {
                let left = unsigned_operand(ctx, l, &lt, left, span)?;
                let right = unsigned_operand(ctx, r, &rt, right, span)?;
                let p = ctx.fresh("ubin");
                let value = match op {
                    BinaryOp::Div | BinaryOp::Rem => RValue::Call {
                        callee: Callee::Builtin(
                            if matches!(op, BinaryOp::Div) {
                                "uint_div"
                            } else {
                                "uint_rem"
                            }
                            .into(),
                        ),
                        args: vec![left, right],
                    },
                    _ => RValue::Binary { op, left, right },
                };
                ctx.assign(p.clone(), Ty::UInt64, value);
                return Ok((p, Ty::UInt64));
            }
            let left = if lt == Ty::Int64 {
                l
            } else {
                coerce_place(ctx, l, &lt, &Ty::Int64, span)?
            };
            let right = if rt == Ty::Int64 {
                r
            } else {
                coerce_place(ctx, r, &rt, &Ty::Int64, span)?
            };
            let p = ctx.fresh("bin");
            let value = match op {
                // Checked in the runtime: division by zero panics instead of trapping.
                BinaryOp::Div | BinaryOp::Rem => RValue::Call {
                    callee: Callee::Builtin(
                        if matches!(op, BinaryOp::Div) {
                            "int_div"
                        } else {
                            "int_rem"
                        }
                        .into(),
                    ),
                    args: vec![left, right],
                },
                _ => RValue::Binary { op, left, right },
            };
            ctx.assign(p.clone(), Ty::Int64, value);
            Ok((p, Ty::Int64))
        }
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr => {
            if !is_int(&lt) || !is_int(&rt) {
                return Err(Error::at(
                    format!("bitwise operator {op:?} expects integer operands"),
                    span,
                ));
            }
            // A UInt64 keeps its 64 bits: `&` `|` `^` with another unsigned
            // operand, and a shift of a UInt64 by any count (D219).
            let shift = matches!(op, BinaryOp::Shl | BinaryOp::Shr);
            if lt == Ty::UInt64 || (!shift && rt == Ty::UInt64) {
                let left = unsigned_operand(ctx, l, &lt, left, span)?;
                let right = if shift {
                    if rt == Ty::Int64 {
                        r
                    } else {
                        coerce_place(ctx, r, &rt, &Ty::Int64, span)?
                    }
                } else {
                    unsigned_operand(ctx, r, &rt, right, span)?
                };
                let p = ctx.fresh("ubit");
                let value = match op {
                    BinaryOp::Shl | BinaryOp::Shr => RValue::Call {
                        callee: Callee::Builtin(
                            if matches!(op, BinaryOp::Shl) {
                                "uint_shl"
                            } else {
                                "uint_shr"
                            }
                            .into(),
                        ),
                        args: vec![left, right],
                    },
                    _ => RValue::Binary { op, left, right },
                };
                ctx.assign(p.clone(), Ty::UInt64, value);
                return Ok((p, Ty::UInt64));
            }
            // Bitwise in Int64; field stores truncate to declared width.
            let left = if lt == Ty::Int64 {
                l
            } else {
                coerce_place(ctx, l, &lt, &Ty::Int64, span)?
            };
            let right = if rt == Ty::Int64 {
                r
            } else {
                coerce_place(ctx, r, &rt, &Ty::Int64, span)?
            };
            let p = ctx.fresh("bit");
            let value = match op {
                // Checked in the runtime: counts ≥ 64 or < 0 are defined (D161).
                BinaryOp::Shl | BinaryOp::Shr => RValue::Call {
                    callee: Callee::Builtin(
                        if matches!(op, BinaryOp::Shl) {
                            "int_shl"
                        } else {
                            "int_shr"
                        }
                        .into(),
                    ),
                    args: vec![left, right],
                },
                _ => RValue::Binary { op, left, right },
            };
            ctx.assign(p.clone(), Ty::Int64, value);
            Ok((p, Ty::Int64))
        }
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            if matches!(lt, Ty::Fn { .. }) || matches!(rt, Ty::Fn { .. }) {
                return Err(Error::at("function values do not support comparison", span));
            }
            if lt == Ty::Bool && rt == Ty::Bool {
                let p = ctx.fresh("cmp");
                ctx.assign(
                    p.clone(),
                    Ty::Bool,
                    RValue::Binary {
                        op,
                        left: l,
                        right: r,
                    },
                );
                return Ok((p, Ty::Bool));
            }
            if lt == Ty::String && rt == Ty::String {
                // String is `Ordered` by scalars, which is byte order (D228).
                if !matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
                    let sign = order::lower_compare(ctx, l, r, &lt, span)?;
                    return Ok(compare_sign(ctx, op, sign));
                }
                let eq_p = ctx.fresh("seq");
                ctx.assign(
                    eq_p.clone(),
                    Ty::Bool,
                    RValue::Call {
                        callee: Callee::Builtin("string_eq".into()),
                        args: vec![l, r],
                    },
                );
                if matches!(op, BinaryOp::Ne) {
                    let p = ctx.fresh("sne");
                    ctx.assign(
                        p.clone(),
                        Ty::Bool,
                        RValue::Unary {
                            op: UnaryOp::Not,
                            arg: eq_p,
                        },
                    );
                    return Ok((p, Ty::Bool));
                }
                return Ok((eq_p, Ty::Bool));
            }
            if is_float(&lt) || is_float(&rt) {
                if !(is_float(&lt) || is_int(&lt)) || !(is_float(&rt) || is_int(&rt)) {
                    return Err(Error::at(format!("cannot compare {lt:?} and {rt:?}"), span));
                }
                let left = if lt == Ty::Float64 {
                    l
                } else {
                    coerce_place(ctx, l, &lt, &Ty::Float64, span)?
                };
                let right = if rt == Ty::Float64 {
                    r
                } else {
                    coerce_place(ctx, r, &rt, &Ty::Float64, span)?
                };
                let p = ctx.fresh("fcmp");
                ctx.assign(p.clone(), Ty::Bool, RValue::Binary { op, left, right });
                return Ok((p, Ty::Bool));
            }
            // A UInt64 compares unsigned (D219).
            if is_int(&lt) && is_int(&rt) && (lt == Ty::UInt64 || rt == Ty::UInt64) {
                let left = unsigned_operand(ctx, l, &lt, left, span)?;
                let right = unsigned_operand(ctx, r, &rt, right, span)?;
                let p = ctx.fresh("ucmp");
                ctx.assign(p.clone(), Ty::Bool, RValue::Binary { op, left, right });
                return Ok((p, Ty::Bool));
            }
            if is_int(&lt) && is_int(&rt) {
                let left = if lt == Ty::Int64 {
                    l
                } else {
                    coerce_place(ctx, l, &lt, &Ty::Int64, span)?
                };
                let right = if rt == Ty::Int64 {
                    r
                } else {
                    coerce_place(ctx, r, &rt, &Ty::Int64, span)?
                };
                let p = ctx.fresh("cmp");
                ctx.assign(p.clone(), Ty::Bool, RValue::Binary { op, left, right });
                return Ok((p, Ty::Bool));
            }
            // A union and one of its members: the member joins the union (D177).
            let (mut l, mut lt, mut r, mut rt) = (l, lt, r, rt);
            if lt != rt && matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
                if sum_info_of(&ctx.types, &lt).is_some() {
                    if let Some(joined) = try_sum_coerce(ctx, r.clone(), &rt, &lt, None, span)? {
                        r = joined;
                        rt = lt.clone();
                    }
                } else if sum_info_of(&ctx.types, &rt).is_some() {
                    if let Some(joined) = try_sum_coerce(ctx, l.clone(), &lt, &rt, None, span)? {
                        l = joined;
                        lt = rt.clone();
                    }
                }
            }
            let shown = display_ty(&lt);
            if lt != rt {
                return Err(Error::at(
                    format!("cannot compare {shown} and {}", display_ty(&rt)),
                    span,
                ));
            }
            let comparable =
                matches!(&lt, Ty::Bytes | Ty::User(_) | Ty::Array { .. } | Ty::Map { .. });
            if comparable && !matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
                return Err(Error::at(
                    format!("{shown} only supports `==` and `!=`"),
                    span,
                ));
            }
            let equal = match &lt {
                Ty::Bytes => lower_bytes_eq(ctx, l, r),
                Ty::User(type_name) => lower_user_eq(ctx, l, r, type_name, span)?,
                Ty::Array { elem } => lower_array_eq(ctx, l, r, elem, span)?.ok_or_else(|| {
                    Error::at(
                        format!(
                            "cannot compare {shown} values: {} has no `==`",
                            display_ty(elem)
                        ),
                        span,
                    )
                })?,
                Ty::Map { key, val } => {
                    lower_map_eq(ctx, l, r, key, val, span)?.ok_or_else(|| {
                        Error::at(
                            format!(
                                "cannot compare {shown} values: {} has no `==`",
                                display_ty(val)
                            ),
                            span,
                        )
                    })?
                }
                _ => {
                    return Err(Error::at(format!("cannot compare {shown} values"), span)
                        .with_help(
                            "`==` is defined for numbers, Bool, String, Bytes, and products, \
                             unions, arrays, and maps of those (D175, D177, D178, D212)",
                        ));
                }
            };
            if matches!(op, BinaryOp::Ne) {
                let p = ctx.fresh("ne");
                ctx.assign(
                    p.clone(),
                    Ty::Bool,
                    RValue::Unary {
                        op: UnaryOp::Not,
                        arg: equal,
                    },
                );
                return Ok((p, Ty::Bool));
            }
            Ok((equal, Ty::Bool))
        }
        BinaryOp::And | BinaryOp::Or => {
            if lt != Ty::Bool || rt != Ty::Bool {
                return Err(Error::at(
                    format!("operator {op:?} expects Bool operands"),
                    span,
                ));
            }
            let p = ctx.fresh("bin");
            ctx.assign(
                p.clone(),
                Ty::Bool,
                RValue::Binary {
                    op,
                    left: l,
                    right: r,
                },
            );
            Ok((p, Ty::Bool))
        }
    }
}

/// `sign op 0` for a -1 / 0 / 1 comparison: `a < b` is `compare(a, b) < 0`.
fn compare_sign(ctx: &mut LowerCtx, op: BinaryOp, sign: Place) -> (Place, Ty) {
    let zero = ctx.fresh("zero");
    ctx.assign(zero.clone(), Ty::Int64, RValue::Int(0));
    let p = ctx.fresh("ord");
    ctx.assign(
        p.clone(),
        Ty::Bool,
        RValue::Binary {
            op,
            left: sign,
            right: zero,
        },
    );
    (p, Ty::Bool)
}

/// D219: an operand of a `UInt64` operation, as a `UInt64`: a `UInt64`, a
/// `UInt32` or `Byte` (widened), or a non-negative integer literal. A signed
/// value has no type in common with a `UInt64`, so mixing them is an error.
fn unsigned_operand(
    ctx: &mut LowerCtx,
    place: Place,
    ty: &Ty,
    expr: &Expr,
    span: Span,
) -> Result<Place> {
    match ty {
        Ty::UInt64 => Ok(place),
        Ty::UInt32 | Ty::Byte => coerce_place(ctx, place, ty, &Ty::UInt64, span),
        _ if int_constant(expr).is_some_and(|value| value >= 0) => {
            coerce_place(ctx, place, ty, &Ty::UInt64, span)
        }
        _ => Err(Error::at(
            format!("cannot mix UInt64 and {} in one operation", display_ty(ty)),
            span,
        )
        .with_help(
            "convert one side: `UInt64(truncating: x)` keeps the bits, `x as? UInt64` checks the value",
        )),
    }
}

/// `a and b` / `a or b` short-circuit (D183): lowered as
/// `if a { b } else { false }` and `if a { true } else { b }`, so `b` runs
/// only when `a` does not decide the result.
fn lower_logical(
    ctx: &mut LowerCtx,
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
    span: Span,
) -> Result<(Place, Ty)> {
    let operands = || Error::at(format!("operator {op:?} expects Bool operands"), span);
    let right_ty = infer_block_ty(ctx, &tail_block(right.clone()))?;
    if !can_coerce_ty(&ctx.types, &right_ty, &Ty::Bool) {
        return Err(operands());
    }
    let decided = Expr {
        kind: ExprKind::Bool(matches!(op, BinaryOp::Or)),
        span: right.span,
    };
    let (then_branch, else_branch) = if matches!(op, BinaryOp::And) {
        (tail_block(right.clone()), tail_block(decided))
    } else {
        (tail_block(decided), tail_block(right.clone()))
    };
    lower_if(ctx, left, &then_branch, Some(&else_branch), span, Some(&Ty::Bool)).map_err(|error| {
        if error.message == "`if` condition must be Bool" {
            operands()
        } else {
            error
        }
    })
}

/// A block whose value is `expr`.
fn tail_block(expr: Expr) -> Block {
    let span = expr.span;
    Block {
        items: Vec::new(),
        tail: Some(Box::new(expr)),
        span,
    }
}

/// `Bytes == Bytes`: same length and bytes (`bytes_equals`).
fn lower_bytes_eq(ctx: &mut LowerCtx, left: Place, right: Place) -> Place {
    let raw = ctx.fresh("beq");
    ctx.assign(
        raw.clone(),
        Ty::Int64,
        RValue::Call {
            callee: Callee::Builtin("bytes_equals".into()),
            args: vec![left, right],
        },
    );
    let p = ctx.fresh("beqb");
    ctx.assign(
        p.clone(),
        Ty::Bool,
        RValue::Cast {
            value: raw,
            from: Ty::Int64,
            to: Ty::Bool,
        },
    );
    p
}

/// D175: two values of one constructor-param product are equal when every
/// field is. Fields are read as borrows (no RC); a nested product recurses,
/// which ends because a type cannot contain itself. A union compares its
/// arms (D177).
fn lower_user_eq(
    ctx: &mut LowerCtx,
    left: Place,
    right: Place,
    type_name: &str,
    span: Span,
) -> Result<Place> {
    // `==` is `Equatable`'s (D228): declared, and with the type's own
    // `equals` when it writes one.
    let ty = Ty::User(type_name.to_string());
    let info = ctx.types.get(type_name).cloned();
    let shown = display_ty(&ty);
    if info.as_ref().is_some_and(|info| info.resource) {
        return Err(Error::at(
            format!("cannot compare {shown} values: a `Resource` is not a value"),
            span,
        ));
    }
    if !ctx.conforms(&ty, "Equatable") {
        return Err(Error::at(
            format!("cannot compare {shown} values: `{shown}` is not `Equatable`"),
            span,
        )
        .with_help(conform_help(&ty, "Equatable", "Equatable", "field by field, or with its own `equals`")));
    }
    if info.is_some_and(|info| info.methods.contains_key("equals") && !info.derived.contains("equals")) {
        let other = ctx.fresh("eqarg");
        ctx.assign(other.clone(), ty, RValue::Use(right));
        let (equal, _) = emit_call_result(
            ctx,
            Callee::Method {
                type_name: type_name.to_string(),
                method: "equals".into(),
            },
            vec![left, other],
            Ty::Bool,
        )?;
        return Ok(equal);
    }
    // A `mixed` value's members are known once every body is (D235).
    if is_mixed(type_name) || ctx.eq_expanding.iter().any(|expanding| expanding == type_name) {
        return Ok(call_eq_function(ctx, left, right, type_name));
    }
    ctx.eq_expanding.push(type_name.to_string());
    let result = lower_user_eq_inline(ctx, left, right, type_name, span);
    ctx.eq_expanding.pop();
    result
}

/// `__eq_{type}(&left, &right)`: both sides are borrowed, passed as `mut`
/// parameters (by address, D199), so the call moves nothing.
fn call_eq_function(ctx: &mut LowerCtx, left: Place, right: Place, type_name: &str) -> Place {
    {
        let mut synthetics = ctx.synthetics.borrow_mut();
        if !synthetics.eq_functions.iter().any(|name| name == type_name) {
            synthetics.eq_functions.push(type_name.to_string());
        }
    }
    let p = ctx.fresh("eqcall");
    ctx.assign(
        p.clone(),
        Ty::Bool,
        RValue::Call {
            callee: Callee::User(eq_function_name(type_name)),
            args: vec![
                Place {
                    name: format!("&{}", left.name),
                },
                Place {
                    name: format!("&{}", right.name),
                },
            ],
        },
    );
    p
}

fn eq_function_name(type_name: &str) -> String {
    format!("__eq_{type_name}")
}

/// Lower each requested `__eq_{type}` (D212); lowering one may request more.
fn lower_eq_functions(
    fns: &HashMap<String, FnSig>,
    types: &HashMap<String, TypeInfo>,
    package_maps: &HashMap<String, ModuleNameMaps>,
    module_exports: &HashMap<String, ModuleExports>,
    entry_mod_id: &str,
    synthetics: &Rc<RefCell<SyntheticRegistry>>,
    done: &mut HashSet<String>,
) -> Result<Vec<Function>> {
    let mut functions = Vec::new();
    loop {
        let pending: Vec<String> = synthetics
            .borrow()
            .eq_functions
            .iter()
            .filter(|name| !done.contains(*name))
            .cloned()
            .collect();
        if pending.is_empty() {
            return Ok(functions);
        }
        for type_name in pending {
            done.insert(type_name.clone());
            let module_id = types
                .get(&type_name)
                .map(|info| info.def_mod.clone())
                .filter(|module| package_maps.contains_key(module))
                .unwrap_or_else(|| entry_mod_id.to_string());
            let maps = package_maps.get(&module_id).unwrap();
            let mut ctx = LowerCtx::new(
                fns.clone(),
                types.clone(),
                maps.name_to_type.clone(),
                maps.name_to_fn.clone(),
                maps.name_to_global.clone(),
                maps.namespaces.clone(),
                module_exports.clone(),
                package_maps.clone(),
                Ty::Bool,
                module_id,
                false,
                Rc::clone(synthetics),
            );
            let ty = Ty::User(type_name.clone());
            let sides = ["left", "right"].map(|name| Place {
                name: format!("(*{name})"),
            });
            ctx.eq_expanding.push(type_name.clone());
            let result = lower_user_eq_inline(
                &mut ctx,
                sides[0].clone(),
                sides[1].clone(),
                &type_name,
                Span::new(0, 0),
            )?;
            let mut body = ctx.stmts;
            let ret = Some(result);
            insert_function_drops(&mut body, &ret, &Ty::Bool, &ctx.drop_places);
            functions.push(Function {
                name: eq_function_name(&type_name),
                is_main: false,
                receiver: None,
                params: vec![("left".into(), ty.clone()), ("right".into(), ty)],
                mut_params: vec![true, true],
                body,
                ret,
                ret_ty: Ty::Bool,
            });
        }
    }
}

fn lower_user_eq_inline(
    ctx: &mut LowerCtx,
    left: Place,
    right: Place,
    type_name: &str,
    span: Span,
) -> Result<Place> {
    let info = ctx
        .types
        .get(type_name)
        .cloned()
        .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), span))?;
    let shown = display_ty(&Ty::User(type_name.to_string()));
    if let Some(sum) = &info.sum {
        return lower_union_eq(ctx, left, right, type_name, sum, span);
    }
    let refusal = if info.resource {
        Some("a `Resource` is not a value")
    } else if info.zero_init {
        Some("C-layout types have no `==`")
    } else {
        None
    };
    if let Some(reason) = refusal {
        return Err(Error::at(
            format!("cannot compare {shown} values: {reason}"),
            span,
        ));
    }
    let mut all: Option<Place> = None;
    // A Void field (`Pass(Void)`) has no storage, and is always equal.
    for field in info.fields.iter().filter(|field| field.ty != Ty::Void) {
        let left_field = borrow_field(ctx, &left, &field.name, &field.ty);
        let right_field = borrow_field(ctx, &right, &field.name, &field.ty);
        let same = lower_field_eq(ctx, left_field, right_field, &field.ty, span)?;
        let same = same.ok_or_else(|| {
            Error::at(
                format!(
                    "cannot compare {shown} values: field `{}` is {}, which has no `==`",
                    field.name,
                    display_ty(&field.ty)
                ),
                span,
            )
        })?;
        all = Some(match all {
            None => same,
            Some(before) => {
                let p = ctx.fresh("eqand");
                ctx.assign(
                    p.clone(),
                    Ty::Bool,
                    RValue::Binary {
                        op: BinaryOp::And,
                        left: before,
                        right: same,
                    },
                );
                p
            }
        });
    }
    Ok(match all {
        Some(p) => p,
        None => {
            let p = ctx.fresh("eqnil");
            ctx.assign(p.clone(), Ty::Bool, RValue::Bool(true));
            p
        }
    })
}

/// D177: two union values are equal when they are the same arm and that
/// arm's payloads are equal. A literal arm, or one whose payload has no
/// fields (`Not`), is equal when the tags are.
fn lower_union_eq(
    ctx: &mut LowerCtx,
    left: Place,
    right: Place,
    type_name: &str,
    sum: &SumInfo,
    span: Span,
) -> Result<Place> {
    let mut tags = Vec::new();
    for place in [&left, &right] {
        let tag = ctx.fresh("ueqtag");
        ctx.assign(
            tag.clone(),
            Ty::Int64,
            RValue::SumTag {
                place: place.clone(),
                sum_type: type_name.to_string(),
            },
        );
        tags.push(tag);
    }
    let (left_tag, right_tag) = (tags[0].clone(), tags[1].clone());
    let result = ctx.fresh("ueq");
    ctx.assign(result.clone(), Ty::Bool, RValue::Bool(false));
    let same_arm = ctx.fresh("ueqsame");
    ctx.assign(
        same_arm.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Eq,
            left: left_tag.clone(),
            right: right_tag,
        },
    );

    // Inside `same_arm`: find the arm, and compare its payloads.
    let outside = std::mem::take(&mut ctx.stmts);
    let mut chain: Vec<Stmt> = Vec::new();
    for variant in sum.variants.iter().rev() {
        let want = ctx.fresh("ueqarm");
        ctx.assign(want.clone(), Ty::Int64, RValue::Int(variant.tag as i64));
        let cond = ctx.fresh("ueqis");
        ctx.assign(
            cond.clone(),
            Ty::Bool,
            RValue::Binary {
                op: BinaryOp::Eq,
                left: left_tag.clone(),
                right: want,
            },
        );
        let before_arm = std::mem::take(&mut ctx.stmts);
        let equal = if variant.lit.is_some() || !has_storage(&ctx.types, &variant.ty) {
            let p = ctx.fresh("ueqlit");
            ctx.assign(p.clone(), Ty::Bool, RValue::Bool(true));
            p
        } else {
            let mut payloads = Vec::new();
            for place in [&left, &right] {
                let payload = ctx.fresh("ueqpay");
                ctx.assign_no_drop(
                    payload.clone(),
                    variant.ty.clone(),
                    RValue::SumPayload {
                        base: place.clone(),
                        tag: variant.tag,
                        payload_ty: variant.ty.clone(),
                    },
                );
                payloads.push(payload);
            }
            let (left_payload, right_payload) = (payloads[0].clone(), payloads[1].clone());
            let equal = lower_field_eq(ctx, left_payload, right_payload, &variant.ty, span)?;
            equal.ok_or_else(|| {
                Error::at(
                    format!(
                        "cannot compare {} values: arm `{}` is {}, which has no `==`",
                        display_ty(&Ty::User(type_name.to_string())),
                        variant.name,
                        display_ty(&variant.ty)
                    ),
                    span,
                )
            })?
        };
        ctx.assign(result.clone(), Ty::Bool, RValue::Use(equal));
        let then_body = std::mem::replace(&mut ctx.stmts, before_arm);
        ctx.stmts.push(Stmt::If {
            cond,
            then_body,
            else_body: chain,
        });
        chain = std::mem::take(&mut ctx.stmts);
    }
    let then_body = std::mem::replace(&mut ctx.stmts, outside);
    let mut then_body = then_body;
    then_body.extend(chain);
    ctx.stmts.push(Stmt::If {
        cond: same_arm,
        then_body,
        else_body: Vec::new(),
    });
    Ok(result)
}

/// D178: two arrays are equal when they have the same count and their
/// elements are equal in order; the loop stops at the first difference.
/// `None` when the element type has no `==`.
fn lower_array_eq(
    ctx: &mut LowerCtx,
    left: Place,
    right: Place,
    elem_ty: &Ty,
    span: Span,
) -> Result<Option<Place>> {
    let mut counts = Vec::new();
    for place in [&left, &right] {
        let count = ctx.fresh("aeqcnt");
        ctx.assign(
            count.clone(),
            Ty::Int64,
            RValue::Call {
                callee: Callee::Builtin("array_count".into()),
                args: vec![place.clone()],
            },
        );
        counts.push(count);
    }
    let result = ctx.fresh("aeq");
    ctx.assign(
        result.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Eq,
            left: counts[0].clone(),
            right: counts[1].clone(),
        },
    );
    let index = ctx.fresh("aeqi");
    ctx.assign(index.clone(), Ty::Int64, RValue::Int(0));
    let mut elems = Vec::new();
    for _ in 0..2 {
        let elem = ctx.fresh("aeqel");
        init_place(ctx, &elem, elem_ty)?;
        elems.push(elem);
    }

    let outside = std::mem::take(&mut ctx.stmts);
    read_array_elem(ctx, &left, &index, &elems[0], elem_ty);
    read_array_elem(ctx, &right, &index, &elems[1], elem_ty);
    let Some(equal) = lower_field_eq(ctx, elems[0].clone(), elems[1].clone(), elem_ty, span)?
    else {
        ctx.stmts = outside;
        return Ok(None);
    };
    let differs = ctx.fresh("aeqne");
    ctx.assign(
        differs.clone(),
        Ty::Bool,
        RValue::Unary {
            op: UnaryOp::Not,
            arg: equal,
        },
    );
    let before_stop = std::mem::take(&mut ctx.stmts);
    ctx.assign(result.clone(), Ty::Bool, RValue::Bool(false));
    ctx.stmts.push(Stmt::Break);
    let stop = std::mem::replace(&mut ctx.stmts, before_stop);
    ctx.stmts.push(Stmt::If {
        cond: differs,
        then_body: stop,
        else_body: Vec::new(),
    });
    let body = std::mem::replace(&mut ctx.stmts, outside);
    // Only walk the elements when the counts agree.
    ctx.stmts.push(Stmt::If {
        cond: result.clone(),
        then_body: vec![Stmt::ForRange {
            index,
            end: counts[0].clone(),
            inclusive: false,
            body,
        }],
        else_body: Vec::new(),
    });
    Ok(Some(result))
}

/// D212: two maps are equal when they hold the same keys with equal values;
/// insertion order does not matter. Each key of `left` is looked up in both
/// maps; the values are borrowed (`MapGet` copies no reference), and the loop
/// stops at the first key `right` lacks or the first unequal value. `None`
/// when the value type has no `==`.
fn lower_map_eq(
    ctx: &mut LowerCtx,
    left: Place,
    right: Place,
    key_ty: &Ty,
    val_ty: &Ty,
    span: Span,
) -> Result<Option<Place>> {
    let mut counts = Vec::new();
    for place in [&left, &right] {
        let count = ctx.fresh("meqcnt");
        ctx.assign(
            count.clone(),
            Ty::Int64,
            RValue::Call {
                callee: Callee::Builtin("map_count".into()),
                args: vec![place.clone()],
            },
        );
        counts.push(count);
    }
    let result = ctx.fresh("meq");
    ctx.assign(
        result.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Eq,
            left: counts[0].clone(),
            right: counts[1].clone(),
        },
    );
    let index = ctx.fresh("meqi");
    ctx.assign(index.clone(), Ty::Int64, RValue::Int(0));
    let key = ctx.fresh("meqk");
    declare_borrow(ctx, &key, key_ty)?;
    let mut values = Vec::new();
    for _ in 0..2 {
        let value = ctx.fresh("meqv");
        declare_borrow(ctx, &value, val_ty)?;
        values.push(value);
    }

    let outside = std::mem::take(&mut ctx.stmts);
    ctx.assign_no_drop(
        key.clone(),
        key_ty.clone(),
        RValue::MapKeyAt {
            map: left.clone(),
            index: index.clone(),
        },
    );
    let lookup_key = coerce_map_key(ctx, key, key_ty);
    let mut found = Vec::new();
    for (map, value) in [(&left, &values[0]), (&right, &values[1])] {
        let hit = ctx.fresh("meqhit");
        ctx.assign_no_drop(
            hit.clone(),
            Ty::Int64,
            RValue::MapGet {
                map: map.clone(),
                key: lookup_key.clone(),
                out: value.clone(),
                val_ty: val_ty.clone(),
            },
        );
        found.push(hit);
    }
    let zero = ctx.fresh("meqz");
    ctx.assign(zero.clone(), Ty::Int64, RValue::Int(0));
    let missing = ctx.fresh("meqmiss");
    ctx.assign(
        missing.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Eq,
            left: found[1].clone(),
            right: zero,
        },
    );
    let stop = vec![
        Stmt::Assign {
            place: result.clone(),
            ty: Ty::Bool,
            value: RValue::Bool(false),
        },
        Stmt::Break,
    ];
    ctx.stmts.push(Stmt::If {
        cond: missing,
        then_body: stop.clone(),
        else_body: Vec::new(),
    });
    let Some(equal) = lower_field_eq(ctx, values[0].clone(), values[1].clone(), val_ty, span)?
    else {
        ctx.stmts = outside;
        return Ok(None);
    };
    let differs = ctx.fresh("meqne");
    ctx.assign(
        differs.clone(),
        Ty::Bool,
        RValue::Unary {
            op: UnaryOp::Not,
            arg: equal,
        },
    );
    ctx.stmts.push(Stmt::If {
        cond: differs,
        then_body: stop,
        else_body: Vec::new(),
    });
    let body = std::mem::replace(&mut ctx.stmts, outside);
    // Only walk the keys when the counts agree.
    ctx.stmts.push(Stmt::If {
        cond: result.clone(),
        then_body: vec![Stmt::ForRange {
            index,
            end: counts[0].clone(),
            inclusive: false,
            body,
        }],
        else_body: Vec::new(),
    });
    Ok(Some(result))
}

/// Declare a local that only ever borrows (a map slot read by `MapGet`): it
/// is zeroed like any local, but never dropped, not even when a comparison
/// nested in a loop declares it again.
fn declare_borrow(ctx: &mut LowerCtx, place: &Place, ty: &Ty) -> Result<()> {
    let start = ctx.stmts.len();
    init_place(ctx, place, ty)?;
    let declared = ctx.stmts.split_off(start);
    ctx.stmts.extend(
        declared
            .into_iter()
            .filter(|stmt| !matches!(stmt, Stmt::Drop { place: dropped, .. } if dropped == place)),
    );
    ctx.drop_places.retain(|(name, _)| *name != place.name);
    Ok(())
}

/// Whether values of `ty` hold anything to compare: a product with a field
/// other than Void, or any other type.
fn has_storage(types: &HashMap<String, TypeInfo>, ty: &Ty) -> bool {
    match ty {
        Ty::Void => false,
        Ty::User(name) => match types.get(name) {
            Some(info) if info.sum.is_none() && info.refined.is_none() => {
                info.fields.iter().any(|field| field.ty != Ty::Void)
            }
            _ => true,
        },
        _ => true,
    }
}

/// A field of `base`, read without a reference of its own: it is only
/// compared, and `base` outlives the comparison.
fn borrow_field(ctx: &mut LowerCtx, base: &Place, name: &str, ty: &Ty) -> Place {
    let p = ctx.fresh("eqf");
    ctx.assign_no_drop(
        p.clone(),
        ty.clone(),
        RValue::Field {
            base: base.clone(),
            field: sanitize_field(name),
        },
    );
    p
}

/// One field's `==` (D175), or `None` when its type has none (Node, a
/// function, an element or value without one).
fn lower_field_eq(
    ctx: &mut LowerCtx,
    left: Place,
    right: Place,
    ty: &Ty,
    span: Span,
) -> Result<Option<Place>> {
    if *ty == Ty::Bool || *ty == Ty::Float64 || is_int(ty) {
        let p = ctx.fresh("eqv");
        ctx.assign(
            p.clone(),
            Ty::Bool,
            RValue::Binary {
                op: BinaryOp::Eq,
                left,
                right,
            },
        );
        return Ok(Some(p));
    }
    match ty {
        Ty::String => {
            let p = ctx.fresh("eqs");
            ctx.assign(
                p.clone(),
                Ty::Bool,
                RValue::Call {
                    callee: Callee::Builtin("string_eq".into()),
                    args: vec![left, right],
                },
            );
            Ok(Some(p))
        }
        Ty::Bytes => Ok(Some(lower_bytes_eq(ctx, left, right))),
        Ty::Array { elem } => lower_array_eq(ctx, left, right, elem, span),
        Ty::Map { key, val } => lower_map_eq(ctx, left, right, key, val, span),
        Ty::User(name) => {
            // `type Qty = Int64 where …` compares its base value.
            if let Some(base) = refined_info(&ctx.types, ty).map(|info| info.base.clone()) {
                let left_value = borrow_field(ctx, &left, "value", &base);
                let right_value = borrow_field(ctx, &right, "value", &base);
                return lower_field_eq(ctx, left_value, right_value, &base, span);
            }
            // A literal union (`"espresso" | "mocha"`) is its tag.
            let literal = [LitKind::String, LitKind::Int, LitKind::Bool]
                .into_iter()
                .any(|kind| is_pure_lit_sum_of_kind(&ctx.types, ty, kind));
            if literal {
                let mut tags = Vec::new();
                for place in [left, right] {
                    let tag = ctx.fresh("eqtag");
                    ctx.assign(
                        tag.clone(),
                        Ty::Int64,
                        RValue::SumTag {
                            place,
                            sum_type: name.clone(),
                        },
                    );
                    tags.push(tag);
                }
                let p = ctx.fresh("eqt");
                let right_tag = tags.pop().unwrap();
                let left_tag = tags.pop().unwrap();
                ctx.assign(
                    p.clone(),
                    Ty::Bool,
                    RValue::Binary {
                        op: BinaryOp::Eq,
                        left: left_tag,
                        right: right_tag,
                    },
                );
                return Ok(Some(p));
            }
            lower_user_eq(ctx, left, right, name, span).map(Some)
        }
        _ => Ok(None),
    }
}

/// A `std/fallible` instance (`May<V>`, `Trial<…>`) for a built-in's
/// result, made when it does not exist yet (`map[key]` in generic code is a
/// `May` of an archetype). std/fallible must be in the program; `May` need
/// not be imported by name.
fn fallible_mono(ctx: &mut LowerCtx, base_src: &str, args: &[Ty], span: Span) -> Result<Ty> {
    let std_base = format!("std_fallible_{base_src}");
    let in_program = ctx.types.contains_key(&mono_type_name(&std_base, args))
        || ctx.synthetics.borrow().generics.contains_key(&std_base);
    let base = ctx
        .name_to_type
        .get(base_src)
        .cloned()
        .or_else(|| in_program.then_some(std_base));
    let Some(base) = base else {
        return Err(Error::coded_at(
            crate::diagnostic::CODE_FALLIBLE_IMPORT,
            format!(
                "`{base_src}` is not in scope — import it from \"std/fallible\" \
                 (e.g. `import {{ Has, Not, May, Pass, Fail, Trial }} from \"std/fallible\"`)"
            ),
            span,
        )
        .with_help(
            "add `import { Has, Not, May, Pass, Fail, Trial } from \"std/fallible\"` \
             (or a selective subset)",
        ));
    };
    let mono = mono_type_name(&base, args);
    if ctx.types.contains_key(&mono) {
        return Ok(Ty::User(mono));
    }
    if !ctx.synthetics.borrow().generics.contains_key(&base) {
        return Err(Error::at(
            format!(
                "type `{base_src}` monomorph `{mono}` missing — add a type annotation using `{base_src}<…>`"
            ),
            span,
        ));
    }
    let name_map = ctx.name_to_type.clone();
    let made = generic::make_instance(ctx, &base, args.to_vec(), &name_map, span)?;
    Ok(Ty::User(made))
}

fn is_residual_union(types: &HashMap<String, TypeInfo>, ty: &Ty) -> bool {
    match ty {
        Ty::User(n) => types
            .get(n)
            .and_then(|i| i.sum.as_ref())
            .and_then(|s| s.residual_tag)
            .is_some(),
        _ => false,
    }
}

/// Find payload type of a named arm in a union (e.g. `Has` in `May__Int64`).
fn union_arm_payload(types: &HashMap<String, TypeInfo>, union: &Ty, arm: &str) -> Option<Ty> {
    let Ty::User(n) = union else {
        return None;
    };
    let sum = types.get(n)?.sum.as_ref()?;
    sum.variants
        .iter()
        .find(|v| v.name == arm)
        .map(|v| v.ty.clone())
}

/// Construct a monomorphized product type by mangled name.
fn construct_product(
    ctx: &mut LowerCtx,
    mangled: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    let info = ctx
        .types
        .get(mangled)
        .ok_or_else(|| Error::at(format!("unknown type `{mangled}`"), span))?
        .clone();
    let p = ctx.fresh("obj");
    let ty = Ty::User(mangled.to_string());
    // Void-only product (Pass(Void)): treat as nullary in C.
    let real_fields: Vec<_> = info
        .fields
        .iter()
        .filter(|f| f.ty != Ty::Void)
        .cloned()
        .collect();
    if info.zero_init {
        if !args.is_empty() {
            return Err(Error::at("zero-init constructor takes no arguments", span));
        }
        ctx.assign(
            p.clone(),
            ty.clone(),
            RValue::StructZero {
                type_name: mangled.to_string(),
            },
        );
    } else if real_fields.is_empty() {
        // Nullary product (Not) or Void-only (Pass(Void)).
        // Accept 0 args, or 1 Void arg for Pass(Void).
        if args.len() > 1 {
            return Err(Error::at("nullary type takes no arguments", span));
        }
        if args.len() == 1 {
            // Evaluate Void arg for side effects / type check.
            let _ = lower_arg_for_formal(
                ctx,
                match &args[0] {
                    Arg::Pos(e) | Arg::Named { value: e, .. } | Arg::Spread { value: e, .. } => e,
                },
                &Ty::Void,
                "value",
                0,
            )?;
        }
        ctx.assign(
            p.clone(),
            ty.clone(),
            RValue::StructLit {
                type_name: mangled.to_string(),
                fields: vec![],
            },
        );
    } else {
        let formal: Vec<(String, Ty)> = real_fields
            .iter()
            .map(|f| (f.name.clone(), f.ty.clone()))
            .collect();
        let defaults: Vec<Option<Expr>> = real_fields.iter().map(|f| f.default.clone()).collect();
        let marked: Vec<bool> = real_fields.iter().map(|f| f.positional).collect();
        // If source fields include Void, formals are only non-Void; call sites use
        // non-Void args only (Pass(box) not Pass(Void, box)).
        let field_places = resolve_call_args_full(
            ctx,
            &formal,
            &defaults,
            &[],
            &marked,
            args,
            span,
            &info.def_mod,
        )?;
        // Shallow product; inject_into_union Dups before the sum takes ownership.
        // Use assign_no_drop so end-of-fn drops don't free the product twice.
        ctx.assign_no_drop(
            p.clone(),
            ty.clone(),
            RValue::StructLit {
                type_name: mangled.to_string(),
                fields: field_places,
            },
        );
    }
    Ok((p, ty))
}

fn inject_into_union(
    ctx: &mut LowerCtx,
    place: Place,
    from: &Ty,
    union: &Ty,
    span: Span,
) -> Result<(Place, Ty)> {
    if from == union {
        return Ok((place, union.clone()));
    }
    if let Some(p) = try_sum_coerce(ctx, place, from, union, None, span)? {
        return Ok((p, union.clone()));
    }
    Err(Error::at(
        format!("cannot inject {from:?} into {union:?}"),
        span,
    ))
}

/// Rewrap a residual arm into another residual-marked union (possibly different module mono).
fn rewrap_residual_into(
    ctx: &mut LowerCtx,
    scrut_p: &Place,
    residual_tag: u32,
    residual_arm: &SumVariant,
    fn_ret: &Ty,
    span: Span,
) -> Result<Place> {
    let target_arm_ty =
        union_arm_payload(&ctx.types, fn_ret, &residual_arm.name).ok_or_else(|| {
            Error::at(
                format!(
                    "`?` residual arm `{}` is not present on enclosing return type {fn_ret:?}",
                    residual_arm.name
                ),
                span,
            )
        })?;

    // Nullary residual (Not).
    if residual_arm.ty == Ty::Void
        || matches!(&target_arm_ty, Ty::User(n) if ctx.types.get(n).map(|i| i.fields.is_empty()).unwrap_or(false))
    {
        let Ty::User(tname) = &target_arm_ty else {
            return Err(Error::at("internal: residual target not a product", span));
        };
        let (atom, aty) = construct_product(ctx, tname, &[], span)?;
        let (inj, _) = inject_into_union(ctx, atom, &aty, fn_ret, span)?;
        return Ok(inj);
    }

    // Product residual (Fail(error), …): extract fields, rebuild target mono product.
    let src_prod = ctx.fresh("rpay");
    ctx.assign_no_drop(
        src_prod.clone(),
        residual_arm.ty.clone(),
        RValue::SumPayload {
            base: scrut_p.clone(),
            tag: residual_tag,
            payload_ty: residual_arm.ty.clone(),
        },
    );

    // Fast path: same product type monomorph.
    if residual_arm.ty == target_arm_ty {
        let (inj, _) = inject_into_union(ctx, src_prod, &residual_arm.ty, fn_ret, span)?;
        return Ok(inj);
    }

    let Ty::User(src_n) = &residual_arm.ty else {
        return Err(Error::at("internal: residual product not User", span));
    };
    let Ty::User(dst_n) = &target_arm_ty else {
        return Err(Error::at("internal: residual target not User", span));
    };
    let src_info = ctx
        .types
        .get(src_n)
        .cloned()
        .ok_or_else(|| Error::at(format!("unknown residual type `{src_n}`"), span))?;
    let dst_info = ctx
        .types
        .get(dst_n)
        .cloned()
        .ok_or_else(|| Error::at(format!("unknown residual target `{dst_n}`"), span))?;
    if src_info.fields.len() != dst_info.fields.len() {
        return Err(Error::at(
            format!(
                "cannot rewrap residual `{}`: field count mismatch",
                residual_arm.name
            ),
            span,
        ));
    }
    let mut field_places = Vec::new();
    for (sf, df) in src_info.fields.iter().zip(dst_info.fields.iter()) {
        if sf.ty != df.ty {
            return Err(Error::at(
                format!(
                    "cannot rewrap residual `{}`: field `{}` type mismatch",
                    residual_arm.name, sf.name
                ),
                span,
            ));
        }
        if sf.ty == Ty::Void {
            continue;
        }
        let fp = ctx.fresh("rf");
        ctx.assign(
            fp.clone(),
            sf.ty.clone(),
            RValue::Field {
                base: src_prod.clone(),
                field: sanitize_field(&sf.name),
            },
        );
        if needs_drop(&sf.ty) {
            ctx.stmts.push(Stmt::Dup {
                place: fp.clone(),
                ty: sf.ty.clone(),
            });
        }
        field_places.push(fp);
    }
    let rebuilt = ctx.fresh("rprod");
    ctx.assign_no_drop(
        rebuilt.clone(),
        target_arm_ty.clone(),
        RValue::StructLit {
            type_name: dst_n.clone(),
            fields: field_places,
        },
    );
    let (inj, _) = inject_into_union(ctx, rebuilt, &target_arm_ty, fn_ret, span)?;
    Ok(inj)
}

/// While probing for a return type: a `?` residual seen so far whose union
/// has an arm named `arm` (`May<Void>` for `Has` / `Not`).
fn probe_residual_with_arm(ctx: &LowerCtx, arm: &str) -> Option<Ty> {
    if !ctx.inferring {
        return None;
    }
    ctx.infer_residuals
        .iter()
        .find(|t| {
            is_residual_union(&ctx.types, t) && union_arm_payload(&ctx.types, t, arm).is_some()
        })
        .cloned()
}

/// D129 §4: in a probe, `Pass(x)` / `Has(x)` typed only from a `?` residual
/// (`Trial<Void, E>`) cannot build its value, since `Trial<T, E>` may not
/// exist yet. Record `x`'s type, which refines the success side when the
/// exits combine, and stand in a place of the residual type (probe MIR is
/// discarded).
fn probe_success_arm(
    ctx: &mut LowerCtx,
    union: &Ty,
    arm: &str,
    args: &[Arg],
) -> Result<Option<(Place, Ty)>> {
    let Ty::User(name) = union else {
        return Ok(None);
    };
    let Some(sum) = ctx.types.get(name).and_then(|info| info.sum.clone()) else {
        return Ok(None);
    };
    let success = sum
        .variants
        .iter()
        .any(|variant| variant.name == arm && Some(variant.tag) == sum.extract_tag);
    let [Arg::Pos(value)] = args else {
        return Ok(None);
    };
    if !success {
        return Ok(None);
    }
    let (_, payload) = lower_expr(ctx, value)?;
    ctx.infer_successes.push(payload);
    Ok(Some((ctx.fresh("probe"), union.clone())))
}

fn lower_pass_fail(
    ctx: &mut LowerCtx,
    callee: &str,
    args: &[Arg],
    span: Span,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
    if args.len() != 1 {
        return Err(Error::at(format!("`{callee}` expects 1 argument"), span));
    }
    // D129: bare Pass/Fail need a residual-union expect. Prefer annotation / fn_ret /
    // residual collapses already seen from `?` in this probe; else require `->`.
    let trial_ty = if let Some(exp) = expect {
        if is_residual_union(&ctx.types, exp) {
            exp.clone()
        } else {
            return Err(Error::at(
                format!("`{callee}` expected a Trial type, found {exp:?}"),
                span,
            ));
        }
    } else if is_residual_union(&ctx.types, &ctx.fn_ret) {
        ctx.fn_ret.clone()
    } else if ctx.inferring {
        if let Some(r) = ctx
            .infer_residuals
            .iter()
            .find(|t| is_residual_union(&ctx.types, t))
            .cloned()
        {
            if let Some(stand_in) = probe_success_arm(ctx, &r, callee, args)? {
                return Ok(stand_in);
            }
            r
        } else {
            return Err(Error::at(
                format!(
                    "cannot infer return type from bare `{callee}(…)` \
                     (constructors alone do not select Trial or another union); \
                     add an explicit `->` annotation"
                ),
                span,
            ));
        }
    } else {
        // Infer Trial<Int64, Int64> — monomorph must already exist from type annotations.
        fallible_mono(ctx, "Trial", &[Ty::Int64, Ty::Int64], span)?
    };
    let arm_ty = union_arm_payload(&ctx.types, &trial_ty, callee)
        .ok_or_else(|| Error::at(format!("`{callee}` is not an arm of {trial_ty:?}"), span))?;
    let Ty::User(arm_name) = arm_ty else {
        return Err(Error::at(
            format!("internal: `{callee}` arm is not a product"),
            span,
        ));
    };
    let (atom, aty) = construct_product(ctx, &arm_name, args, span)?;
    inject_into_union(ctx, atom, &aty, &trial_ty, span)
}

fn lower_has(
    ctx: &mut LowerCtx,
    args: &[Arg],
    span: Span,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
    if args.len() != 1 {
        return Err(Error::at("`Has` expects 1 argument", span));
    }
    let may_ty = if let Some(exp) = expect {
        if is_residual_union(&ctx.types, exp) {
            exp.clone()
        } else {
            return Err(Error::at(
                format!("`Has` expected a May type, found {exp:?}"),
                span,
            ));
        }
    } else if is_residual_union(&ctx.types, &ctx.fn_ret) {
        ctx.fn_ret.clone()
    } else if let Some(hint) = probe_residual_with_arm(ctx, "Has") {
        if let Some(stand_in) = probe_success_arm(ctx, &hint, "Has", args)? {
            return Ok(stand_in);
        }
        hint
    } else {
        // Infer May<Int64> when no annotation (historical Has(int) default).
        fallible_mono(ctx, "May", &[Ty::Int64], span)?
    };
    let arm_ty = union_arm_payload(&ctx.types, &may_ty, "Has")
        .ok_or_else(|| Error::at(format!("`Has` is not an arm of {may_ty:?}"), span))?;
    let Ty::User(arm_name) = arm_ty else {
        return Err(Error::at("internal: Has arm is not a product", span));
    };
    let (atom, aty) = construct_product(ctx, &arm_name, args, span)?;
    inject_into_union(ctx, atom, &aty, &may_ty, span)
}

fn lower_not(
    ctx: &mut LowerCtx,
    args: &[Arg],
    span: Span,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
    if !args.is_empty() {
        return Err(Error::at("`Not()` takes no arguments", span));
    }
    let may_ty = if let Some(exp) = expect {
        if is_residual_union(&ctx.types, exp) {
            exp.clone()
        } else {
            return Err(Error::at(
                format!("`Not()` expected a May type, found {exp:?}"),
                span,
            ));
        }
    } else if is_residual_union(&ctx.types, &ctx.fn_ret) {
        ctx.fn_ret.clone()
    } else if let Some(hint) = probe_residual_with_arm(ctx, "Not") {
        hint
    } else {
        fallible_mono(ctx, "May", &[Ty::Int64], span)?
    };
    let arm_ty = union_arm_payload(&ctx.types, &may_ty, "Not")
        .ok_or_else(|| Error::at(format!("`Not` is not an arm of {may_ty:?}"), span))?;
    let Ty::User(arm_name) = arm_ty else {
        // Nullary Not is User with empty fields.
        return Err(Error::at("internal: Not arm is not a product", span));
    };
    let (atom, aty) = construct_product(ctx, &arm_name, args, span)?;
    inject_into_union(ctx, atom, &aty, &may_ty, span)
}

fn fn_ty_of_sig(params: &[(String, Ty)], ret: &Ty) -> Ty {
    Ty::Fn {
        params: params.iter().map(|(_, t)| t.clone()).collect(),
        ret: Box::new(ret.clone()),
    }
}

fn refuse_fn_value(
    sig_multi: bool,
    is_extern: bool,
    has_mut: bool,
    name: &str,
    span: Span,
) -> Result<()> {
    if has_mut {
        return Err(Error::at(
            format!("cannot take `{name}` as a value: it has a `mut` parameter"),
            span,
        ));
    }
    if is_extern {
        return Err(Error::at(
            format!("cannot take a pointer to extern function `{name}`"),
            span,
        ));
    }
    if sig_multi {
        return Err(Error::at(
            format!("cannot take a pointer to multi-clause function `{name}`"),
            span,
        ));
    }
    Ok(())
}

fn make_fn_value(
    ctx: &mut LowerCtx,
    recv: Option<Place>,
    recv_ty: Option<Ty>,
    target: FnTarget,
    ty: Ty,
) -> (Place, Ty) {
    let p = ctx.fresh("fn");
    ctx.assign(
        p.clone(),
        ty.clone(),
        RValue::MakeFn {
            recv,
            recv_ty,
            target,
        },
    );
    (p, ty)
}

fn form_free_fn(ctx: &mut LowerCtx, name: &str, span: Span) -> Result<(Place, Ty)> {
    let mangled = ctx
        .name_to_fn
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string());
    let sig = ctx
        .fns
        .get(&mangled)
        .ok_or_else(|| Error::at(format!("undefined name `{name}`"), span))?
        .clone();
    refuse_fn_value(
        sig.multi_clause,
        sig.extern_symbol.is_some(),
        sig.mutable.contains(&true),
        name,
        span,
    )?;
    let ty = fn_ty_of_sig(&sig.params, &sig.ret);
    Ok(make_fn_value(
        ctx,
        None,
        None,
        FnTarget::Free {
            mangled: sig.mangled.clone(),
        },
        ty,
    ))
}

fn form_unbound_method(
    ctx: &mut LowerCtx,
    type_name: &str,
    method: &str,
    span: Span,
) -> Result<(Place, Ty)> {
    let info = ctx
        .types
        .get(type_name)
        .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), span))?
        .clone();
    if matches!(method, "read" | "readForm" | "fromForm") {
        if let Some(mangled_fn) = lookup_generated_read_fn(ctx, &info, type_name, method) {
            let sig = ctx.fns.get(&mangled_fn).unwrap().clone();
            refuse_fn_value(sig.multi_clause, false, sig.mutable.contains(&true), method, span)?;
            let ty = fn_ty_of_sig(&sig.params, &sig.ret);
            return Ok(make_fn_value(
                ctx,
                None,
                None,
                FnTarget::Free {
                    mangled: sig.mangled.clone(),
                },
                ty,
            ));
        }
    }
    let sig = info
        .methods
        .get(method)
        .ok_or_else(|| Error::at(no_member_message(&type_name, method, "method"), span))?;
    ctx.infer_check_callee(&format!("{type_name}__{method}"), method, span)?;
    if !sig.exported && ctx.module_id != info.def_mod {
        return Err(Error::at(format!("method `{method}` is private"), span));
    }
    refuse_fn_value(sig.multi_clause, false, sig.mutable.contains(&true), method, span)?;
    let mut params = vec![Ty::User(type_name.to_string())];
    params.extend(sig.params.iter().map(|(_, t)| t.clone()));
    let ty = Ty::Fn {
        params,
        ret: Box::new(sig.ret.clone()),
    };
    Ok(make_fn_value(
        ctx,
        None,
        None,
        FnTarget::Method {
            type_name: type_name.to_string(),
            method: method.to_string(),
            bound: false,
        },
        ty,
    ))
}

fn form_bound_method(
    ctx: &mut LowerCtx,
    recv: Place,
    recv_ty: Ty,
    method: &str,
    span: Span,
) -> Result<(Place, Ty)> {
    let type_name = match &recv_ty {
        Ty::User(n) => n.clone(),
        _ => {
            return Err(Error::at(
                format!("cannot bind method `{method}` on non-user type"),
                span,
            ));
        }
    };
    let info = ctx
        .types
        .get(&type_name)
        .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), span))?
        .clone();
    let sig = info
        .methods
        .get(method)
        .ok_or_else(|| Error::at(no_member_message(&type_name, method, "method"), span))?;
    ctx.infer_check_callee(&format!("{type_name}__{method}"), method, span)?;
    if !sig.exported && ctx.module_id != info.def_mod {
        return Err(Error::at(format!("method `{method}` is private"), span));
    }
    refuse_fn_value(sig.multi_clause, false, sig.mutable.contains(&true), method, span)?;
    let ty = fn_ty_of_sig(&sig.params, &sig.ret);
    Ok(make_fn_value(
        ctx,
        Some(recv),
        Some(recv_ty),
        FnTarget::Method {
            type_name,
            method: method.to_string(),
            bound: true,
        },
        ty,
    ))
}

fn lower_indirect_call(
    ctx: &mut LowerCtx,
    fun: Place,
    fun_ty: Ty,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    let Ty::Fn { params, ret } = fun_ty else {
        return Err(Error::at(
            format!("value of type {} is not callable", display_ty(&fun_ty)),
            span,
        ));
    };
    if let Some(Arg::Named { name, name_span, .. }) =
        args.iter().find(|arg| matches!(arg, Arg::Named { .. }))
    {
        return Err(Error::at(
            format!("a call through a function value passes `{name}` by position"),
            *name_span,
        )
        .with_help("a function type has no parameter names, so its arguments go by position"));
    }
    let formal: Vec<(String, Ty)> = params
        .iter()
        .enumerate()
        .map(|(i, t)| (format!("a{i}"), t.clone()))
        .collect();
    let arg_places = resolve_call_args(ctx, &formal, args, span)?;
    emit_call_result(
        ctx,
        Callee::Indirect {
            fun,
            arg_tys: params,
            ret_ty: ret.as_ref().clone(),
        },
        arg_places,
        ret.as_ref().clone(),
    )
}

fn lower_call(
    ctx: &mut LowerCtx,
    callee: &str,
    args: &[Arg],
    span: Span,
    expect: Option<&Ty>,
    type_args: &[TypeExpr],
) -> Result<(Place, Ty)> {
    // `empty<Int64>()`: type arguments written out (D230).
    let explicit = resolve_type_args(ctx, type_args)?;
    if !explicit.is_empty() {
        if let Some(found) = explicit_type_call(ctx, callee, args, span, expect, type_args, &explicit)? {
            return Ok(found);
        }
    }
    if let Some(b) = ctx.lookup(callee) {
        if matches!(b.ty, Ty::Fn { .. }) {
            return lower_indirect_call(ctx, b.place, b.ty, args, span);
        }
    }
    // Inside a method, a sibling method wins over a free function.
    if let Some(type_name) = ctx.method_type.clone() {
        if sibling_visible(ctx, &type_name, callee) {
            return lower_sibling_call(ctx, &type_name, callee, args, span);
        }
    }
    // So it does in a built-in type's methods, written with `extend` or in a
    // conformance: the call is on `self`.
    if ctx.lookup(callee).is_none() && extension_sibling(ctx, callee, span) {
        return lower_method_call(ctx, &crate::ast_build::name("self"), callee, span, args);
    }
    match callee {
        "Pass" | "Fail" => lower_pass_fail(ctx, callee, args, span, expect),
        "Has" => lower_has(ctx, args, span, expect),
        "Not" => lower_not(ctx, args, span, expect),
        "Bytes" => {
            // `Bytes(n)` — zero-filled buffer (R9 / D18). Not a user type.
            lower_bytes_new(ctx, args, span)
        }
        "Buffer" => lower_buffer_new(ctx, args, span),
        "Int64" | "Int32" | "UInt64" | "UInt32" | "Byte" => {
            lower_truncating(ctx, callee, args, span)
        }
        "Float64" => lower_float_rounding(ctx, args, span),
        // A derived `compare`'s and `hash`'s bodies (D228).
        "__compare" => order::lower_compare_intrinsic(ctx, args, span),
        "__hash" => order::lower_hash_intrinsic(ctx, args, span),
        "__show" => order::lower_show_intrinsic(ctx, args, span),
        // The exactness test of a generated `as Float64` reader (D224).
        "__float64Exact" => lower_float_exact(ctx, args, span),
        "Map" => lower_map_new(ctx, args, span, expect),
        "__jsx_el" => lower_html_el(ctx, args, span),
        "__jsx_child" => lower_html_child(ctx, args, span),
        "__jsx_attr" => lower_html_attr(ctx, args, span),
        "Element" => lower_html_element_ctor(ctx, args, span),
        "Text" => lower_html_text_ctor(ctx, args, span),
        "Fragment" => lower_html_frag_ctor(ctx, args, span),
        other => {
            // Type constructor via local or imported name.
            if let Some(base_mangled) = ctx.resolve_type_name(other) {
                // A generic type's instance: from the expected type, else
                // from its arguments (D228 §5).
                let mangled_ty = match resolve_ctor_mono(ctx, &base_mangled, other, expect, span) {
                    Ok(chosen) => chosen.unwrap_or(base_mangled),
                    Err(error) => match generic::infer_constructor(ctx, &base_mangled, other, args, span)? {
                        Some(inferred) => inferred,
                        None => return Err(error),
                    },
                };
                let info = ctx
                    .types
                    .get(&mangled_ty)
                    .ok_or_else(|| {
                        Error::at(
                            format!(
                                "generic type `{other}` requires a type annotation (e.g. `{other}<Int64>`)"
                            ),
                            span,
                        )
                    })?
                    .clone();
                // A resource is built only by its own module (D168): a second
                // box over the same fd or handle would close it twice.
                if info.resource && info.def_mod != ctx.module_id {
                    return Err(Error::at(
                        format!("`{other}(…)` can only be called in the module that defines `{other}`"),
                        span,
                    )
                    .with_help(format!(
                        "`{other}` has `fn drop()`, so a second one over the same handle would close it twice; get a `{other}` from a function that module exports"
                    )));
                }
                let p = ctx.fresh("obj");
                let ty = Ty::User(mangled_ty.clone());
                if info.zero_init {
                    if !args.is_empty() {
                        return Err(Error::at(
                            format!("`{other}()` takes no arguments (zero-init constructor)"),
                            span,
                        ));
                    }
                    ctx.assign(
                        p.clone(),
                        ty.clone(),
                        RValue::StructZero {
                            type_name: mangled_ty,
                        },
                    );
                } else {
                    let formal: Vec<(String, Ty)> = info
                        .fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect();
                    let defaults: Vec<Option<Expr>> =
                        info.fields.iter().map(|f| f.default.clone()).collect();
                    let marked: Vec<bool> = info.fields.iter().map(|f| f.positional).collect();
                    let field_places = resolve_call_args_full(
                        ctx,
                        &formal,
                        &defaults,
                        &[],
                        &marked,
                        args,
                        span,
                        &info.def_mod,
                    )?;
                    ctx.assign(
                        p.clone(),
                        ty.clone(),
                        RValue::StructLit {
                            type_name: mangled_ty,
                            fields: field_places.clone(),
                        },
                    );
                    // StructLit copies field pointers; take our own refs so arg temps can drop.
                    // Drop types are already a unique box (D155); duping the box leaks.
                    if needs_drop(&ty) && !type_has_drop(&ctx.types, &ty) {
                        ctx.stmts.push(Stmt::Dup {
                            place: p.clone(),
                            ty: ty.clone(),
                        });
                    }
                    run_constructor_setters(ctx, &info, &ty, &p, &field_places)?;
                }
                return Ok((p, ty));
            }

            // Free function via local or imported name.
            let mangled = ctx
                .name_to_fn
                .get(other)
                .cloned()
                .unwrap_or_else(|| other.to_string());
            // A generic function's call names its instance (D228 §5).
            let generic = ctx.synthetics.borrow().generic_fns.contains_key(&mangled);
            let mangled = if generic {
                generic::instantiate_call(ctx, &mangled, other, args, span, expect, &explicit)?
            } else {
                mangled
            };
            ctx.infer_check_callee(&mangled, other, span)?;
            let sig = ctx
                .fns
                .get(&mangled)
                .ok_or_else(|| Error::at(format!("unknown function `{other}`"), span))?
                .clone();
            let arg_places = resolve_call_args_full(
                ctx,
                &sig.params,
                &sig.defaults,
                &sig.mutable,
                &sig.positional,
                args,
                span,
                &sig.def_mod,
            )?;
            let callee = if let Some(sym) = &sig.extern_symbol {
                Callee::Extern {
                    c_symbol: sym.clone(),
                    arg_tys: sig.params.iter().map(|(_, t)| t.clone()).collect(),
                    ret_ty: sig.ret.clone(),
                }
            } else {
                Callee::User(sig.mangled.clone())
            };
            emit_call_result(ctx, callee, arg_places, sig.ret)
        }
    }
}

fn lower_start(ctx: &mut LowerCtx, callee: &str, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    if ctx
        .lookup(callee)
        .is_some_and(|b| matches!(b.ty, Ty::Fn { .. }))
    {
        return Err(Error::at(
            "`-->` requires a named free function, not a function value",
            span,
        ));
    }
    if ctx.resolve_type_name(callee).is_some() {
        return Err(Error::at(
            format!("`-->` requires a function, not type `{callee}`"),
            span,
        ));
    }
    let mangled = ctx
        .name_to_fn
        .get(callee)
        .cloned()
        .unwrap_or_else(|| callee.to_string());
    let sig = ctx
        .fns
        .get(&mangled)
        .ok_or_else(|| Error::at(format!("unknown function `{callee}`"), span))?
        .clone();
    if sig.extern_symbol.is_some() {
        return Err(Error::at(
            format!("`-->` requires a Thoxa function (not extern `{callee}`)"),
            span,
        ));
    }
    if sig.mutable.contains(&true) {
        return Err(Error::at(
            format!("`-->` cannot start `{callee}`: it takes a `mut` parameter, and the task outlives the call"),
            span,
        ));
    }
    let arg_places = resolve_call_args_full(
        ctx,
        &sig.params,
        &sig.defaults,
        &[],
        &sig.positional,
        args,
        span,
        &sig.def_mod,
    )?;
    let arg_tys: Vec<Ty> = sig.params.iter().map(|(_, t)| t.clone()).collect();
    ctx.stmts.push(Stmt::Start {
        callee: Callee::User(sig.mangled.clone()),
        args: arg_places,
        arg_tys,
        ret_ty: sig.ret,
    });
    let p = ctx.fresh("u");
    ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
    Ok((p, Ty::Void))
}

/// Coerce a place to a target type when a conversion is defined (FFI integer widths).
fn refined_info<'a>(types: &'a HashMap<String, TypeInfo>, ty: &Ty) -> Option<&'a RefinedInfo> {
    match ty {
        Ty::User(n) => types.get(n).and_then(|i| i.refined.as_ref()),
        _ => None,
    }
}

fn unwrap_refined(ctx: &mut LowerCtx, place: Place, from: &Ty) -> Result<(Place, Ty)> {
    let Some(base) = refined_info(&ctx.types, from).map(|i| i.base.clone()) else {
        return Ok((place, from.clone()));
    };
    let p = ctx.fresh("unref");
    ctx.assign(
        p.clone(),
        base.clone(),
        RValue::Field {
            base: place,
            field: sanitize_field("value"),
        },
    );
    // Like any field load: the base value is shared with the refined box, so
    // own a separate reference (a refined String appended to an array was
    // freed with the box while the array still held it).
    if needs_drop(&base) {
        ctx.stmts.push(Stmt::Dup {
            place: p.clone(),
            ty: base.clone(),
        });
    }
    Ok((p, base))
}

fn wrap_refined(ctx: &mut LowerCtx, value: Place, refined_ty: &Ty, span: Span) -> Result<Place> {
    let Ty::User(name) = refined_ty else {
        return Err(Error::at("internal: wrap_refined on non-user", span));
    };
    let p = ctx.fresh("refn");
    ctx.assign(
        p.clone(),
        refined_ty.clone(),
        RValue::StructLit {
            type_name: name.clone(),
            fields: vec![value],
        },
    );
    if needs_drop(refined_ty) {
        ctx.stmts.push(Stmt::Dup {
            place: p.clone(),
            ty: refined_ty.clone(),
        });
    }
    Ok(p)
}

fn inhabit_literal(
    ctx: &mut LowerCtx,
    place: Place,
    lit_ty: Ty,
    lit: crate::ast::WhereLit,
    expect: Option<&Ty>,
    span: Span,
) -> Result<(Place, Ty)> {
    let Some(exp) = expect else {
        return Ok((place, lit_ty));
    };
    let Some(info) = refined_info(&ctx.types, exp).cloned() else {
        return Ok((place, lit_ty));
    };
    if info.base != lit_ty {
        return Ok((place, lit_ty));
    }
    let base_name = match &lit_ty {
        Ty::Int64 => "Int64",
        Ty::Float64 => "Float64",
        Ty::String => "String",
        _ => "value",
    };
    if !crate::refine::lit_inhabits(base_name, &info.clause.expr, &lit) {
        let got = crate::refine::lit_show(&lit);
        let expected = crate::refine::expected_clause(&info.clause.expr, base_name);
        return Err(Error::at(
            format!("{got} does not fit in {}: expected {expected}", display_ty(exp)),
            span,
        ));
    }
    let wrapped = wrap_refined(ctx, place, exp, span)?;
    Ok((wrapped, exp.clone()))
}

fn try_refine_coerce(
    ctx: &mut LowerCtx,
    place: Place,
    from: &Ty,
    to: &Ty,
    lit: Option<&crate::ast::TypeLit>,
    span: Span,
) -> Result<Option<Place>> {
    let _ = lit;
    // Refined → base (and refined → looser refined via base).
    let from_ref = refined_info(&ctx.types, from).cloned();
    let to_ref = refined_info(&ctx.types, to).cloned();
    if let Some(rf) = from_ref {
        if &rf.base == to {
            let (p, _) = unwrap_refined(ctx, place, from)?;
            return Ok(Some(p));
        }
        if let Some(rt) = to_ref {
            if rf.base == rt.base {
                let (p, _) = unwrap_refined(ctx, place, from)?;
                if refine_implies(&rf.clause.expr, &rt.clause.expr, &rf.base) {
                    return Ok(Some(wrap_refined(ctx, p, to, span)?));
                }
            }
        }
    }
    Ok(None)
}

fn refine_implies(from: &crate::ast::WhereExpr, to: &crate::ast::WhereExpr, base: &Ty) -> bool {
    if base != &Ty::Int64 {
        return false;
    }
    // Conservative: only when `from` is `to & extra` (syntactic) or equal.
    format!("{from:?}") == format!("{to:?}")
}

/// `unknown type `X``, naming the import for the `std/fallible` types, which
/// are not ambient.
fn unknown_type_error(name: &str, span: Span) -> Error {
    let error = Error::at(format!("unknown type `{name}`"), span);
    match name {
        "May" | "Trial" => error.with_help(format!(
            "`{name}` comes from std/fallible: import {{ {} }} from \"std/fallible\"",
            if name == "May" { "May, Has, Not" } else { "Trial, Pass, Fail" }
        )),
        _ => error,
    }
}

fn coerce_place(ctx: &mut LowerCtx, place: Place, from: &Ty, to: &Ty, span: Span) -> Result<Place> {
    if from == to {
        return Ok(place);
    }
    if !can_numeric_cast(from, to) {
        return Err(Error::at(format!("cannot coerce {from:?} to {to:?}"), span));
    }
    // D224: an Int64 or UInt64 becomes a Float64 only when it is exact (past
    // 2^53 it would round): a literal is checked here, anything else at run
    // time. Narrower integers always are.
    if *to == Ty::Float64 && matches!(from, Ty::Int64 | Ty::UInt64) {
        if let Some(value) = literal_int_value(ctx, &place) {
            if (value as f64) as i128 != value {
                return Err(Error::at(
                    format!("{value} does not fit in Float64 exactly (it would round)"),
                    span,
                )
                .with_help(format!("write `Float64(rounding: {value})` to round it")));
            }
        } else {
            let p = ctx.fresh("fexact");
            let builtin = if *from == Ty::Int64 { "f64_from_i64" } else { "f64_from_u64" };
            ctx.assign(
                p.clone(),
                Ty::Float64,
                RValue::Call {
                    callee: Callee::Builtin(builtin.into()),
                    args: vec![place],
                },
            );
            return Ok(p);
        }
    }
    let checked = match (int_range(from), int_range(to)) {
        // Narrowing (D167): a literal is checked here, anything else at run time.
        (Some((from_min, from_max)), Some((to_min, to_max)))
            if from_min < to_min || from_max > to_max =>
        {
            match literal_int_value(ctx, &place) {
                Some(value) if value >= to_min && value <= to_max => false,
                Some(value) => {
                    let name = display_ty(to);
                    return Err(Error::at(
                        format!("{value} does not fit in {name} ({to_min} to {to_max})"),
                        span,
                    )
                    .with_help(format!(
                        "write `{name}(truncating: {value})` to keep the low bits"
                    )));
                }
                None => true,
            }
        }
        _ => false,
    };
    let p = ctx.fresh("cast");
    let value = if checked {
        RValue::NarrowChecked {
            value: place,
            from: from.clone(),
            to: to.clone(),
        }
    } else {
        RValue::Cast {
            value: place,
            from: from.clone(),
            to: to.clone(),
        }
    };
    ctx.assign(p.clone(), to.clone(), value);
    Ok(p)
}

/// The values an integer type holds, as `(min, max)`.
fn int_range(ty: &Ty) -> Option<(i128, i128)> {
    match ty {
        Ty::Int64 => Some((i64::MIN as i128, i64::MAX as i128)),
        Ty::Int32 => Some((i32::MIN as i128, i32::MAX as i128)),
        Ty::UInt64 => Some((0, u64::MAX as i128)),
        Ty::UInt32 => Some((0, u32::MAX as i128)),
        Ty::Byte => Some((0, u8::MAX as i128)),
        _ => None,
    }
}

/// The value of `place` when it is the temporary holding an integer literal,
/// or a negated one. Named locals may be reassigned, so they never count.
fn literal_int_value(ctx: &LowerCtx, place: &Place) -> Option<i128> {
    let temp_of = |prefix: &str, name: &str| {
        name.strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
    };
    let assigned = |target: &Place| {
        ctx.stmts.iter().rev().find_map(|stmt| match stmt {
            Stmt::Assign { place, value, .. } if place == target => Some(value),
            _ => None,
        })
    };
    if temp_of("_i", &place.name) {
        return match assigned(place)? {
            RValue::Int(value) => Some(*value as i128),
            _ => None,
        };
    }
    if temp_of("_big", &place.name) {
        return match assigned(place)? {
            RValue::Int(bits) => Some(*bits as u64 as i128),
            _ => None,
        };
    }
    if temp_of("_neg", &place.name) {
        if let RValue::Unary {
            op: UnaryOp::Neg,
            arg,
        } = assigned(place)?
        {
            if temp_of("_i", &arg.name) {
                if let RValue::Int(value) = assigned(arg)? {
                    return Some(-(*value as i128));
                }
            }
        }
    }
    None
}

/// `Int32(truncating: value)` and the other integer types keep the low bits
/// of `value` (two's complement), with no range check (D167).
/// `Float64(rounding: x)`: an integer as the nearest Float64, unchecked
/// (D224), as `T(truncating: x)` keeps an integer's low bits.
fn lower_float_rounding(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    let value = match args {
        [Arg::Named { name, value, .. }] if name == "rounding" => value,
        _ => {
            return Err(Error::at("`Float64(…)` takes one argument, `rounding:`", span).with_help(
                "an integer passed where Float64 is expected converts on its own, and panics if it would round; `Float64(rounding: value)` rounds instead",
            ));
        }
    };
    let (place, from) = lower_expr(ctx, value)?;
    if !is_int(&from) {
        return Err(Error::at(
            format!("`rounding:` expects an integer, found {}", display_ty(&from)),
            value.span,
        ));
    }
    let p = ctx.fresh("round");
    ctx.assign(
        p.clone(),
        Ty::Float64,
        RValue::Cast {
            value: place,
            from,
            to: Ty::Float64,
        },
    );
    Ok((p, Ty::Float64))
}

/// `__float64Exact(value)`: whether an Int64 / UInt64 is exactly a Float64.
fn lower_float_exact(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    let [Arg::Pos(value)] = args else {
        return Err(Error::at("internal: `__float64Exact` takes one value", span));
    };
    let (place, from) = lower_expr(ctx, value)?;
    let builtin = match from {
        Ty::Int64 => "f64_exact_i64",
        Ty::UInt64 => "f64_exact_u64",
        _ => return Err(Error::at("internal: `__float64Exact` takes an Int64 or UInt64", span)),
    };
    let raw = ctx.fresh("fexact");
    ctx.assign(
        raw.clone(),
        Ty::Int64,
        RValue::Call {
            callee: Callee::Builtin(builtin.into()),
            args: vec![place],
        },
    );
    let p = ctx.fresh("fexactb");
    ctx.assign(
        p.clone(),
        Ty::Bool,
        RValue::Cast {
            value: raw,
            from: Ty::Int64,
            to: Ty::Bool,
        },
    );
    Ok((p, Ty::Bool))
}

fn lower_truncating(
    ctx: &mut LowerCtx,
    type_name: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    let to = match type_name {
        "Int64" => Ty::Int64,
        "Int32" => Ty::Int32,
        "UInt64" => Ty::UInt64,
        "UInt32" => Ty::UInt32,
        _ => Ty::Byte,
    };
    let value = match args {
        [Arg::Named { name, value, .. }] if name == "truncating" => value,
        _ => {
            return Err(Error::at(
                format!("`{type_name}(…)` takes one argument, `truncating:`"),
                span,
            )
            .with_help(format!(
                "a value passed where {type_name} is expected converts on its own, and panics if it does not fit; `{type_name}(truncating: value)` keeps the low bits instead"
            )));
        }
    };
    let (place, from) = lower_expr(ctx, value)?;
    if !is_int(&from) {
        return Err(Error::at(
            format!(
                "`truncating:` expects an integer, found {}",
                display_ty(&from)
            ),
            value.span,
        ));
    }
    if from == to {
        return Ok((place, to));
    }
    let p = ctx.fresh("trunc");
    ctx.assign(
        p.clone(),
        to.clone(),
        RValue::Cast {
            value: place,
            from,
            to: to.clone(),
        },
    );
    Ok((p, to))
}

fn lower_if(
    ctx: &mut LowerCtx,
    cond: &Expr,
    then_branch: &Block,
    else_branch: Option<&Block>,
    span: Span,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
    let (cond_p, cond_ty) = lower_expr(ctx, cond)?;
    if cond_ty != Ty::Bool {
        return Err(Error::at("`if` condition must be Bool", cond.span));
    }

    let then_ty = infer_block_ty(ctx, then_branch)?;
    let else_ty = if let Some(eb) = else_branch {
        infer_block_ty(ctx, eb)?
    } else {
        Ty::Void
    };

    // Expect-type inject (D119 §2.7): `-> Outcome` / `let x: Outcome = if …`.
    let result_ty = if let Some(exp) = expect {
        // Without `else` there is no value on the false path, so the expected
        // type cannot be met; fall through to unify (Void) and its error.
        if *exp != Ty::Void
            && else_branch.is_some()
            && branch_fits(ctx, then_branch, &then_ty, exp)
            && (else_branch.is_some_and(|eb| branch_fits(ctx, eb, &else_ty, exp))
                || else_ty == Ty::Void)
        {
            exp.clone()
        } else {
            unify_if_branch_tys(ctx, &then_ty, &else_ty, else_branch.is_none(), span)?
        }
    } else {
        unify_if_branch_tys(ctx, &then_ty, &else_ty, else_branch.is_none(), span)?
    };

    let out = if result_ty == Ty::Void || is_never(&result_ty) {
        None
    } else {
        Some(ctx.fresh("if"))
    };

    let outer_stmts = std::mem::take(&mut ctx.stmts);

    ctx.push_scope();
    let then_place = lower_block_into(ctx, then_branch, &result_ty)?;
    ctx.pop_scope();
    let mut then_body = std::mem::take(&mut ctx.stmts);
    if let (Some(out_p), Some(tp)) = (&out, then_place) {
        // Own a ref in `out` so branch temps can be dropped safely.
        // Overwrite-drop the previous occupant (loop-reused `_if`).
        assign_result_place(&mut then_body, out_p, tp, &result_ty);
    }

    let else_body = if let Some(eb) = else_branch {
        ctx.push_scope();
        let else_place = lower_block_into(ctx, eb, &result_ty)?;
        ctx.pop_scope();
        let mut body = std::mem::take(&mut ctx.stmts);
        if let (Some(out_p), Some(ep)) = (&out, else_place) {
            assign_result_place(&mut body, out_p, ep, &result_ty);
        }
        body
    } else {
        Vec::new()
    };

    ctx.stmts = outer_stmts;
    if let Some(ref out_p) = out {
        init_place(ctx, out_p, &result_ty)?;
    }

    ctx.stmts.push(Stmt::If {
        cond: cond_p,
        then_body,
        else_body,
    });

    match out {
        Some(p) => Ok((p, result_ty)),
        None => {
            let p = ctx.fresh("u");
            ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
            Ok((p, Ty::Void))
        }
    }
}

fn unify_if_branch_tys(
    ctx: &mut LowerCtx,
    then_ty: &Ty,
    else_ty: &Ty,
    else_absent: bool,
    span: Span,
) -> Result<Ty> {
    match (then_ty, else_ty) {
        (Ty::Never, x) | (x, Ty::Never) => Ok(x.clone()),
        (Ty::Void, Ty::Void) => Ok(Ty::Void),
        // No `else`: the `if` has no value when the condition is false, so it is
        // Void (a statement). Using it as a value is a type error, not a zero/NULL.
        (_, Ty::Void) if else_absent => Ok(Ty::Void),
        (a, b) if a == b => Ok(a.clone()),
        (a, b) if ty_eq(&ctx.types, a, b) => {
            // Transparent aliases: prefer the then-branch name as the result type.
            Ok(a.clone())
        }
        (a, b) if can_coerce_ty(&ctx.types, a, b) => Ok(b.clone()),
        (a, b) if can_coerce_ty(&ctx.types, b, a) => Ok(a.clone()),
        (a, b) => {
            // Unannotated lub: synthetic member-set union of true types (D119).
            if let Some(u) = try_lub_union(ctx, a, b, span)? {
                return Ok(u);
            }
            Err(Error::at(
                format!(
                    "if branches have different types ({a:?} vs {b:?}); annotate with a union type"
                ),
                span,
            ))
        }
    }
}

/// Collect atom / lit members of a type for set-union lub (expand sums).
fn collect_union_members(
    types: &HashMap<String, TypeInfo>,
    ty: &Ty,
) -> Option<Vec<(String, Ty, Option<TypeLit>)>> {
    match ty {
        Ty::User(n) => {
            let info = types.get(n)?;
            if let Some(sum) = &info.sum {
                let mut out = Vec::new();
                for v in &sum.variants {
                    out.push((v.name.clone(), v.ty.clone(), v.lit.clone()));
                }
                Some(out)
            } else {
                // Nominal atom (product / empty type).
                Some(vec![(n.clone(), ty.clone(), None)])
            }
        }
        // Builtins as single-member sets so D131 can join String | User | Array.
        Ty::String => Some(vec![("String".into(), ty.clone(), None)]),
        Ty::Node => Some(vec![("Node".into(), ty.clone(), None)]),
        Ty::Int64 => Some(vec![("Int64".into(), ty.clone(), None)]),
        Ty::Bool => Some(vec![("Bool".into(), ty.clone(), None)]),
        Ty::Bytes => Some(vec![("Bytes".into(), ty.clone(), None)]),
        Ty::Array { .. } => {
            let key = format!("Array_{}", display_ty(ty).replace(['<', '>', ' '], "_"));
            Some(vec![(key, ty.clone(), None)])
        }
        _ => None,
    }
}

/// Reverse local type map: mangled → source name (for synthetic union arm labels).
fn source_name_of(ctx: &LowerCtx, mangled: &str) -> String {
    for (src, m) in &ctx.name_to_type {
        if m == mangled {
            return src.clone();
        }
    }
    mangled.to_string()
}

/// Ensure a synthetic sum type for the given members exists; return its Ty.
fn ensure_synthetic_sum(
    ctx: &mut LowerCtx,
    members: Vec<(String, Ty, Option<TypeLit>)>,
    span: Span,
) -> Result<Ty> {
    // Canonical name from sorted keys (order-independent).
    let mut keyed: Vec<(String, String, Ty, Option<TypeLit>)> = members
        .into_iter()
        .map(|(name, ty, lit)| {
            // Same scheme as `sum_variant_key` (C-safe).
            let key = if let Some(ref l) = lit {
                match l {
                    TypeLit::Int(n) => format!("liti_{n}"),
                    TypeLit::Bool(b) => format!("litb_{b}"),
                    TypeLit::String(s) => {
                        let safe: String = s
                            .chars()
                            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                            .collect();
                        format!("lits_{safe}")
                    }
                }
            } else if let Ty::User(n) = &ty {
                format!("ty_{n}")
            } else {
                format!(
                    "other_{}",
                    format!("{ty:?}").replace(|c: char| !c.is_ascii_alphanumeric(), "_")
                )
            };
            (key, name, ty, lit)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.dedup_by(|a, b| a.0 == b.0);

    if keyed.len() < 2 {
        return Err(Error::at(
            "internal: synthetic union needs at least two members",
            span,
        ));
    }

    let syn_name = format!(
        "$union${}",
        keyed
            .iter()
            .map(|(k, _, _, _)| k.as_str())
            .collect::<Vec<_>>()
            .join("$")
    );

    if ctx.types.contains_key(&syn_name) {
        return Ok(Ty::User(syn_name));
    }

    // Validate members are injectible atoms (same rules as declared unions).
    for (_, vname, ty, lit) in &keyed {
        if lit.is_some() {
            continue;
        }
        let Ty::User(member_name) = ty else {
            return Err(Error::at(
                format!("cannot form union member from {ty:?}"),
                span,
            ));
        };
        if let Some(minfo) = ctx.types.get(member_name) {
            if minfo.sum.is_some() {
                return Err(Error::at(
                    format!("nested union member `{vname}` not yet supported"),
                    span,
                ));
            }
            for f in &minfo.fields {
                if !union_member_field_ok(&f.ty) {
                    return Err(Error::at(
                        format!(
                            "union member `{vname}` field `{}` must be a scalar, String/Bytes, User, Array, or Map",
                            f.name
                        ),
                        span,
                    ));
                }
            }
        }
    }

    let mut variants = Vec::new();
    let mut layout_vars = Vec::new();
    for (i, (_, name, ty, lit)) in keyed.iter().enumerate() {
        let tag = i as u32;
        let payload_ty = if lit.is_some() { Ty::Void } else { ty.clone() };
        // Prefer source atom name for patterns when User.
        let vname = if lit.is_some() {
            name.clone()
        } else if let Ty::User(n) = ty {
            source_name_of(ctx, n)
        } else {
            name.clone()
        };
        variants.push(SumVariant {
            tag,
            name: vname.clone(),
            ty: payload_ty.clone(),
            lit: lit.clone(),
            mark: None,
            unwrap_payload: false,
        });
        layout_vars.push(SumVariantLayout {
            tag,
            name: vname,
            payload_ty,
        });
    }

    let sum = SumInfo {
        variants,
        residual_tag: None,
        extract_tag: None,
    };
    let info = TypeInfo {
        fields: Vec::new(),
        methods: HashMap::new(),
        zero_init: false,
        def_mod: ctx.module_id.clone(),
        sum: Some(sum),
        refined: None,
        resource: false,
        traits: Vec::new(),
        derived: HashSet::new(),
        instance: None,
        templates: HashMap::new(),
    };
    let layout = TypeLayout {
        name: syn_name.clone(),
        fields: Vec::new(),
        sum: Some(layout_vars),
        sum_scalar: None,
        has_drop: false,
        own_hash: false,
        own_equals: false,
    };

    ctx.types.insert(syn_name.clone(), info.clone());
    ctx.synthetics
        .borrow_mut()
        .entries
        .insert(syn_name.clone(), (info, layout));

    Ok(Ty::User(syn_name))
}

fn try_lub_union(ctx: &mut LowerCtx, a: &Ty, b: &Ty, span: Span) -> Result<Option<Ty>> {
    let Some(mut ma) = collect_union_members(&ctx.types, a) else {
        return Ok(None);
    };
    let Some(mb) = collect_union_members(&ctx.types, b) else {
        return Ok(None);
    };
    ma.extend(mb);
    Ok(Some(ensure_synthetic_sum(ctx, ma, span)?))
}

fn lower_block_into(ctx: &mut LowerCtx, block: &Block, expect: &Ty) -> Result<Option<Place>> {
    lower_block_items(ctx, &block.items)?;
    if let Some(tail) = &block.tail {
        let exp = if *expect != Ty::Void {
            Some(expect)
        } else {
            None
        };
        let (place, ty) = lower_expr_expect(ctx, tail, exp)?;
        if is_never(&ty) {
            return Ok(None);
        }
        if *expect != Ty::Void && &ty != expect {
            match try_sum_coerce(
                ctx,
                place.clone(),
                &ty,
                expect,
                expr_as_type_lit(tail).as_ref(),
                tail.span,
            )? {
                Some(p) => return Ok(Some(p)),
                None if can_numeric_cast(&ty, expect) => {
                    return Ok(Some(coerce_place(ctx, place, &ty, expect, tail.span)?));
                }
                None => {
                    if let Some(p) = convert::encode_where_json_expected(ctx, place, &ty, expect)? {
                        return Ok(Some(p));
                    }
                    return Err(Error::at(
                        format!("expected {expect:?}, found {ty:?}"),
                        tail.span,
                    ));
                }
            }
        }
        Ok(Some(place))
    } else if *expect == Ty::Void {
        Ok(None)
    } else {
        Err(Error::at("missing block value", block.span))
    }
}

fn infer_block_ty(ctx: &LowerCtx, block: &Block) -> Result<Ty> {
    let mut tmp = LowerCtx {
        stmts: Vec::new(),
        temps: ctx.temps,
        env: ctx.env.clone(),
        bound_locals: ctx.bound_locals.clone(),
        fns: ctx.fns.clone(),
        types: ctx.types.clone(),
        name_to_type: ctx.name_to_type.clone(),
        name_to_fn: ctx.name_to_fn.clone(),
        name_to_global: ctx.name_to_global.clone(),
        namespaces: ctx.namespaces.clone(),
        module_exports: ctx.module_exports.clone(),
        package_maps: ctx.package_maps.clone(),
        fn_ret: ctx.fn_ret.clone(),
        module_id: ctx.module_id.clone(),
        is_entry: ctx.is_entry,
        loop_depth: ctx.loop_depth,
        drop_places: ctx.drop_places.clone(),
        synthetics: Rc::clone(&ctx.synthetics),
        inferring: ctx.inferring,
        infer_self: ctx.infer_self.clone(),
        infer_blocked: ctx.infer_blocked.clone(),
        infer_waited: Rc::clone(&ctx.infer_waited),
        // Share residual collapses so bare Pass/Fail inside `if` can resolve (D129).
        infer_residuals: ctx.infer_residuals.clone(),
        infer_values: Vec::new(),
        infer_successes: Vec::new(),
        generics: ctx.generics.clone(),
        route_clause: ctx.route_clause,
        mut_field_args: Vec::new(),
        method_type: ctx.method_type.clone(),
        method_fields: ctx.method_fields.clone(),
        current_method: ctx.current_method.clone(),
        trait_scope: ctx.trait_scope.clone(),
        eq_expanding: ctx.eq_expanding.clone(),
    };
    if block.tail.is_none() {
        return Ok(Ty::Void);
    }
    lower_block_items(&mut tmp, &block.items)?;
    let (_, ty) = lower_expr(&mut tmp, block.tail.as_ref().unwrap())?;
    Ok(ty)
}

fn is_generated_route_handler(name: &str) -> bool {
    name.starts_with("__") && name.contains("_route_")
}

fn sanitize_name(name: &str) -> String {
    format!("v_{name}")
}

fn sanitize_field(name: &str) -> String {
    format!("f_{name}")
}
