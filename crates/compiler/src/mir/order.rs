//! D228 §4: `Ordered` — `compare` and `<` `<=` `>` `>=`.
//!
//! A comparison works on -1 / 0 / 1 in an `Int64` and meets `Order` (std/core's
//! `"less" | "equal" | "greater"`) only where a `compare` returns or is
//! called. Integers and `Bool` compare by value; `Float64` and `String` call
//! the `compare` std writes for them in Thoxa; a declared type calls its
//! `compare`, its own or the derived one, whose body is `__compare(self,
//! other)`: field by field in declaration order, a union by member and then
//! payload. A generic type's instance and a union have no `compare` method
//! and are compared field by field where they are met.

use super::{
    borrow_field, display_ty, emit_call_result, extension_method, has_storage, is_int,
    lower_expr, lower_extension_call, refined_info, try_sum_inject_lit, unwrap_refined, Callee,
    LowerCtx, Place, RValue, Stmt, Ty,
};
use crate::ast::{Arg, BinaryOp, TypeLit};
use crate::error::{Error, Result};
use crate::span::Span;

/// `std/core`'s `Order`.
fn order_type(ctx: &LowerCtx, span: Span) -> Result<String> {
    ctx.module_exports
        .get("std_core")
        .and_then(|core| core.types.get("Order"))
        .cloned()
        .ok_or_else(|| Error::at("`Order` (std/core) is not loaded", span))
}

/// `if cond { then } else { otherwise }` as statements.
fn emit_if(
    ctx: &mut LowerCtx,
    cond: Place,
    then: impl FnOnce(&mut LowerCtx) -> Result<()>,
    otherwise: impl FnOnce(&mut LowerCtx) -> Result<()>,
) -> Result<()> {
    let outside = std::mem::take(&mut ctx.stmts);
    then(ctx)?;
    let then_body = std::mem::take(&mut ctx.stmts);
    otherwise(ctx)?;
    let else_body = std::mem::replace(&mut ctx.stmts, outside);
    ctx.stmts.push(Stmt::If {
        cond,
        then_body,
        else_body,
    });
    Ok(())
}

fn int(ctx: &mut LowerCtx, value: i64) -> Place {
    let place = ctx.fresh("ordint");
    ctx.assign(place.clone(), Ty::Int64, RValue::Int(value));
    place
}

fn binary(ctx: &mut LowerCtx, op: BinaryOp, left: Place, right: Place) -> Place {
    let place = ctx.fresh("ordcmp");
    ctx.assign(place.clone(), Ty::Bool, RValue::Binary { op, left, right });
    place
}

/// -1 / 0 / 1 into `result` from `left < right` and `left > right`.
fn sign_into(ctx: &mut LowerCtx, result: &Place, left: Place, right: Place) -> Result<()> {
    let less = binary(ctx, BinaryOp::Lt, left.clone(), right.clone());
    emit_if(
        ctx,
        less,
        |ctx| {
            let minus = int(ctx, -1);
            ctx.assign(result.clone(), Ty::Int64, RValue::Use(minus));
            Ok(())
        },
        |ctx| {
            let greater = binary(ctx, BinaryOp::Gt, left, right);
            emit_if(
                ctx,
                greater,
                |ctx| {
                    let one = int(ctx, 1);
                    ctx.assign(result.clone(), Ty::Int64, RValue::Use(one));
                    Ok(())
                },
                |_| Ok(()),
            )
        },
    )
}

/// An `Order` as -1 / 0 / 1.
fn order_to_int(ctx: &mut LowerCtx, order: Place, span: Span) -> Result<Place> {
    let order_ty = order_type(ctx, span)?;
    let Some(sum) = ctx.types.get(&order_ty).and_then(|info| info.sum.clone()) else {
        return Err(Error::at("`Order` is not a union", span));
    };
    let tag_of = |text: &str| {
        sum.variants
            .iter()
            .find(|variant| variant.lit == Some(TypeLit::String(text.into())))
            .map(|variant| variant.tag as i64)
            .unwrap_or_default()
    };
    let (less, greater) = (tag_of("less"), tag_of("greater"));
    let tag = ctx.fresh("ordtag");
    ctx.assign(
        tag.clone(),
        Ty::Int64,
        RValue::SumTag {
            place: order,
            sum_type: order_ty,
        },
    );
    let result = int(ctx, 0);
    let less_tag = int(ctx, less);
    let is_less = binary(ctx, BinaryOp::Eq, tag.clone(), less_tag);
    emit_if(
        ctx,
        is_less,
        |ctx| {
            let minus = int(ctx, -1);
            ctx.assign(result.clone(), Ty::Int64, RValue::Use(minus));
            Ok(())
        },
        |ctx| {
            let greater_tag = int(ctx, greater);
            let is_greater = binary(ctx, BinaryOp::Eq, tag, greater_tag);
            emit_if(
                ctx,
                is_greater,
                |ctx| {
                    let one = int(ctx, 1);
                    ctx.assign(result.clone(), Ty::Int64, RValue::Use(one));
                    Ok(())
                },
                |_| Ok(()),
            )
        },
    )?;
    Ok(result)
}

