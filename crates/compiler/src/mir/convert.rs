//! D191 §1–§2: `x as T` and `x as? T`.
//!
//! The operand is lowered first, expecting `T` when it is a literal, so
//! `3 as Quantity`, `1 as Float64`, and `[] as Array<Int64>` type directly.
//! The pair (operand type, `T`) then names one conversion:
//! - none needed, or one an expected type already makes: a member into its
//!   union, a literal union into its base, a refined value into its base,
//!   integer widening, an integer into `Float64`;
//! - anything encodable into `Json` (`toJson`);
//! - a checked conversion, `Trial<T, Array<Issue>>`, through a reader the codec
//!   pass generated for this cast's type in this module: `__asJson_K` (from a
//!   `Json`), `__asBase_K` (into a refined type or a literal union), and
//!   `__asNarrow_K` (into a narrower integer).
//!
//! A `from` clause on the target that takes the operand's type comes first and
//! replaces the built-in conversion (§3).
//!
//! Text is parsed, not converted: `String as Order` points at `T.read`.

use super::codec::{bind_fresh, json_ty, to_json};
use super::{
    coerce_place, display_ty, emit_call_result, int_range, is_never, is_pure_lit_sum_of_kind,
    lower_arg_for_formal, lower_expr, lower_expr_expect, lower_try_sum, refined_info, resolve_ty,
    try_sum_coerce, type_source_name, unwrap_refined, Callee, LitKind, LowerCtx, Place, Ty,
};
use crate::conversion_desugar::conversion_fn_name;
use crate::ast::{Arg, Expr, ExprKind, TypeExpr, UnaryOp};
use crate::ast_build::{call, cast_reader_key, name};
use crate::error::{Error, Result};
use crate::span::Span;

pub(super) fn lower_cast(
    ctx: &mut LowerCtx,
    operand: &Expr,
    target: &TypeExpr,
    propagate: bool,
    span: Span,
) -> Result<(Place, Ty)> {
    let to = resolve_ty(target, &ctx.types, &ctx.name_to_type)?;
    let (place, from) = if takes_expected_type(operand) && holds_literals(ctx, &to) {
        lower_expr_expect(ctx, operand, Some(&to))?
    } else {
        lower_expr(ctx, operand)?
    };
    let (place, ty, checked) = match declared(ctx, place.clone(), &from, &to, span)? {
        Some(converted) => converted,
        None => convert(ctx, place, &from, &to, target, span)?,
    };
    if !propagate {
        return Ok((place, ty));
    }
    if !checked {
        return Err(Error::at(
            format!(
                "`{} as {}` cannot fail, so `as?` has nothing to return; write `as`",
                display_ty(&from),
                display_ty(&to)
            ),
            span,
        ));
    }
    let sum = match &ty {
        Ty::User(trial) => ctx.types.get(trial).and_then(|info| info.sum.clone()),
        _ => None,
    }
    .ok_or_else(|| Error::at("internal: a checked conversion is not a Trial", span))?;
    lower_try_sum(ctx, place, ty, sum, span)
}

/// A literal (or array / map literal) takes the target as its expected type,
/// as it would in an annotated `let`.
fn takes_expected_type(operand: &Expr) -> bool {
    match &operand.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::ArrayLit(_)
        | ExprKind::MapLit(_) => true,
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => matches!(expr.kind, ExprKind::Int(_) | ExprKind::Float(_)),
        _ => false,
    }
}

