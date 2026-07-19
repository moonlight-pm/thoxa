//! Mid-level IR: typed enough for RC insert + C codegen.
//! R0: functions, literals, builtin calls, return.

use crate::ast::{self, ExprKind, Item};
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Program {
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub is_main: bool,
    pub body: Vec<Stmt>,
    pub ret: Option<Operand>,
    pub ret_ty: Ty,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// Evaluate call for side effects (discard result).
    Eval(RValue),
}

#[derive(Debug, Clone)]
pub enum RValue {
    Call { callee: Callee, args: Vec<Operand> },
}

#[derive(Debug, Clone)]
pub enum Callee {
    /// Runtime / builtin: `println`, etc.
    Builtin(String),
}

#[derive(Debug, Clone)]
pub enum Operand {
    Int(i64),
    /// C string literal content (unescaped Thoxa string).
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Void,
    Int64,
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
        Some(other) => {
            return Err(Error::at(
                format!("unsupported return type `{other}` in R0"),
                f.return_type.as_ref().unwrap().span,
            ));
        }
        None => Ty::Void,
    };

    if is_main && ret_ty != Ty::Int64 {
        return Err(Error::at(
            "`main` must return Int64",
            f.name_span,
        ));
    }

    let mut body = Vec::new();
    for expr in &f.body.stmts {
        body.push(lower_stmt_expr(expr)?);
    }

    let ret = match &f.body.tail {
        Some(expr) => Some(lower_operand(expr)?),
        None if ret_ty == Ty::Void => None,
        None => {
            return Err(Error::at(
                "missing return value",
                f.body.span,
            ));
        }
    };

    if let Some(Operand::Int(_)) = &ret {
        // ok for Int64
    } else if ret.is_some() && ret_ty == Ty::Int64 {
        return Err(Error::new("R0 main body must end with an integer literal"));
    }

    Ok(Function {
        name: f.name.clone(),
        is_main,
        body,
        ret,
        ret_ty,
    })
}

fn lower_stmt_expr(expr: &ast::Expr) -> Result<Stmt> {
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            let mut ops = Vec::new();
            for a in args {
                ops.push(lower_operand(a)?);
            }
            // R0 builtins only
            match callee.as_str() {
                "println" | "print" => {
                    if ops.len() != 1 {
                        return Err(Error::at(
                            format!("`{callee}` expects 1 argument"),
                            expr.span,
                        ));
                    }
                    Ok(Stmt::Eval(RValue::Call {
                        callee: Callee::Builtin(callee.clone()),
                        args: ops,
                    }))
                }
                other => Err(Error::at(
                    format!("unknown function `{other}` (R0 only allows print/println)"),
                    expr.span,
                )),
            }
        }
        _ => Err(Error::at(
            "R0 only allows calls as statements before the tail expression",
            expr.span,
        )),
    }
}

fn lower_operand(expr: &ast::Expr) -> Result<Operand> {
    match &expr.kind {
        ExprKind::Int(n) => Ok(Operand::Int(*n)),
        ExprKind::String(s) => Ok(Operand::Str(s.clone())),
        _ => Err(Error::at(
            "R0 operands must be integer or string literals",
            expr.span,
        )),
    }
}
