//! Mid-level IR: typed enough for RC insert + C codegen.
//! R3: user types (unboxed structs), construction, methods, field access.

use crate::ast::{
    self, Arg, BinaryOp, Block, BlockItem, Expr, ExprKind, Item, MatchArm, Method, Pattern,
    TypeDef, TypeExpr, UnaryOp,
};
use crate::error::{Error, Result};
use crate::span::Span;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Program {
    pub types: Vec<TypeLayout>,
    pub functions: Vec<Function>,
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
    Eval(RValue),
    If {
        cond: Place,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    Return { value: Place },
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
    TrialMake { tag: TrialTag, payload: Place },
    TrialTag(Place),
    TrialPayload(Place),
    /// Construct a user type from field places (declaration order).
    StructLit { type_name: String, fields: Vec<Place> },
    /// `base.field`
    Field { base: Place, field: String },
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
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Void,
    Int64,
    Bool,
    CStr,
    TrialI64I64,
    User(String),
}

#[derive(Clone)]
struct FnSig {
    params: Vec<(String, Ty)>,
    ret: Ty,
}

#[derive(Clone)]
struct MethodSig {
    /// Method params (not including receiver).
    params: Vec<(String, Ty)>,
    ret: Ty,
    exported: bool,
}

#[derive(Clone)]
struct TypeInfo {
    fields: Vec<(String, Ty)>,
    methods: HashMap<String, MethodSig>,
}

struct LowerCtx {
    stmts: Vec<Stmt>,
    temps: u32,
    env: Vec<HashMap<String, Ty>>,
    fns: HashMap<String, FnSig>,
    types: HashMap<String, TypeInfo>,
    fn_ret: Ty,
}

impl LowerCtx {
    fn new(
        fns: HashMap<String, FnSig>,
        types: HashMap<String, TypeInfo>,
        fn_ret: Ty,
    ) -> Self {
        Self {
            stmts: Vec::new(),
            temps: 0,
            env: vec![HashMap::new()],
            fns,
            types,
            fn_ret,
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
                return Some(ty.clone());
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
    let mut types_info: HashMap<String, TypeInfo> = HashMap::new();
    let mut type_layouts: Vec<TypeLayout> = Vec::new();
    let mut type_defs: Vec<&TypeDef> = Vec::new();
    let mut free_fns: Vec<&ast::Function> = Vec::new();

    for item in &module.items {
        match item {
            Item::Type(td) => {
                if types_info.contains_key(&td.name) {
                    return Err(Error::at(
                        format!("duplicate type `{}`", td.name),
                        td.name_span,
                    ));
                }
                let mut fields = Vec::new();
                for p in &td.params {
                    let ty = resolve_ty(&p.ty, &types_info)?;
                    // R3: only scalar fields (or nested later).
                    if !matches!(ty, Ty::Int64 | Ty::Bool) {
                        return Err(Error::at(
                            "R3 type fields must be Int64 or Bool",
                            p.ty.span,
                        ));
                    }
                    fields.push((p.name.clone(), ty));
                }
                types_info.insert(
                    td.name.clone(),
                    TypeInfo {
                        fields: fields.clone(),
                        methods: HashMap::new(),
                    },
                );
                type_layouts.push(TypeLayout {
                    name: td.name.clone(),
                    fields: fields
                        .iter()
                        .map(|(n, t)| FieldLayout {
                            name: n.clone(),
                            c_name: sanitize_field(n),
                            ty: t.clone(),
                        })
                        .collect(),
                });
                type_defs.push(td);
            }
            Item::Function(f) => free_fns.push(f),
        }
    }

    // Fill method signatures (types must already exist for return types).
    for td in &type_defs {
        for m in &td.methods {
            let ret = match m.return_type.as_ref() {
                Some(t) => resolve_ty(t, &types_info)?,
                None => Ty::Void,
            };
            let params = m
                .params
                .iter()
                .map(|p| Ok((p.name.clone(), resolve_ty(&p.ty, &types_info)?)))
                .collect::<Result<Vec<_>>>()?;
            let sig = MethodSig {
                params,
                ret,
                exported: m.exported,
            };
            types_info
                .get_mut(&td.name)
                .unwrap()
                .methods
                .insert(m.name.clone(), sig);
        }
    }

    let mut fns: HashMap<String, FnSig> = HashMap::new();
    for f in &free_fns {
        let ret = resolve_return_ty(f, &types_info)?;
        let params = f
            .params
            .iter()
            .map(|p| Ok((p.name.clone(), resolve_ty(&p.ty, &types_info)?)))
            .collect::<Result<Vec<_>>>()?;
        if fns
            .insert(
                f.name.clone(),
                FnSig {
                    params,
                    ret,
                },
            )
            .is_some()
        {
            return Err(Error::at(
                format!("duplicate function `{}`", f.name),
                f.name_span,
            ));
        }
    }
    if !fns.contains_key("main") {
        return Err(Error::new("program requires `fn main`"));
    }

    let mut functions = Vec::new();

    // Lower methods as free functions with receiver.
    for td in &type_defs {
        for m in &td.methods {
            functions.push(lower_method(td, m, &fns, &types_info)?);
        }
    }
    for f in &free_fns {
        functions.push(lower_function(f, &fns, &types_info)?);
    }

    Ok(Program {
        types: type_layouts,
        functions,
    })
}

fn resolve_return_ty(f: &ast::Function, types: &HashMap<String, TypeInfo>) -> Result<Ty> {
    let is_main = f.name == "main";
    match f.return_type.as_ref() {
        None if is_main => Ok(Ty::Int64),
        None => Ok(Ty::Void),
        Some(t) => {
            let ty = resolve_ty(t, types)?;
            if is_main && ty != Ty::Int64 {
                return Err(Error::at("`main` must return Int64", f.name_span));
            }
            Ok(ty)
        }
    }
}

fn resolve_ty(t: &TypeExpr, types: &HashMap<String, TypeInfo>) -> Result<Ty> {
    match (t.name.as_str(), t.args.as_slice()) {
        ("Int64", []) => Ok(Ty::Int64),
        ("Bool", []) => Ok(Ty::Bool),
        ("Void", []) => Ok(Ty::Void),
        ("Trial", [a, b]) => {
            let ok = resolve_ty(a, types)?;
            let err = resolve_ty(b, types)?;
            if ok == Ty::Int64 && err == Ty::Int64 {
                Ok(Ty::TrialI64I64)
            } else {
                Err(Error::at(
                    "only Trial<Int64, Int64> is supported",
                    t.span,
                ))
            }
        }
        ("Trial", _) => Err(Error::at("Trial expects two type arguments", t.span)),
        (name, []) if types.contains_key(name) => Ok(Ty::User(name.to_string())),
        (other, args) if !args.is_empty() => Err(Error::at(
            format!("unsupported generic type `{other}`"),
            t.span,
        )),
        (other, _) => Err(Error::at(format!("unknown type `{other}`"), t.span)),
    }
}

fn lower_function(
    f: &ast::Function,
    fns: &HashMap<String, FnSig>,
    types: &HashMap<String, TypeInfo>,
) -> Result<Function> {
    let is_main = f.name == "main";
    let ret_ty = resolve_return_ty(f, types)?;
    let mut params = Vec::new();
    for p in &f.params {
        let ty = resolve_ty(&p.ty, types)?;
        params.push((sanitize_name(&p.name), ty));
    }

    let mut ctx = LowerCtx::new(fns.clone(), types.clone(), ret_ty.clone());
    for p in &f.params {
        let ty = resolve_ty(&p.ty, types)?;
        ctx.define(&p.name, ty);
    }

    let ret = lower_block_value(&mut ctx, &f.body, &ret_ty)?;

    Ok(Function {
        name: f.name.clone(),
        is_main,
        receiver: None,
        params,
        body: ctx.stmts,
        ret,
        ret_ty,
    })
}

fn lower_method(
    td: &TypeDef,
    m: &Method,
    fns: &HashMap<String, FnSig>,
    types: &HashMap<String, TypeInfo>,
) -> Result<Function> {
    let ret_ty = match m.return_type.as_ref() {
        Some(t) => resolve_ty(t, types)?,
        None => Ty::Void,
    };
    let recv_ty = Ty::User(td.name.clone());
    let recv_place = "_self".to_string();

    let mut params = Vec::new();
    for p in &m.params {
        let ty = resolve_ty(&p.ty, types)?;
        params.push((sanitize_name(&p.name), ty));
    }

    let mut ctx = LowerCtx::new(fns.clone(), types.clone(), ret_ty.clone());

    // Bind fields as bare names from receiver (no `self` in Thoxa).
    let info = types.get(&td.name).unwrap();
    for (fname, fty) in &info.fields {
        let p = Place {
            name: sanitize_name(fname),
        };
        ctx.assign(
            p,
            fty.clone(),
            RValue::Field {
                base: Place {
                    name: recv_place.clone(),
                },
                field: sanitize_field(fname),
            },
        );
        ctx.define(fname, fty.clone());
    }
    for p in &m.params {
        let ty = resolve_ty(&p.ty, types)?;
        ctx.define(&p.name, ty);
    }

    let ret = lower_block_value(&mut ctx, &m.body, &ret_ty)?;

    Ok(Function {
        name: format!("{}__{}", td.name, m.name),
        is_main: false,
        receiver: Some((recv_place, recv_ty)),
        params,
        body: ctx.stmts,
        ret,
        ret_ty,
    })
}

fn lower_block_value(ctx: &mut LowerCtx, block: &Block, expect: &Ty) -> Result<Option<Place>> {
    ctx.push_scope();
    for item in &block.items {
        match item {
            BlockItem::Let {
                name,
                name_span,
                init,
            } => {
                let (place, ty) = lower_expr(ctx, init)?;
                let bound = Place {
                    name: sanitize_name(name),
                };
                ctx.assign(bound, ty.clone(), RValue::Use(place));
                ctx.define(name, ty);
                let _ = name_span;
            }
            BlockItem::Expr(e) => {
                let _ = lower_expr(ctx, e)?;
            }
        }
    }

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
            let ty = ctx
                .lookup(name)
                .ok_or_else(|| Error::at(format!("undefined name `{name}`"), expr.span))?;
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
                        RValue::Unary { op: *op, arg },
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
            }
        }
        ExprKind::Binary { op, left, right } => lower_binary(ctx, *op, left, right, expr.span),
        ExprKind::Call { callee, args } => lower_call(ctx, callee, args, expr.span),
        ExprKind::Field {
            base,
            name,
            name_span,
        } => lower_field(ctx, base, name, *name_span),
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
                        ctx.assign(bound, ty.clone(), RValue::Use(place));
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
        ExprKind::Try(inner) => lower_try(ctx, inner, expr.span),
        ExprKind::Match { scrutinee, arms } => lower_match(ctx, scrutinee, arms, expr.span),
    }
}