/// A `from` clause on the target that takes the operand's type (D191 §3). It
/// replaces any built-in conversion for the pair. The clauses are the
/// functions `__conversion_T_N` in the target's module.
fn declared(
    ctx: &mut LowerCtx,
    place: Place,
    from: &Ty,
    to: &Ty,
    span: Span,
) -> Result<Option<(Place, Ty, bool)>> {
    let Ty::User(target) = to else {
        return Ok(None);
    };
    let Some(info) = ctx.types.get(target).cloned() else {
        return Ok(None);
    };
    let type_name = type_source_name(&info, target);
    for index in 0.. {
        let short = conversion_fn_name(&type_name, index);
        let long = format!("{}_{short}", info.def_mod);
        let Some(sig) = ctx.fns.get(&short).or_else(|| ctx.fns.get(&long)).cloned() else {
            return Ok(None);
        };
        let param = sig.params[0].1.clone();
        if param != *from {
            continue;
        }
        let exported = ctx
            .module_exports
            .get(&info.def_mod)
            .is_some_and(|exports| exports.fns.contains_key(&short));
        if ctx.module_id != info.def_mod && !exported {
            return Err(Error::at(
                format!(
                    "the conversion from {} to {} is not exported: write `export from(…)` in `{type_name}`",
                    display_ty(from),
                    display_ty(to)
                ),
                span,
            ));
        }
        let input = bind_fresh(ctx, "__castIn", from, place);
        let argument = lower_arg_for_formal(ctx, &name(&input), &param, "source", 0)?;
        let (place, ty) =
            emit_call_result(ctx, Callee::User(sig.mangled.clone()), vec![argument], sig.ret)?;
        let checked = ty != *to;
        return Ok(Some((place, ty, checked)));
    }
    Ok(None)
}

/// A target a literal can be written as directly: anything but a plain
/// product, which a literal reaches only through a `from` clause
/// (`"a@b" as Email`).
fn holds_literals(ctx: &LowerCtx, to: &Ty) -> bool {
    match to {
        Ty::User(name) => ctx.types.get(name).is_none_or(|info| {
            info.sum.is_some() || info.refined.is_some() || json_ty(ctx).as_ref() == Some(to)
        }),
        _ => true,
    }
}

/// `place` converted from `from` to `to`: the result, its type, and whether
/// the conversion can fail (the result is then a `Trial`).
fn convert(
    ctx: &mut LowerCtx,
    place: Place,
    from: &Ty,
    to: &Ty,
    target: &TypeExpr,
    span: Span,
) -> Result<(Place, Ty, bool)> {
    if from == to {
        return Ok((place, to.clone(), false));
    }
    if refined_info(&ctx.types, from).is_some_and(|info| &info.base == to) {
        let (place, ty) = unwrap_refined(ctx, place, from)?;
        return Ok((place, ty, false));
    }
    if let Some(place) = try_sum_coerce(ctx, place.clone(), from, to, None, span)? {
        return Ok((place, to.clone(), false));
    }
    // An Int64 / UInt64 is a Float64 only when exact (D224). A literal is
    // checked now, so it cannot fail: `1 as Float64` is a Float64.
    let literal = super::literal_int_value(ctx, &place).is_some();
    if *to == Ty::Float64 && *from == Ty::Int64 && !literal {
        return checked(ctx, "asFloat", place, from, to, target, span);
    }
    if *to == Ty::Float64 && *from == Ty::UInt64 && !literal {
        return checked(ctx, "asFloatUnsigned", place, from, to, target, span);
    }
    if widens(from, to) {
        return Ok((coerce_place(ctx, place, from, to, span)?, to.clone(), false));
    }
    if json_ty(ctx).as_ref() == Some(to) {
        let (place, ty) = to_json(ctx, place, from)?;
        return Ok((place, ty, false));
    }
    if json_ty(ctx).as_ref() == Some(from) {
        return checked(ctx, "asJson", place, from, to, target, span);
    }
    if refined_info(&ctx.types, to).is_some() || is_literal_union(ctx, to) {
        return checked(ctx, "asBase", place, from, to, target, span);
    }
    if *from == Ty::Int64 && matches!(to, Ty::Int32 | Ty::UInt32 | Ty::UInt64 | Ty::Byte) {
        return checked(ctx, "asNarrow", place, from, to, target, span);
    }
    Err(no_conversion(ctx, from, to, span))
}

/// Integer widening and a narrower integer → `Float64`: every value of
/// `from` is a value of `to` (an Int64 / UInt64 into Float64 is checked,
/// D224).
fn widens(from: &Ty, to: &Ty) -> bool {
    if *to == Ty::Float64 {
        return int_range(from).is_some();
    }
    match (int_range(from), int_range(to)) {
        (Some((from_min, from_max)), Some((to_min, to_max))) => {
            from_min >= to_min && from_max <= to_max
        }
        _ => false,
    }
}

