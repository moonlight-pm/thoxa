//! D191 §3: `from` clauses.
//!
//! `type T { from(source: S) -> T { … } }` declares a conversion from `S` into
//! `T`. Each clause becomes a free function `__conversion_T_N` in the type's
//! module, exported when the clause is; `x as T` finds it there by name. The
//! clauses stay on the type so the codec pass can tell that a type reads
//! itself (a `from(json: Json)` clause). A generic type's clauses become one
//! function per instance, made in MIR with the instance's types (D217).

use crate::ast::*;
use crate::ast_build::cast_reader_key;
use crate::error::{Error, Result};
use crate::loader::Package;
use std::collections::HashSet;

pub fn desugar_package(package: &mut Package) -> Result<()> {
    for module in &mut package.modules {
        desugar_module(&mut module.ast).map_err(|error| {
            if error.path.is_none() {
                error.with_path(module.path.clone())
            } else {
                error
            }
        })?;
    }
    Ok(())
}

pub fn desugar_module(module: &mut Module) -> Result<()> {
    let mut functions = Vec::new();
    for item in &module.items {
        let Item::Type(td) = item else {
            continue;
        };
        if td.conversions.is_empty() {
            continue;
        }
        let mut sources = HashSet::new();
        for (index, clause) in td.conversions.iter().enumerate() {
            check_clause(td, clause, &mut sources)?;
            if !td.type_params.is_empty() {
                continue;
            }
            functions.push(Function {
                exported: clause.exported,
                name: conversion_fn_name(&td.name, index),
                name_span: clause.span,
                // `as` calls it by position (D200).
                params: vec![Param {
                    positional: true,
                    ..clause.param.clone()
                }],
                return_type: Some(clause.return_type.clone()),
                body: clause.body.clone(),
                type_params: Vec::new(),
                generic: Vec::new(),
            });
        }
    }
    module.items.extend(functions.into_iter().map(Item::Function));
    Ok(())
}

/// `Box<T>` for `type Box<T>`, `Point` for `type Point`.
fn self_name(td: &TypeDef) -> String {
    if td.type_params.is_empty() {
        td.name.clone()
    } else {
        format!("{}<{}>", td.name, td.type_params.join(", "))
    }
}

/// The function a type's `index`-th `from` clause becomes.
pub fn conversion_fn_name(type_name: &str, index: usize) -> String {
    format!("__conversion_{type_name}_{index}")
}

fn check_clause(td: &TypeDef, clause: &Conversion, sources: &mut HashSet<String>) -> Result<()> {
    if clause.param.default.is_some() {
        return Err(Error::at(
            "a `from` clause's parameter cannot have a default",
            clause.param.name_span,
        ));
    }
    let source = &clause.param.ty;
    // `Box<T>` inside `type Box<T>`: the type's own name and parameters.
    let names_self = |ty: &TypeExpr| {
        ty.name == td.name
            && ty.args.len() == td.type_params.len()
            && ty.args.iter().zip(&td.type_params).all(|(arg, param)| {
                matches!(arg, TypeArg::Type(inner) if inner.name == *param && inner.args.is_empty())
            })
    };
    if names_self(source) {
        return Err(Error::at(
            format!("`{}` already converts to itself; a `from` clause converts from another type", td.name),
            clause.span,
        ));
    }
    if !sources.insert(cast_reader_key(source)) {
        return Err(Error::at(
            format!("`{}` has two `from` clauses for the same source type", td.name),
            clause.span,
        ));
    }
    let returns = &clause.return_type;
    let trial_of_self = returns.name == "Trial"
        && returns.args.len() == 2
        && matches!(&returns.args[0], TypeArg::Type(ok) if names_self(ok));
    if !names_self(returns) && !trial_of_self {
        return Err(Error::at(
            format!(
                "a `from` clause in `{0}` returns `{0}`, or `Trial<{0}, E>` when the conversion can fail",
                self_name(td)
            ),
            returns.span,
        ));
    }
    Ok(())
}
