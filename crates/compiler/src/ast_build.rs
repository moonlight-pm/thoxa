//! Small builders for synthesized AST (generated codecs, D149 / D182).

use crate::ast::*;
use crate::span::Span;

pub(crate) fn if_expr(cond: Expr, then_branch: Block, else_branch: Option<Block>) -> Expr {
    Expr {
        kind: ExprKind::If {
            cond: Box::new(cond),
            then_branch,
            else_branch,
        },
        span: dummy(),
    }
}

pub(crate) fn match_expr(scrutinee: Expr, arms: Vec<MatchArm>) -> Expr {
    Expr {
        kind: ExprKind::Match {
            scrutinee: Box::new(scrutinee),
            arms,
        },
        span: dummy(),
    }
}

pub(crate) fn match_arm_block(
    name: &str,
    binds: Vec<Option<String>>,
    items: Vec<BlockItem>,
    tail: Option<Expr>,
) -> MatchArm {
    MatchArm {
        pattern: Pattern::Variant {
            name: name.into(),
            binds,
            bind_span: dummy(),
            span: dummy(),
        },
        guard: None,
        body: Block {
            items,
            tail: tail.map(Box::new),
            span: dummy(),
        },
    }
}

pub(crate) fn un(op: UnaryOp, expr: Expr) -> Expr {
    Expr {
        kind: ExprKind::Unary {
            op,
            expr: Box::new(expr),
        },
        span: dummy(),
    }
}

pub(crate) fn bool_lit(b: bool) -> Expr {
    Expr {
        kind: ExprKind::Bool(b),
        span: dummy(),
    }
}

pub(crate) fn field(base: Expr, fname: &str) -> Expr {
    Expr {
        kind: ExprKind::Field {
            base: Box::new(base),
            name: fname.into(),
            name_span: dummy(),
        },
        span: dummy(),
    }
}

pub(crate) fn block_stmts(items: Vec<BlockItem>) -> Block {
    Block {
        items,
        tail: None,
        span: dummy(),
    }
}

pub(crate) fn dummy() -> Span {
    Span::dummy()
}

pub(crate) fn ty_named(name: &str) -> TypeExpr {
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

pub(crate) fn ty_array(elem: TypeExpr) -> TypeExpr {
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

pub(crate) fn ty_may(inner: TypeExpr) -> TypeExpr {
    TypeExpr {
        name: "May".into(),
        args: vec![TypeArg::Type(inner)],
        span: dummy(),
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    }
}

pub(crate) fn param(name: &str, ty: TypeExpr) -> Param {
    Param {
        name: name.into(),
        name_span: dummy(),
        positional: false,
        ty,
        default: None,
    }
}

pub(crate) fn param_default(name: &str, ty: TypeExpr, default: Expr) -> Param {
    Param {
        name: name.into(),
        name_span: dummy(),
        positional: false,
        ty,
        default: Some(default),
    }
}

pub(crate) fn call(callee: &str, args: Vec<Arg>) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: callee.into(),
            args,
        },
        span: dummy(),
    }
}

pub(crate) fn method_call(receiver: Expr, method: &str, args: Vec<Arg>) -> Expr {
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

pub(crate) fn try_expr(e: Expr) -> Expr {
    Expr {
        kind: ExprKind::Try(Box::new(e)),
        span: dummy(),
    }
}

pub(crate) fn name(s: &str) -> Expr {
    Expr {
        kind: ExprKind::Name(s.into()),
        span: dummy(),
    }
}

pub(crate) fn str_lit(s: &str) -> Expr {
    Expr {
        kind: ExprKind::String(s.into()),
        span: dummy(),
    }
}

pub(crate) fn int_lit(n: i64) -> Expr {
    Expr {
        kind: ExprKind::Int(n),
        span: dummy(),
    }
}

pub(crate) fn interp_int(var: &str) -> Expr {
    Expr {
        kind: ExprKind::Interpolate(vec![InterpPart::Expr(name(var))]),
        span: dummy(),
    }
}

pub(crate) fn bin(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        span: dummy(),
    }
}

pub(crate) fn let_bind(name: &str, mutable: bool, init: Expr) -> BlockItem {
    BlockItem::Let {
        name: name.into(),
        name_span: dummy(),
        mutable,
        ty: None,
        init,
    }
}

pub(crate) fn let_bind_ty(name: &str, mutable: bool, ty: TypeExpr, init: Expr) -> BlockItem {
    BlockItem::Let {
        name: name.into(),
        name_span: dummy(),
        mutable,
        ty: Some(ty),
        init,
    }
}

pub(crate) fn assign(name: &str, value: Expr) -> BlockItem {
    BlockItem::Assign {
        target: AssignTarget::Name {
            name: name.into(),
            name_span: dummy(),
        },
        value,
    }
}

pub(crate) fn ty_map(key: TypeExpr, value: TypeExpr) -> TypeExpr {
    TypeExpr {
        name: "Map".into(),
        args: vec![TypeArg::Type(key), TypeArg::Type(value)],
        span: dummy(),
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    }
}

/// `{ items; tail }` as an expression.
pub(crate) fn block(items: Vec<BlockItem>, tail: Expr) -> Expr {
    Expr {
        kind: ExprKind::Block(Block {
            items,
            tail: Some(Box::new(tail)),
            span: dummy(),
        }),
        span: dummy(),
    }
}

/// `for name in collection { items }`, or `for name, value in map { items }`.
pub(crate) fn for_each(
    name: &str,
    value: Option<&str>,
    collection: Expr,
    items: Vec<BlockItem>,
) -> BlockItem {
    BlockItem::Expr(Expr {
        kind: ExprKind::For {
            name: name.into(),
            name_span: dummy(),
            value: value.map(|value| (value.to_string(), dummy())),
            start: Box::new(collection),
            end: None,
            inclusive: false,
            body: Block {
                items,
                tail: None,
                span: dummy(),
            },
        },
        span: dummy(),
    })
}

pub(crate) fn empty_array() -> Expr {
    Expr {
        kind: ExprKind::ArrayLit(vec![]),
        span: dummy(),
    }
}