/// -1 / 0 / 1 as an `Order`.
pub(super) fn int_to_order(ctx: &mut LowerCtx, sign: Place, span: Span) -> Result<(Place, Ty)> {
    let order_ty = Ty::User(order_type(ctx, span)?);
    let inject = |ctx: &mut LowerCtx, text: &str| -> Result<Place> {
        try_sum_inject_lit(ctx, &TypeLit::String(text.into()), &order_ty, span)?
            .ok_or_else(|| Error::at(format!("`Order` has no `\"{text}\"`"), span))
    };
    let result = ctx.fresh("order");
    let equal = inject(ctx, "equal")?;
    ctx.assign(result.clone(), order_ty.clone(), RValue::Use(equal));
    let zero = int(ctx, 0);
    let less = binary(ctx, BinaryOp::Lt, sign.clone(), zero.clone());
    emit_if(
        ctx,
        less,
        |ctx| {
            let value = inject(ctx, "less")?;
            ctx.assign(result.clone(), order_ty.clone(), RValue::Use(value));
            Ok(())
        },
        |ctx| {
            let greater = binary(ctx, BinaryOp::Gt, sign, zero);
            emit_if(
                ctx,
                greater,
                |ctx| {
                    let value = inject(ctx, "greater")?;
                    ctx.assign(result.clone(), order_ty.clone(), RValue::Use(value));
                    Ok(())
                },
                |_| Ok(()),
            )
        },
    )?;
    Ok((result, order_ty))
}

/// "`Point` is not `Ordered`", with how to declare it.
pub(super) fn not_ordered(ty: &Ty, span: Span) -> Error {
    let shown = display_ty(ty);
    Error::at(format!("cannot order {shown} values: `{shown}` is not `Ordered`"), span).with_help(
        format!("declare `conform {shown} to Equatable, Ordered`: field by field, or with its own `compare`"),
    )
}

/// -1 / 0 / 1 for two values of `ty`, which must be `Ordered`.
pub(super) fn lower_compare(
    ctx: &mut LowerCtx,
    left: Place,
    right: Place,
    ty: &Ty,
    span: Span,
) -> Result<Place> {
    if !ctx.conforms(ty, "Ordered") {
        return Err(not_ordered(ty, span));
    }
    if is_int(ty) || *ty == Ty::Bool {
        let result = int(ctx, 0);
        sign_into(ctx, &result, left, right)?;
        return Ok(result);
    }
    if refined_info(&ctx.types, ty).is_some() {
        let (left, base) = unwrap_refined(ctx, left, ty)?;
        let (right, _) = unwrap_refined(ctx, right, ty)?;
        return lower_compare(ctx, left, right, &base, span);
    }
    // std writes `compare` for Float64 and String, in Thoxa.
    if let Some(sig) = extension_method(ctx, ty, "compare", span)? {
        let (order, _) = call_extension_compare(ctx, left, right, ty, sig, span)?;
        return order_to_int(ctx, order, span);
    }
    let Ty::User(type_name) = ty else {
        return Err(not_ordered(ty, span));
    };
    let info = ctx.types.get(type_name).cloned();
    if info.as_ref().is_some_and(|info| info.methods.contains_key("compare")) {
        let other = ctx.fresh("cmparg");
        ctx.assign(other.clone(), ty.clone(), RValue::Use(right));
        let order_ty = Ty::User(order_type(ctx, span)?);
        let (order, _) = emit_call_result(
            ctx,
            Callee::Method {
                type_name: type_name.clone(),
                method: "compare".into(),
            },
            vec![left, other],
            order_ty,
        )?;
        return order_to_int(ctx, order, span);
    }
    lower_structural_compare(ctx, left, right, type_name, span)
}