fn is_literal_union(ctx: &LowerCtx, ty: &Ty) -> bool {
    [LitKind::String, LitKind::Int, LitKind::Bool]
        .into_iter()
        .any(|kind| is_pure_lit_sum_of_kind(&ctx.types, ty, kind))
}

/// A checked conversion through the reader the codec pass generated for this
/// cast's type: `__{kind}_{key}`.
fn checked(
    ctx: &mut LowerCtx,
    kind: &str,
    place: Place,
    from: &Ty,
    to: &Ty,
    target: &TypeExpr,
    span: Span,
) -> Result<(Place, Ty, bool)> {
    let reader = format!("__{kind}_{}", cast_reader_key(target));
    let Some(param) = ctx
        .name_to_fn
        .get(&reader)
        .and_then(|mangled| ctx.fns.get(mangled))
        .and_then(|sig| sig.params.first().map(|(_, ty)| ty.clone()))
    else {
        if json_ty(ctx).is_none() {
            return Err(Error::at(
                format!(
                    "`{} as {}` is a checked conversion, and its failures are std/json `Issue` values: import {{ Issue }} from \"std/json\"",
                    display_ty(from),
                    display_ty(to)
                ),
                span,
            ));
        }
        return Err(no_conversion(ctx, from, to, span));
    };
    // The reader takes the base type: unwrap a refined operand, widen an
    // integer (`qty as Dozen`, an `Int32` into an `Int64`-based type).
    let (place, from) = if refined_info(&ctx.types, from).is_some() && *from != param {
        unwrap_refined(ctx, place, from)?
    } else {
        (place, from.clone())
    };
    let place = if from == param {
        place
    } else if widens(&from, &param) {
        coerce_place(ctx, place, &from, &param, span)?
    } else {
        return Err(no_conversion(ctx, &from, to, span));
    };
    let input = bind_fresh(ctx, "__castIn", &param, place);
    let (place, ty) = lower_expr(ctx, &call(&reader, vec![Arg::Pos(name(&input))]))?;
    Ok((place, ty, true))
}

fn no_conversion(ctx: &LowerCtx, from: &Ty, to: &Ty, span: Span) -> Error {
    let message = format!("no conversion from {} to {}", display_ty(from), display_ty(to));
    let parsed = *from == Ty::String && matches!(to, Ty::User(_)) && !is_literal_union(ctx, to);
    let error = Error::at(message, span);
    if parsed {
        let target = display_ty(to);
        error.with_help(format!(
            "text is parsed, not converted: write `{target}.read(text)` for JSON or `{target}.readForm(text)` for a form"
        ))
    } else if *from == Ty::Float64 && int_range(to).is_some() {
        error.with_help("a Float64 does not convert to an integer type (no rounding rule is chosen)")
    } else {
        error
    }
}

/// D191 §4: a value whose type has a JSON encoding, where a `Json` is
/// expected (an argument, a field, a `let` annotation, an array element, a
/// return, an assignment), converts as `x as Json` would. Nothing else becomes
/// implicit: decoding needs `as`.
pub(super) fn encode_where_json_expected(
    ctx: &mut LowerCtx,
    place: Place,
    from: &Ty,
    expected: &Ty,
) -> Result<Option<Place>> {
    if json_ty(ctx).as_ref() != Some(expected)
        || from == expected
        || *from == Ty::Void
        || is_never(from)
    {
        return Ok(None);
    }
    let (place, _) = to_json(ctx, place, from)?;
    Ok(Some(place))
}

/// A hint for a `Json` given where another type is expected: decoding is
/// written with `as`.
pub(super) fn decode_hint(ctx: &LowerCtx, from: &Ty, expected: &Ty) -> Option<String> {
    (json_ty(ctx).as_ref() == Some(from) && json_ty(ctx).as_ref() != Some(expected)).then(|| {
        format!(
            "a Json value does not convert implicitly: write `value as {}` and handle the Trial",
            display_ty(expected)
        )
    })
}
