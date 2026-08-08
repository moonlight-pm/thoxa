//! Mid-level IR: typed enough for RC insert + C codegen.
//! R8: multi-file modules, import/export, module-level let/mut.

use crate::ast::{
    self, Arg, AssignTarget, BinaryOp, Block, BlockItem, Expr, ExprKind, InterpPart, Item,
    MatchArm, Method, Pattern, TypeArg, TypeDef, TypeExpr, TypeLit, TypeMark, UnaryOp,
};
use crate::error::{Error, Result};
use crate::loader::{self, BoundImport, Package, SourceModule};
use crate::span::Span;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

/// Synthetic union types inferred for unannotated `if` lub (D119).
#[derive(Default)]
struct SyntheticRegistry {
    /// mangled name → (type info, layout)
    entries: HashMap<String, (TypeInfo, TypeLayout)>,
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
    /// `base.field[index] = value` (fixed-array element store).
    IndexAssign {
        base: Place,
        field: String,
        index: Place,
        ty: Ty,
        value: Place,
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
    /// Construct a May: `payload` is live for Has; ignored for Not.
    MayMake {
        tag: MayTag,
        payload: Place,
        may_ty: Ty,
    },
    MayTag(Place),
    /// Extract `.value` from a May place (Has side).
    MayValue(Place),
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
    /// `base.field[index]` — fixed-array element load.
    Index {
        base: Place,
        field: String,
        index: Place,
    },
    /// Inject a member value into a union (D118).
    SumInject {
        sum_type: String,
        tag: u32,
        payload: Place,
        payload_ty: Ty,
    },
    /// Read `.tag` from a union place.
    SumTag(Place),
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
    /// Convert Int64/Bool/Array → owned String.
    ToString {
        value: Place,
        from: Ty,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrialTag {
    Pass = 0,
    Fail = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MayTag {
    Has = 0,
    Not = 1,
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
    /// Heap RC growable Buffer (`ThoxaBuffer*`) — COW on mutate when shared.
    Buffer,
    /// Dynamic growable Array (`ThoxaArray*`) — heap RC; functional append/dropLast.
    Array {
        elem: Box<Ty>,
    },
    /// Fixed inline array field type `Array<T, N>` (not a first-class value).
    FixedArray {
        elem: Box<Ty>,
        len: u64,
    },
    /// FFI pointer `Address<T>` (param types only in R7).
    Address(Box<Ty>),
    /// `Trial<Ok, Err>` — monomorphized in cgen.
    Trial {
        ok: Box<Ty>,
        err: Box<Ty>,
    },
    /// `May<T>` — monomorphized in cgen (`Has` / `Not`).
    May {
        inner: Box<Ty>,
    },
    User(String),
}

/// True when a local of this type needs an end-of-scope drop.
/// User types always participate (cgen walks fields; no-op when none are heap).
fn needs_drop(ty: &Ty) -> bool {
    match ty {
        Ty::String | Ty::Bytes | Ty::Buffer | Ty::Array { .. } | Ty::User(_) => true,
        Ty::Trial { ok, err } => needs_drop(ok) || needs_drop(err),
        Ty::May { inner } => needs_drop(inner),
        _ => false,
    }
}

/// Element types supported in dynamic `Array<T>` (phase A).
/// User is allowed when fields are constructor-param scalars/heap (checked at resolve).
fn is_array_elem_ty(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Int64 | Ty::Bool | Ty::String | Ty::Bytes | Ty::User(_)
    )
}

fn array_elem_is_heap(ty: &Ty) -> bool {
    matches!(ty, Ty::String | Ty::Bytes)
}

fn array_elem_is_user(ty: &Ty) -> bool {
    matches!(ty, Ty::User(_))
}

/// Constructor-param User types only: fields Int64/Bool/String/Bytes (D65).
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
            info.fields
                .iter()
                .all(|f| matches!(f.ty, Ty::Int64 | Ty::Bool | Ty::String | Ty::Bytes))
        }
        _ => true,
    }
}

fn is_trial(ty: &Ty) -> bool {
    matches!(ty, Ty::Trial { .. })
}

fn is_may(ty: &Ty) -> bool {
    matches!(ty, Ty::May { .. })
}

fn trial_parts(ty: &Ty) -> Option<(&Ty, &Ty)> {
    match ty {
        Ty::Trial { ok, err } => Some((ok.as_ref(), err.as_ref())),
        _ => None,
    }
}

fn may_inner(ty: &Ty) -> Option<&Ty> {
    match ty {
        Ty::May { inner } => Some(inner.as_ref()),
        _ => None,
    }
}

/// Types allowed as Trial ok/err or May payloads.
/// Nested `Array` / `May` (D113) and nested `Trial` (D114) allowed.
fn is_trial_payload_ty(ty: &Ty) -> bool {
    match ty {
        Ty::Void
        | Ty::Int32
        | Ty::Int64
        | Ty::UInt32
        | Ty::UInt64
        | Ty::Byte
        | Ty::Bool
        | Ty::String
        | Ty::Bytes
        | Ty::User(_) => true,
        Ty::Array { elem } => is_array_elem_ty(elem),
        Ty::May { inner } => is_trial_payload_ty(inner),
        Ty::Trial { ok, err } => is_trial_payload_ty(ok) && is_trial_payload_ty(err),
        _ => false,
    }
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
}

#[derive(Clone)]
struct FieldInfo {
    name: String,
    ty: Ty,
    /// `mut` field (required for field assign / Address coercion of whole struct uses place mut).
    mutable: bool,
    /// Constructor-param default (`type T(x: Int64 = 0)`); ignored for body fields.
    default: Option<Expr>,
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
        for (name, (info, _)) in synthetics.borrow().entries.iter() {
            types.entry(name.clone()).or_insert_with(|| info.clone());
        }
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
            package_maps,
            fn_ret,
            module_id,
            is_entry,
            loop_depth: 0,
            drop_places: Vec::new(),
            synthetics,
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
        self.note_drop_place(&place, &ty);
        self.stmts.push(Stmt::Assign { place, ty, value });
    }

    /// Record a place that owns a heap / Trial-with-heap value (for early-return drops).
    fn note_drop_place(&mut self, place: &Place, ty: &Ty) {
        if needs_drop(ty) {
            self.drop_places.push((place.name.clone(), ty.clone()));
        }
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
    /// Method field locals (D107) also FieldAssign through the receiver.
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
    let pkg = Package {
        project_root: PathBuf::from("."),
        modules: vec![SourceModule {
            id: "Main".into(),
            path: PathBuf::from("Main.thx"),
            ast: module.clone(),
            is_entry,
        }],
    };
    lower_package_with(&pkg, opts)
}

/// Lower a multi-file package into one MIR program (requires `fn main`).
pub fn lower_package(package: &Package) -> Result<Program> {
    lower_package_with(package, LowerOptions::default())
}

