//! AST for the current language subset (R0–R5).

use crate::span::Span;

#[derive(Debug, Clone)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Function(Function),
    Type(TypeDef),
}

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    pub name_span: Span,
    /// Constructor parameters — for R3 these are the stored fields (in order).
    pub params: Vec<Param>,
    pub methods: Vec<Method>,
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
}

#[derive(Debug, Clone)]
pub struct TypeExpr {
    pub name: String,
    pub args: Vec<TypeExpr>,
    pub span: Span,
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
        init: Expr,
    },
    /// `name = expr` — only legal for `mut` bindings.
    Assign {
        name: String,
        name_span: Span,
        value: Expr,
    },
    Expr(Expr),
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
    If {
        cond: Box<Expr>,
        then_branch: Block,
        else_branch: Option<Block>,
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
    pub body: Block,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Variant {
        name: String,
        bind: String,
        bind_span: Span,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
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
}
