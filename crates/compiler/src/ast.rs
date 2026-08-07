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

/// `import { a, b as c } from "…"` or `import ns from "…"`.
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
    /// Constructor parameters — R3 style `type Point(x: Int64, y: Int64)`.
    /// Empty when the type uses body fields (R7 C-layout `type Regs { … }`).
    pub params: Vec<Param>,
    /// Body fields — R7 style `[mut|let] name: Type` inside the type brace.
    /// Empty when using constructor params (R3).
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<Method>,
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
    pub name: String,
    pub args: Vec<TypeArg>,
    pub span: Span,
}

/// Type argument: a nested type (`Trial<Int64, Int64>`) or a const int (`Array<Byte, 4>`).
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
    Bool(bool),
    /// Plain string literal (no interpolation).
    String(String),
    /// `"…{expr}…"` with literal and expression parts (after `{{`/`}}` unescape).
    Interpolate(Vec<InterpPart>),
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
    /// `loop { body }` — yields `Void`.
    Loop {
        body: Block,
    },
    /// `for name in lo..hi` / `lo..=hi` or `for name in collection { body }` — yields `Void`.
    For {
        name: String,
        name_span: Span,
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
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    /// Optional `if expr` after the pattern (D111). Value-match only for now.
    pub guard: Option<Expr>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    /// `Pass(name)` / `Fail(name)` / `Has(name)` — `bind` is `None` for discard `_` (D66).
    /// Nullary `Not` has `bind: None` and no parentheses (`has_payload: false`).
    /// Body `_` is not special — ordinary name lookup (no pattern binding is introduced).
    Variant {
        name: String,
        /// Bound name, or `None` for `_` discard / nullary patterns.
        bind: Option<String>,
        bind_span: Span,
        /// `false` for nullary `Not` (no `(…)`); `true` for `Pass`/`Fail`/`Has(…)`.
        has_payload: bool,
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