/// Lower a multi-file package with explicit options.
pub fn lower_package_with(package: &Package, opts: LowerOptions) -> Result<Program> {
    let import_maps = loader::build_import_maps(package)?;

    // ---------- Phase 1: register types (mangled names) ----------
    let mut types_info: HashMap<String, TypeInfo> = HashMap::new();
    let mut type_layouts: Vec<TypeLayout> = Vec::new();
    // (module_id, mangled_type_name, &TypeDef)
    let mut type_defs: Vec<(String, String, &TypeDef)> = Vec::new();
    let mut module_exports: HashMap<String, ModuleExports> = HashMap::new();

    for m in &package.modules {
        module_exports.entry(m.id.clone()).or_default();
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
                    zero_init: !td.fields.is_empty() && td.union_members.is_none(),
                    def_mod: m.id.clone(),
                    sum: None,
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
        per_mod_types.insert(m.id.clone(), map);
    }

    // Fill type fields with resolved types.
    for (mod_id, mangled, td) in &type_defs {
        let name_map = per_mod_types.get(mod_id).cloned().unwrap_or_default();
        let mut fields: Vec<FieldInfo> = Vec::new();
        let zero_init;
        let mut sum_info: Option<SumInfo> = None;

        // Union alias: `type Dir = Up | Down` (D118).
        if let Some(members) = &td.union_members {
            if !td.params.is_empty() || !td.fields.is_empty() || !td.methods.is_empty() {
                return Err(Error::at(
                    "union type cannot have constructor params, body fields, or methods",
                    td.name_span,
                ));
            }
            zero_init = false;
            let mut variants = Vec::new();
            let mut seen_names = HashSet::new();
            let mut residual_tag: Option<u32> = None;
            let mut extract_tag: Option<u32> = None;
            for (i, mem) in members.iter().enumerate() {
                let tag = i as u32;
                let mark = mem.mark;
                if matches!(mark, Some(TypeMark::Residual)) {
                    if residual_tag.is_some() {
                        return Err(Error::at(
                            "union may have at most one residual mark (`?`)",
                            mem.span,
                        ));
                    }
                    residual_tag = Some(tag);
                }
                if matches!(mark, Some(TypeMark::Extract)) {
                    if extract_tag.is_some() {
                        return Err(Error::at(
                            "union may have at most one extract mark (`!`)",
                            mem.span,
                        ));
                    }
                    extract_tag = Some(tag);
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
                    variants.push(SumVariant {
                        tag,
                        name: vname,
                        ty: Ty::Void,
                        lit: Some(lit.clone()),
                        mark,
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
                // Member must be a product type (not another sum) for v1.
                if let Some(minfo) = types_info.get(member_name) {
                    if minfo.sum.is_some() {
                        return Err(Error::at(
                            format!("nested union member `{vname}` not yet supported"),
                            mem.span,
                        ));
                    }
                    // Heap fields in sum payloads need RC rules; v1 is scalar/empty only.
                    for f in &minfo.fields {
                        if !matches!(
                            f.ty,
                            Ty::Int64
                                | Ty::Int32
                                | Ty::UInt32
                                | Ty::UInt64
                                | Ty::Byte
                                | Ty::Bool
                                | Ty::Void
                        ) {
                            return Err(Error::at(
                                format!(
                                    "union member `{vname}` field `{}` must be a scalar for now (got {:?})",
                                    f.name, f.ty
                                ),
                                mem.span,
                            ));
                        }
                    }
                }
                variants.push(SumVariant {
                    tag,
                    name: vname,
                    ty,
                    lit: None,
                    mark,
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
                });
            }
        } else {
            zero_init = false;
            let mut seen_default = false;
            for p in &td.params {
                let ty = resolve_ty(&p.ty, &types_info, &name_map)?;
                // R3+ : scalars + heap String/Bytes; Array<T> for Request/Response headers (D69).
                // Nested User as a direct field still deferred; Array of User is fine (D65).
                let ok = matches!(
                    ty,
                    Ty::Int64 | Ty::Bool | Ty::String | Ty::Bytes | Ty::Array { .. }
                );
                if !ok {
                    return Err(Error::at(
                        "constructor-param fields must be Int64, Bool, String, Bytes, or Array",
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
                });
            }
        }
        let info = types_info.get_mut(mangled).unwrap();
        info.fields = fields.clone();
        info.zero_init = zero_init;
        info.sum = sum_info.clone();
        let sum_layout = sum_info.map(|s| {
            s.variants
                .iter()
                .map(|v| SumVariantLayout {
                    tag: v.tag,
                    name: v.name.clone(),
                    payload_ty: v.ty.clone(),
                })
                .collect()
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
            let mut params = Vec::new();
            let mut defaults = Vec::new();
            for p in &m.params {
                params.push((p.name.clone(), resolve_ty(&p.ty, &types_info, &name_map)?));
                defaults.push(p.default.clone());
            }
            // Same ordering rule as free functions (D98/D106).
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
            types_info.get_mut(mangled).unwrap().methods.insert(
                m.name.clone(),
                MethodSig {
                    params,
                    defaults,
                    ret,
                    exported: m.exported,
                    def_mod: mod_id.clone(),
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
                        if p.default.is_some() {
                            return Err(Error::at(
                                "extern fn parameters cannot have defaults",
                                p.name_span,
                            ));
                        }
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
                                defaults: vec![None; params.len()],
                                ret: ret.clone(),
                                extern_symbol: Some(c_symbol.clone()),
                                mangled: mangled.clone(),
                                def_mod: m.id.clone(),
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
                    let mut params = Vec::new();
                    let mut defaults = Vec::new();
                    for p in &f.params {
                        params.push((p.name.clone(), resolve_ty(&p.ty, &types_info, &name_map)?));
                        defaults.push(p.default.clone());
                    }
                    // Non-defaulted params may not follow a defaulted one (positional omit rule).
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
                    if fns
                        .insert(
                            mangled.clone(),
                            FnSig {
                                params,
                                defaults,
                                ret,
                                extern_symbol: None,
                                mangled: mangled.clone(),
                                def_mod: m.id.clone(),
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

    if opts.require_main && !fns.contains_key("main") {
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
    let synthetics = Rc::new(RefCell::new(SyntheticRegistry::default()));

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
                HashMap::new(),
                Ty::Void,
                m.id.clone(),
                m.is_entry,
                Rc::clone(&synthetics),
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

    let mut functions = Vec::new();

    for (mod_id, mangled_ty, td) in &type_defs {
        let m = package.modules.iter().find(|x| x.id == *mod_id).unwrap();
        let maps = package_maps.get(mod_id).unwrap();
        for method in &td.methods {
            functions.push(lower_method(
                mangled_ty,
                td,
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
            )?);
        }
    }

    for (mod_id, mangled, f, is_entry) in &free_fns {
        let maps = package_maps.get(mod_id).unwrap();
        functions.push(lower_function(
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
        )?);
    }

    // Synthetic unions from unannotated if lub (D119).
    for (_, (_, layout)) in synthetics.borrow().entries.iter() {
        type_layouts.push(layout.clone());
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
            Item::SessionStmt(_) => {
                // Must be lifted by session::prepare before lower.
            }
            Item::Type(_) | Item::Import(_) => {}
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
        ("Buffer", []) => Ok(Ty::Buffer),
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
        ("May", [TypeArg::Type(a)]) => {
            let inner = resolve_ty(a, types, name_map)?;
            if !is_trial_payload_ty(&inner) {
                return Err(Error::at(
                    format!("May payload type is not supported: {inner:?}"),
                    t.span,
                ));
            }
            Ok(Ty::May {
                inner: Box::new(inner),
            })
        }
        ("May", _) => Err(Error::at("May expects one type argument", t.span)),
        ("Array", [TypeArg::Type(elem)]) => {
            let et = resolve_ty(elem, types, name_map)?;
            if !is_array_elem_ty(&et) {
                return Err(Error::at(
                    format!(
                        "Array element type not yet supported: {et:?} (Int64, Bool, String, Bytes, User)"
                    ),
                    elem.span,
                ));
            }
            if array_elem_is_user(&et) && !is_array_user_elem(&et, types) {
                return Err(Error::at(
                    format!(
                        "Array element User type must be constructor-param with Int64/Bool/String/Bytes fields: {et:?}"
                    ),
                    elem.span,
                ));
            }
            Ok(Ty::Array { elem: Box::new(et) })
        }
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
    package_maps: &HashMap<String, ModuleNameMaps>,
    module_id: &str,
    synthetics: &Rc<RefCell<SyntheticRegistry>>,
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
        package_maps.clone(),
        ret_ty.clone(),
        module_id.to_string(),
        is_entry,
        Rc::clone(synthetics),
    );
    for p in &f.params {
        let ty = resolve_ty(&p.ty, types, name_to_type)?;
        let place = Place {
            name: sanitize_name(&p.name),
        };
        // Params are borrowed from the caller (same as end-of-fn drops: Assign only).
        // Do not register them in drop_places — early return / `?` must not drop them.
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
    package_maps: &HashMap<String, ModuleNameMaps>,
    module_id: &str,
    is_entry: bool,
    synthetics: &Rc<RefCell<SyntheticRegistry>>,
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
        package_maps.clone(),
        ret_ty.clone(),
        module_id.to_string(),
        is_entry,
        Rc::clone(synthetics),
    );

    // Bind fields as bare names from receiver (no `self` in Thoxa).
    // Heap fields must Dup: the local is dropped at method end while `_self` is a
    // param (not dropped here). Without Dup, drop(field) would free the caller's ref.
    //
    // D107 write-back: ctor-param fields and C-layout `mut` fields are mutable
    // locals; reassignment also stores through `_self`.
    let info = types.get(mangled_type).unwrap();
    let recv = Place {
        name: recv_place.clone(),
    };
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
                base: recv.clone(),
                field: sanitize_field(&f.name),
            },
        );
        if needs_drop(&f.ty) {
            ctx.stmts.push(Stmt::Dup {
                place: p.clone(),
                ty: f.ty.clone(),
            });
        }
        // Ctor-param types (!zero_init): fields are writable in methods.
        // C-layout (zero_init): only `mut` body fields.
        let writable = if info.zero_init {
            f.mutable
        } else {
            true
        };
        let writeback = if writable {
            Some((recv.clone(), sanitize_field(&f.name)))
        } else {
            None
        };
        ctx.define_full(&f.name, f.ty.clone(), writable, p, writeback);
    }
    for p in &m.params {
        let ty = resolve_ty(&p.ty, types, name_to_type)?;
        let place = Place {
            name: sanitize_name(&p.name),
        };
        // Params borrowed from caller — not in drop_places (see lower_function).
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
    let skip = if needs_drop(ret_ty) || is_trial(ret_ty) || is_may(ret_ty) {
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
            Stmt::Loop { body } => collect_drop_places(body, out),
            Stmt::ForRange { body, .. } => collect_drop_places(body, out),
            _ => {}
        }
    }
}

fn lower_block_value(ctx: &mut LowerCtx, block: &Block, expect: &Ty) -> Result<Option<Place>> {
    ctx.push_scope();
    lower_block_items(ctx, &block.items)?;

    let result = if let Some(tail) = &block.tail {
        // Empty `[]` as block tail: take elem type from expected Array<T> (e.g. fn return).
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
                None if is_int(&ty) && is_int(expect) => {
                    coerce_place(ctx, place, &ty, expect, tail.span)?
                }
                None => {
                    return Err(Error::at(
                        format!("expected {expect:?}, found {ty:?}"),
                        tail.span,
                    ));
                }
            }
        } else {
            place
        };
        Some(place)
    } else if *expect == Ty::Void {
        None
    } else {
        return Err(Error::at("missing block value", block.span));
    };
    ctx.pop_scope();
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
                "Array element User type must be constructor-param with Int64/Bool/String/Bytes fields: {elem:?}"
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
    for item in items {
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
                                format!(
                                    "empty array `[]` needs Array<T> annotation, got {expect:?}"
                                ),
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
                                    "Array element User type must be constructor-param with Int64/Bool/String/Bytes fields: {:?}",
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
                        continue;
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
                                    format!(
                                        "fixed Array element must be an ABI scalar, got {elem:?}"
                                    ),
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
                            let bound = Place {
                                name: sanitize_name(name),
                            };
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
                            continue;
                        }
                    }
                }

                let (place, ty) = if let Some(ann) = ann {
                    let expect = resolve_ty(ann, &ctx.types, &ctx.name_to_type)?;
                    let (place, ty) = lower_expr_expect(ctx, init, Some(&expect))?;
                    if &ty != &expect {
                        match try_sum_coerce(
                            ctx,
                            place.clone(),
                            &ty,
                            &expect,
                            expr_as_type_lit(init).as_ref(),
                            *name_span,
                        )? {
                            Some(p) => (p, expect),
                            None if is_int(&ty) && is_int(&expect) => {
                                let p = coerce_place(ctx, place, &ty, &expect, *name_span)?;
                                (p, expect)
                            }
                            None => {
                                return Err(Error::at(
                                    format!(
                                        "type annotation mismatch for `{name}`: expected {expect:?}, found {ty:?}"
                                    ),
                                    *name_span,
                                ));
                            }
                        }
                    } else {
                        (place, expect)
                    }
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
        AssignTarget::Name { name, name_span } => ctx.reassign_name(name, *name_span, value),
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
        AssignTarget::DynIndex {
            name,
            name_span,
            index,
        } => {
            let binding = ctx
                .lookup(name)
                .ok_or_else(|| Error::at(format!("undefined name `{name}`"), *name_span))?;
            if !binding.mutable {
                return Err(Error::at(
                    format!("cannot assign through immutable binding `{name}`"),
                    *name_span,
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
                        base: binding.place,
                        index: idx_p,
                        elem_ty,
                        value: v,
                    });
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
                    });
                    Ok(())
                }
                other => Err(Error::at(
                    format!("index assign on non-Array type {other:?}"),
                    *name_span,
                )),
            }
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
    lower_expr_expect(ctx, expr, None)
}

fn lower_expr_expect(
    ctx: &mut LowerCtx,
    expr: &Expr,
    expect: Option<&Ty>,
) -> Result<(Place, Ty)> {
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
                    ctx.assign(p.clone(), Ty::Bool, RValue::Unary { op: *op, arg });
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
        ExprKind::ArrayLit(elems) => lower_array_lit(ctx, elems, expr.span),
        ExprKind::MethodCall {
            receiver,
            method,
            method_span,
            args,
        } => lower_method_call(ctx, receiver, method, *method_span, args),
        ExprKind::If { .. } => unreachable!("If handled in lower_expr_expect"),
        ExprKind::While { cond, body } => lower_while(ctx, cond, body, expr.span),
        ExprKind::Loop { body } => lower_loop(ctx, body, expr.span),
        ExprKind::For {
            name,
            name_span,
            start,
            end,
            inclusive,
            body,
        } => lower_for(
            ctx,
            name,
            *name_span,
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
        ExprKind::Match { scrutinee, arms } => lower_match(ctx, scrutinee, arms, expr.span),
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
            // `return []` with `-> Array<T>`: empty lit takes T from the return type.
            let (place, ty) = if is_empty_array_lit(e) {
                if let Ty::Array { elem } = &fn_ret {
                    lower_empty_array_typed(ctx, elem.as_ref(), e.span)?
                } else {
                    lower_expr(ctx, e)?
                }
            } else {
                lower_expr(ctx, e)?
            };
            if ty != fn_ret {
                // Allow int width widen/narrow via existing coerce for ints if needed.
                if is_int(&ty) && is_int(&fn_ret) {
                    let place = coerce_place(ctx, place, &ty, &fn_ret, e.span)?;
                    Some(place)
                } else if fn_ret == Ty::Void && ty == Ty::Void {
                    None
                } else {
                    return Err(Error::at(
                        format!("return type mismatch: expected {fn_ret:?}, found {ty:?}"),
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

    // Type as fn_ret so `if c { return x } else { y }` unifies; place is a dummy for non-Void.
    if fn_ret == Ty::Void {
        Ok(void_place(ctx))
    } else if let Some(p) = ret_place {
        // Unreachable after Return, but keep a typed place for if/match result slots.
        Ok((p, fn_ret))
    } else {
        Ok(void_place(ctx))
    }
}

/// Drop live `needs_drop` locals (except moved return value), then `Return`.
/// Set matches end-of-fn `insert_function_drops` / `collect_drop_places`: **Assign**
/// places only (params are never registered in `drop_places`).
fn push_early_return(ctx: &mut LowerCtx, ret: Option<Place>, ret_ty: &Ty) {
    append_return_with_drops(&mut ctx.stmts, &ctx.drop_places, ret, ret_ty);
}

/// Shared drop-then-return for explicit `return` and `?` Fail paths.
fn append_return_with_drops(
    stmts: &mut Vec<Stmt>,
    drop_places: &[(String, Ty)],
    ret: Option<Place>,
    ret_ty: &Ty,
) {
    let skip = if needs_drop(ret_ty) || is_trial(ret_ty) || is_may(ret_ty) {
        ret.as_ref().map(|p| p.name.clone())
    } else {
        None
    };
    let mut seen = std::collections::HashSet::new();
    for (name, ty) in drop_places {
        if skip.as_ref() == Some(name) {
            continue;
        }
        if seen.insert(name.clone()) {
            stmts.push(Stmt::Drop {
                place: Place { name: name.clone() },
                ty: ty.clone(),
            });
        }
    }
    stmts.push(Stmt::Return { value: ret });
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
    Ok(void_place(ctx))
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
    start: &Expr,
    end: Option<&Expr>,
    inclusive: bool,
    body: &Block,
    _span: Span,
) -> Result<(Place, Ty)> {
    // Collection iteration: `for x in xs`
    if end.is_none() {
        return lower_for_array(ctx, name, name_span, start, body);
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

    let idx = Place {
        name: sanitize_name(name),
    };
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
    let elem_ty = match &arr_ty {
        Ty::Array { elem } => elem.as_ref().clone(),
        other => {
            return Err(Error::at(
                format!("`for … in` collection must be Array, got {other:?}"),
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
    // Each iteration: drop previous (NULL first), then load + own the next element.
    let elem_p = Place {
        name: sanitize_name(name),
    };
    init_place(ctx, &elem_p, &elem_ty)?;

    let outer = std::mem::take(&mut ctx.stmts);
    ctx.loop_depth += 1;
    ctx.push_scope();
    ctx.define(name, elem_ty.clone(), false, elem_p.clone());

    if array_elem_is_user(&elem_ty) {
        // ArrayGet (cgen) field-drops prior ownership of `elem_p`, then memcpy + field-dup.
        // No separate Drop here — would double-free with the get pre-drop.
        ctx.stmts.push(Stmt::Assign {
            place: elem_p.clone(),
            ty: elem_ty.clone(),
            value: RValue::ArrayGet {
                arr: arr_p.clone(),
                index: idx.clone(),
                elem_ty: elem_ty.clone(),
            },
        });
    } else if array_elem_is_heap(&elem_ty) {
        // Drop previous iteration's ownership (no-op when NULL on first entry).
        ctx.stmts.push(Stmt::Drop {
            place: elem_p.clone(),
            ty: elem_ty.clone(),
        });
        ctx.stmts.push(Stmt::Assign {
            place: elem_p.clone(),
            ty: elem_ty.clone(),
            value: RValue::Call {
                callee: Callee::Builtin("array_get_ptr".into()),
                args: vec![arr_p.clone(), idx.clone()],
            },
        });
        // get_ptr does not dup; own a ref for the loop variable lifetime.
        ctx.stmts.push(Stmt::Dup {
            place: elem_p.clone(),
            ty: elem_ty.clone(),
        });
    } else if elem_ty == Ty::Bool {
        let raw = ctx.fresh("araw");
        ctx.stmts.push(Stmt::Assign {
            place: raw.clone(),
            ty: Ty::Int64,
            value: RValue::Call {
                callee: Callee::Builtin("array_get_i64".into()),
                args: vec![arr_p.clone(), idx.clone()],
            },
        });
        ctx.stmts.push(Stmt::Assign {
            place: elem_p.clone(),
            ty: Ty::Bool,
            value: RValue::Cast {
                value: raw,
                from: Ty::Int64,
                to: Ty::Bool,
            },
        });
    } else {
        ctx.stmts.push(Stmt::Assign {
            place: elem_p.clone(),
            ty: elem_ty.clone(),
            value: RValue::Call {
                callee: Callee::Builtin("array_get_i64".into()),
                args: vec![arr_p.clone(), idx.clone()],
            },
        });
    }
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

fn lower_array_lit(ctx: &mut LowerCtx, elems: &[Expr], span: Span) -> Result<(Place, Ty)> {
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
    let mut places = Vec::new();
    let mut elem_ty: Option<Ty> = None;
    for e in elems {
        let (p, ty) = lower_expr(ctx, e)?;
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
                            "array element User type must be constructor-param with Int64/Bool/String/Bytes fields: {ty:?}"
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
                match ty {
                    Ty::String => v,
                    Ty::Int64 | Ty::Bool => {
                        let p = ctx.fresh("ts");
                        ctx.assign(
                            p.clone(),
                            Ty::String,
                            RValue::ToString { value: v, from: ty },
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
                        if matches!(
                            elem.as_ref(),
                            Ty::Int64 | Ty::Bool | Ty::String
                        ) =>
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
                    Ty::Array { ref elem } => {
                        return Err(Error::at(
                            format!(
                                "cannot interpolate Array<{elem:?}> into String (supported: Int64, Bool, String)"
                            ),
                            span,
                        ));
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
                    RValue::StringConcat { left, right: piece },
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
        Ty::Array { .. } => {
            if name != "count" {
                return Err(Error::at(
                    format!("no field `{name}` on Array (use .count, .append, .dropLast, [i])"),
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
                    format!("fixed array field `{name}` must be indexed (e.g. `.{name}[i]`)"),
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
            .ok_or_else(|| {
                Error::at(
                    format!("no field `{name}` on `{type_name}`"),
                    *name_span,
                )
            })?;
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
                    },
                );
                return Ok((p, elem_ty));
            }
            Ty::Array { elem } => {
                // Load dynamic Array field, then index like a local Array.
                let arr_p = ctx.fresh("afld");
                let arr_ty = finfo.ty.clone();
                ctx.assign(
                    arr_p.clone(),
                    arr_ty.clone(),
                    RValue::Field {
                        base: struct_p,
                        field: sanitize_field(name),
                    },
                );
                if needs_drop(&arr_ty) {
                    ctx.stmts.push(Stmt::Dup {
                        place: arr_p.clone(),
                        ty: arr_ty,
                    });
                }
                return lower_array_index(ctx, arr_p, elem.as_ref().clone(), index);
            }
            other => {
                return Err(Error::at(
                    format!("cannot index non-array field type {other:?}"),
                    *name_span,
                ));
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
            },
        );
        return Ok((p, elem_ty));
    }
    let elem_ty = match &base_ty {
        Ty::Array { elem } => elem.as_ref().clone(),
        other => {
            return Err(Error::at(
                format!("cannot index non-Array type {other:?}"),
                span,
            ));
        }
    };
    lower_array_index(ctx, base_p, elem_ty, index)
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
                    let defaults: Vec<Option<Expr>> =
                        info.fields.iter().map(|f| f.default.clone()).collect();
                    let field_places = resolve_call_args_with_defaults(
                        ctx,
                        &formal,
                        &defaults,
                        args,
                        method_span,
                        &info.def_mod,
                    )?;
                    ctx.assign(
                        p.clone(),
                        ty.clone(),
                        RValue::StructLit {
                            type_name: mangled_ty.clone(),
                            fields: field_places,
                        },
                    );
                    // StructLit copies field pointers; take our own refs so arg temps can drop.
                    if needs_drop(&ty) {
                        ctx.stmts.push(Stmt::Dup {
                            place: p.clone(),
                            ty: ty.clone(),
                        });
                    }
                }
                return Ok((p, ty));
            }
            // Free function
            if let Some(mangled_fn) = exp.fns.get(method) {
                let sig = ctx.fns.get(mangled_fn).unwrap().clone();
                let arg_places = resolve_call_args_with_defaults(
                    ctx,
                    &sig.params,
                    &sig.defaults,
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

    // Buffer mutating methods must use the binding place without Dup and write back
    // the returned pointer (COW may allocate a new unique buffer).
    if matches!(method, "append" | "appendBytes" | "freeze") {
        if let ExprKind::Name(recv_name) = &receiver.kind {
            if let Some(binding) = ctx.lookup(recv_name) {
                if binding.ty == Ty::Buffer {
                    return lower_buffer_method(
                        ctx,
                        binding.place.clone(),
                        method,
                        args,
                        method_span,
                        /*write_back*/ method != "freeze",
                    );
                }
            }
        }
    }

    // User method receivers: use the binding place when the receiver is a name so
    // D107 field write-back mutates the caller's object (not a needs_drop temp copy).
    let (recv_p, recv_ty) = if let ExprKind::Name(n) = &receiver.kind {
        if let Some(b) = ctx.lookup(n) {
            if matches!(&b.ty, Ty::User(_)) {
                (b.place.clone(), b.ty.clone())
            } else {
                lower_expr(ctx, receiver)?
            }
        } else {
            lower_expr(ctx, receiver)?
        }
    } else {
        lower_expr(ctx, receiver)?
    };
    match &recv_ty {
        Ty::Bytes => lower_bytes_method(ctx, recv_p, method, args, method_span),
        Ty::Buffer => lower_buffer_method(ctx, recv_p, method, args, method_span, false),
        Ty::Array { elem } => {
            lower_array_method(ctx, recv_p, elem.as_ref(), method, args, method_span)
        }
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
            let arg_places = resolve_call_args_with_defaults(
                ctx,
                &sig.params,
                &sig.defaults,
                args,
                method_span,
                &sig.def_mod,
            )?;
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
            let places = resolve_call_args(ctx, &formal, args, span)?;
            emit_call_result(
                ctx,
                Callee::Builtin("buffer_with_capacity".into()),
                places,
                Ty::Buffer,
            )
        }
        _ => Err(Error::at(
            "`Buffer()` / `Buffer(capacity)` expects 0 or 1 argument",
            span,
        )),
    }
}

/// Array methods — functional (return a new array; do not write back).
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
                let p = ctx.fresh("aapp");
                ctx.assign(
                    p.clone(),
                    arr_ty.clone(),
                    RValue::ArrayAppend {
                        arr: recv,
                        item,
                        elem_ty: elem.clone(),
                    },
                );
                return Ok((p, arr_ty));
            }
            let item = if *elem == Ty::Bool {
                // Runtime slots are int64.
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
            } else {
                item
            };
            let all = vec![recv, item];
            let builtin = if array_elem_is_heap(elem) {
                "array_append_ptr"
            } else {
                "array_append_i64"
            };
            emit_call_result(ctx, Callee::Builtin(builtin.into()), all, arr_ty)
        }
        "dropLast" => {
            if !args.is_empty() {
                return Err(Error::at("Array.dropLast() takes no arguments", span));
            }
            if array_elem_is_user(elem) {
                let p = ctx.fresh("adrop");
                ctx.assign(
                    p.clone(),
                    arr_ty.clone(),
                    RValue::ArrayDropLast {
                        arr: recv,
                        elem_ty: elem.clone(),
                    },
                );
                return Ok((p, arr_ty));
            }
            emit_call_result(
                ctx,
                Callee::Builtin("array_drop_last".into()),
                vec![recv],
                arr_ty,
            )
        }
        other => Err(Error::at(
            format!("no method `{other}` on Array (use .append, .dropLast)"),
            span,
        )),
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
            // `find(needle[, from]) -> May<Int64>` — Has(i) or Not (never -1 at surface).
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
                vec![("needle".into(), Ty::Bytes), ("from".into(), Ty::Int64)]
            };
            let places = resolve_call_args(ctx, &formal, needle_args, span)?;
            let mut all = vec![recv];
            all.extend(places);
            if from_default {
                let z = ctx.fresh("from");
                ctx.assign(z.clone(), Ty::Int64, RValue::Int(0));
                all.push(z);
            }
            let idx = ctx.fresh("findi");
            ctx.assign(
                idx.clone(),
                Ty::Int64,
                RValue::Call {
                    callee: Callee::Builtin("bytes_find".into()),
                    args: all,
                },
            );
            // Wrap runtime -1 / index into May<Int64>.
            let may_ty = Ty::May {
                inner: Box::new(Ty::Int64),
            };
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
            let mut then_body = Vec::new();
            let dummy = Place {
                name: format!("_notp{}", ctx.temps),
            };
            ctx.temps += 1;
            then_body.push(Stmt::Assign {
                place: out.clone(),
                ty: may_ty.clone(),
                value: RValue::MayMake {
                    tag: MayTag::Not,
                    payload: dummy,
                    may_ty: may_ty.clone(),
                },
            });
            let mut else_body = Vec::new();
            else_body.push(Stmt::Assign {
                place: out.clone(),
                ty: may_ty.clone(),
                value: RValue::MayMake {
                    tag: MayTag::Has,
                    payload: idx,
                    may_ty: may_ty.clone(),
                },
            });
            ctx.stmts = outer;
            ctx.stmts.push(Stmt::If {
                cond: missing,
                then_body,
                else_body,
            });
            Ok((out, may_ty))
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
        other => Err(Error::at(format!("no method `{other}` on Bytes"), span)),
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
        ctx.assign(p.clone(), ret.clone(), RValue::Call { callee, args });
        Ok((p, ret))
    }
}

fn resolve_call_args(
    ctx: &mut LowerCtx,
    formal: &[(String, Ty)],
    args: &[Arg],
    span: Span,
) -> Result<Vec<Place>> {
    resolve_call_args_with_defaults(ctx, formal, &[], args, span, "")
}

/// Like [`resolve_call_args`], but `defaults[i]` fills formal `i` when omitted.
/// `defaults` may be shorter than `formal` (missing entries mean no default).
/// Default expressions resolve free names in `def_mod` (defining module — D116).
fn resolve_call_args_with_defaults(
    ctx: &mut LowerCtx,
    formal: &[(String, Ty)],
    defaults: &[Option<Expr>],
    args: &[Arg],
    span: Span,
    def_mod: &str,
) -> Result<Vec<Place>> {
    if args.is_empty() && formal.is_empty() {
        return Ok(Vec::new());
    }

    let default_at = |i: usize| -> Option<&Expr> {
        defaults.get(i).and_then(|d| d.as_ref())
    };

    let all_named = args.iter().all(|a| matches!(a, Arg::Named { .. }));
    let all_pos = args.iter().all(|a| matches!(a, Arg::Pos(_)));

    if !all_named && !all_pos {
        return Err(Error::at("cannot mix positional and named arguments", span));
    }

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
        for (i, arg) in args.iter().enumerate() {
            let expr = match arg {
                Arg::Pos(e) => e,
                Arg::Named { .. } => unreachable!(),
            };
            let p = lower_arg_for_formal(ctx, expr, &formal[i].1, &formal[i].0, i)?;
            places.push(p);
        }
        for i in args.len()..formal.len() {
            let expr = default_at(i).expect("checked above");
            let p = ctx.with_def_mod_maps(def_mod, |ctx| {
                lower_arg_for_formal(ctx, expr, &formal[i].1, &formal[i].0, i)
            })?;
            places.push(p);
        }
        return Ok(places);
    }

    // Named: match by name; omitted formals use defaults; declaration order for result.
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
        let (expr, is_default) = if let Some(e) = by_name.remove(fname) {
            (e, false)
        } else if let Some(d) = default_at(i) {
            (d, true)
        } else {
            return Err(Error::at(format!("missing argument `{fname}`"), span));
        };
        let p = if is_default {
            ctx.with_def_mod_maps(def_mod, |ctx| {
                lower_arg_for_formal(ctx, expr, fty, fname, i)
            })?
        } else {
            lower_arg_for_formal(ctx, expr, fty, fname, i)?
        };
        places.push(p);
    }
    if let Some((extra, _)) = by_name.iter().next() {
        return Err(Error::at(format!("unknown argument `{extra}`"), span));
    }
    Ok(places)
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
                // Place is the binding itself; cgen emits `&place` for Address args.
                return Ok(binding.place);
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
    } else if is_int(&ty) && is_int(formal_ty) {
        coerce_place(ctx, p, &ty, formal_ty, expr.span)
    } else if let Some(p) = try_sum_coerce(
        ctx,
        p,
        &ty,
        formal_ty,
        expr_as_type_lit(expr).as_ref(),
        expr.span,
    )? {
        Ok(p)
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
    let (scrut_p, scrut_ty) = lower_expr(ctx, inner)?;
    if is_may(&scrut_ty) {
        return lower_try_may(ctx, scrut_p, scrut_ty, span);
    }
    if let Ty::User(ref n) = &scrut_ty {
        if let Some(sum) = ctx.types.get(n).and_then(|i| i.sum.clone()) {
            return lower_try_sum(ctx, scrut_p, scrut_ty, sum, span);
        }
    }
    let Some((ok_ty, err_ty)) = trial_parts(&scrut_ty) else {
        return Err(Error::at(
            "`?` expects a Trial, May, or residual-marked union value",
            span,
        ));
    };
    let ok_ty = ok_ty.clone();
    let err_ty = err_ty.clone();
    let fn_ret = ctx.fn_ret.clone();
    let (ret_ok, ret_err) = match &fn_ret {
        Ty::Trial { ok, err } => (ok.as_ref().clone(), err.as_ref().clone()),
        _ => {
            return Err(Error::at(
                "`?` on Trial requires the enclosing function to return Trial",
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
    ctx.assign(tag.clone(), Ty::Int64, RValue::TrialTag(scrut_p.clone()));
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
    // Drops use the same helper as explicit `return` (Assign-tracked locals only; not params).
    let outer = std::mem::take(&mut ctx.stmts);
    let mut then_body = Vec::new();
    if scrut_ty == fn_ret {
        append_return_with_drops(
            &mut then_body,
            &ctx.drop_places,
            Some(scrut_p.clone()),
            &fn_ret,
        );
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
                value: RValue::TrialErr(scrut_p.clone()),
            });
            // needs_drop (not is_heap): User err payloads field-walk via emit_rc.
            if needs_drop(&err_ty) {
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
        append_return_with_drops(&mut then_body, &ctx.drop_places, Some(ret_trial), &fn_ret);
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
            value: RValue::TrialOk(scrut_p),
        });
        // Extracted payload shares ownership with the trial until fn end; dup if droppable
        // (String/Bytes/User — User field-walks heap members once).
        if needs_drop(&ok_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: payload.clone(),
                ty: ok_ty.clone(),
            });
        }
    }
    let _ = ret_ok;
    Ok((payload, ok_ty))
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
    let fn_ret = ctx.fn_ret.clone();
    if fn_ret != scrut_ty {
        return Err(Error::at(
            "`?` on a union requires the enclosing function to return the same union type",
            span,
        ));
    }

    // Success extract type.
    let success_ty = sum_success_extract_ty(ctx, &sum, span)?;

    let tag = ctx.fresh("stag");
    ctx.assign(tag.clone(), Ty::Int64, RValue::SumTag(scrut_p.clone()));
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
    // then: residual → return scrut (whole sum value)
    let mut then_body = Vec::new();
    // drops before return
    let mut drops = Vec::new();
    for (n, ty) in &ctx.drop_places {
        if n != &scrut_p.name {
            drops.push(Stmt::Drop {
                place: Place { name: n.clone() },
                ty: ty.clone(),
            });
        }
    }
    then_body.extend(drops);
    then_body.push(Stmt::Return {
        value: Some(scrut_p.clone()),
    });

    // else: extract success
    let mut else_body = Vec::new();
    let success_place = ctx.fresh("sok");
    // Temporarily use else_body via ctx.stmts
    ctx.stmts = std::mem::take(&mut else_body);
    let ok_p = emit_sum_success_extract(ctx, &scrut_p, &sum, &success_ty, span)?;
    ctx.assign(
        success_place.clone(),
        success_ty.clone(),
        RValue::Use(ok_p),
    );
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
        // Whole-arm extract (`Done!`): yield the arm type.
        // One-field product: still whole arm unless marked on type arg (future).
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
            let fields = ctx
                .types
                .get(n)
                .map(|i| i.fields.as_slice())
                .unwrap_or(&[]);
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

fn emit_sum_success_extract(
    ctx: &mut LowerCtx,
    scrut_p: &Place,
    sum: &SumInfo,
    success_ty: &Ty,
    span: Span,
) -> Result<Place> {
    let (tag, arm_ty) = if let Some(et) = sum.extract_tag {
        let v = sum
            .variants
            .iter()
            .find(|v| v.tag == et)
            .ok_or_else(|| Error::at("internal: bad extract tag", span))?;
        (v.tag, v.ty.clone())
    } else {
        let v = sum
            .variants
            .iter()
            .find(|v| Some(v.tag) != sum.residual_tag)
            .ok_or_else(|| Error::at("internal: no success arm", span))?;
        (v.tag, v.ty.clone())
    };

    if arm_ty == Ty::Void || success_ty == &Ty::Void {
        let p = ctx.fresh("svoid");
        ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
        return Ok(p);
    }

    let payload = ctx.fresh("spay");
    ctx.assign(
        payload.clone(),
        arm_ty.clone(),
        RValue::SumPayload {
            base: scrut_p.clone(),
            tag,
            payload_ty: arm_ty.clone(),
        },
    );

    // Unwrap single field when success_ty is the field type.
    if let Ty::User(n) = &arm_ty {
        let field_name = ctx.types.get(n).and_then(|info| {
            if info.fields.len() == 1 && info.fields[0].ty == *success_ty {
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
            return Ok(fp);
        }
    }
    let _ = span;
    Ok(payload)
}

fn lower_try_may(
    ctx: &mut LowerCtx,
    may_p: Place,
    may_ty: Ty,
    span: Span,
) -> Result<(Place, Ty)> {
    let inner_ty = may_inner(&may_ty)
        .ok_or_else(|| Error::at("`?` expects a May value", span))?
        .clone();
    let fn_ret = ctx.fn_ret.clone();
    if !is_may(&fn_ret) {
        return Err(Error::at(
            "`?` on May requires the enclosing function to return May",
            span,
        ));
    }
    // Payload monomorph must match when rewrapping is a no-op; different May inners
    // are ok only when returning the same May value wholesale on Not.
    let ret_inner = may_inner(&fn_ret).unwrap().clone();

    let tag = ctx.fresh("mtag");
    ctx.assign(tag.clone(), Ty::Int64, RValue::MayTag(may_p.clone()));
    let zero = ctx.fresh("mz");
    ctx.assign(zero.clone(), Ty::Int64, RValue::Int(0));
    let is_not = ctx.fresh("isn");
    ctx.assign(
        is_not.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Ne,
            left: tag,
            right: zero,
        },
    );

    let outer = std::mem::take(&mut ctx.stmts);
    let mut then_body = Vec::new();
    if may_ty == fn_ret {
        append_return_with_drops(
            &mut then_body,
            &ctx.drop_places,
            Some(may_p.clone()),
            &fn_ret,
        );
    } else {
        // Not has no payload — build Not of enclosing May.
        let dummy = Place {
            name: format!("_qnot{}", ctx.temps),
        };
        ctx.temps += 1;
        let ret_may = Place {
            name: format!("_qret{}", ctx.temps),
        };
        ctx.temps += 1;
        then_body.push(Stmt::Assign {
            place: ret_may.clone(),
            ty: fn_ret.clone(),
            value: RValue::MayMake {
                tag: MayTag::Not,
                payload: dummy,
                may_ty: fn_ret.clone(),
            },
        });
        append_return_with_drops(&mut then_body, &ctx.drop_places, Some(ret_may), &fn_ret);
    }
    ctx.stmts = outer;
    ctx.stmts.push(Stmt::If {
        cond: is_not,
        then_body,
        else_body: Vec::new(),
    });

    let payload = ctx.fresh("has");
    if inner_ty == Ty::Void {
        // no payload
    } else {
        ctx.stmts.push(Stmt::Assign {
            place: payload.clone(),
            ty: inner_ty.clone(),
            value: RValue::MayValue(may_p),
        });
        if needs_drop(&inner_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: payload.clone(),
                ty: inner_ty.clone(),
            });
        }
    }
    // When Has payload types differ, coerce integers if needed.
    if inner_ty != ret_inner && is_int(&inner_ty) && is_int(&ret_inner) {
        let coerced = coerce_place(ctx, payload, &inner_ty, &ret_inner, span)?;
        return Ok((coerced, ret_inner));
    }
    if inner_ty != ret_inner && inner_ty != Ty::Void {
        return Err(Error::at(
            format!("`?` May payload {inner_ty:?} does not match enclosing May<{ret_inner:?}>"),
            span,
        ));
    }
    Ok((payload, inner_ty))
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
        format!("other_{}", format!("{:?}", v.ty).replace(|c: char| !c.is_ascii_alphanumeric(), "_"))
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
        (Ty::May { inner: ai }, Ty::May { inner: bi }) => ty_eq(types, ai, bi),
        (Ty::Trial { ok: ao, err: ae }, Ty::Trial { ok: bo, err: be }) => {
            ty_eq(types, ao, bo) && ty_eq(types, ae, be)
        }
        (Ty::Array { elem: ae }, Ty::Array { elem: be }) => ty_eq(types, ae, be),
        (
            Ty::FixedArray {
                elem: ae,
                len: al,
            },
            Ty::FixedArray {
                elem: be,
                len: bl,
            },
        ) => al == bl && ty_eq(types, ae, be),
        (Ty::Address(ae), Ty::Address(be)) => ty_eq(types, ae, be),
        _ => false,
    }
}

/// Whether `from` can be coerced to `to` (equality, inject, or transparent remap).
fn can_coerce_ty(types: &HashMap<String, TypeInfo>, from: &Ty, to: &Ty) -> bool {
    if ty_eq(types, from, to) {
        return true;
    }
    let (Ty::User(from_n), Ty::User(to_n)) = (from, to) else {
        return false;
    };
    let Some(sum) = types.get(to_n).and_then(|i| i.sum.as_ref()) else {
        return false;
    };
    // Atom inject into union.
    sum.variants.iter().any(|v| match &v.ty {
        Ty::User(n) => n == from_n,
        _ => false,
    })
}

/// Remap a sum value to another union with the same member set (D119).
fn emit_sum_remap(
    ctx: &mut LowerCtx,
    place: Place,
    from_n: &str,
    to_n: &str,
    from_sum: &SumInfo,
    to_sum: &SumInfo,
) -> Result<Place> {
    let mut to_by_key: HashMap<String, (u32, Ty)> = HashMap::new();
    for v in &to_sum.variants {
        to_by_key.insert(sum_variant_key(v), (v.tag, v.ty.clone()));
    }

    let out_ty = Ty::User(to_n.to_string());
    let out = ctx.fresh("srm");
    init_place(ctx, &out, &out_ty)?;

    let tag_p = ctx.fresh("srt");
    ctx.assign(tag_p.clone(), Ty::Int64, RValue::SumTag(place.clone()));

    let mut acc: Vec<Stmt> = Vec::new();
    for fv in from_sum.variants.iter().rev() {
        let key = sum_variant_key(fv);
        let Some(&(to_tag, ref to_payload_ty)) = to_by_key.get(&key) else {
            return Err(Error::new(format!(
                "internal: cannot remap union member `{key}` from `{from_n}` to `{to_n}`"
            )));
        };
        let to_payload_ty = to_payload_ty.clone();

        let outer = std::mem::take(&mut ctx.stmts);
        let payload = if fv.ty != Ty::Void {
            let pp = ctx.fresh("srp");
            ctx.assign(
                pp.clone(),
                fv.ty.clone(),
                RValue::SumPayload {
                    base: place.clone(),
                    tag: fv.tag,
                    payload_ty: fv.ty.clone(),
                },
            );
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
        ctx.stmts.push(Stmt::Assign {
            place: out.clone(),
            ty: out_ty.clone(),
            value: RValue::Use(injected),
        });
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
    // Transparent aliases: same member set, possibly different tags/names (D119).
    if let (Ty::User(from_n), Ty::User(to_n)) = (from, to) {
        if let (Some(fs), Some(ts)) = (
            ctx.types.get(from_n).and_then(|i| i.sum.clone()),
            ctx.types.get(to_n).and_then(|i| i.sum.clone()),
        ) {
            if member_sets_eq(&fs, &ts) {
                return Ok(Some(emit_sum_remap(
                    ctx, place, from_n, to_n, &fs, &ts,
                )?));
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
    _span: Span,
) -> Result<Option<Place>> {
    let (Ty::User(from_n), Ty::User(to_n)) = (from, to) else {
        return Ok(None);
    };
    let Some(info) = ctx.types.get(to_n) else {
        return Ok(None);
    };
    let Some(sum) = info.sum.as_ref() else {
        return Ok(None);
    };
    let Some(v) = sum.variants.iter().find(|v| match &v.ty {
        Ty::User(n) => n == from_n,
        _ => false,
    }) else {
        return Ok(None);
    };
    let tag = v.tag;
    let payload_ty = v.ty.clone();
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
) -> Result<(Place, Ty)> {
    if arms.is_empty() {
        return Err(Error::at("match requires at least one arm", span));
    }

    let (scrut_p, scrut_ty) = lower_expr(ctx, scrutinee)?;
    if is_may(&scrut_ty) {
        if arms.len() != 2 {
            return Err(Error::at(
                "May match requires exactly two arms (Has / Not)",
                span,
            ));
        }
        return lower_match_may(ctx, scrut_p, scrut_ty, arms, span);
    }
    if is_trial(&scrut_ty) {
        if arms.len() != 2 {
            return Err(Error::at(
                "Trial match requires exactly two arms (Pass / Fail)",
                span,
            ));
        }
        return lower_match_trial(ctx, scrut_p, scrut_ty, arms, span);
    }
    if matches!(scrut_ty, Ty::String | Ty::Int64 | Ty::Bool) {
        return lower_match_value(ctx, scrut_p, scrut_ty, arms, span);
    }
    if let Ty::User(ref n) = &scrut_ty {
        if ctx.types.get(n).and_then(|i| i.sum.as_ref()).is_some() {
            return lower_match_sum(ctx, scrut_p, scrut_ty, arms, span);
        }
    }
    Err(Error::at(
        "match supports Trial, May, String, Int64, Bool, or union types",
        span,
    ))
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
                // Share ownership of heap result with the arm temp (dropped at fn end).
                if needs_drop(&ty) {
                    body.push(Stmt::Dup {
                        place: p.clone(),
                        ty: ty.clone(),
                    });
                }
                body.push(Stmt::Assign {
                    place: result_place.clone(),
                    ty: ty.clone(),
                    value: RValue::Use(p),
                });
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

/// Match on a user union (D118): constructor arms + optional `else`.
fn lower_match_sum(
    ctx: &mut LowerCtx,
    scrut_p: Place,
    scrut_ty: Ty,
    arms: &[MatchArm],
    span: Span,
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
    ctx.assign(tag_p.clone(), Ty::Int64, RValue::SumTag(scrut_p.clone()));

    let result_place = ctx.fresh("mres");
    let mut result_ty: Option<Ty> = None;
    let mut covered: HashSet<u32> = HashSet::new();
    let mut has_else = false;

    // Forward pass: build list of (cond_setup, cond_place, then_body) or else_body.
    struct ArmCode {
        /// None = else/bind arm
        tag: Option<u32>,
        body: Vec<Stmt>,
    }
    let mut codes: Vec<ArmCode> = Vec::new();

    for arm in arms {
        if arm.guard.is_some() {
            return Err(Error::at(
                "guards on union match arms are not yet supported",
                span,
            ));
        }
        match &arm.pattern {
            Pattern::Else { .. } => {
                has_else = true;
                let outer = std::mem::take(&mut ctx.stmts);
                lower_block_items(ctx, &arm.body.items)?;
                let arm_val = match &arm.body.tail {
                    Some(t) => {
                        let (p, ty) = lower_expr(ctx, t)?;
                        Some((p, ty))
                    }
                    None => None,
                };
                let mut body = std::mem::take(&mut ctx.stmts);
                ctx.stmts = outer;
                finish_match_arm_ty(
                    ctx,
                    &result_place,
                    &mut result_ty,
                    arm_val,
                    &mut body,
                    span,
                )?;
                codes.push(ArmCode { tag: None, body });
            }
            Pattern::Bind { name, span: bspan } => {
                // Bare `Up` is a nullary variant if it names a union member (D118);
                // otherwise catch-all bind of the whole sum value.
                if let Some(variant) = sum.variants.iter().find(|v| v.name == *name).cloned() {
                    let member_fields = match &variant.ty {
                        Ty::User(mn) => ctx
                            .types
                            .get(mn)
                            .map(|i| i.fields.clone())
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    };
                    if !member_fields.is_empty() {
                        return Err(Error::at(
                            format!("`{name}` has a payload; use `{name}(x)` pattern"),
                            *bspan,
                        ));
                    }
                    if !covered.insert(variant.tag) {
                        return Err(Error::at(
                            format!("duplicate match arm for `{name}`"),
                            *bspan,
                        ));
                    }
                    let outer = std::mem::take(&mut ctx.stmts);
                    lower_block_items(ctx, &arm.body.items)?;
                    let arm_val = match &arm.body.tail {
                        Some(t) => {
                            let (p, ty) = lower_expr(ctx, t)?;
                            Some((p, ty))
                        }
                        None => None,
                    };
                    let mut body = std::mem::take(&mut ctx.stmts);
                    ctx.stmts = outer;
                    finish_match_arm_ty(
                        ctx,
                        &result_place,
                        &mut result_ty,
                        arm_val,
                        &mut body,
                        span,
                    )?;
                    codes.push(ArmCode {
                        tag: Some(variant.tag),
                        body,
                    });
                } else {
                    has_else = true;
                    let outer = std::mem::take(&mut ctx.stmts);
                    let bound = ctx.fresh("sbind");
                    ctx.assign(
                        bound.clone(),
                        scrut_ty.clone(),
                        RValue::Use(scrut_p.clone()),
                    );
                    ctx.define(name, scrut_ty.clone(), false, bound);
                    lower_block_items(ctx, &arm.body.items)?;
                    let arm_val = match &arm.body.tail {
                        Some(t) => {
                            let (p, ty) = lower_expr(ctx, t)?;
                            Some((p, ty))
                        }
                        None => None,
                    };
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
                    codes.push(ArmCode { tag: None, body });
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
                    .ok_or_else(|| {
                        Error::at(
                            format!("`{name}` is not a member of this union"),
                            *pspan,
                        )
                    })?
                    .clone();
                if !covered.insert(variant.tag) {
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
                if binds.is_empty() {
                    if !member_fields.is_empty() {
                        return Err(Error::at(
                            format!("`{name}` has a payload; use `{name}(…)` pattern"),
                            *pspan,
                        ));
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
                    let payload = ctx.fresh("spay");
                    ctx.assign(
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
                            ctx.assign(
                                fp.clone(),
                                fty.clone(),
                                RValue::Field {
                                    base: payload.clone(),
                                    field: sanitize_field(&member_fields[i].name),
                                },
                            );
                            ctx.define(bname, fty, false, fp);
                        }
                    }
                }
                lower_block_items(ctx, &arm.body.items)?;
                let arm_val = match &arm.body.tail {
                    Some(t) => {
                        let (p, ty) = lower_expr(ctx, t)?;
                        Some((p, ty))
                    }
                    None => None,
                };
                let mut body = std::mem::take(&mut ctx.stmts);
                ctx.stmts = outer;
                finish_match_arm_ty(
                    ctx,
                    &result_place,
                    &mut result_ty,
                    arm_val,
                    &mut body,
                    span,
                )?;
                codes.push(ArmCode {
                    tag: Some(variant.tag),
                    body,
                });
            }
            Pattern::IntLit { .. }
            | Pattern::BoolLit { .. }
            | Pattern::StringLit { .. } => {
                let (lit, pspan) = match &arm.pattern {
                    Pattern::IntLit { value, span } => (TypeLit::Int(*value), *span),
                    Pattern::BoolLit { value, span } => (TypeLit::Bool(*value), *span),
                    Pattern::StringLit { value, span } => {
                        (TypeLit::String(value.clone()), *span)
                    }
                    _ => unreachable!(),
                };
                let variant = sum
                    .variants
                    .iter()
                    .find(|v| v.lit.as_ref() == Some(&lit))
                    .ok_or_else(|| {
                        Error::at("literal is not a member of this union", pspan)
                    })?
                    .clone();
                if !covered.insert(variant.tag) {
                    return Err(Error::at("duplicate match arm for literal", pspan));
                }
                let outer = std::mem::take(&mut ctx.stmts);
                lower_block_items(ctx, &arm.body.items)?;
                let arm_val = match &arm.body.tail {
                    Some(t) => {
                        let (p, ty) = lower_expr(ctx, t)?;
                        Some((p, ty))
                    }
                    None => None,
                };
                let mut body = std::mem::take(&mut ctx.stmts);
                ctx.stmts = outer;
                finish_match_arm_ty(
                    ctx,
                    &result_place,
                    &mut result_ty,
                    arm_val,
                    &mut body,
                    span,
                )?;
                codes.push(ArmCode {
                    tag: Some(variant.tag),
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

    let all_tags: HashSet<u32> = sum.variants.iter().map(|v| v.tag).collect();
    if !has_else && covered != all_tags {
        return Err(Error::at(
            "non-exhaustive union match (missing arms or `else`)",
            span,
        ));
    }

    // Nest ifs from the end.
    let mut acc: Vec<Stmt> = Vec::new();
    for code in codes.into_iter().rev() {
        match code.tag {
            None => {
                acc = code.body;
            }
            Some(tag) => {
                let expected = ctx.fresh("ste");
                ctx.assign(expected.clone(), Ty::Int64, RValue::Int(tag as i64));
                let cond = ctx.fresh("sc");
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
                    then_body: code.body,
                    else_body: acc,
                }];
            }
        }
    }
    ctx.stmts.extend(acc);

    // Result place is declared zeroed with other locals; arms assign into it.
    let ty = result_ty.unwrap_or(Ty::Void);
    Ok((result_place, ty))
}

/// Value match: literal arms + required final `else` (D87).
/// Lowers to a cascade of `if scrut == lit { arm } else { … }`.
fn lower_match_value(
    ctx: &mut LowerCtx,
    scrut_p: Place,
    scrut_ty: Ty,
    arms: &[MatchArm],
    span: Span,
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
                            return Err(Error::at(
                                format!("duplicate match arm for {value}"),
                                *ps,
                            ));
                        }
                        seen_i.push(*value);
                    }
                    Pattern::BoolLit { value, span: ps } => {
                        if seen_b.contains(value) {
                            return Err(Error::at(
                                format!("duplicate match arm for {value}"),
                                *ps,
                            ));
                        }
                        seen_b.push(*value);
                    }
                    _ => {}
                }
            }
        }
    }

    // Unify arm body types.
    let mut result_ty: Option<Ty> = None;
    for arm in lit_arms.iter().chain(std::iter::once(&catch_arm)) {
        ctx.push_scope();
        if let Pattern::Bind { name, .. } = &arm.pattern {
            // Scrutinee binding for type inference of the arm body.
            let p = Place {
                name: sanitize_name(name),
            };
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

    let out = if result_ty == Ty::Void {
        None
    } else {
        Some(ctx.fresh("m"))
    };

    // Build cascade from the inside out: catch-all body, then each lit arm wrapping it.
    let outer = std::mem::take(&mut ctx.stmts);

    ctx.push_scope();
    if let Pattern::Bind { name, .. } = &catch_arm.pattern {
        let p = Place {
            name: sanitize_name(name),
        };
        // Bind to scrutinee; Dup heap so arm can drop without freeing caller.
        ctx.assign(p.clone(), scrut_ty.clone(), RValue::Use(scrut_p.clone()));
        if needs_drop(&scrut_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: p.clone(),
                ty: scrut_ty.clone(),
            });
        }
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

fn lower_match_trial(
    ctx: &mut LowerCtx,
    scrut_p: Place,
    scrut_ty: Ty,
    arms: &[MatchArm],
    span: Span,
) -> Result<(Place, Ty)> {
    let mut pass_arm: Option<&MatchArm> = None;
    let mut fail_arm: Option<&MatchArm> = None;
    for arm in arms {
        if arm.guard.is_some() {
            return Err(Error::at(
                "match guards are only supported on value-match arms",
                span,
            ));
        }
        match &arm.pattern {
            Pattern::Variant {
                name,
                binds,
                span: pspan,
                ..
            } if name == "Pass" || name == "Fail" => {
                if binds.len() != 1 {
                    return Err(Error::at(
                        format!("`{name}` pattern requires a binding `(name)` or `(_)`"),
                        *pspan,
                    ));
                }
                if name == "Pass" {
                    if pass_arm.is_some() {
                        return Err(Error::at("duplicate Pass arm", span));
                    }
                    pass_arm = Some(arm);
                } else {
                    if fail_arm.is_some() {
                        return Err(Error::at("duplicate Fail arm", span));
                    }
                    fail_arm = Some(arm);
                }
            }
            Pattern::Variant {
                name, span: pspan, ..
            } => {
                return Err(Error::at(
                    format!("unsupported match pattern `{name}` for Trial"),
                    *pspan,
                ));
            }
            Pattern::StringLit { span: pspan, .. }
            | Pattern::IntLit { span: pspan, .. }
            | Pattern::BoolLit { span: pspan, .. }
            | Pattern::Or { span: pspan, .. }
            | Pattern::Bind { span: pspan, .. }
            | Pattern::Else { span: pspan } => {
                return Err(Error::at(
                    "literal/`else`/or/bind patterns are not valid for Trial match",
                    *pspan,
                ));
            }
        }
    }
    let pass_arm = pass_arm.ok_or_else(|| Error::at("match missing Pass arm", span))?;
    let fail_arm = fail_arm.ok_or_else(|| Error::at("match missing Fail arm", span))?;

    let Some((ok_ty, err_ty)) = trial_parts(&scrut_ty) else {
        return Err(Error::at("match only supports Trial", span));
    };
    let ok_ty = ok_ty.clone();
    let err_ty = err_ty.clone();

    let pass_bind = match &pass_arm.pattern {
        Pattern::Variant { binds, .. } => binds.first().cloned().flatten(),
        _ => None,
    };
    let fail_bind = match &fail_arm.pattern {
        Pattern::Variant { binds, .. } => binds.first().cloned().flatten(),
        _ => None,
    };

    let pass_ty = {
        ctx.push_scope();
        if let Some(ref name) = pass_bind {
            let pp = Place {
                name: sanitize_name(name),
            };
            ctx.define(name, ok_ty.clone(), false, pp);
        }
        let ty = infer_block_ty(ctx, &pass_arm.body)?;
        ctx.pop_scope();
        ty
    };
    let fail_ty = {
        ctx.push_scope();
        if let Some(ref name) = fail_bind {
            let fp = Place {
                name: sanitize_name(name),
            };
            ctx.define(name, err_ty.clone(), false, fp);
        }
        let ty = infer_block_ty(ctx, &fail_arm.body)?;
        ctx.pop_scope();
        ty
    };
    let result_ty = unify_match_arm_tys(&pass_ty, &fail_ty, span)?;

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
    let pass_place = ctx.fresh("mb");
    if ok_ty != Ty::Void {
        ctx.stmts.push(Stmt::Assign {
            place: pass_place.clone(),
            ty: ok_ty.clone(),
            value: RValue::TrialOk(scrut_p.clone()),
        });
        if needs_drop(&ok_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: pass_place.clone(),
                ty: ok_ty.clone(),
            });
        }
    }
    if let Some(ref name) = pass_bind {
        ctx.define(name, ok_ty.clone(), false, pass_place);
    }
    let pass_val = lower_block_into(ctx, &pass_arm.body, &result_ty)?;
    ctx.pop_scope();
    let mut then_body = std::mem::take(&mut ctx.stmts);
    assign_match_result(&mut then_body, &out, pass_val, &result_ty);

    ctx.push_scope();
    let fail_place = ctx.fresh("mb");
    if err_ty != Ty::Void {
        ctx.stmts.push(Stmt::Assign {
            place: fail_place.clone(),
            ty: err_ty.clone(),
            value: RValue::TrialErr(scrut_p),
        });
        if needs_drop(&err_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: fail_place.clone(),
                ty: err_ty.clone(),
            });
        }
    }
    if let Some(ref name) = fail_bind {
        ctx.define(name, err_ty.clone(), false, fail_place);
    }
    let fail_val = lower_block_into(ctx, &fail_arm.body, &result_ty)?;
    ctx.pop_scope();
    let mut else_body = std::mem::take(&mut ctx.stmts);
    assign_match_result(&mut else_body, &out, fail_val, &result_ty);

    ctx.stmts = outer;
    if let Some(ref out_p) = out {
        init_place(ctx, out_p, &result_ty)?;
    }
    ctx.stmts.push(Stmt::If {
        cond: is_pass,
        then_body,
        else_body,
    });

    match_result_place(ctx, out, result_ty)
}

fn lower_match_may(
    ctx: &mut LowerCtx,
    scrut_p: Place,
    scrut_ty: Ty,
    arms: &[MatchArm],
    span: Span,
) -> Result<(Place, Ty)> {
    let mut has_arm: Option<&MatchArm> = None;
    let mut not_arm: Option<&MatchArm> = None;
    for arm in arms {
        if arm.guard.is_some() {
            return Err(Error::at(
                "match guards are only supported on value-match arms",
                span,
            ));
        }
        match &arm.pattern {
            Pattern::Variant {
                name,
                binds,
                span: pspan,
                ..
            } if name == "Has" => {
                if binds.len() != 1 {
                    return Err(Error::at(
                        "`Has` pattern requires a binding `(name)` or `(_)`",
                        *pspan,
                    ));
                }
                if has_arm.is_some() {
                    return Err(Error::at("duplicate Has arm", span));
                }
                has_arm = Some(arm);
            }
            Pattern::Variant {
                name,
                binds,
                span: pspan,
                ..
            } if name == "Not" => {
                if !binds.is_empty() {
                    return Err(Error::at(
                        "`Not` pattern takes no binding (write `Not { … }`)",
                        *pspan,
                    ));
                }
                if not_arm.is_some() {
                    return Err(Error::at("duplicate Not arm", span));
                }
                not_arm = Some(arm);
            }
            Pattern::Variant {
                name, span: pspan, ..
            } => {
                return Err(Error::at(
                    format!("unsupported match pattern `{name}` for May"),
                    *pspan,
                ));
            }
            Pattern::StringLit { span: pspan, .. }
            | Pattern::IntLit { span: pspan, .. }
            | Pattern::BoolLit { span: pspan, .. }
            | Pattern::Or { span: pspan, .. }
            | Pattern::Bind { span: pspan, .. }
            | Pattern::Else { span: pspan } => {
                return Err(Error::at(
                    "literal/`else`/or/bind patterns are not valid for May match",
                    *pspan,
                ));
            }
        }
    }
    let has_arm = has_arm.ok_or_else(|| Error::at("match missing Has arm", span))?;
    let not_arm = not_arm.ok_or_else(|| Error::at("match missing Not arm", span))?;

    let inner_ty = may_inner(&scrut_ty)
        .ok_or_else(|| Error::at("match only supports May", span))?
        .clone();

    let has_bind = match &has_arm.pattern {
        Pattern::Variant { binds, .. } => binds.first().cloned().flatten(),
        _ => None,
    };

    let has_ty = {
        ctx.push_scope();
        if let Some(ref name) = has_bind {
            let pp = Place {
                name: sanitize_name(name),
            };
            ctx.define(name, inner_ty.clone(), false, pp);
        }
        let ty = infer_block_ty(ctx, &has_arm.body)?;
        ctx.pop_scope();
        ty
    };
    let not_ty = {
        ctx.push_scope();
        let ty = infer_block_ty(ctx, &not_arm.body)?;
        ctx.pop_scope();
        ty
    };
    let result_ty = unify_match_arm_tys(&has_ty, &not_ty, span)?;

    let out = if result_ty == Ty::Void {
        None
    } else {
        Some(ctx.fresh("m"))
    };

    let tag = ctx.fresh("mtag");
    ctx.assign(tag.clone(), Ty::Int64, RValue::MayTag(scrut_p.clone()));
    let zero = ctx.fresh("mz");
    ctx.assign(zero.clone(), Ty::Int64, RValue::Int(0));
    let is_has = ctx.fresh("ish");
    ctx.assign(
        is_has.clone(),
        Ty::Bool,
        RValue::Binary {
            op: BinaryOp::Eq,
            left: tag,
            right: zero,
        },
    );

    let outer = std::mem::take(&mut ctx.stmts);

    ctx.push_scope();
    let has_place = ctx.fresh("mb");
    if inner_ty != Ty::Void {
        ctx.stmts.push(Stmt::Assign {
            place: has_place.clone(),
            ty: inner_ty.clone(),
            value: RValue::MayValue(scrut_p),
        });
        if needs_drop(&inner_ty) {
            ctx.stmts.push(Stmt::Dup {
                place: has_place.clone(),
                ty: inner_ty.clone(),
            });
        }
    }
    if let Some(ref name) = has_bind {
        ctx.define(name, inner_ty.clone(), false, has_place);
    }
    let has_val = lower_block_into(ctx, &has_arm.body, &result_ty)?;
    ctx.pop_scope();
    let mut then_body = std::mem::take(&mut ctx.stmts);
    assign_match_result(&mut then_body, &out, has_val, &result_ty);

    ctx.push_scope();
    let not_val = lower_block_into(ctx, &not_arm.body, &result_ty)?;
    ctx.pop_scope();
    let mut else_body = std::mem::take(&mut ctx.stmts);
    assign_match_result(&mut else_body, &out, not_val, &result_ty);

    ctx.stmts = outer;
    if let Some(ref out_p) = out {
        init_place(ctx, out_p, &result_ty)?;
    }
    ctx.stmts.push(Stmt::If {
        cond: is_has,
        then_body,
        else_body,
    });

    match_result_place(ctx, out, result_ty)
}

fn unify_match_arm_tys(a: &Ty, b: &Ty, span: Span) -> Result<Ty> {
    match (a, b) {
        (Ty::Void, Ty::Void) => Ok(Ty::Void),
        (x, y) if x == y => Ok(x.clone()),
        (x, y) => Err(Error::at(
            format!("match arms have different types ({x:?} vs {y:?})"),
            span,
        )),
    }
}

fn assign_match_result(
    body: &mut Vec<Stmt>,
    out: &Option<Place>,
    val: Option<Place>,
    result_ty: &Ty,
) {
    if let (Some(out_p), Some(v)) = (out, val) {
        if needs_drop(result_ty) {
            body.push(Stmt::Dup {
                place: v.clone(),
                ty: result_ty.clone(),
            });
        }
        body.push(Stmt::Assign {
            place: out_p.clone(),
            ty: result_ty.clone(),
            value: RValue::Use(v),
        });
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
        Ty::Bool => ctx.assign(place.clone(), ty.clone(), RValue::Bool(false)),
        Ty::String | Ty::Bytes | Ty::Buffer | Ty::Array { .. } => {
            ctx.note_drop_place(place, ty);
            ctx.stmts.push(Stmt::Assign {
                place: place.clone(),
                ty: ty.clone(),
                value: RValue::Null,
            });
        }
        Ty::Void => {}
        Ty::FixedArray { .. } | Ty::Address(_) => {
            return Err(Error::new(format!("cannot allocate local of type {ty:?}")));
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
        Ty::May { inner } => {
            // Zero as Not so branch-skipped drops are no-ops.
            let z = ctx.fresh("mz");
            init_place(ctx, &z, inner)?;
            ctx.assign(
                place.clone(),
                ty.clone(),
                RValue::MayMake {
                    tag: MayTag::Not,
                    payload: z,
                    may_ty: ty.clone(),
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
                // Zero-ish placeholder so branch-skipped drops are safe (NULL heap fields).
                let mut fields = Vec::new();
                for f in &info.fields {
                    let fp = ctx.fresh("zf");
                    match &f.ty {
                        Ty::Int64 => ctx.assign(fp.clone(), Ty::Int64, RValue::Int(0)),
                        Ty::Bool => ctx.assign(fp.clone(), Ty::Bool, RValue::Bool(false)),
                        Ty::String | Ty::Bytes | Ty::Buffer | Ty::Array { .. } => {
                            ctx.note_drop_place(&fp, &f.ty);
                            ctx.stmts.push(Stmt::Assign {
                                place: fp.clone(),
                                ty: f.ty.clone(),
                                value: RValue::Null,
                            });
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
            ctx.assign(p.clone(), Ty::Int64, RValue::Binary { op, left, right });
            Ok((p, Ty::Int64))
        }
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr => {
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
            ctx.assign(p.clone(), Ty::Int64, RValue::Binary { op, left, right });
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
            if lt == Ty::String && rt == Ty::String {
                if !matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
                    return Err(Error::at("String only supports `==` and `!=`", span));
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
        return Err(Error::at(format!("`{callee}` expects 1 argument"), span));
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
        // Droppable payload (String/Bytes/User): bump so Trial and the temp both own
        // (temp dropped at fn end). User Dup field-walks heap members once via emit_rc.
        if needs_drop(&expected) {
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

fn lower_has(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    if args.len() != 1 {
        return Err(Error::at("`Has` expects 1 argument", span));
    }
    let expr = match &args[0] {
        Arg::Pos(e) => e,
        Arg::Named { value, .. } => value,
    };
    let (payload_raw, pty) = lower_expr(ctx, expr)?;

    if let Ty::May { inner } = ctx.fn_ret.clone() {
        let expected = inner.as_ref().clone();
        let payload = coerce_payload(ctx, payload_raw, &pty, &expected, span)?;
        if needs_drop(&expected) {
            ctx.stmts.push(Stmt::Dup {
                place: payload.clone(),
                ty: expected.clone(),
            });
        }
        let p = ctx.fresh("may");
        let ty = ctx.fn_ret.clone();
        ctx.assign(
            p.clone(),
            ty.clone(),
            RValue::MayMake {
                tag: MayTag::Has,
                payload,
                may_ty: ty.clone(),
            },
        );
        return Ok((p, ty));
    }

    // Infer Has(Int) → May<Int64>.
    let payload = if pty == Ty::Int64 {
        payload_raw
    } else if is_int(&pty) {
        coerce_place(ctx, payload_raw, &pty, &Ty::Int64, span)?
    } else {
        return Err(Error::at(
            format!("`Has` payload type {pty:?} needs an annotated May return type"),
            span,
        ));
    };
    let ty = Ty::May {
        inner: Box::new(Ty::Int64),
    };
    let p = ctx.fresh("may");
    ctx.assign(
        p.clone(),
        ty.clone(),
        RValue::MayMake {
            tag: MayTag::Has,
            payload,
            may_ty: ty.clone(),
        },
    );
    Ok((p, ty))
}

fn lower_not(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    if !args.is_empty() {
        return Err(Error::at("`Not()` takes no arguments", span));
    }
    let ty = if let Ty::May { .. } = &ctx.fn_ret {
        ctx.fn_ret.clone()
    } else {
        // Bare Not() outside `-> May<…>` (e.g. `Pass(Not())` with Trial ok = May):
        // default May<Int64> like Has(int) inference; monomorph must match via coerce.
        Ty::May {
            inner: Box::new(Ty::Int64),
        }
    };
    let dummy = ctx.fresh("notp");
    // Dummy payload zero for compound literal (ignored when tag=Not).
    if let Ty::May { inner } = &ty {
        init_place(ctx, &dummy, inner)?;
    }
    let p = ctx.fresh("may");
    ctx.assign(
        p.clone(),
        ty.clone(),
        RValue::MayMake {
            tag: MayTag::Not,
            payload: dummy,
            may_ty: ty.clone(),
        },
    );
    Ok((p, ty))
}

fn lower_call(ctx: &mut LowerCtx, callee: &str, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
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
        "Has" => lower_has(ctx, args, span),
        "Not" => lower_not(ctx, args, span),
        "Bytes" => {
            // `Bytes(n)` — zero-filled buffer (R9 / D18). Not a user type.
            lower_bytes_new(ctx, args, span)
        }
        "Buffer" => lower_buffer_new(ctx, args, span),
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
                    let defaults: Vec<Option<Expr>> =
                        info.fields.iter().map(|f| f.default.clone()).collect();
                    let field_places = resolve_call_args_with_defaults(
                        ctx,
                        &formal,
                        &defaults,
                        args,
                        span,
                        &info.def_mod,
                    )?;
                    ctx.assign(
                        p.clone(),
                        ty.clone(),
                        RValue::StructLit {
                            type_name: mangled_ty,
                            fields: field_places,
                        },
                    );
                    // StructLit copies field pointers; take our own refs so arg temps can drop.
                    if needs_drop(&ty) {
                        ctx.stmts.push(Stmt::Dup {
                            place: p.clone(),
                            ty: ty.clone(),
                        });
                    }
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
            let arg_places = resolve_call_args_with_defaults(
                ctx,
                &sig.params,
                &sig.defaults,
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

/// Coerce a place to a target type when a conversion is defined (FFI integer widths).
fn coerce_place(ctx: &mut LowerCtx, place: Place, from: &Ty, to: &Ty, span: Span) -> Result<Place> {
    if from == to {
        return Ok(place);
    }
    let ok = is_int(from) && is_int(to);
    if !ok {
        return Err(Error::at(format!("cannot coerce {from:?} to {to:?}"), span));
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
        if *exp != Ty::Void
            && can_coerce_ty(&ctx.types, &then_ty, exp)
            && (else_branch.is_none()
                || can_coerce_ty(&ctx.types, &else_ty, exp)
                || else_ty == Ty::Void)
        {
            exp.clone()
        } else {
            unify_if_branch_tys(ctx, &then_ty, &else_ty, else_branch.is_none(), span)?
        }
    } else {
        unify_if_branch_tys(ctx, &then_ty, &else_ty, else_branch.is_none(), span)?
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

fn unify_if_branch_tys(
    ctx: &mut LowerCtx,
    then_ty: &Ty,
    else_ty: &Ty,
    else_absent: bool,
    span: Span,
) -> Result<Ty> {
    match (then_ty, else_ty) {
        (Ty::Void, Ty::Void) => Ok(Ty::Void),
        (a, Ty::Void) if else_absent => Ok(a.clone()),
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
                if !matches!(
                    f.ty,
                    Ty::Int64
                        | Ty::Int32
                        | Ty::UInt32
                        | Ty::UInt64
                        | Ty::Byte
                        | Ty::Bool
                        | Ty::Void
                ) {
                    return Err(Error::at(
                        format!(
                            "union member `{vname}` field `{}` must be a scalar for now",
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
        let payload_ty = if lit.is_some() {
            Ty::Void
        } else {
            ty.clone()
        };
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
    };
    let layout = TypeLayout {
        name: syn_name.clone(),
        fields: Vec::new(),
        sum: Some(layout_vars),
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
                None if is_int(&ty) && is_int(expect) => {
                    return Ok(Some(coerce_place(ctx, place, &ty, expect, tail.span)?));
                }
                None => {
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