fn lower_field(
    ctx: &mut LowerCtx,
    base: &Expr,
    name: &str,
    name_span: Span,
) -> Result<(Place, Ty)> {
    let (base_p, base_ty) = lower_expr(ctx, base)?;
    let type_name = match &base_ty {
        Ty::User(n) => n.clone(),
        _ => {
            return Err(Error::at(
                format!("field access on non-struct type {base_ty:?}"),
                name_span,
            ));
        }
    };
    let info = ctx
        .types
        .get(&type_name)
        .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), name_span))?;
    let field_ty = info
        .fields
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, t)| t.clone())
        .ok_or_else(|| Error::at(format!("no field `{name}` on `{type_name}`"), name_span))?;
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

fn lower_method_call(
    ctx: &mut LowerCtx,
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[Arg],
) -> Result<(Place, Ty)> {
    let (recv_p, recv_ty) = lower_expr(ctx, receiver)?;
    let type_name = match &recv_ty {
        Ty::User(n) => n.clone(),
        _ => {
            return Err(Error::at(
                format!("method call on non-struct type {recv_ty:?}"),
                method_span,
            ));
        }
    };
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
    let p = ctx.fresh("mcall");
    ctx.assign(
        p.clone(),
        sig.ret.clone(),
        RValue::Call {
            callee: Callee::Method {
                type_name: type_name.clone(),
                method: method.to_string(),
            },
            args: all_args,
        },
    );
    Ok((p, sig.ret.clone()))
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
            let (p, ty) = lower_expr(ctx, expr)?;
            if ty != formal[i].1 {
                return Err(Error::at(
                    format!(
                        "argument {} (`{}`): expected {:?}, found {:?}",
                        i + 1,
                        formal[i].0,
                        formal[i].1,
                        ty
                    ),
                    expr.span,
                ));
            }
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
    let mut by_name: HashMap<String, (Place, Ty, Span)> = HashMap::new();
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
                let (p, ty) = lower_expr(ctx, value)?;
                by_name.insert(name.clone(), (p, ty, value.span));
            }
            Arg::Pos(_) => unreachable!(),
        }
    }
    let mut places = Vec::new();
    for (fname, fty) in formal {
        let (p, ty, vspan) = by_name.remove(fname).ok_or_else(|| {
            Error::at(format!("missing argument `{fname}`"), span)
        })?;
        if &ty != fty {
            return Err(Error::at(
                format!("argument `{fname}`: expected {fty:?}, found {ty:?}"),
                vspan,
            ));
        }
        places.push(p);
    }
    if let Some((extra, _)) = by_name.iter().next() {
        return Err(Error::at(format!("unknown argument `{extra}`"), span));
    }
    Ok(places)
}

