//! Mid-level IR: typed enough for RC insert + C codegen.
//! R1: locals, arithmetic, bool ops, if-expressions.

use crate::ast::{
    self, BinaryOp, Block, BlockItem, Expr, ExprKind, Item, UnaryOp,
};
use crate::error::{Error, Result};
use crate::span::Span;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Program {
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub is_main: bool,
    pub body: Vec<Stmt>,
    pub ret: Option<Place>,
    pub ret_ty: Ty,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// `ty name = rv;`
    Assign { place: Place, ty: Ty, value: RValue },
    /// Evaluate for side effects.
    Eval(RValue),
    /// `if (cond) { … } else { … }` — both sides may assign into `out` if set.
    If {
        cond: Place,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
}

#[derive(Debug, Clone)]
pub enum RValue {
    Use(Place),
    Int(i64),
    Bool(bool),
    Str(String),
    Unary { op: UnaryOp, arg: Place },
    Binary { op: BinaryOp, left: Place, right: Place },
    Call { callee: Callee, args: Vec<Place> },
}

#[derive(Debug, Clone)]
pub enum Callee {
    Builtin(String),
}

/// SSA-ish temporary or named local.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Void,
    Int64,
    Bool,
    /// C string literal / const char* for println (R1 only).
    CStr,
}

struct LowerCtx {
    stmts: Vec<Stmt>,
    temps: u32,
    /// name → type for locals in scope
    env: Vec<HashMap<String, Ty>>,
}

impl LowerCtx {
    fn new() -> Self {
        Self {
            stmts: Vec::new(),
            temps: 0,
            env: vec![HashMap::new()],
        }
    }

    fn push_scope(&mut self) {
        self.env.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.env.pop();
    }

    fn define(&mut self, name: &str, ty: Ty) {
        self.env
            .last_mut()
            .unwrap()
            .insert(name.to_string(), ty);
    }

    fn lookup(&self, name: &str) -> Option<Ty> {
        for scope in self.env.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(*ty);
            }
        }
        None
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
        }
        self.stmts.push(Stmt::Assign { place, ty, value });
    }
}

pub fn lower_module(module: &ast::Module) -> Result<Program> {
    let mut functions = Vec::new();
    for item in &module.items {
        match item {
            Item::Function(f) => functions.push(lower_function(f)?),
        }
    }
    if !functions.iter().any(|f| f.is_main) {
        return Err(Error::new("program requires `fn main`"));
    }
    Ok(Program { functions })
}

fn lower_function(f: &ast::Function) -> Result<Function> {
    let is_main = f.name == "main";
    let ret_ty = match f.return_type.as_ref().map(|t| t.name.as_str()) {
        Some("Int64") | None if is_main => Ty::Int64,
        Some("Int64") => Ty::Int64,
        Some("Void") => Ty::Void,
        Some("Bool") => Ty::Bool,
        Some(other) => {
            return Err(Error::at(
                format!("unsupported return type `{other}`"),
                f.return_type.as_ref().unwrap().span,
            ));
        }
        None => Ty::Void,
    };

    if is_main && ret_ty != Ty::Int64 {
        return Err(Error::at("`main` must return Int64", f.name_span));
    }

    let mut ctx = LowerCtx::new();
    let ret = lower_block_value(&mut ctx, &f.body, ret_ty)?;

    Ok(Function {
        name: f.name.clone(),
        is_main,
        body: ctx.stmts,
        ret,
        ret_ty,
    })
}

