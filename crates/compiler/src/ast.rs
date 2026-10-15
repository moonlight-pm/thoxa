//! AST for the current language subset (R0–R9).

use crate::span::Span;

#[derive(Debug, Clone)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Function(Function),
    Type(TypeDef),
    Extern(ExternFn),
    Import(Import),
    /// Top-level `let` / `mut` (module-level value; `mut` is never exported).
    Global(Global),
    /// Top-level statement for **session scripts** (no `fn main`).
    /// Host/session prep lifts these into `Shell.step` body (D90).
    /// Not valid in ordinary programs after session prepare.
    SessionStmt(BlockItem),
}

/// `import { a, b as c } from "…"`, `import ns from "…"`, or
/// `import ns, { a, b as c } from "…"`.
#[derive(Debug, Clone)]
pub struct Import {
    pub kind: ImportKind,
    pub path: String,
    pub path_span: Span,
}

#[derive(Debug, Clone)]
pub enum ImportKind {
    /// `import { name [as alias], … } from "…"`
    Selective(Vec<ImportName>),
    /// `import binding from "…"` — namespace, camelCase binding only.
    Namespace { binding: String, binding_span: Span },
    /// `import ns, { name [as alias], … } from "…"` — namespace + selective in one clause.
    Combined {
        binding: String,
        binding_span: Span,
        names: Vec<ImportName>,
    },
}

#[derive(Debug, Clone)]
pub struct ImportName {
    pub name: String,
    pub name_span: Span,
    pub alias: Option<(String, Span)>,
}

/// Module-level `export? let|mut name = expr`.
#[derive(Debug, Clone)]
pub struct Global {
    pub exported: bool,
    pub mutable: bool,
    pub name: String,
    pub name_span: Span,
    /// Optional annotation: `mut connection: May<Db> = Not()`.
    pub ty: Option<TypeExpr>,
    pub init: Expr,
}

/// `extern fn name(…) -> T from "lib" ["c_symbol"]` — no body.
#[derive(Debug, Clone)]
pub struct ExternFn {
    pub exported: bool,
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    /// Library name from `from "…"`, e.g. `"c"`.
    pub library: String,
    pub library_span: Span,
    /// Optional C symbol when it differs from the Thoxa name.
    pub c_symbol: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub exported: bool,
    pub name: String,
    pub name_span: Span,
    /// Type parameters: `type Box<T>(value: T)` / `type Opt<T> = …` (D118 prelude path).
    pub type_params: Vec<String>,
    /// Constructor parameters — R3 style `type Point(x: Int64, y: Int64)`.
    /// Empty when the type uses body fields (R7 C-layout `type Regs { … }`).
    /// Empty for nullary `type Up` and for union aliases.
    pub params: Vec<Param>,
    /// Body fields — R7 style `[mut|let] name: Type` inside the type brace.
    /// Empty when using constructor params (R3).
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<Method>,
    /// HTTP-style route clauses in the type body (`GET "/…" { … }`, **D150**).
    /// Cleared by `route_desugar` into handlers + `route`.
    pub routes: Vec<RouteClause>,
    /// `type Name = A | B | …` (D118). When set, params/fields/methods must be empty.
    pub union_members: Option<Vec<TypeExpr>>,
}

/// `GET "/home" { … }` / `PUT "/user/:id" (id: Int64) -> T { … }` inside a type body (**D150**).
#[derive(Debug, Clone)]
pub struct RouteClause {
    /// HTTP method as written (`GET`, `PUT`, …).
    pub method: String,
    pub method_span: Span,
    /// Path pattern string literal (e.g. `"/user/:user_id"`).
    pub pattern: String,
    pub pattern_span: Span,
    /// Optional formals after the pattern (path captures, etc.).
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Block,
}

/// A field declared in a type body: `mut a: UInt32` or `let cLine: Byte`.
#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub mutable: bool,
    pub name: String,
    pub name_span: Span,
    pub ty: TypeExpr,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub exported: bool,
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub exported: bool,
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub name_span: Span,
    /// Positional marker `!` (accepted; call sites may still use order or names).
    pub positional: bool,
    pub ty: TypeExpr,
    /// Default value (`name: T = expr`). Positional (`!`) params cannot have defaults.
    /// Omitted arguments are filled at the call site (SPEC §4.1 / M3).
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct TypeExpr {
    /// Named type (`Int64`, `Has`, `Circle`). Empty when `kind` is a literal type.
    pub name: String,
    pub args: Vec<TypeArg>,
    pub span: Span,
    /// Literal type (`0`, `true`, `"x"`) — D118. When set, `name` is unused.
    pub lit: Option<TypeLit>,
    /// Trailing extract mark on this type as a whole: `Has<T>!` or `Not?` (D118).
    pub mark: Option<TypeMark>,
    /// Inline member-set union `A | B` (D131). When set, `name`/`lit` are unused.
    pub union_members: Option<Vec<TypeExpr>>,
    /// Function type `(a: T) -> R` (D132). When set, `name`/`lit`/`union_members` unused.
    pub fn_type: Option<Box<FnType>>,
    /// `T where …` refinement (builtin validation experiment).
    pub where_clause: Option<WhereClause>,
}

