//! Mid-level IR: typed enough for RC insert + C codegen.
//! R8: multi-file modules, import/export, module-level let/mut.

use crate::ast::{
    self, Arg, AssignTarget, BinaryOp, Block, BlockItem, Expr, ExprKind, InterpPart, Item,
    MatchArm, Method, Pattern, TypeArg, TypeDef, TypeExpr, UnaryOp,
};
use crate::error::{Error, Result};
use crate::loader::{self, BoundImport, Package, SourceModule};
use crate::span::Span;
use std::collections::HashMap;
use std::path::PathBuf;

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
}

#[derive(Debug, Clone)]
pub struct FieldLayout {
    pub name: String,
    pub c_name: String,
    pub ty: Ty,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub is_main: bool,
    /// Optional implicit receiver for methods: (c_param_name, type).
    pub receiver: Option<(String, Ty)>,
    pub params: Vec<(String, Ty)>,
    pub body: Vec<Stmt>,
    pub ret: Option<Place>,
    pub ret_ty: Ty,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Assign { place: Place, ty: Ty, value: RValue },
    /// `base.field = value` (C-layout field store; truncates via Cast in value if needed).
    FieldAssign {
        base: Place,
        field: String,
        ty: Ty,
        value: Place,
    },
    /// `base.field[index] = value` (fixed-array element store).
    IndexAssign {
        base: Place,
        field: String,
        index: Place,
        ty: Ty,
        value: Place,
    },
    Eval(RValue),
    If {
        cond: Place,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    Return { value: Place },
    /// Bump refcount of a heap object (or live Trial heap payload) at `place`.
    Dup { place: Place, ty: Ty },
    /// Drop a heap object (or live Trial heap payload) at `place` (refcount--).
    Drop { place: Place, ty: Ty },
}

#[derive(Debug, Clone)]
pub enum RValue {
    Use(Place),
    Int(i64),
    Bool(bool),
    /// Heap string from UTF-8 bytes (owned, refcount 1).
    StringLit(String),
    /// Null heap pointer (for uninit String slots).
    Null,
    Unary { op: UnaryOp, arg: Place },
    Binary { op: BinaryOp, left: Place, right: Place },
    Call { callee: Callee, args: Vec<Place> },
    /// Construct a Trial: `payload` is the live side (ok for Pass, err for Fail).
    TrialMake {
        tag: TrialTag,
        payload: Place,
        trial_ty: Ty,
    },
    TrialTag(Place),
    /// Extract `.ok` from a Trial place.
    TrialOk(Place),
    /// Extract `.err` from a Trial place.
    TrialErr(Place),
    /// Construct a user type from field places (declaration order).
    StructLit { type_name: String, fields: Vec<Place> },
    /// Zero-initialize a C-layout / user struct (`TypeName()`).
    StructZero { type_name: String },
    /// `base.field`
    Field { base: Place, field: String },
    /// `base.field[index]` — fixed-array element load.
    Index {
        base: Place,
        field: String,
        index: Place,
    },
    /// Convert Int64/Bool → owned String.
    ToString { value: Place, from: Ty },
    /// Non-consuming concat → owned String.
    StringConcat { left: Place, right: Place },
    /// Numeric / trivial cast between compatible types.
    Cast { value: Place, from: Ty, to: Ty },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrialTag {
    Pass = 0,
    Fail = 1,
}

#[derive(Debug, Clone)]
pub enum Callee {
    Builtin(String),
    User(String),
    /// Method: Type_method — C name built in cgen.
    Method { type_name: String, method: String },
    /// Direct C symbol (libc). Args already coerced to declared types.
    Extern {
        c_symbol: String,
        /// Parameter types (for C marshaling of String/Bytes).
        arg_tys: Vec<Ty>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Void,
    Int32,
    Int64,
    UInt32,
    UInt64,
    /// `Byte` / `UInt8` — fixed 8-bit unsigned.
    Byte,
    Bool,
    /// Heap RC String (`ThoxaString*`).
    String,
    /// Heap RC Bytes (`ThoxaBytes*`) — fixed length, mutable contents.
    Bytes,
    /// Fixed inline array field type `Array<T, N>` (not a first-class value).
    FixedArray { elem: Box<Ty>, len: u64 },
    /// FFI pointer `Address<T>` (param types only in R7).
    Address(Box<Ty>),
    /// `Trial<Ok, Err>` — monomorphized in cgen.
    Trial { ok: Box<Ty>, err: Box<Ty> },
    User(String),
}

fn is_heap(ty: &Ty) -> bool {
    matches!(ty, Ty::String | Ty::Bytes)
}

/// True when a local of this type needs an end-of-scope drop (heap ptr or Trial with heap side).
fn needs_drop(ty: &Ty) -> bool {
    is_heap(ty)
        || matches!(
            ty,
            Ty::Trial { ok, err } if is_heap(ok) || is_heap(err)
        )
}

fn is_trial(ty: &Ty) -> bool {
    matches!(ty, Ty::Trial { .. })
}

fn trial_parts(ty: &Ty) -> Option<(&Ty, &Ty)> {
    match ty {
        Ty::Trial { ok, err } => Some((ok.as_ref(), err.as_ref())),
        _ => None,
    }
}

/// Types allowed as Trial ok/err payloads.
fn is_trial_payload_ty(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Void
            | Ty::Int32
            | Ty::Int64
            | Ty::UInt32
            | Ty::UInt64
            | Ty::Byte
            | Ty::Bool
            | Ty::String
            | Ty::Bytes
            | Ty::User(_)
    )
}

fn is_int(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Int32 | Ty::Int64 | Ty::UInt32 | Ty::UInt64 | Ty::Byte
    )
}

fn is_abi_scalar(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Int32 | Ty::Int64 | Ty::UInt32 | Ty::UInt64 | Ty::Byte
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
}

#[derive(Clone)]
struct FnSig {
    params: Vec<(String, Ty)>,
    ret: Ty,
    /// If set, this name is an `extern fn` (call C, no Thoxa body).
    extern_symbol: Option<String>,
    /// Mangled MIR/C function name (may differ from source name for non-entry modules).
    mangled: String,
}

#[derive(Clone)]
struct MethodSig {
    /// Method params (not including receiver).
    params: Vec<(String, Ty)>,
    ret: Ty,
    exported: bool,
}

#[derive(Clone)]
struct FieldInfo {
    name: String,
    ty: Ty,
    /// `mut` field (required for field assign / Address coercion of whole struct uses place mut).
    mutable: bool,
}

#[derive(Clone)]
struct TypeInfo {
    fields: Vec<FieldInfo>,
    methods: HashMap<String, MethodSig>,
    /// True when constructed with `TypeName()` zero-init (body fields, R7).
    zero_init: bool,
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
    fn_ret: Ty,
    /// Current module id being lowered.
    module_id: String,
    is_entry: bool,
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
        fn_ret: Ty,
        module_id: String,
        is_entry: bool,
    ) -> Self {
        Self {
            stmts: Vec::new(),
            temps: 0,
            env: vec![HashMap::new()],
            fns,
            types,
            name_to_type,
            name_to_fn,
            name_to_global,
            namespaces,
            module_exports,
            fn_ret,
            module_id,
            is_entry,
        }
    }

    fn push_scope(&mut self) {
        self.env.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.env.pop();
    }

