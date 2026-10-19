//! D196: `extend` blocks.
//!
//! `extend String { export fn splitOnce(separator!: String) -> … }` declares
//! methods on a built-in type, written in Thoxa; the receiver is `self`
//! (D228). Only std modules may
//! extend a type, so a type's methods do not depend on what a program imports.
//! Each method becomes a free function `__extend_String_splitOnce` whose first
//! parameter is the receiver, exported when the method is; a method call on a
//! value of that type finds it by name.

use crate::ast::*;
use crate::ast_build::cast_reader_key;
use crate::error::{Error, Result};
use crate::loader::{std_lib_root, Package};

/// Built-in types an `extend` block may name.
const EXTENDABLE: &[&str] = &[
    "String", "Bytes", "Buffer", "Array", "Map", "Int64", "Int32", "UInt64", "UInt32", "Byte",
    "Float64", "Bool",
];

pub fn desugar_package(package: &mut Package) -> Result<()> {
    let std_root = std_lib_root().ok().and_then(|root| root.canonicalize().ok());
    for module in &mut package.modules {
        let in_std = std_root
            .as_ref()
            .is_some_and(|root| module.path.starts_with(root));
        let first_extension = module.ast.items.iter().find_map(|item| match item {
            Item::Extend(extension) => Some(extension.span),
            _ => None,
        });
        if let (Some(span), false) = (first_extension, in_std) {
            return Err(Error::at(
                "only a std module can extend a built-in type",
                span,
            )
            .with_help("write a function that takes the value instead")
            .with_path(module.path.clone()));
        }
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
        let Item::Extend(extension) = item else {
            continue;
        };
        check_type(&extension.ty)?;
        let key = cast_reader_key(&extension.ty);
        for method in &extension.methods {
            if let Some(param) = method.params.iter().find(|p| p.name == "self") {
                return Err(Error::at(
                    "`self` is the receiver; a parameter cannot take its name",
                    param.name_span,
                ));
            }
            let receiver = Param {
                name: "self".into(),
                name_span: extension.span,
                positional: true,
                mutable: false,
                ty: extension.ty.clone(),
                default: None,
            };
            functions.push(Function {
                exported: method.exported,
                name: extension_fn_name(&key, &method.name),
                name_span: method.name_span,
                params: std::iter::once(receiver).chain(method.params.iter().cloned()).collect(),
                return_type: method.return_type.clone(),
                body: method.body.clone(),
                type_params: Vec::new(),
                generic: Vec::new(),
            });
        }
    }
    module.items.retain(|item| !matches!(item, Item::Extend(_)));
    module.items.extend(functions.into_iter().map(Item::Function));
    Ok(())
}

/// The function an `extend` method becomes; `key` names the type
/// (`String`, `Array_String`).
pub fn extension_fn_name(key: &str, method: &str) -> String {
    format!("__extend_{key}_{method}")
}

fn check_type(ty: &TypeExpr) -> Result<()> {
    if ty.lit.is_some()
        || ty.union_members.is_some()
        || ty.fn_type.is_some()
        || ty.where_clause.is_some()
        || !EXTENDABLE.contains(&ty.name.as_str())
    {
        return Err(Error::at(
            "`extend` names a built-in type (String, Bytes, Array<…>, …); a declared type's methods go in its body",
            ty.span,
        ));
    }
    Ok(())
}