fn call_extension_compare(
    ctx: &mut LowerCtx,
    left: Place,
    right: Place,
    ty: &Ty,
    sig: super::FnSig,
    span: Span,
) -> Result<(Place, Ty)> {
    let right = super::codec::bind_fresh(ctx, "__other", ty, right);
    lower_extension_call(
        ctx,
        left,
        ty,
        sig,
        "compare",
        &[Arg::Pos(crate::ast_build::name(&right))],
        span,
    )
}

/// Field by field in declaration order; a union by member, then payload.
pub(super) fn lower_structural_compare(
    ctx: &mut LowerCtx,
    left: Place,
    right: Place,
    type_name: &str,
    span: Span,
) -> Result<Place> {
    let info = ctx
        .types
        .get(type_name)
        .cloned()
        .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), span))?;
    let result = int(ctx, 0);
    if let Some(sum) = &info.sum {
        let mut tags = Vec::new();
        for place in [&left, &right] {
            let tag = ctx.fresh("ordarm");
            ctx.assign(
                tag.clone(),
                Ty::Int64,
                RValue::SumTag {
                    place: place.clone(),
                    sum_type: type_name.to_string(),
                },
            );
            tags.push(tag);
        }
        let (left_tag, right_tag) = (tags[0].clone(), tags[1].clone());
        sign_into(ctx, &result, left_tag.clone(), right_tag)?;
        for variant in &sum.variants {
            if variant.lit.is_some() || !has_storage(&ctx.types, &variant.ty) {
                continue;
            }
            let zero = int(ctx, 0);
            let tied = binary(ctx, BinaryOp::Eq, result.clone(), zero);
            let want = int(ctx, variant.tag as i64);
            let this_arm = binary(ctx, BinaryOp::Eq, left_tag.clone(), want);
            let both = binary(ctx, BinaryOp::And, tied, this_arm);
            emit_if(
                ctx,
                both,
                |ctx| {
                    let mut payloads = Vec::new();
                    for place in [&left, &right] {
                        let payload = ctx.fresh("ordpay");
                        ctx.assign_no_drop(
                            payload.clone(),
                            variant.ty.clone(),
                            RValue::SumPayload {
                                base: place.clone(),
                                tag: variant.tag,
                                payload_ty: variant.ty.clone(),
                            },
                        );
                        payloads.push(payload);
                    }
                    let sign = lower_compare(
                        ctx,
                        payloads[0].clone(),
                        payloads[1].clone(),
                        &variant.ty,
                        span,
                    )?;
                    ctx.assign(result.clone(), Ty::Int64, RValue::Use(sign));
                    Ok(())
                },
                |_| Ok(()),
            )?;
        }
        return Ok(result);
    }
    for field in info.fields.iter().filter(|field| field.ty != Ty::Void) {
        let zero = int(ctx, 0);
        let tied = binary(ctx, BinaryOp::Eq, result.clone(), zero);
        emit_if(
            ctx,
            tied,
            |ctx| {
                let left_field = borrow_field(ctx, &left, &field.name, &field.ty);
                let right_field = borrow_field(ctx, &right, &field.name, &field.ty);
                let sign = lower_compare(ctx, left_field, right_field, &field.ty, span)
                    .map_err(|error| {
                        if error.message.starts_with("cannot order") {
                            Error::at(
                                format!(
                                    "cannot order {} values: field `{}` is {}, which is not `Ordered`",
                                    display_ty(&Ty::User(type_name.to_string())),
                                    field.name,
                                    display_ty(&field.ty)
                                ),
                                span,
                            )
                        } else {
                            error
                        }
                    })?;
                ctx.assign(result.clone(), Ty::Int64, RValue::Use(sign));
                Ok(())
            },
            |_| Ok(()),
        )?;
    }
    Ok(result)
}

/// `__compare(a, b)`, a derived `compare`'s body: the structural order as
/// an `Order`.
pub(super) fn lower_compare_intrinsic(
    ctx: &mut LowerCtx,
    args: &[Arg],
    span: Span,
) -> Result<(Place, Ty)> {
    let [Arg::Pos(left), Arg::Pos(right)] = args else {
        return Err(Error::at("`__compare` takes two values", span));
    };
    let (left, ty) = lower_expr(ctx, left)?;
    let (right, _) = lower_expr(ctx, right)?;
    let Ty::User(type_name) = &ty else {
        return Err(Error::at("`__compare` compares declared types", span));
    };
    let sign = lower_structural_compare(ctx, left, right, type_name, span)?;
    int_to_order(ctx, sign, span)
}