    fn define(&mut self, name: &str, ty: Ty, mutable: bool, place: Place) {
        self.env.last_mut().unwrap().insert(
            name.to_string(),
            Binding {
                ty,
                mutable,
                place,
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
            });
        }
        None
    }

    fn resolve_type_name(&self, name: &str) -> Option<String> {
        self.name_to_type.get(name).cloned()
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
        self.stmts.push(Stmt::Assign { place, ty, value });
    }

    /// Bind a name to an already-lowered value (fresh ownership from `place`).
    fn bind_name(&mut self, name: &str, ty: Ty, mutable: bool, place: Place) {
        let bound = Place {
            name: sanitize_name(name),
        };
        self.assign(bound.clone(), ty.clone(), RValue::Use(place));
        self.define(name, ty, mutable, bound);
    }

    /// Reassign a mut binding; drops the old heap value if any.
    fn reassign_name(&mut self, name: &str, name_span: Span, value: &Expr) -> Result<()> {
        let binding = self
            .lookup(name)
            .ok_or_else(|| Error::at(format!("undefined name `{name}`"), name_span))?;
        if !binding.mutable {
            return Err(Error::at(
                format!("cannot assign to immutable binding `{name}`"),
                name_span,
            ));
        }
        let (place, ty) = lower_expr(self, value)?;
        if ty != binding.ty {
            return Err(Error::at(
                format!(
                    "cannot reassign `{name}`: expected {:?}, found {:?}",
                    binding.ty, ty
                ),
                value.span,
            ));
        }
        let bound = binding.place.clone();
        if needs_drop(&binding.ty) {
            self.stmts.push(Stmt::Drop {
                place: bound.clone(),
                ty: binding.ty.clone(),
            });
        }
        // Dup so the RHS temp and the binding both own (end-of-fn drops both).
        self.assign(bound, ty, RValue::Use(place));
        Ok(())
    }
}

/// Lower a single-file module (wrapper used by tests).
pub fn lower_module(module: &ast::Module) -> Result<Program> {
    let pkg = Package {
        project_root: PathBuf::from("."),
        modules: vec![SourceModule {
            id: "Main".into(),
            path: PathBuf::from("Main.thx"),
            ast: module.clone(),
            is_entry: true,
        }],
    };
    lower_package(&pkg)
}

