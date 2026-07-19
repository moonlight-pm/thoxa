//! AST for the current language subset (R0–R1).

use crate::span::Span;

#[derive(Debug, Clone)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Function(Function),
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
    pub ty: Option<TypeExpr>,
}

#[derive(Debug, Clone)]
pub struct TypeExpr {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Block {
    /// Non-tail items (lets and side-effecting expressions).
    pub items: Vec<BlockItem>,
    /// Final expression value of the block (if any).
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum BlockItem {
    Let {
        name: String,
        name_span: Span,
        init: Expr,
    },
    /// Expression evaluated for side effects (result discarded).
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
    String(String),
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
    Call {
        callee: String,
        args: Vec<Expr>,
    },
    If {
        cond: Box<Expr>,
        then_branch: Block,
        else_branch: Option<Block>,
    },
    Block(Block),
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
