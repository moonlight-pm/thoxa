//! Where-clause analysis for builtin validation (experiment).

use crate::ast::{WhereAtom, WhereClause, WhereExpr, WhereLit, WherePred};
use crate::error::{Error, Result};
use crate::span::Span;

const HOLES: &[&str] = &["value", "expected", "path"];

pub fn check_templates(clause: &WhereClause) -> Result<()> {
    walk_atoms(&clause.expr, &mut |atom| check_template(atom))
}

fn check_template(atom: &WhereAtom) -> Result<()> {
    let Some(msg) = &atom.message else {
        return Ok(());
    };
    for hole in template_holes(msg) {
        if !HOLES.contains(&hole.as_str()) {
            return Err(Error::at(
                format!(
                    "unknown where-message hole `{{{hole}}}` (use {{value}}, {{expected}}, {{path}})"
                ),
                atom.span,
            ));
        }
    }
    Ok(())
}

fn template_holes(msg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = msg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = bytes[i + 1..].iter().position(|&c| c == b'}') {
                out.push(msg[i + 1..i + 1 + end].to_string());
                i += end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

pub fn fill_template(tmpl: &str, value: &str, expected: &str, path: &str) -> String {
    tmpl.replace("{value}", value)
        .replace("{expected}", expected)
        .replace("{path}", path)
}

pub fn expected_desc(pred: &WherePred, base: &str) -> String {
    let noun = match base {
        "String" | "Array" => "length",
        "Float64" => "float",
        "Bool" => "boolean",
        _ => "integer",
    };
    match pred {
        WherePred::Ge(l) => format!("{noun} >= {}", lit_show(l)),
        WherePred::Le(l) => format!("{noun} <= {}", lit_show(l)),
        WherePred::Gt(l) => format!("{noun} > {}", lit_show(l)),
        WherePred::Lt(l) => format!("{noun} < {}", lit_show(l)),
        WherePred::Eq(l) => format!("{noun} == {}", lit_show(l)),
        WherePred::Ne(l) => format!("{noun} != {}", lit_show(l)),
        WherePred::Range { lo, hi } => match (lo, hi) {
            (Some(a), Some(b)) => format!("{noun} {}..{}", lit_show(a), lit_show(b)),
            (Some(a), None) => format!("{noun} >= {}", lit_show(a)),
            (None, Some(b)) => format!("{noun} <= {}", lit_show(b)),
            (None, None) => noun.into(),
        },
        WherePred::Lit(l) => lit_show(l),
    }
}

pub fn lit_show(l: &WhereLit) -> String {
    match l {
        WhereLit::Int(n) => n.to_string(),
        WhereLit::Float(n) => format!("{n}"),
        WhereLit::String(s) => format!("\"{s}\""),
    }
}

pub fn generated_message(path: &str, expected: &str, value: &str) -> String {
    if path.is_empty() {
        format!("expected {expected}, got {value}")
    } else {
        format!("{path}: expected {expected}, got {value}")
    }
}

/// True when a constant inhabits the clause (Int64 / String / Float64).
pub fn lit_inhabits(base: &str, expr: &WhereExpr, lit: &WhereLit) -> bool {
    match expr {
        WhereExpr::And(a, b) => lit_inhabits(base, a, lit) && lit_inhabits(base, b, lit),
        WhereExpr::Or(a, b) => lit_inhabits(base, a, lit) || lit_inhabits(base, b, lit),
        WhereExpr::Atom(atom) => pred_holds(base, &atom.pred, lit),
    }
}

fn pred_holds(base: &str, pred: &WherePred, lit: &WhereLit) -> bool {
    let length_mode = matches!(base, "String" | "Array");
    match (pred, lit) {
        (WherePred::Ge(WhereLit::Int(b)), WhereLit::Int(v)) => cmp_num(*v, *b, length_mode) >= 0,
        (WherePred::Le(WhereLit::Int(b)), WhereLit::Int(v)) => cmp_num(*v, *b, length_mode) <= 0,
        (WherePred::Gt(WhereLit::Int(b)), WhereLit::Int(v)) => cmp_num(*v, *b, length_mode) > 0,
        (WherePred::Lt(WhereLit::Int(b)), WhereLit::Int(v)) => cmp_num(*v, *b, length_mode) < 0,
        (WherePred::Eq(WhereLit::Int(b)), WhereLit::Int(v)) => v == b,
        (WherePred::Ne(WhereLit::Int(b)), WhereLit::Int(v)) => v != b,
        (WherePred::Ge(WhereLit::Float(b)), WhereLit::Float(v)) => v >= b,
        (WherePred::Le(WhereLit::Float(b)), WhereLit::Float(v)) => v <= b,
        (WherePred::Gt(WhereLit::Float(b)), WhereLit::Float(v)) => v > b,
        (WherePred::Lt(WhereLit::Float(b)), WhereLit::Float(v)) => v < b,
        (
            WherePred::Range { lo, hi },
            WhereLit::Int(v),
        ) => {
            let n = if length_mode {
                // String literal length is not this Int; treat Int as the count.
                *v
            } else {
                *v
            };
            range_int(lo.as_ref(), hi.as_ref(), n)
        }
        (WherePred::Lit(WhereLit::Int(b)), WhereLit::Int(v)) => v == b,
        (WherePred::Eq(WhereLit::String(b)), WhereLit::String(v)) => v == b,
        (WherePred::Ne(WhereLit::String(b)), WhereLit::String(v)) => v != b,
        (WherePred::Lit(WhereLit::String(b)), WhereLit::String(v)) => v == b,
        (
            WherePred::Range { lo, hi },
            WhereLit::String(s),
        ) => range_int(lo.as_ref(), hi.as_ref(), s.chars().count() as i64),
        (WherePred::Ge(WhereLit::Int(b)), WhereLit::String(s)) => (s.chars().count() as i64) >= *b,
        (WherePred::Le(WhereLit::Int(b)), WhereLit::String(s)) => (s.chars().count() as i64) <= *b,
        _ => false,
    }
}

fn cmp_num(v: i64, b: i64, _length_mode: bool) -> i64 {
    v.cmp(&b) as i64
}

fn range_int(lo: Option<&WhereLit>, hi: Option<&WhereLit>, n: i64) -> bool {
    let ok_lo = match lo {
        Some(WhereLit::Int(a)) => n >= *a,
        None => true,
        _ => false,
    };
    let ok_hi = match hi {
        Some(WhereLit::Int(b)) => n <= *b,
        None => true,
        _ => false,
    };
    ok_lo && ok_hi
}

/// Compile error if the clause cannot hold for any value of `base`.
pub fn reject_empty(base: &str, clause: &WhereClause, span: Span) -> Result<()> {
    if base == "Int64" && int_empty(&clause.expr) {
        return Err(Error::at(
            "where clause is empty — no Int64 satisfies it",
            span,
        ));
    }
    if base == "String" && string_empty(&clause.expr) {
        return Err(Error::at(
            "where clause is empty — no String satisfies it",
            span,
        ));
    }
    Ok(())
}

fn int_empty(expr: &WhereExpr) -> bool {
    intervals(expr).is_empty()
}

#[derive(Clone, Copy)]
struct Iv {
    lo: i64,
    hi: i64,
}

fn intervals(expr: &WhereExpr) -> Vec<Iv> {
    match expr {
        WhereExpr::And(a, b) => intersect(&intervals(a), &intervals(b)),
        WhereExpr::Or(a, b) => merge_or(intervals(a), intervals(b)),
        WhereExpr::Atom(atom) => pred_intervals(&atom.pred),
    }
}

fn pred_intervals(pred: &WherePred) -> Vec<Iv> {
    let max = i64::MAX;
    let min = i64::MIN;
    match pred {
        WherePred::Ge(WhereLit::Int(n)) => vec![Iv { lo: *n, hi: max }],
        WherePred::Le(WhereLit::Int(n)) => vec![Iv { lo: min, hi: *n }],
        WherePred::Gt(WhereLit::Int(n)) => {
            if *n == max {
                vec![]
            } else {
                vec![Iv { lo: n + 1, hi: max }]
            }
        }
        WherePred::Lt(WhereLit::Int(n)) => {
            if *n == min {
                vec![]
            } else {
                vec![Iv { lo: min, hi: n - 1 }]
            }
        }
        WherePred::Eq(WhereLit::Int(n)) | WherePred::Lit(WhereLit::Int(n)) => {
            vec![Iv { lo: *n, hi: *n }]
        }
        WherePred::Ne(WhereLit::Int(_)) => vec![Iv { lo: min, hi: max }],
        WherePred::Range { lo, hi } => {
            let a = match lo {
                Some(WhereLit::Int(n)) => *n,
                None => min,
                _ => return vec![],
            };
            let b = match hi {
                Some(WhereLit::Int(n)) => *n,
                None => max,
                _ => return vec![],
            };
            if a > b {
                vec![]
            } else {
                vec![Iv { lo: a, hi: b }]
            }
        }
        _ => vec![Iv { lo: min, hi: max }],
    }
}

fn intersect(a: &[Iv], b: &[Iv]) -> Vec<Iv> {
    let mut out = Vec::new();
    for x in a {
        for y in b {
            let lo = x.lo.max(y.lo);
            let hi = x.hi.min(y.hi);
            if lo <= hi {
                out.push(Iv { lo, hi });
            }
        }
    }
    out
}

fn merge_or(mut a: Vec<Iv>, b: Vec<Iv>) -> Vec<Iv> {
    a.extend(b);
    a
}

fn string_empty(expr: &WhereExpr) -> bool {
    // Only detect contradictory length ranges.
    int_empty(expr)
}

pub fn and_clauses(a: &WhereExpr, b: &WhereExpr) -> WhereExpr {
    WhereExpr::And(Box::new(a.clone()), Box::new(b.clone()))
}

pub fn walk_atoms(expr: &WhereExpr, f: &mut impl FnMut(&WhereAtom) -> Result<()>) -> Result<()> {
    match expr {
        WhereExpr::And(a, b) | WhereExpr::Or(a, b) => {
            walk_atoms(a, f)?;
            walk_atoms(b, f)
        }
        WhereExpr::Atom(atom) => f(atom),
    }
}