/// Lower a multi-file package into one MIR program.
pub fn lower_package(package: &Package) -> Result<Program> {
    let import_maps = loader::build_import_maps(package)?;

    // ---------- Phase 1: register types (mangled names) ----------
    let mut types_info: HashMap<String, TypeInfo> = HashMap::new();
    let mut type_layouts: Vec<TypeLayout> = Vec::new();
    // (module_id, mangled_type_name, &TypeDef)
    let mut type_defs: Vec<(String, String, &TypeDef)> = Vec::new();
    let mut module_exports: HashMap<String, ModuleExports> = HashMap::new();

    for m in &package.modules {
        module_exports
            .entry(m.id.clone())
            .or_default();
        for item in &m.ast.items {
            let Item::Type(td) = item else { continue };
            let mangled = mangle_type(&m.id, &td.name, m.is_entry);
            if types_info.contains_key(&mangled) {
                return Err(Error::at(
                    format!("duplicate type `{}`", td.name),
                    td.name_span,
                ));
            }
            // Field types may reference other types; resolve after all types registered.
            // First pass: placeholder with empty fields if needed — we resolve fields in pass 1b
            // after all type names exist.
            type_defs.push((m.id.clone(), mangled.clone(), td));
            types_info.insert(
                mangled.clone(),
                TypeInfo {
                    fields: Vec::new(),
                    methods: HashMap::new(),
                    zero_init: !td.fields.is_empty(),
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
                map.insert(
                    td.name.clone(),
                    mangle_type(&m.id, &td.name, m.is_entry),
                );
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
        per_mod_types.insert(m.id.clone(), map);
    }

    // Fill type fields with resolved types.
    for (mod_id, mangled, td) in &type_defs {
        let name_map = per_mod_types.get(mod_id).cloned().unwrap_or_default();
        let mut fields: Vec<FieldInfo> = Vec::new();
        let zero_init;
        if !td.fields.is_empty() {
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
                });
            }
        } else {
            zero_init = false;
            for p in &td.params {
                let ty = resolve_ty(&p.ty, &types_info, &name_map)?;
                if !matches!(ty, Ty::Int64 | Ty::Bool) {
                    return Err(Error::at(
                        "constructor-param fields must be Int64 or Bool",
                        p.ty.span,
                    ));
                }
                fields.push(FieldInfo {
                    name: p.name.clone(),
                    ty,
                    mutable: false,
                });
            }
        }
        let info = types_info.get_mut(mangled).unwrap();
        info.fields = fields.clone();
        info.zero_init = zero_init;
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
        });
    }

    // Methods
    for (mod_id, mangled, td) in &type_defs {
        let name_map = per_mod_types.get(mod_id).cloned().unwrap_or_default();
        for m in &td.methods {
            let ret = match m.return_type.as_ref() {
                Some(t) => resolve_ty(t, &types_info, &name_map)?,
                None => Ty::Void,
            };
            let params = m
                .params
                .iter()
                .map(|p| Ok((p.name.clone(), resolve_ty(&p.ty, &types_info, &name_map)?)))
                .collect::<Result<Vec<_>>>()?;
            types_info.get_mut(mangled).unwrap().methods.insert(
                m.name.clone(),
                MethodSig {
                    params,
                    ret,
                    exported: m.exported,
                },
            );
        }
    }

    // ---------- Phase 2: functions + externs ----------
    let mut fns: HashMap<String, FnSig> = HashMap::new();
    let mut extern_decls: Vec<ExternDecl> = Vec::new();
    // (module, mangled, &Function, is_entry)
    let mut free_fns: Vec<(String, String, &ast::Function, bool)> = Vec::new();

    for m in &package.modules {
        let name_map = per_mod_types.get(&m.id).cloned().unwrap_or_default();
        for item in &m.ast.items {
            match item {
                Item::Extern(e) => {
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
                    for p in &e.params {
                        let ty = resolve_ty(&p.ty, &types_info, &name_map)?;
                        if matches!(ty, Ty::Void) || !is_ffi_ty(&ty) {
                            return Err(Error::at(
                                format!("type {ty:?} is not FFI-marshalable as a parameter"),
                                p.ty.span,
                            ));
                        }
                        params.push((p.name.clone(), ty));
                    }
                    let c_symbol = e.c_symbol.clone().unwrap_or_else(|| e.name.clone());
                    let mangled = mangle_fn(&m.id, &e.name, m.is_entry);
                    if fns
                        .insert(
                            mangled.clone(),
                            FnSig {
                                params: params.clone(),
                                ret: ret.clone(),
                                extern_symbol: Some(c_symbol.clone()),
                                mangled: mangled.clone(),
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
                Item::Function(f) => {
                    let mangled = if m.is_entry && f.name == "main" {
                        "main".to_string()
                    } else {
                        mangle_fn(&m.id, &f.name, m.is_entry)
                    };
                    let ret = resolve_return_ty(f, m.is_entry, &types_info, &name_map)?;
                    let params = f
                        .params
                        .iter()
                        .map(|p| {
                            Ok((p.name.clone(), resolve_ty(&p.ty, &types_info, &name_map)?))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if fns
                        .insert(
                            mangled.clone(),
                            FnSig {
                                params,
                                ret,
                                extern_symbol: None,
                                mangled: mangled.clone(),
                            },
                        )
                        .is_some()
                    {
                        return Err(Error::at(
                            format!("duplicate function `{}`", f.name),
                            f.name_span,
                        ));
                    }
                    if f.exported {
                        module_exports
                            .get_mut(&m.id)
                            .unwrap()
                            .fns
                            .insert(f.name.clone(), mangled.clone());
                    }
                    free_fns.push((m.id.clone(), mangled, f, m.is_entry));
                }
                _ => {}
            }
        }
    }

    if !fns.contains_key("main") {
        return Err(Error::new("program requires `fn main`"));
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

    for m in &package.modules {
        let (name_to_type, name_to_fn, name_to_global, namespaces) =
            build_module_name_maps(m, &import_maps, &module_exports, &all_globals, &per_mod_types, &fns)?;

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
                Ty::Void,
                m.id.clone(),
                m.is_entry,
            );
            let (place, ty) = lower_expr(&mut ctx, &g.init)?;
            // Move init value into global place.
            ctx.assign(
                Place {
                    name: place_name.clone(),
                },
                ty.clone(),
                RValue::Use(place),
            );
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
    }

    // ---------- Phase 4: lower methods + free functions ----------
    let mut functions = Vec::new();

    for (mod_id, mangled_ty, td) in &type_defs {
        let m = package
            .modules
            .iter()
            .find(|x| x.id == *mod_id)
            .unwrap();
        let (name_to_type, name_to_fn, name_to_global, namespaces) =
            build_module_name_maps(m, &import_maps, &module_exports, &all_globals, &per_mod_types, &fns)?;
        for method in &td.methods {
            functions.push(lower_method(
                mangled_ty,
                td,
                method,
                &fns,
                &types_info,
                &name_to_type,
                &name_to_fn,
                &name_to_global,
                &namespaces,
                &module_exports,
                mod_id,
                m.is_entry,
            )?);
        }
    }

    for (mod_id, mangled, f, is_entry) in &free_fns {
        let m = package
            .modules
            .iter()
            .find(|x| x.id == *mod_id)
            .unwrap();
        let (name_to_type, name_to_fn, name_to_global, namespaces) =
            build_module_name_maps(m, &import_maps, &module_exports, &all_globals, &per_mod_types, &fns)?;
        functions.push(lower_function(
            f,
            mangled,
            *is_entry,
            &fns,
            &types_info,
            &name_to_type,
            &name_to_fn,
            &name_to_global,
            &namespaces,
            &module_exports,
            mod_id,
        )?);
    }

    Ok(Program {
        types: type_layouts,
        externs: extern_decls,
        globals: program_globals,
        init: init_stmts,
        functions,
    })
}

fn mangle_type(mod_id: &str, name: &str, is_entry: bool) -> String {
    if is_entry {
        name.to_string()
    } else {
        format!("{mod_id}_{name}")
    }
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
            _ => {}
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
        None if is_main => Ok(Ty::Int64),
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

fn resolve_ty(
    t: &TypeExpr,
    types: &HashMap<String, TypeInfo>,
    name_map: &HashMap<String, String>,
) -> Result<Ty> {
    match (t.name.as_str(), t.args.as_slice()) {
        ("Int32", []) => Ok(Ty::Int32),
        ("Int64", []) => Ok(Ty::Int64),
        ("UInt32", []) => Ok(Ty::UInt32),
        ("UInt64", []) => Ok(Ty::UInt64),
        ("Byte" | "UInt8", []) => Ok(Ty::Byte),
        ("Bool", []) => Ok(Ty::Bool),
        ("Void", []) => Ok(Ty::Void),
        ("String", []) => Ok(Ty::String),
        ("Bytes", []) => Ok(Ty::Bytes),
        ("Trial", [TypeArg::Type(a), TypeArg::Type(b)]) => {
            let ok = resolve_ty(a, types, name_map)?;
            let err = resolve_ty(b, types, name_map)?;
            if !is_trial_payload_ty(&ok) {
                return Err(Error::at(
                    format!("Trial ok type is not supported: {ok:?}"),
                    t.span,
                ));
            }
            if !is_trial_payload_ty(&err) {
                return Err(Error::at(
                    format!("Trial err type is not supported: {err:?}"),
                    t.span,
                ));
            }
            Ok(Ty::Trial {
                ok: Box::new(ok),
                err: Box::new(err),
            })
        }
        ("Trial", _) => Err(Error::at("Trial expects two type arguments", t.span)),
        ("Array", [TypeArg::Type(elem), TypeArg::Int { value: n, span: nspan }]) => {
            if *n < 0 {
                return Err(Error::at("Array length must be non-negative", *nspan));
            }
            let et = resolve_ty(elem, types, name_map)?;
            if !is_abi_scalar(&et) {
                return Err(Error::at(
                    format!("Array element must be an ABI scalar, got {et:?}"),
                    elem.span,
                ));
            }
            Ok(Ty::FixedArray {
                elem: Box::new(et),
                len: *n as u64,
            })
        }
        ("Array", _) => Err(Error::at(
            "Array expects element type and const length: Array<T, N>",
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
                Err(Error::at(format!("unknown type `{name}`"), t.span))
            }
        }
        (other, args) if !args.is_empty() => Err(Error::at(
            format!("unsupported generic type `{other}`"),
            t.span,
        )),
        (other, _) => Err(Error::at(format!("unknown type `{other}`"), t.span)),
    }
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
    module_id: &str,
) -> Result<Function> {
    let is_main = is_entry && f.name == "main";
    let ret_ty = resolve_return_ty(f, is_entry, types, name_to_type)?;
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
        ret_ty.clone(),
        module_id.to_string(),
        is_entry,
    );
    for p in &f.params {
        let ty = resolve_ty(&p.ty, types, name_to_type)?;
        let place = Place {
            name: sanitize_name(&p.name),
        };
        ctx.define(&p.name, ty, false, place);
    }

    let ret = lower_block_value(&mut ctx, &f.body, &ret_ty)?;
    insert_function_drops(&mut ctx.stmts, &ret, &ret_ty);

    Ok(Function {
        name: mangled.to_string(),
        is_main,
        receiver: None,
        params,
        body: ctx.stmts,
        ret,
        ret_ty,
    })
}

fn lower_method(
    mangled_type: &str,
    _td: &TypeDef,
    m: &Method,
    fns: &HashMap<String, FnSig>,
    types: &HashMap<String, TypeInfo>,
    name_to_type: &HashMap<String, String>,
    name_to_fn: &HashMap<String, String>,
    name_to_global: &HashMap<String, GlobalInfo>,
    namespaces: &HashMap<String, String>,
    module_exports: &HashMap<String, ModuleExports>,
    module_id: &str,
    is_entry: bool,
) -> Result<Function> {
    let ret_ty = match m.return_type.as_ref() {
        Some(t) => resolve_ty(t, types, name_to_type)?,
        None => Ty::Void,
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
        ret_ty.clone(),
        module_id.to_string(),
        is_entry,
    );

    // Bind fields as bare names from receiver (no `self` in Thoxa).
    let info = types.get(mangled_type).unwrap();
    for f in &info.fields {
        if matches!(f.ty, Ty::FixedArray { .. }) {
            continue;
        }
        let p = Place {
            name: sanitize_name(&f.name),
        };
        ctx.assign(
            p.clone(),
            f.ty.clone(),
            RValue::Field {
                base: Place {
                    name: recv_place.clone(),
                },
                field: sanitize_field(&f.name),
            },
        );
        ctx.define(&f.name, f.ty.clone(), false, p);
    }
    for p in &m.params {
        let ty = resolve_ty(&p.ty, types, name_to_type)?;
        let place = Place {
            name: sanitize_name(&p.name),
        };
        ctx.define(&p.name, ty, false, place);
    }

    let ret = lower_block_value(&mut ctx, &m.body, &ret_ty)?;
    insert_function_drops(&mut ctx.stmts, &ret, &ret_ty);

    Ok(Function {
        name: format!("{mangled_type}__{}", m.name),
        is_main: false,
        receiver: Some((recv_place, recv_ty)),
        params,
        body: ctx.stmts,
        ret,
        ret_ty,
    })
}

/// Drop every heap / Trial-with-heap local at function end except the returned place (moved out).
fn insert_function_drops(stmts: &mut Vec<Stmt>, ret: &Option<Place>, ret_ty: &Ty) {
    let mut places: Vec<(String, Ty)> = Vec::new();
    collect_drop_places(stmts, &mut places);
    let skip = if needs_drop(ret_ty) || is_trial(ret_ty) {
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
            _ => {}
        }
    }
}

fn lower_block_value(ctx: &mut LowerCtx, block: &Block, expect: &Ty) -> Result<Option<Place>> {
    ctx.push_scope();
    lower_block_items(ctx, &block.items)?;

    let result = if let Some(tail) = &block.tail {
        let (place, ty) = lower_expr(ctx, tail)?;
        if *expect != Ty::Void && &ty != expect {
            return Err(Error::at(
                format!("expected {expect:?}, found {ty:?}"),
                tail.span,
            ));
        }
        Some(place)
    } else if *expect == Ty::Void {
        None
    } else {
        return Err(Error::at("missing block value", block.span));
    };
    ctx.pop_scope();
    Ok(result)
}

fn lower_block_items(ctx: &mut LowerCtx, items: &[BlockItem]) -> Result<()> {
    for item in items {
        match item {
            BlockItem::Let {
                name,
                name_span: _,
                mutable,
                init,
            } => {
                let (place, ty) = lower_expr(ctx, init)?;
                ctx.bind_name(name, ty, *mutable, place);
            }
            BlockItem::Assign { target, value } => {
                lower_assign(ctx, target, value)?;
            }
            BlockItem::Expr(e) => {
                let _ = lower_expr(ctx, e)?;
            }
        }
    }
    Ok(())
}

fn lower_assign(ctx: &mut LowerCtx, target: &AssignTarget, value: &Expr) -> Result<()> {
    match target {
        AssignTarget::Name { name, name_span } => {
            ctx.reassign_name(name, *name_span, value)
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
            if matches!(finfo.ty, Ty::FixedArray { .. }) {
                return Err(Error::at(
                    "cannot assign whole fixed array field (assign elements with [i])",
                    *field_span,
                ));
            }
            let field_ty = finfo.ty.clone();
            let (v, vty) = lower_expr(ctx, value)?;
            let v = coerce_to_field(ctx, v, &vty, &field_ty, value.span)?;
            ctx.stmts.push(Stmt::FieldAssign {
                base: binding.place,
                field: sanitize_field(field),
                ty: field_ty,
                value: v,
            });
            Ok(())
        }
        AssignTarget::Index {
            base,
            base_span,
            field,
            field_span,
            index,
        } => {
            let binding = ctx
                .lookup(base)
                .ok_or_else(|| Error::at(format!("undefined name `{base}`"), *base_span))?;
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
            });
            Ok(())
        }
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
    if is_int(from) && is_int(to) {
        return coerce_place(ctx, place, from, to, span);
    }
    Err(Error::at(
        format!("cannot store {from:?} into field of type {to:?}"),
        span,
    ))
}

fn lower_expr(ctx: &mut LowerCtx, expr: &Expr) -> Result<(Place, Ty)> {
    match &expr.kind {
        ExprKind::Int(n) => {
            let p = ctx.fresh("i");
            ctx.assign(p.clone(), Ty::Int64, RValue::Int(*n));
            Ok((p, Ty::Int64))
        }
        ExprKind::Bool(b) => {
            let p = ctx.fresh("b");
            ctx.assign(p.clone(), Ty::Bool, RValue::Bool(*b));
            Ok((p, Ty::Bool))
        }
        ExprKind::String(s) => {
            let p = ctx.fresh("s");
            ctx.assign(p.clone(), Ty::String, RValue::StringLit(s.clone()));
            Ok((p, Ty::String))
        }
        ExprKind::Interpolate(parts) => lower_interpolate(ctx, parts, expr.span),
        ExprKind::Name(name) => {
            // Unit value `Void` (type name used as value).
            if name == "Void" && ctx.lookup(name).is_none() {
                let p = ctx.fresh("void");
                return Ok((p, Ty::Void));
            }
            let binding = ctx
                .lookup(name)
                .ok_or_else(|| Error::at(format!("undefined name `{name}`"), expr.span))?;
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
        }
        ExprKind::Unary { op, expr: inner } => {
            let (arg, arg_ty) = lower_expr(ctx, inner)?;
            match op {
                UnaryOp::Neg => {
                    if !is_int(&arg_ty) {
                        return Err(Error::at("unary `-` expects an integer", expr.span));
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
                    ctx.assign(
                        p.clone(),
                        Ty::Bool,
                        RValue::Unary { op: *op, arg },
                    );
                    Ok((p, Ty::Bool))
                }
                UnaryOp::BitNot => {
                    if !is_int(&arg_ty) {
                        return Err(Error::at("bitwise `~` expects an integer", expr.span));
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
        ExprKind::Call { callee, args } => lower_call(ctx, callee, args, expr.span),
        ExprKind::Field {
            base,
            name,
            name_span,
        } => lower_field(ctx, base, name, *name_span),
        ExprKind::Index { base, index } => lower_index(ctx, base, index, expr.span),
        ExprKind::MethodCall {
            receiver,
            method,
            method_span,
            args,
        } => lower_method_call(ctx, receiver, method, *method_span, args),
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => lower_if(ctx, cond, then_branch, else_branch.as_ref(), expr.span),
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
        ExprKind::Match { scrutinee, arms } => lower_match(ctx, scrutinee, arms, expr.span),
    }
}

fn lower_interpolate(
    ctx: &mut LowerCtx,
    parts: &[InterpPart],
    span: Span,
) -> Result<(Place, Ty)> {
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
                match ty {
                    Ty::String => v,
                    Ty::Int64 | Ty::Bool => {
                        let p = ctx.fresh("ts");
                        ctx.assign(
                            p.clone(),
                            Ty::String,
                            RValue::ToString {
                                value: v,
                                from: ty,
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
                    other => {
                        return Err(Error::at(
                            format!("cannot interpolate {other:?} into String"),
                            span,
                        ));
                    }
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
                    RValue::StringConcat {
                        left,
                        right: piece,
                    },
                );
                p
            }
        });
    }
    Ok((acc.unwrap(), Ty::String))
}

fn lower_field(
    ctx: &mut LowerCtx,
    base: &Expr,
    name: &str,
    name_span: Span,
) -> Result<(Place, Ty)> {
    // Namespace member: `geometry.origin` (module-level let).
    if let ExprKind::Name(ns) = &base.kind {
        if let Some(mod_id) = ctx.namespaces.get(ns).cloned() {
            let exp = ctx.module_exports.get(&mod_id).cloned().unwrap_or_default();
            if let Some(info) = exp.globals.get(name) {
                return Ok((info.place.clone(), info.ty.clone()));
            }
            return Err(Error::at(
                format!("no exported value `{name}` on namespace `{ns}`"),
                name_span,
            ));
        }
    }

    let (base_p, base_ty) = lower_expr(ctx, base)?;
    match &base_ty {
        Ty::Bytes => {
            if name != "count" {
                return Err(Error::at(
                    format!("no field `{name}` on Bytes (use .count, .get, .set, .slice, .toString)"),
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
        Ty::User(type_name) => {
            let info = ctx
                .types
                .get(type_name)
                .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), name_span))?;
            let field_ty = info
                .fields
                .iter()
                .find(|f| f.name == name)
                .map(|f| f.ty.clone())
                .ok_or_else(|| {
                    Error::at(format!("no field `{name}` on `{type_name}`"), name_span)
                })?;
            if matches!(field_ty, Ty::FixedArray { .. }) {
                return Err(Error::at(
                    format!(
                        "fixed array field `{name}` must be indexed (e.g. `.{name}[i]`)"
                    ),
                    name_span,
                ));
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
            Ok((p, field_ty))
        }
        other => Err(Error::at(
            format!("field access on non-struct type {other:?}"),
            name_span,
        )),
    }
}

fn lower_index(
    ctx: &mut LowerCtx,
    base: &Expr,
    index: &Expr,
    span: Span,
) -> Result<(Place, Ty)> {
    // Expect `struct.field[i]` — peel Field so we don't try to load the whole array.
    let (struct_expr, field_name, field_span) = match &base.kind {
        ExprKind::Field {
            base: s,
            name,
            name_span,
        } => (s.as_ref(), name.as_str(), *name_span),
        _ => {
            return Err(Error::at(
                "indexing is only supported on fixed-array fields (`s.field[i]`)",
                span,
            ));
        }
    };
    let (base_p, base_ty) = lower_expr(ctx, struct_expr)?;
    let type_name = match &base_ty {
        Ty::User(n) => n.clone(),
        other => {
            return Err(Error::at(
                format!("cannot index field on non-struct type {other:?}"),
                field_span,
            ));
        }
    };
    let info = ctx
        .types
        .get(&type_name)
        .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), field_span))?
        .clone();
    let finfo = info
        .fields
        .iter()
        .find(|f| f.name == field_name)
        .ok_or_else(|| {
            Error::at(
                format!("no field `{field_name}` on `{type_name}`"),
                field_span,
            )
        })?;
    let (elem_ty, len) = match &finfo.ty {
        Ty::FixedArray { elem, len } => (elem.as_ref().clone(), *len),
        other => {
            return Err(Error::at(
                format!("cannot index non-array field type {other:?}"),
                field_span,
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
            base: base_p,
            field: sanitize_field(field_name),
            index: idx_p,
        },
    );
    Ok((p, elem_ty))
}

fn lower_method_call(
    ctx: &mut LowerCtx,
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[Arg],
) -> Result<(Place, Ty)> {
    // Static methods: `Bytes.fromString(…)`
    if let ExprKind::Name(type_name) = &receiver.kind {
        if type_name == "Bytes" && ctx.lookup(type_name).is_none() {
            return lower_bytes_static(ctx, method, args, method_span);
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
                    let field_places = resolve_call_args(ctx, &formal, args, method_span)?;
                    ctx.assign(
                        p.clone(),
                        ty.clone(),
                        RValue::StructLit {
                            type_name: mangled_ty.clone(),
                            fields: field_places,
                        },
                    );
                }
                return Ok((p, ty));
            }
            // Free function
            if let Some(mangled_fn) = exp.fns.get(method) {
                let sig = ctx.fns.get(mangled_fn).unwrap().clone();
                let arg_places = resolve_call_args(ctx, &sig.params, args, method_span)?;
                let callee = if let Some(sym) = &sig.extern_symbol {
                    Callee::Extern {
                        c_symbol: sym.clone(),
                        arg_tys: sig.params.iter().map(|(_, t)| t.clone()).collect(),
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

    let (recv_p, recv_ty) = lower_expr(ctx, receiver)?;
    match &recv_ty {
        Ty::Bytes => lower_bytes_method(ctx, recv_p, method, args, method_span),
        Ty::User(type_name) => {
            let type_name = type_name.clone();
            let info = ctx
                .types
                .get(&type_name)
                .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), method_span))?
                .clone();
            let sig = info.methods.get(method).ok_or_else(|| {
                Error::at(
                    format!("no method `{method}` on `{type_name}`"),
                    method_span,
                )
            })?;
            if !sig.exported {
                return Err(Error::at(
                    format!("method `{method}` is private"),
                    method_span,
                ));
            }
            let arg_places = resolve_call_args(ctx, &sig.params, args, method_span)?;
            let mut all_args = vec![recv_p];
            all_args.extend(arg_places);
            emit_call_result(
                ctx,
                Callee::Method {
                    type_name: type_name.clone(),
                    method: method.to_string(),
                },
                all_args,
                sig.ret.clone(),
            )
        }
        other => Err(Error::at(
            format!("method call on non-struct type {other:?}"),
            method_span,
        )),
    }
}

fn lower_bytes_static(
    ctx: &mut LowerCtx,
    method: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    match method {
        "fromString" => {
            let formal = vec![("s".into(), Ty::String)];
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
    let formal = vec![("n".into(), Ty::Int64)];
    let places = resolve_call_args(ctx, &formal, args, span)?;
    emit_call_result(
        ctx,
        Callee::Builtin("bytes_new".into()),
        places,
        Ty::Bytes,
    )
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
            let formal = vec![("i".into(), Ty::Int64)];
            let places = resolve_call_args(ctx, &formal, args, span)?;
            let mut all = vec![recv];
            all.extend(places);
            emit_call_result(ctx, Callee::Builtin("bytes_get".into()), all, Ty::Int64)
        }
        "set" => {
            let formal = vec![("i".into(), Ty::Int64), ("b".into(), Ty::Int64)];
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
            // `find(needle)` or `find(needle, from)` — index or -1.
            let (needle_args, from_default) = match args.len() {
                1 => (args, true),
                2 => (args, false),
                _ => {
                    return Err(Error::at(
                        "Bytes.find expects 1 or 2 arguments (needle[, from])",
                        span,
                    ));
                }
            };
            let formal = if from_default {
                vec![("needle".into(), Ty::Bytes)]
            } else {
                vec![
                    ("needle".into(), Ty::Bytes),
                    ("from".into(), Ty::Int64),
                ]
            };
            let places = resolve_call_args(ctx, &formal, needle_args, span)?;
            let mut all = vec![recv];
            all.extend(places);
            if from_default {
                let z = ctx.fresh("from");
                ctx.assign(z.clone(), Ty::Int64, RValue::Int(0));
                all.push(z);
            }
            emit_call_result(ctx, Callee::Builtin("bytes_find".into()), all, Ty::Int64)
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
        other => Err(Error::at(
            format!("no method `{other}` on Bytes"),
            span,
        )),
    }
}

/// Emit a call that either assigns a result or is evaluated for side effects (Void).
fn emit_call_result(
    ctx: &mut LowerCtx,
    callee: Callee,
    args: Vec<Place>,
    ret: Ty,
) -> Result<(Place, Ty)> {
    if ret == Ty::Void {
        ctx.stmts.push(Stmt::Eval(RValue::Call { callee, args }));
        let p = ctx.fresh("u");
        ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
        Ok((p, Ty::Void))
    } else {
        let p = ctx.fresh("mcall");
        ctx.assign(
            p.clone(),
            ret.clone(),
            RValue::Call { callee, args },
        );
        Ok((p, ret))
    }
}

fn resolve_call_args(
    ctx: &mut LowerCtx,
    formal: &[(String, Ty)],
    args: &[Arg],
    span: Span,
) -> Result<Vec<Place>> {
    if args.is_empty() && formal.is_empty() {
        return Ok(Vec::new());
    }

    let all_named = args.iter().all(|a| matches!(a, Arg::Named { .. }));
    let all_pos = args.iter().all(|a| matches!(a, Arg::Pos(_)));

    if !all_named && !all_pos {
        return Err(Error::at(
            "cannot mix positional and named arguments",
            span,
        ));
    }

    if all_pos {
        if args.len() != formal.len() {
            return Err(Error::at(
                format!(
                    "expected {} argument(s), found {}",
                    formal.len(),
                    args.len()
                ),
                span,
            ));
        }
        let mut places = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            let expr = match arg {
                Arg::Pos(e) => e,
                Arg::Named { .. } => unreachable!(),
            };
            let p = lower_arg_for_formal(ctx, expr, &formal[i].1, &formal[i].0, i)?;
            places.push(p);
        }
        return Ok(places);
    }

    // Named: match by name, require all formals present, declaration order for result.
    if args.len() != formal.len() {
        return Err(Error::at(
            format!(
                "expected {} argument(s), found {}",
                formal.len(),
                args.len()
            ),
            span,
        ));
    }
    let mut by_name: HashMap<String, &Expr> = HashMap::new();
    for arg in args {
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
                by_name.insert(name.clone(), value);
            }
            Arg::Pos(_) => unreachable!(),
        }
    }
    let mut places = Vec::new();
    for (i, (fname, fty)) in formal.iter().enumerate() {
        let expr = by_name.remove(fname).ok_or_else(|| {
            Error::at(format!("missing argument `{fname}`"), span)
        })?;
        let p = lower_arg_for_formal(ctx, expr, fty, fname, i)?;
        places.push(p);
    }
    if let Some((extra, _)) = by_name.iter().next() {
        return Err(Error::at(format!("unknown argument `{extra}`"), span));
    }
    Ok(places)
}

/// Lower one call argument against a formal type (int coerce, Address implicit).
fn lower_arg_for_formal(
    ctx: &mut LowerCtx,
    expr: &Expr,
    formal_ty: &Ty,
    formal_name: &str,
    index: usize,
) -> Result<Place> {
    // Implicit address: mut struct binding → Address<T>
    if let Ty::Address(pointee) = formal_ty {
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
        let binding = ctx.lookup(name).ok_or_else(|| {
            Error::at(format!("undefined name `{name}`"), expr.span)
        })?;
        if !binding.mutable {
            return Err(Error::at(
                format!(
                    "argument `{formal_name}`: coercing to Address requires a `mut` binding"
                ),
                expr.span,
            ));
        }
        match (&binding.ty, pointee.as_ref()) {
            (Ty::User(got), Ty::User(want)) if got == want => {
                // Place is the binding itself; cgen emits `&place` for Address args.
                return Ok(binding.place);
            }
            (got, want) => {
                return Err(Error::at(
                    format!(
                        "argument `{formal_name}`: expected Address<{want:?}>, found {got:?}"
                    ),
                    expr.span,
                ));
            }
        }
    }

    let (p, ty) = lower_expr(ctx, expr)?;
    if &ty == formal_ty {
        Ok(p)
    } else if is_int(&ty) && is_int(formal_ty) {
        coerce_place(ctx, p, &ty, formal_ty, expr.span)
    } else {
        Err(Error::at(
            format!(
                "argument {} (`{formal_name}`): expected {formal_ty:?}, found {ty:?}",
                index + 1
            ),
            expr.span,
        ))
    }
}

fn lower_try(ctx: &mut LowerCtx, inner: &Expr, span: Span) -> Result<(Place, Ty)> {
    let (trial_p, trial_ty) = lower_expr(ctx, inner)?;
    let Some((ok_ty, err_ty)) = trial_parts(&trial_ty) else {
        return Err(Error::at("`?` expects a Trial value", span));
    };
    let ok_ty = ok_ty.clone();
    let err_ty = err_ty.clone();
    let fn_ret = ctx.fn_ret.clone();
    let (ret_ok, ret_err) = match &fn_ret {
        Ty::Trial { ok, err } => (ok.as_ref().clone(), err.as_ref().clone()),
        _ => {
            return Err(Error::at(
                "`?` requires the enclosing function to return Trial",
                span,
            ));
        }
    };
    if ret_err != err_ty {
        return Err(Error::at(
            "`?` error type must match the enclosing function's Trial error type",
            span,
        ));
    }

    let tag = ctx.fresh("tag");
    ctx.assign(
        tag.clone(),
        Ty::Int64,
        RValue::TrialTag(trial_p.clone()),
    );
    let zero = ctx.fresh("z");
    ctx.assign(zero.clone(), Ty::Int64, RValue::Int(0));
    let is_fail = ctx.fresh("isf");
    ctx.assign(
        is_fail.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Ne,
            left: tag,
            right: zero,
        },
    );

    // On Fail: return a Trial of the *enclosing* type (re-wrap err if ok monomorph differs).
    let outer = std::mem::take(&mut ctx.stmts);
    let mut then_body = Vec::new();
    if trial_ty == fn_ret {
        then_body.push(Stmt::Return {
            value: trial_p.clone(),
        });
    } else {
        // Extract err, build Fail of fn_ret.
        let err_p = Place {
            name: format!("_qerr{}", ctx.temps),
        };
        ctx.temps += 1;
        if err_ty != Ty::Void {
            then_body.push(Stmt::Assign {
                place: err_p.clone(),
                ty: err_ty.clone(),
                value: RValue::TrialErr(trial_p.clone()),
            });
            if is_heap(&err_ty) {
                then_body.push(Stmt::Dup {
                    place: err_p.clone(),
                    ty: err_ty.clone(),
                });
            }
        }
        let ret_trial = Place {
            name: format!("_qret{}", ctx.temps),
        };
        ctx.temps += 1;
        then_body.push(Stmt::Assign {
            place: ret_trial.clone(),
            ty: fn_ret.clone(),
            value: RValue::TrialMake {
                tag: TrialTag::Fail,
                payload: err_p,
                trial_ty: fn_ret.clone(),
            },
        });
        then_body.push(Stmt::Return { value: ret_trial });
    }
    ctx.stmts = outer;
    ctx.stmts.push(Stmt::If {
        cond: is_fail,
        then_body,
        else_body: Vec::new(),
    });

    let payload = ctx.fresh("ok");
    if ok_ty == Ty::Void {
        // No payload field; leave a dummy place typed Void.
    } else {
        ctx.stmts.push(Stmt::Assign {
            place: payload.clone(),
            ty: ok_ty.clone(),
            value: RValue::TrialOk(trial_p),
        });
        // Extracted payload shares ownership with the trial until fn end; dup if heap.
        if is_heap(&ok_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: payload.clone(),
                ty: ok_ty.clone(),
            });
        }
    }
    let _ = ret_ok;
    Ok((payload, ok_ty))
}

fn lower_match(
    ctx: &mut LowerCtx,
    scrutinee: &Expr,
    arms: &[MatchArm],
    span: Span,
) -> Result<(Place, Ty)> {
    if arms.len() != 2 {
        return Err(Error::at(
            "match on Trial requires exactly Pass and Fail arms",
            span,
        ));
    }

    let mut pass_arm: Option<&MatchArm> = None;
    let mut fail_arm: Option<&MatchArm> = None;
    for arm in arms {
        match &arm.pattern {
            Pattern::Variant { name, .. } if name == "Pass" => {
                if pass_arm.is_some() {
                    return Err(Error::at("duplicate Pass arm", span));
                }
                pass_arm = Some(arm);
            }
            Pattern::Variant { name, .. } if name == "Fail" => {
                if fail_arm.is_some() {
                    return Err(Error::at("duplicate Fail arm", span));
                }
                fail_arm = Some(arm);
            }
            Pattern::Variant { name, span: pspan, .. } => {
                return Err(Error::at(
                    format!("unsupported match pattern `{name}`"),
                    *pspan,
                ));
            }
        }
    }
    let pass_arm = pass_arm.ok_or_else(|| Error::at("match missing Pass arm", span))?;
    let fail_arm = fail_arm.ok_or_else(|| Error::at("match missing Fail arm", span))?;

    let (scrut_p, scrut_ty) = lower_expr(ctx, scrutinee)?;
    let Some((ok_ty, err_ty)) = trial_parts(&scrut_ty) else {
        return Err(Error::at("match only supports Trial", span));
    };
    let ok_ty = ok_ty.clone();
    let err_ty = err_ty.clone();

    let pass_bind = match &pass_arm.pattern {
        Pattern::Variant { bind, .. } => bind.clone(),
    };
    let fail_bind = match &fail_arm.pattern {
        Pattern::Variant { bind, .. } => bind.clone(),
    };

    let pass_ty = {
        ctx.push_scope();
        let pp = Place {
            name: sanitize_name(&pass_bind),
        };
        ctx.define(&pass_bind, ok_ty.clone(), false, pp);
        let ty = infer_block_ty(ctx, &pass_arm.body)?;
        ctx.pop_scope();
        ty
    };
    let fail_ty = {
        ctx.push_scope();
        let fp = Place {
            name: sanitize_name(&fail_bind),
        };
        ctx.define(&fail_bind, err_ty.clone(), false, fp);
        let ty = infer_block_ty(ctx, &fail_arm.body)?;
        ctx.pop_scope();
        ty
    };
    let result_ty = match (&pass_ty, &fail_ty) {
        (Ty::Void, Ty::Void) => Ty::Void,
        (a, b) if a == b => a.clone(),
        (a, b) => {
            return Err(Error::at(
                format!("match arms have different types ({a:?} vs {b:?})"),
                span,
            ));
        }
    };

    let out = if result_ty == Ty::Void {
        None
    } else {
        Some(ctx.fresh("m"))
    };

    let tag = ctx.fresh("mtag");
    ctx.assign(tag.clone(), Ty::Int64, RValue::TrialTag(scrut_p.clone()));
    let zero = ctx.fresh("mz");
    ctx.assign(zero.clone(), Ty::Int64, RValue::Int(0));
    let is_pass = ctx.fresh("isp");
    ctx.assign(
        is_pass.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Eq,
            left: tag,
            right: zero,
        },
    );

    let outer = std::mem::take(&mut ctx.stmts);

    ctx.push_scope();
    let pass_payload = ctx.fresh("pp");
    if ok_ty != Ty::Void {
        ctx.stmts.push(Stmt::Assign {
            place: pass_payload.clone(),
            ty: ok_ty.clone(),
            value: RValue::TrialOk(scrut_p.clone()),
        });
        if is_heap(&ok_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: pass_payload.clone(),
                ty: ok_ty.clone(),
            });
        }
    }
    let pass_place = Place {
        name: sanitize_name(&pass_bind),
    };
    if ok_ty != Ty::Void {
        ctx.assign(pass_place.clone(), ok_ty.clone(), RValue::Use(pass_payload));
    }
    ctx.define(&pass_bind, ok_ty.clone(), false, pass_place);
    let pass_val = lower_block_into(ctx, &pass_arm.body, &result_ty)?;
    ctx.pop_scope();
    let mut then_body = std::mem::take(&mut ctx.stmts);
    if let (Some(out_p), Some(pv)) = (&out, pass_val) {
        // Copy result into out (dup if heap/trial-with-heap).
        if needs_drop(&result_ty) {
            then_body.push(Stmt::Dup {
                place: pv.clone(),
                ty: result_ty.clone(),
            });
        }
        then_body.push(Stmt::Assign {
            place: out_p.clone(),
            ty: result_ty.clone(),
            value: RValue::Use(pv),
        });
    }

    ctx.push_scope();
    let fail_payload = ctx.fresh("fp");
    if err_ty != Ty::Void {
        ctx.stmts.push(Stmt::Assign {
            place: fail_payload.clone(),
            ty: err_ty.clone(),
            value: RValue::TrialErr(scrut_p),
        });
        if is_heap(&err_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: fail_payload.clone(),
                ty: err_ty.clone(),
            });
        }
    }
    let fail_place = Place {
        name: sanitize_name(&fail_bind),
    };
    if err_ty != Ty::Void {
        ctx.assign(fail_place.clone(), err_ty.clone(), RValue::Use(fail_payload));
    }
    ctx.define(&fail_bind, err_ty.clone(), false, fail_place);
    let fail_val = lower_block_into(ctx, &fail_arm.body, &result_ty)?;
    ctx.pop_scope();
    let mut else_body = std::mem::take(&mut ctx.stmts);
    if let (Some(out_p), Some(fv)) = (&out, fail_val) {
        if needs_drop(&result_ty) {
            else_body.push(Stmt::Dup {
                place: fv.clone(),
                ty: result_ty.clone(),
            });
        }
        else_body.push(Stmt::Assign {
            place: out_p.clone(),
            ty: result_ty.clone(),
            value: RValue::Use(fv),
        });
    }

    ctx.stmts = outer;
    if let Some(ref out_p) = out {
        init_place(ctx, out_p, &result_ty)?;
    }
    ctx.stmts.push(Stmt::If {
        cond: is_pass,
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

fn init_place(ctx: &mut LowerCtx, place: &Place, ty: &Ty) -> Result<()> {
    match ty {
        Ty::Int32 | Ty::Int64 | Ty::UInt32 | Ty::UInt64 | Ty::Byte => {
            ctx.assign(place.clone(), ty.clone(), RValue::Int(0))
        }
        Ty::Bool => ctx.assign(place.clone(), ty.clone(), RValue::Bool(false)),
        Ty::String | Ty::Bytes => {
            ctx.stmts.push(Stmt::Assign {
                place: place.clone(),
                ty: ty.clone(),
                value: RValue::Null,
            });
        }
        Ty::Void => {}
        Ty::FixedArray { .. } | Ty::Address(_) => {
            return Err(Error::new(format!(
                "cannot allocate local of type {ty:?}"
            )));
        }
        Ty::Trial { ok, .. } => {
            let z = ctx.fresh("tz");
            init_place(ctx, &z, ok)?;
            ctx.assign(
                place.clone(),
                ty.clone(),
                RValue::TrialMake {
                    tag: TrialTag::Pass,
                    payload: z,
                    trial_ty: ty.clone(),
                },
            );
        }
        Ty::User(name) => {
            let info = ctx
                .types
                .get(name)
                .ok_or_else(|| Error::new(format!("unknown type `{name}`")))?
                .clone();
            if info.zero_init {
                ctx.assign(
                    place.clone(),
                    ty.clone(),
                    RValue::StructZero {
                        type_name: name.clone(),
                    },
                );
            } else {
                let mut fields = Vec::new();
                for f in &info.fields {
                    let fp = ctx.fresh("zf");
                    match &f.ty {
                        Ty::Int64 => ctx.assign(fp.clone(), Ty::Int64, RValue::Int(0)),
                        Ty::Bool => ctx.assign(fp.clone(), Ty::Bool, RValue::Bool(false)),
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
    let (l, lt) = lower_expr(ctx, left)?;
    let (r, rt) = lower_expr(ctx, right)?;
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            if !is_int(&lt) || !is_int(&rt) {
                return Err(Error::at(
                    format!("operator {op:?} expects integer operands"),
                    span,
                ));
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
            ctx.assign(
                p.clone(),
                Ty::Int64,
                RValue::Binary { op, left, right },
            );
            Ok((p, Ty::Int64))
        }
        BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr => {
            if !is_int(&lt) || !is_int(&rt) {
                return Err(Error::at(
                    format!("bitwise operator {op:?} expects integer operands"),
                    span,
                ));
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
            ctx.assign(
                p.clone(),
                Ty::Int64,
                RValue::Binary { op, left, right },
            );
            Ok((p, Ty::Int64))
        }
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
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
                ctx.assign(
                    p.clone(),
                    Ty::Bool,
                    RValue::Binary {
                        op,
                        left,
                        right,
                    },
                );
                return Ok((p, Ty::Bool));
            }
            Err(Error::at(
                "comparison operands must have the same type",
                span,
            ))
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

fn lower_pass_fail(
    ctx: &mut LowerCtx,
    callee: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    if args.len() != 1 {
        return Err(Error::at(
            format!("`{callee}` expects 1 argument"),
            span,
        ));
    }
    let expr = match &args[0] {
        Arg::Pos(e) => e,
        Arg::Named { value, .. } => value,
    };
    let (payload_raw, pty) = lower_expr(ctx, expr)?;

    // Prefer the enclosing function's Trial return type when present.
    if let Ty::Trial { ok, err } = ctx.fn_ret.clone() {
        let expected = if callee == "Pass" {
            ok.as_ref().clone()
        } else {
            err.as_ref().clone()
        };
        let payload = coerce_payload(ctx, payload_raw, &pty, &expected, span)?;
        // Heap payload: bump so Trial and the temp both own (temp dropped at fn end).
        if is_heap(&expected) {
            ctx.stmts.push(Stmt::Dup {
                place: payload.clone(),
                ty: expected.clone(),
            });
        }
        let tag = if callee == "Pass" {
            TrialTag::Pass
        } else {
            TrialTag::Fail
        };
        let p = ctx.fresh("tr");
        let ty = ctx.fn_ret.clone();
        ctx.assign(
            p.clone(),
            ty.clone(),
            RValue::TrialMake {
                tag,
                payload,
                trial_ty: ty.clone(),
            },
        );
        return Ok((p, ty));
    }

    // Infer: Pass/Fail(Int) → Trial<Int64, Int64>; Fail(String) alone not enough.
    let payload = if pty == Ty::Int64 {
        payload_raw
    } else if is_int(&pty) {
        coerce_place(ctx, payload_raw, &pty, &Ty::Int64, span)?
    } else if pty == Ty::String && callee == "Fail" {
        return Err(Error::at(
            "`Fail(String)` requires an enclosing `-> Trial<…, String>` (or annotate the function)",
            span,
        ));
    } else if pty == Ty::Void && callee == "Pass" {
        return Err(Error::at(
            "`Pass(Void)` requires an enclosing `-> Trial<Void, …>`",
            span,
        ));
    } else {
        return Err(Error::at(
            format!("`{callee}` payload type {pty:?} needs an annotated Trial return type"),
            span,
        ));
    };
    let tag = if callee == "Pass" {
        TrialTag::Pass
    } else {
        TrialTag::Fail
    };
    let ty = Ty::Trial {
        ok: Box::new(Ty::Int64),
        err: Box::new(Ty::Int64),
    };
    let p = ctx.fresh("tr");
    ctx.assign(
        p.clone(),
        ty.clone(),
        RValue::TrialMake {
            tag,
            payload,
            trial_ty: ty.clone(),
        },
    );
    Ok((p, ty))
}

/// Coerce a Pass/Fail payload into the formal ok/err type.
fn coerce_payload(
    ctx: &mut LowerCtx,
    place: Place,
    from: &Ty,
    to: &Ty,
    span: Span,
) -> Result<Place> {
    if from == to {
        return Ok(place);
    }
    if is_int(from) && is_int(to) {
        return coerce_place(ctx, place, from, to, span);
    }
    if *to == Ty::Void && *from == Ty::Void {
        return Ok(place);
    }
    Err(Error::at(
        format!("Trial payload: expected {to:?}, found {from:?}"),
        span,
    ))
}

fn lower_call(
    ctx: &mut LowerCtx,
    callee: &str,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    match callee {
        "println" | "print" => {
            if args.len() != 1 {
                return Err(Error::at(format!("`{callee}` expects 1 argument"), span));
            }
            let expr = match &args[0] {
                Arg::Pos(e) => e,
                Arg::Named { value, .. } => value,
            };
            let (arg, ty) = lower_expr(ctx, expr)?;
            let (print_arg, print_ty) = match &ty {
                Ty::String | Ty::Int64 | Ty::Bool => (arg, ty),
                t if is_int(t) => {
                    let widened = coerce_place(ctx, arg, t, &Ty::Int64, span)?;
                    (widened, Ty::Int64)
                }
                other => {
                    return Err(Error::at(
                        format!("`{callee}` cannot print {other:?}"),
                        span,
                    ));
                }
            };
            let builtin = match (callee, &print_ty) {
                ("println", Ty::String) => "println_string",
                ("print", Ty::String) => "print_string",
                ("println", Ty::Int64) => "println_i64",
                ("print", Ty::Int64) => "print_i64",
                ("println", Ty::Bool) => "println_bool",
                ("print", Ty::Bool) => "print_bool",
                _ => {
                    return Err(Error::at(
                        format!("`{callee}` cannot print {print_ty:?}"),
                        span,
                    ));
                }
            };
            ctx.stmts.push(Stmt::Eval(RValue::Call {
                callee: Callee::Builtin(builtin.into()),
                args: vec![print_arg],
            }));
            let p = ctx.fresh("u");
            ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
            Ok((p, Ty::Void))
        }
        "Pass" | "Fail" => lower_pass_fail(ctx, callee, args, span),
        "Bytes" => {
            // `Bytes(n)` — zero-filled buffer (R9 / D18). Not a user type.
            lower_bytes_new(ctx, args, span)
        }
        other => {
            // Type constructor via local or imported name.
            if let Some(mangled_ty) = ctx.resolve_type_name(other) {
                let info = ctx.types.get(&mangled_ty).unwrap().clone();
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
                    let field_places = resolve_call_args(ctx, &formal, args, span)?;
                    ctx.assign(
                        p.clone(),
                        ty.clone(),
                        RValue::StructLit {
                            type_name: mangled_ty,
                            fields: field_places,
                        },
                    );
                }
                return Ok((p, ty));
            }

            // Free function via local or imported name.
            let mangled = ctx
                .name_to_fn
                .get(other)
                .cloned()
                .unwrap_or_else(|| other.to_string());
            let sig = ctx
                .fns
                .get(&mangled)
                .ok_or_else(|| Error::at(format!("unknown function `{other}`"), span))?
                .clone();
            let arg_places = resolve_call_args(ctx, &sig.params, args, span)?;
            let callee = if let Some(sym) = &sig.extern_symbol {
                Callee::Extern {
                    c_symbol: sym.clone(),
                    arg_tys: sig.params.iter().map(|(_, t)| t.clone()).collect(),
                }
            } else {
                Callee::User(sig.mangled.clone())
            };
            emit_call_result(ctx, callee, arg_places, sig.ret)
        }
    }
}

/// Coerce a place to a target type when a conversion is defined (FFI integer widths).
fn coerce_place(
    ctx: &mut LowerCtx,
    place: Place,
    from: &Ty,
    to: &Ty,
    span: Span,
) -> Result<Place> {
    if from == to {
        return Ok(place);
    }
    let ok = is_int(from) && is_int(to);
    if !ok {
        return Err(Error::at(
            format!("cannot coerce {from:?} to {to:?}"),
            span,
        ));
    }
    let p = ctx.fresh("cast");
    ctx.assign(
        p.clone(),
        to.clone(),
        RValue::Cast {
            value: place,
            from: from.clone(),
            to: to.clone(),
        },
    );
    Ok(p)
}

fn lower_if(
    ctx: &mut LowerCtx,
    cond: &Expr,
    then_branch: &Block,
    else_branch: Option<&Block>,
    span: Span,
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

    let result_ty = match (&then_ty, &else_ty) {
        (Ty::Void, Ty::Void) => Ty::Void,
        (a, Ty::Void) if else_branch.is_none() => a.clone(),
        (a, b) if a == b => a.clone(),
        (a, b) => {
            return Err(Error::at(
                format!("if branches have different types ({a:?} vs {b:?})"),
                span,
            ));
        }
    };

    let out = if result_ty == Ty::Void {
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
        if needs_drop(&result_ty) {
            then_body.push(Stmt::Dup {
                place: tp.clone(),
                ty: result_ty.clone(),
            });
        }
        then_body.push(Stmt::Assign {
            place: out_p.clone(),
            ty: result_ty.clone(),
            value: RValue::Use(tp),
        });
    }

    let else_body = if let Some(eb) = else_branch {
        ctx.push_scope();
        let else_place = lower_block_into(ctx, eb, &result_ty)?;
        ctx.pop_scope();
        let mut body = std::mem::take(&mut ctx.stmts);
        if let (Some(out_p), Some(ep)) = (&out, else_place) {
            if needs_drop(&result_ty) {
                body.push(Stmt::Dup {
                    place: ep.clone(),
                    ty: result_ty.clone(),
                });
            }
            body.push(Stmt::Assign {
                place: out_p.clone(),
                ty: result_ty.clone(),
                value: RValue::Use(ep),
            });
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

fn lower_block_into(
    ctx: &mut LowerCtx,
    block: &Block,
    expect: &Ty,
) -> Result<Option<Place>> {
    lower_block_items(ctx, &block.items)?;
    if let Some(tail) = &block.tail {
        let (place, ty) = lower_expr(ctx, tail)?;
        if *expect != Ty::Void && &ty != expect {
            return Err(Error::at(
                format!("expected {expect:?}, found {ty:?}"),
                tail.span,
            ));
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
        fns: ctx.fns.clone(),
        types: ctx.types.clone(),
        name_to_type: ctx.name_to_type.clone(),
        name_to_fn: ctx.name_to_fn.clone(),
        name_to_global: ctx.name_to_global.clone(),
        namespaces: ctx.namespaces.clone(),
        module_exports: ctx.module_exports.clone(),
        fn_ret: ctx.fn_ret.clone(),
        module_id: ctx.module_id.clone(),
        is_entry: ctx.is_entry,
    };
    if block.tail.is_none() {
        return Ok(Ty::Void);
    }
    lower_block_items(&mut tmp, &block.items)?;
    let (_, ty) = lower_expr(&mut tmp, block.tail.as_ref().unwrap())?;
    Ok(ty)
}

fn sanitize_name(name: &str) -> String {
    format!("v_{name}")
}

fn sanitize_field(name: &str) -> String {
    format!("f_{name}")
}
