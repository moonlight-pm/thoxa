//! D182 §1: `toJson` / `encode` on values whose types have no generated
//! encoder of their own: scalars, `Array`, `Map<String, T>`, `May`, and `Json`.
//!
//! The conversion is built as Thoxa and lowered with `std/json`'s names in
//! scope, so the caller need not import them. Elements and payloads convert by
//! calling `toJson` on them: a named type inside uses its generated encoder,
//! and nested structure comes back here.

use super::{display_ty, lower_expr_expect, type_source_name, LowerCtx, Place, Ty};
use crate::ast::{Arg, BlockItem, Expr};
use crate::ast_build::*;
use crate::error::{Error, Result};
use crate::span::Span;

const JSON_MODULE: &str = "std_json";

/// `std/json` types that are themselves JSON (`Json` and its members).
const JSON_FAMILY: &[&str] = &[
    "Json", "JNull", "JBool", "JInt", "JFloat", "JStr", "JArr", "JObj",
];

/// How MIR turns a value into `Json`.
enum Encoding {
    /// A member constructor over the value: `JStr(value)`.
    Scalar(&'static str),
    Array,
    Map,
    May,
    /// Already `Json`, or one of its members.
    Itself,
}

/// `Some` when MIR builds the conversion for `ty`; `None` leaves a named type
/// to its generated (or own) encoder.
pub(super) fn lower_structural_encode(
    ctx: &mut LowerCtx,
    value: Place,
    ty: &Ty,
    method: &str,
    span: Span,
) -> Result<Option<(Place, Ty)>> {
    let Some(encoding) = encoding_of(ctx, ty, span)? else {
        return Ok(None);
    };
    let Some(json) = json_ty(ctx) else {
        return Err(Error::at(
            format!(
                "`{method}` needs std/json in the program: import {{ Json }} from \"std/json\""
            ),
            span,
        ));
    };
    ctx.push_scope();
    let input = format!("__codecIn{}", ctx.temps);
    ctx.bind_name(&input, ty.clone(), false, value);
    let converted = encode_expr(&input, &encoding);
    let (expr, expect) = if method == "encode" {
        (call("encode", vec![Arg::Pos(converted)]), Ty::String)
    } else {
        (converted, json)
    };
    let lowered = ctx.with_def_mod_maps(JSON_MODULE, |ctx| {
        lower_expr_expect(ctx, &expr, Some(&expect))
    });
    ctx.pop_scope();
    lowered.map(Some)
}

fn encoding_of(ctx: &LowerCtx, ty: &Ty, span: Span) -> Result<Option<Encoding>> {
    let encoding = match ty {
        Ty::String => Encoding::Scalar("JStr"),
        Ty::Int64 | Ty::Int32 | Ty::UInt32 | Ty::UInt64 | Ty::Byte => Encoding::Scalar("JInt"),
        Ty::Float64 => Encoding::Scalar("JFloat"),
        Ty::Bool => Encoding::Scalar("JBool"),
        Ty::Array { .. } => Encoding::Array,
        Ty::Map { key, .. } if **key == Ty::String => Encoding::Map,
        Ty::Map { .. } => {
            return Err(Error::at(
                format!(
                    "{} has no JSON encoding: object keys are strings",
                    display_ty(ty)
                ),
                span,
            ))
        }
        Ty::User(name) if is_json_family(ctx, name) => Encoding::Itself,
        Ty::User(name) if is_may(ctx, name) => Encoding::May,
        Ty::User(_) => return Ok(None),
        other => {
            return Err(Error::at(
                format!("{} has no JSON encoding", display_ty(other)),
                span,
            ))
        }
    };
    Ok(Some(encoding))
}

fn json_ty(ctx: &LowerCtx) -> Option<Ty> {
    let name = format!("{JSON_MODULE}_Json");
    ctx.types.contains_key(&name).then_some(Ty::User(name))
}

fn is_json_family(ctx: &LowerCtx, name: &str) -> bool {
    ctx.types.get(name).is_some_and(|info| {
        info.def_mod == JSON_MODULE && JSON_FAMILY.contains(&type_source_name(info, name).as_str())
    })
}

/// `May<T>`: a union of `Not` and `Has(value)`.
fn is_may(ctx: &LowerCtx, name: &str) -> bool {
    let Some(sum) = ctx.types.get(name).and_then(|info| info.sum.as_ref()) else {
        return false;
    };
    let mut arms: Vec<&str> = sum.variants.iter().map(|v| v.name.as_str()).collect();
    arms.sort_unstable();
    arms == ["Has", "Not"]
}

/// The conversion of the value bound to `input`, ending in a `Json`.
fn encode_expr(input: &str, encoding: &Encoding) -> Expr {
    let as_json = |value: Expr| {
        block(
            vec![let_bind_ty("__json", false, ty_named("Json"), value)],
            name("__json"),
        )
    };
    let element = |value: &str| method_call(name(value), "toJson", vec![]);
    match encoding {
        Encoding::Scalar(ctor) => as_json(call(ctor, vec![Arg::Pos(name(input))])),
        Encoding::Itself => as_json(name(input)),
        Encoding::Array => block(
            vec![
                let_bind_ty("__out", true, ty_array(ty_named("Json")), empty_array()),
                for_each(
                    "__x",
                    None,
                    name(input),
                    vec![BlockItem::Expr(method_call(
                        name("__out"),
                        "append",
                        vec![Arg::Pos(element("__x"))],
                    ))],
                ),
            ],
            as_json(call("JArr", vec![Arg::Pos(name("__out"))])),
        ),
        Encoding::Map => block(
            vec![
                let_bind_ty(
                    "__out",
                    true,
                    ty_map(ty_named("String"), ty_named("Json")),
                    call("Map", vec![]),
                ),
                for_each(
                    "__k",
                    Some("__x"),
                    name(input),
                    vec![BlockItem::Expr(method_call(
                        name("__out"),
                        "set",
                        vec![Arg::Pos(name("__k")), Arg::Pos(element("__x"))],
                    ))],
                ),
            ],
            as_json(call("JObj", vec![Arg::Pos(name("__out"))])),
        ),
        Encoding::May => match_expr(
            name(input),
            vec![
                match_arm_block(
                    "Not",
                    vec![],
                    vec![],
                    Some(as_json(call("JNull", vec![]))),
                ),
                match_arm_block(
                    "Has",
                    vec![Some("__x".into())],
                    vec![],
                    Some(as_json(element("__x"))),
                ),
            ],
        ),
    }
}