/// `where >= 1 & <= 12 "…"`.
#[derive(Debug, Clone)]
pub struct WhereClause {
    pub expr: WhereExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum WhereExpr {
    And(Box<WhereExpr>, Box<WhereExpr>),
    Or(Box<WhereExpr>, Box<WhereExpr>),
    Atom(WhereAtom),
}

#[derive(Debug, Clone)]
pub struct WhereAtom {
    pub pred: WherePred,
    pub message: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum WherePred {
    Ge(WhereLit),
    Le(WhereLit),
    Gt(WhereLit),
    Lt(WhereLit),
    Eq(WhereLit),
    Ne(WhereLit),
    /// Inclusive range. Missing end is unbounded.
    Range {
        lo: Option<WhereLit>,
        hi: Option<WhereLit>,
    },
    /// Membership (`where "espresso"`).
    Lit(WhereLit),
}

#[derive(Debug, Clone)]
pub enum WhereLit {
    Int(i64),
    Float(f64),
    String(String),
}

/// `(name: Type, …) -> Ret` in type position (D132).
#[derive(Debug, Clone)]
pub struct FnType {
    pub params: Vec<(String, TypeExpr)>,
    pub ret: TypeExpr,
}

/// Literal types in typespace (D118): `type Flag = 0 | 1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeLit {
    Int(i64),
    Bool(bool),
    String(String),
}

/// Type-level extract marks (D118): `!` success, `?` residual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeMark {
    /// Success extract (`!`).
    Extract,
    /// Residual extract (`?`).
    Residual,
}

/// Type argument: a nested type (`Trial<Int64, Int64>`) or a const int (`Array<Byte, 4>`).
/// Nested types may carry `!` / `?` marks (`Has<T!>`).
#[derive(Debug, Clone)]
pub enum TypeArg {
    Type(TypeExpr),
    Int { value: i64, span: Span },
}