fn lower_block_value(ctx: &mut LowerCtx, block: &Block, expect: Ty) -> Result<Option<Place>> {
    ctx.push_scope();
    for item in &block.items {
        match item {
            BlockItem::Let {
                name,
                name_span,
                init,
            } => {
                let (place, ty) = lower_expr(ctx, init)?;
                // Bind under user name
                let bound = Place {
                    name: sanitize_name(name),
                };
                ctx.assign(bound.clone(), ty, RValue::Use(place));
                ctx.define(name, ty);
                // Keep span used so unused warning doesn't fire
                let _ = name_span;
            }
            BlockItem::Expr(e) => {
                let (place, ty) = lower_expr(ctx, e)?;
                // Side-effecting calls already emitted; discard value
                let _ = (place, ty);
            }
        }
    }

    let result = if let Some(tail) = &block.tail {
        let (place, ty) = lower_expr(ctx, tail)?;
        if expect != Ty::Void && ty != expect && !(expect == Ty::Int64 && ty == Ty::Int64) {
            // Soft: allow if types match; error on clear mismatch
            if ty != expect {
                return Err(Error::at(
                    format!("expected {expect:?}, found {ty:?}"),
                    tail.span,
                ));
            }
        }
        Some(place)
    } else if expect == Ty::Void {
        None
    } else {
        return Err(Error::at("missing block value", block.span));
    };
    ctx.pop_scope();
    Ok(result)
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
            ctx.assign(p.clone(), Ty::CStr, RValue::Str(s.clone()));
            Ok((p, Ty::CStr))
        }
        ExprKind::Name(name) => {
            let ty = ctx.lookup(name).ok_or_else(|| {
                Error::at(format!("undefined name `{name}`"), expr.span)
            })?;
            Ok((
                Place {
                    name: sanitize_name(name),
                },
                ty,
            ))
        }
        ExprKind::Unary { op, expr: inner } => {
            let (arg, arg_ty) = lower_expr(ctx, inner)?;
            match op {
                UnaryOp::Neg => {
                    if arg_ty != Ty::Int64 {
                        return Err(Error::at("unary `-` expects Int64", expr.span));
                    }
                    let p = ctx.fresh("neg");
                    ctx.assign(
                        p.clone(),
                        Ty::Int64,
                        RValue::Unary {
                            op: *op,
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
                        RValue::Unary {
                            op: *op,
                            arg,
                        },
                    );
                    Ok((p, Ty::Bool))
                }
            }
        }
        ExprKind::Binary { op, left, right } => lower_binary(ctx, *op, left, right, expr.span),
        ExprKind::Call { callee, args } => lower_call(ctx, callee, args, expr.span),
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => lower_if(ctx, cond, then_branch, else_branch.as_ref(), expr.span),
        ExprKind::Block(b) => {
            // Infer type from tail by lowering into a temp — require Int64 or Bool from tail
            // Peek: lower with Void if no tail, else we need a two-pass; for R1 use Int64 if unclear
            if b.tail.is_none() {
                lower_block_value(ctx, b, Ty::Void)?;
                let p = ctx.fresh("void");
                ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
                return Ok((p, Ty::Void));
            }
            // Lower tail type by recursive structure — try Int64 first via probing is hard;
            // Lower items then tail with flexible type.
            ctx.push_scope();
            for item in &b.items {
                match item {
                    BlockItem::Let {
                        name,
                        init,
                        name_span: _,
                    } => {
                        let (place, ty) = lower_expr(ctx, init)?;
                        let bound = Place {
                            name: sanitize_name(name),
                        };
                        ctx.assign(bound, ty, RValue::Use(place));
                        ctx.define(name, ty);
                    }
                    BlockItem::Expr(e) => {
                        let _ = lower_expr(ctx, e)?;
                    }
                }
            }
            let (place, ty) = lower_expr(ctx, b.tail.as_ref().unwrap())?;
            ctx.pop_scope();
            Ok((place, ty))
        }
    }
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
    let (out_ty, need) = match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            (Ty::Int64, Ty::Int64)
        }
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            // comparisons: both same, int or bool
            if lt != rt {
                return Err(Error::at("comparison operands must have the same type", span));
            }
            if lt != Ty::Int64 && lt != Ty::Bool {
                return Err(Error::at("comparison only supports Int64 and Bool", span));
            }
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
        BinaryOp::And | BinaryOp::Or => (Ty::Bool, Ty::Bool),
    };
    if lt != need || rt != need {
        return Err(Error::at(
            format!("operator {op:?} expects {need:?} operands"),
            span,
        ));
    }
    let p = ctx.fresh("bin");
    ctx.assign(
        p.clone(),
        out_ty,
        RValue::Binary {
            op,
            left: l,
            right: r,
        },
    );
    Ok((p, out_ty))
}