fn lower_try(ctx: &mut LowerCtx, inner: &Expr, span: Span) -> Result<(Place, Ty)> {
    let (trial_p, trial_ty) = lower_expr(ctx, inner)?;
    if trial_ty != Ty::TrialI64I64 {
        return Err(Error::at("`?` expects a Trial value", span));
    }
    if ctx.fn_ret != Ty::TrialI64I64 {
        return Err(Error::at(
            "`?` requires the enclosing function to return Trial",
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

    let outer = std::mem::take(&mut ctx.stmts);
    let then_body = vec![Stmt::Return {
        value: trial_p.clone(),
    }];
    ctx.stmts = outer;
    ctx.stmts.push(Stmt::If {
        cond: is_fail,
        then_body,
        else_body: Vec::new(),
    });

    let payload = ctx.fresh("ok");
    ctx.assign(
        payload.clone(),
        Ty::Int64,
        RValue::TrialPayload(trial_p),
    );
    Ok((payload, Ty::Int64))
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
    if scrut_ty != Ty::TrialI64I64 {
        return Err(Error::at("match only supports Trial in R2", span));
    }

    let pass_bind = match &pass_arm.pattern {
        Pattern::Variant { bind, .. } => bind.clone(),
    };
    let fail_bind = match &fail_arm.pattern {
        Pattern::Variant { bind, .. } => bind.clone(),
    };

    let pass_ty = {
        ctx.push_scope();
        ctx.define(&pass_bind, Ty::Int64);
        let ty = infer_block_ty(ctx, &pass_arm.body)?;
        ctx.pop_scope();
        ty
    };
    let fail_ty = {
        ctx.push_scope();
        ctx.define(&fail_bind, Ty::Int64);
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
    ctx.assign(
        pass_payload.clone(),
        Ty::Int64,
        RValue::TrialPayload(scrut_p.clone()),
    );
    let pass_place = Place {
        name: sanitize_name(&pass_bind),
    };
    ctx.assign(pass_place, Ty::Int64, RValue::Use(pass_payload));
    ctx.define(&pass_bind, Ty::Int64);
    let pass_val = lower_block_into(ctx, &pass_arm.body, &result_ty)?;
    ctx.pop_scope();
    let mut then_body = std::mem::take(&mut ctx.stmts);
    if let (Some(out_p), Some(pv)) = (&out, pass_val) {
        then_body.push(Stmt::Assign {
            place: out_p.clone(),
            ty: result_ty.clone(),
            value: RValue::Use(pv),
        });
    }

    ctx.push_scope();
    let fail_payload = ctx.fresh("fp");
    ctx.assign(
        fail_payload.clone(),
        Ty::Int64,
        RValue::TrialPayload(scrut_p),
    );
    let fail_place = Place {
        name: sanitize_name(&fail_bind),
    };
    ctx.assign(fail_place, Ty::Int64, RValue::Use(fail_payload));
    ctx.define(&fail_bind, Ty::Int64);
    let fail_val = lower_block_into(ctx, &fail_arm.body, &result_ty)?;
    ctx.pop_scope();
    let mut else_body = std::mem::take(&mut ctx.stmts);
    if let (Some(out_p), Some(fv)) = (&out, fail_val) {
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
        Ty::Int64 => ctx.assign(place.clone(), ty.clone(), RValue::Int(0)),
        Ty::Bool => ctx.assign(place.clone(), ty.clone(), RValue::Bool(false)),
        Ty::CStr => ctx.assign(place.clone(), ty.clone(), RValue::Str(String::new())),
        Ty::Void => {}
        Ty::TrialI64I64 => {
            let z = ctx.fresh("tz");
            ctx.assign(z.clone(), Ty::Int64, RValue::Int(0));
            ctx.assign(
                place.clone(),
                ty.clone(),
                RValue::TrialMake {
                    tag: TrialTag::Pass,
                    payload: z,
                },
            );
        }
        Ty::User(name) => {
            let info = ctx
                .types
                .get(name)
                .ok_or_else(|| Error::new(format!("unknown type `{name}`")))?
                .clone();
            let mut fields = Vec::new();
            for (_, fty) in &info.fields {
                let fp = ctx.fresh("zf");
                match fty {
                    Ty::Int64 => ctx.assign(fp.clone(), Ty::Int64, RValue::Int(0)),
                    Ty::Bool => ctx.assign(fp.clone(), Ty::Bool, RValue::Bool(false)),
                    _ => {
                        return Err(Error::new(format!(
                            "cannot zero-init field type {fty:?}"
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
    let (out_ty, need) = match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            (Ty::Int64, Ty::Int64)
        }
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
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
        out_ty.clone(),
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
            let builtin = match (callee, &ty) {
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
            let p = ctx.fresh("u");
            ctx.assign(p.clone(), Ty::Int64, RValue::Int(0));
            Ok((p, Ty::Void))
        }
        "Pass" | "Fail" => {
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
            let (payload, pty) = lower_expr(ctx, expr)?;
            if pty != Ty::Int64 {
                return Err(Error::at(
                    format!("`{callee}` expects Int64 payload"),
                    span,
                ));
            }
            let tag = if callee == "Pass" {
                TrialTag::Pass
            } else {
                TrialTag::Fail
            };
            let p = ctx.fresh("tr");
            ctx.assign(
                p.clone(),
                Ty::TrialI64I64,
                RValue::TrialMake { tag, payload },
            );
            Ok((p, Ty::TrialI64I64))
        }
        other if ctx.types.contains_key(other) => {
            // Constructor: TypeName(field: …)
            let info = ctx.types.get(other).unwrap().clone();
            let formal: Vec<(String, Ty)> = info.fields.clone();
            let field_places = resolve_call_args(ctx, &formal, args, span)?;
            let p = ctx.fresh("obj");
            let ty = Ty::User(other.to_string());
            ctx.assign(
                p.clone(),
                ty.clone(),
                RValue::StructLit {
                    type_name: other.to_string(),
                    fields: field_places,
                },
            );
            Ok((p, ty))
        }
        other => {
            let sig = ctx
                .fns
                .get(other)
                .ok_or_else(|| Error::at(format!("unknown function `{other}`"), span))?
                .clone();
            let arg_places = resolve_call_args(ctx, &sig.params, args, span)?;
            let p = ctx.fresh("call");
            ctx.assign(
                p.clone(),
                sig.ret.clone(),
                RValue::Call {
                    callee: Callee::User(other.to_string()),
                    args: arg_places,
                },
            );
            Ok((p, sig.ret))
        }
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
                ctx.assign(bound, ty.clone(), RValue::Use(place));
                ctx.define(name, ty);
            }
            BlockItem::Expr(e) => {
                let _ = lower_expr(ctx, e)?;
            }
        }
    }
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
        fn_ret: ctx.fn_ret.clone(),
    };
    if block.tail.is_none() {
        return Ok(Ty::Void);
    }
    for item in &block.items {
        if let BlockItem::Let { name, init, .. } = item {
            let (place, ty) = lower_expr(&mut tmp, init)?;
            let bound = Place {
                name: sanitize_name(name),
            };
            tmp.assign(bound, ty.clone(), RValue::Use(place));
            tmp.define(name, ty);
        } else if let BlockItem::Expr(e) = item {
            let _ = lower_expr(&mut tmp, e)?;
        }
    }
    let (_, ty) = lower_expr(&mut tmp, block.tail.as_ref().unwrap())?;
    Ok(ty)
}

fn sanitize_name(name: &str) -> String {
    format!("v_{name}")
}

fn sanitize_field(name: &str) -> String {
    format!("f_{name}")
}