#[derive(Debug, Clone)]
pub struct Block {
    pub items: Vec<BlockItem>,
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum BlockItem {
    Let {
        name: String,
        name_span: Span,
        /// `true` for `mut name = …`; `false` for `let name = …`.
        mutable: bool,
        /// Optional type annotation: `let xs: Array<Int64> = []`.
        ty: Option<TypeExpr>,
        init: Expr,
    },
    /// Assignment to a place: `name = …`, `name.field = …`, or `name.field[i] = …`.
    Assign {
        target: AssignTarget,
        value: Expr,
    },
    Expr(Expr),
}

/// Left-hand side of an assignment statement.
#[derive(Debug, Clone)]
pub enum AssignTarget {
    /// `name = expr` — only legal for `mut` bindings.
    Name { name: String, name_span: Span },
    /// `base.field = expr` — field must be `mut`.
    Field {
        base: String,
        base_span: Span,
        field: String,
        field_span: Span,
    },
    /// `base.field[index] = expr` — fixed-array element write.
    Index {
        base: String,
        base_span: Span,
        field: String,
        field_span: Span,
        index: Expr,
    },
    /// `name[index] = expr` — dynamic Array element write on a mut binding.
    DynIndex {
        name: String,
        name_span: Span,
        index: Expr,
    },
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    /// Float64 literal (`1.5`, `1e10`, `3.14f64`).
    Float(f64),
    Bool(bool),
    /// Plain string literal (no interpolation).
    String(String),
    /// `"…{expr}…"` with literal and expression parts (after `{{`/`}}` unescape).
    Interpolate(Vec<InterpPart>),
    /// `sql"…{expr}…"` — tagged interpolator (D144). v0 tag is `sql` only.
    /// Holes are embedded as SQL (strings quoted; `Sql` fragments spliced).
    TaggedInterp {
        tag: String,
        parts: Vec<InterpPart>,
    },
    Name(String),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `f(…)` or `Point(x: 1, y: 2)` — free function / constructor call.
    Call {
        callee: String,
        args: Vec<Arg>,
    },
    /// `(expr)(args)` / call through a function value (D132).
    IndirectCall {
        callee: Box<Expr>,
        args: Vec<Arg>,
    },
    /// `--> name(args)` — begin the call; do not wait (experiment/implicit-async).
    Start {
        callee: String,
        callee_span: Span,
        args: Vec<Arg>,
    },
    /// `recv.field` or `recv.method(…)` after postfix lowering.
    Field {
        base: Box<Expr>,
        name: String,
        name_span: Span,
    },
    /// `recv.method(args)`
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        method_span: Span,
        args: Vec<Arg>,
    },
    /// `base[index]` — fixed-array field element (R7) or dynamic Array element.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// `[e1, e2, …]` — dynamic Array literal.
    ArrayLit(Vec<Expr>),
    /// `{ k: v, … }` — Map literal (expression position; empty `{}` is still a block).
    MapLit(Vec<(Expr, Expr)>),
    If {
        cond: Box<Expr>,
        then_branch: Block,
        else_branch: Option<Block>,
    },
    /// `while cond { body }` — yields `Void`.
    While {
        cond: Box<Expr>,
        body: Block,
    },
    /// `loop { body }` — yields `Void` if `break` can exit; otherwise diverges
    /// (Never / bottom type, D128) so it can be a function's last expression.
    Loop {
        body: Block,
    },
    /// `for name in lo..hi` / `lo..=hi` or `for name in collection { body }` — yields `Void`.
    /// `for k, v in map` sets `value`.
    For {
        name: String,
        name_span: Span,
        /// Second binder for `for k, v in map`.
        value: Option<(String, Span)>,
        /// Range start, or the collection expression when `end` is None.
        start: Box<Expr>,
        /// Range end when iterating a range; `None` means `for x in collection`.
        end: Option<Box<Expr>>,
        /// `true` for `..=` (inclusive end), `false` for `..` (exclusive). Ignored for collection for.
        inclusive: bool,
        body: Block,
    },
    /// `break` — exits the nearest enclosing loop; type `Void`.
    Break,
    /// `continue` — next iteration of the nearest enclosing loop; type `Void`.
    Continue,
    /// `return` / `return expr` — early exit from the enclosing function/method.
    /// Typed as the function return type (bare `return` requires `Void`).
    Return {
        value: Option<Box<Expr>>,
    },
    Block(Block),
    Try(Box<Expr>),
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
}

#[derive(Debug, Clone)]
pub enum InterpPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub enum Arg {
    /// Positional argument.
    Pos(Expr),
    /// Named argument `name: expr`.
    Named {
        name: String,
        name_span: Span,
        value: Expr,
    },
    /// JSX `{...expr}` — HTML: `Array<Attr>`; component: same type as the ctor.
    Spread { value: Expr, span: Span },
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    /// Optional `if expr` after the pattern (D111 / D120).
    pub guard: Option<Expr>,
    /// Arm body: block `{ … }` or sugar `=> expr` (desugared to a one-expr block, D130).
    pub body: Block,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    /// `Pass(name)` / `Fail(name)` / `Has(name)` / `Rect(w, h)` — payload binds in field order.
    /// Empty `binds` = tag-only: nullary members, or payload members with no destructure (D130).
    /// Entry `None` = discard `_` (D66). Body `_` is ordinary name lookup.
    Variant {
        name: String,
        /// One entry per constructor field; empty = tag-only (nullary or discard-all payload).
        binds: Vec<Option<String>>,
        bind_span: Span,
        span: Span,
    },
    /// String literal pattern: `"exit" { … }` (scrutinee must be String).
    StringLit { value: String, span: Span },
    /// Integer literal pattern: `0 { … }` (scrutinee must be Int64).
    IntLit { value: i64, span: Span },
    /// Bool literal pattern: `true` / `false` (scrutinee must be Bool).
    BoolLit { value: bool, span: Span },
    /// Or-pattern of value literals: `0 | 1 | 2 { … }` (D108). Only for value match.
    Or { alts: Vec<Pattern>, span: Span },
    /// Bare name bind: `n { … }` binds the scrutinee (D110). Value-match catch-all.
    Bind { name: String, span: Span },
    /// Catch-all `else { … }` — required for value matches unless a Bind arm ends the match (D87/D110).
    Else { span: Span },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    /// Bitwise complement `~`
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    /// Bitwise and `&`
    BitAnd,
    /// Bitwise or `|`
    BitOr,
    /// Bitwise xor `^`
    BitXor,
    /// Shift left `<<`
    Shl,
    /// Shift right `>>`
    Shr,
}