fn lower_call(
    ctx: &mut LowerCtx,
    callee: &str,
    args: &[Expr],
    span: Span,
) -> Result<(Place, Ty)> {
    match callee {
        "println" | "print" => {
            if args.len() != 1 {
                return Err(Error::at(format!("`{callee}` expects 1 argument"), span));
            }
            let (arg, ty) = lower_expr(ctx, &args[0])?;
            let builtin = match (callee, ty) {
                ("println", Ty::CStr) => "println",
                ("print", Ty::CStr) => "print",
                ("println", Ty::Int64) => "println_i64",
                ("print", Ty::Int64) => "print_i64",
                ("println", Ty::Bool) => "println_bool",
                ("print", Ty::Bool) => "print_bool",
                _ => {
                    return Err(Error::at(
                        format!("`{callee}` cannot print {ty:?}"),
                        span,
                    ));
                }
            };
            ctx.stmts.push(Stmt::Eval(RValue::Call {
                callee: Callee::Builtin(builtin.into()),
                args: vec![arg],
            }));
            // Unit result
            let p = ctx.fresh("u");
            ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
            Ok((p, Ty::Void))
        }
        other => Err(Error::at(
            format!("unknown function `{other}`"),
            span,
        )),
    }
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

    let result_ty = match (then_ty, else_ty) {
        (Ty::Void, Ty::Void) => Ty::Void,
        (a, Ty::Void) if else_branch.is_none() => a,
        (a, b) if a == b => a,
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

    // Lower each branch into its own statement list.
    let outer_stmts = std::mem::take(&mut ctx.stmts);

    ctx.push_scope();
    let then_place = lower_block_into(ctx, then_branch, result_ty)?;
    ctx.pop_scope();
    let mut then_body = std::mem::take(&mut ctx.stmts);
    if let (Some(out_p), Some(tp)) = (&out, then_place) {
        then_body.push(Stmt::Assign {
            place: out_p.clone(),
            ty: result_ty,
            value: RValue::Use(tp),
        });
    }

    let else_body = if let Some(eb) = else_branch {
        ctx.push_scope();
        let else_place = lower_block_into(ctx, eb, result_ty)?;
        ctx.pop_scope();
        let mut body = std::mem::take(&mut ctx.stmts);
        if let (Some(out_p), Some(ep)) = (&out, else_place) {
            body.push(Stmt::Assign {
                place: out_p.clone(),
                ty: result_ty,
                value: RValue::Use(ep),
            });
        }
        body
    } else {
        Vec::new()
    };

    ctx.stmts = outer_stmts;
    if let Some(ref out_p) = out {
        let init = match result_ty {
            Ty::Int64 => RValue::Int(0),
            Ty::Bool => RValue::Bool(false),
            Ty::CStr => RValue::Str(String::new()),
            Ty::Void => RValue::Int(0),
        };
        ctx.assign(out_p.clone(), result_ty, init);
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
    expect: Ty,
) -> Result<Option<Place>> {
    for item in &block.items {
        match item {
            BlockItem::Let {
                name,
                init,
                name_span: _,
            } => {
                let (place, ty) = lower_expr(ctx, init)?;
                let bound = Place {
                    name: sanitize_name(name),
                };
                ctx.assign(bound, ty, RValue::Use(place));
                ctx.define(name, ty);
            }
            BlockItem::Expr(e) => {
                let _ = lower_expr(ctx, e)?;
            }
        }
    }
    if let Some(tail) = &block.tail {
        let (place, ty) = lower_expr(ctx, tail)?;
        if expect != Ty::Void && ty != expect {
            return Err(Error::at(
                format!("expected {expect:?}, found {ty:?}"),
                tail.span,
            ));
        }
        Ok(Some(place))
    } else if expect == Ty::Void {
        Ok(None)
    } else {
        Err(Error::at("missing block value", block.span))
    }
}

fn infer_block_ty(ctx: &LowerCtx, block: &Block) -> Result<Ty> {
    // Lightweight inference: look at tail expression structure without full lower.
    // Use a nested lower in a throwaway context that shares env.
    let mut tmp = LowerCtx {
        stmts: Vec::new(),
        temps: ctx.temps,
        env: ctx.env.clone(),
    };
    if block.tail.is_none() && block.items.is_empty() {
        return Ok(Ty::Void);
    }
    if block.tail.is_none() {
        // only side effects
        return Ok(Ty::Void);
    }
    // Lower fully in temp (mutates temps count — ok)
    match lower_block_into(&mut tmp, block, Ty::Void) {
        Ok(Some(_)) => {
            // re-lower with loose: get type from lower_expr on tail only after items
            let mut tmp2 = LowerCtx {
                stmts: Vec::new(),
                temps: ctx.temps,
                env: ctx.env.clone(),
            };
            for item in &block.items {
                if let BlockItem::Let { name, init, .. } = item {
                    let (place, ty) = lower_expr(&mut tmp2, init)?;
                    let bound = Place {
                        name: sanitize_name(name),
                    };
                    tmp2.assign(bound, ty, RValue::Use(place));
                    tmp2.define(name, ty);
                } else if let BlockItem::Expr(e) = item {
                    let _ = lower_expr(&mut tmp2, e)?;
                }
            }
            let (_, ty) = lower_expr(&mut tmp2, block.tail.as_ref().unwrap())?;
            Ok(ty)
        }
        Ok(None) => Ok(Ty::Void),
        Err(e) => {
            // try infer from tail only for type
            let mut tmp2 = LowerCtx {
                stmts: Vec::new(),
                temps: ctx.temps,
                env: ctx.env.clone(),
            };
            for item in &block.items {
                if let BlockItem::Let { name, init, .. } = item {
                    let (place, ty) = lower_expr(&mut tmp2, init)?;
                    let bound = Place {
                        name: sanitize_name(name),
                    };
                    tmp2.assign(bound, ty, RValue::Use(place));
                    tmp2.define(name, ty);
                }
            }
            if let Some(tail) = &block.tail {
                let (_, ty) = lower_expr(&mut tmp2, tail)?;
                Ok(ty)
            } else {
                Err(e)
            }
        }
    }
}

fn sanitize_name(name: &str) -> String {
    // Avoid clashing with C keywords / our temps
    format!("v_{name}")
}