/// `__hash(value, mut hasher)`, a derived `hash`'s body, and `value.hash(mut
/// hasher)` on a type without the method (a union, a generic type's
/// instance): a union feeds its member, then its payload; a product each
/// field, in declaration order, each through its own `hash` (D228 §4).
pub(super) fn lower_structural_hash(
    ctx: &mut LowerCtx,
    value: Place,
    ty: &Ty,
    hasher: &str,
    span: Span,
) -> Result<()> {
    let Ty::User(type_name) = ty else {
        return hash_part(ctx, value, ty, hasher, span);
    };
    let info = ctx
        .types
        .get(type_name)
        .cloned()
        .ok_or_else(|| Error::at(format!("unknown type `{type_name}`"), span))?;
    if let Some(sum) = &info.sum {
        let tag = ctx.fresh("hashtag");
        ctx.assign(
            tag.clone(),
            Ty::Int64,
            RValue::SumTag {
                place: value.clone(),
                sum_type: type_name.clone(),
            },
        );
        mix(ctx, tag.clone(), hasher, span)?;
        for variant in &sum.variants {
            if variant.lit.is_some() || !has_storage(&ctx.types, &variant.ty) {
                continue;
            }
            let want = int(ctx, variant.tag as i64);
            let this_arm = binary(ctx, BinaryOp::Eq, tag.clone(), want);
            emit_if(
                ctx,
                this_arm,
                |ctx| {
                    let payload = ctx.fresh("hashpay");
                    ctx.assign_no_drop(
                        payload.clone(),
                        variant.ty.clone(),
                        RValue::SumPayload {
                            base: value.clone(),
                            tag: variant.tag,
                            payload_ty: variant.ty.clone(),
                        },
                    );
                    hash_part(ctx, payload, &variant.ty, hasher, span)
                },
                |_| Ok(()),
            )?;
        }
        return Ok(());
    }
    for field in info.fields.iter().filter(|field| field.ty != Ty::Void) {
        let part = borrow_field(ctx, &value, &field.name, &field.ty);
        hash_part(ctx, part, &field.ty, hasher, span)?;
    }
    Ok(())
}

/// `hasher.mix(value)`.
fn mix(ctx: &mut LowerCtx, value: Place, hasher: &str, _span: Span) -> Result<()> {
    let part = super::codec::bind_fresh(ctx, "__mixed", &Ty::Int64, value);
    let call = crate::ast_build::method_call(
        crate::ast_build::name(hasher),
        "mix",
        vec![Arg::Pos(crate::ast_build::name(&part))],
    );
    lower_expr(ctx, &call)?;
    Ok(())
}

/// One part's `hash`: its own method, std's for a built-in, or the
/// structural one for a type without the method.
fn hash_part(ctx: &mut LowerCtx, part: Place, ty: &Ty, hasher: &str, span: Span) -> Result<()> {
    let part = super::codec::bind_fresh(ctx, "__hashed", ty, part);
    let call = crate::ast_build::method_call(
        crate::ast_build::name(&part),
        "hash",
        vec![Arg::Pos(crate::ast::Expr {
            kind: crate::ast::ExprKind::MutArg(hasher.to_string()),
            span,
        })],
    );
    lower_expr(ctx, &call)?;
    Ok(())
}

/// `__hash(value, mut hasher)`.
pub(super) fn lower_hash_intrinsic(ctx: &mut LowerCtx, args: &[Arg], span: Span) -> Result<(Place, Ty)> {
    let [Arg::Pos(value), Arg::Pos(hasher)] = args else {
        return Err(Error::at("`__hash` takes a value and `mut hasher`", span));
    };
    let crate::ast::ExprKind::MutArg(hasher) = &hasher.kind else {
        return Err(Error::at("`__hash` takes `mut hasher`", span));
    };
    let (place, ty) = lower_expr(ctx, value)?;
    lower_structural_hash(ctx, place, &ty, hasher, span)?;
    let unit = ctx.fresh("u");
    ctx.assign(unit.clone(), Ty::Int64, RValue::Int(0));
    Ok((unit, Ty::Void))
}
