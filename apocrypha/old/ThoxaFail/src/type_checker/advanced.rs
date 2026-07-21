use crate::ast::{Expr, TypedExpr, TypeMember};
use crate::types::{Type, TypeEnv, MemberType};
use crate::symbols::{SymbolTable, ImportedBinding};
use std::collections::HashMap;
use super::resolution::{resolve_type_expr, resolve_member_type, types_compatible};

/// Temporary helper to create TypedExpr from Expr and Type
/// This will be removed once the full refactoring is complete
fn make_typed_expr(expr: Expr, ty: Type) -> TypedExpr {
    match &expr {
        Expr::Number(value) => TypedExpr::Number {
            value: *value,
            ty,
        },
        Expr::StringLiteral(value) => TypedExpr::StringLiteral {
            value: value.clone(),
            ty,
        },
        Expr::Identifier(name) => TypedExpr::Identifier {
            name: name.clone(),
            ty,
        },
        _ => {
            // All other cases use Untyped fallback
            TypedExpr::Untyped { expr, ty }
        }
    }
}

/// Type check advanced expressions that require complex logic
/// Returns None if the expression doesn't match any of the advanced cases
pub fn type_check_advanced_expr(
    expr: &Expr,
    env: &mut TypeEnv,
    symbols: &SymbolTable,
    type_check_expr: &dyn Fn(&Expr, &mut TypeEnv, &SymbolTable) -> Result<TypedExpr, String>
) -> Option<Result<TypedExpr, String>> {
    match expr {
        Expr::Identifier(name) => {
            // Handle complex import cases that aren't in basic expressions
            // Basic identifier lookup is handled in expressions.rs, but imports need special handling
            if let Some(import) = symbols.get_import(name) {
                // Handle imported bindings
                match import {
                    ImportedBinding::TypeInstance { module_name, .. } => {
                        // Look up the module type
                        if let Some(module_type) = symbols.get_type(module_name) {
                            let members = module_type.members.iter()
                                .map(|(k, v)| {
                                    let (type_expr, exported) = match v {
                                        crate::symbols::TypeMemberInfo::LetBinding { type_expr, exported, .. } => {
                                            (type_expr, *exported)
                                        }
                                        crate::symbols::TypeMemberInfo::FunctionDef {  return_type, exported, .. } => {
                                            // For functions, create a function type
                                            // For now, just use return type as a placeholder
                                            (return_type, *exported)
                                        }
                                    };
                                    let ty = resolve_member_type(type_expr, env, symbols).ok()?;
                                    Some((k.clone(), MemberType { ty, exported }))
                                })
                                .collect::<Option<HashMap<_, _>>>()
                                .ok_or_else(|| "Failed to resolve module member types".to_string());
                            let members = match members {
                                Ok(m) => m,
                                Err(e) => return Some(Err(e)),
                            };
                            
                            let methods = module_type.members.iter()
                                .filter_map(|(k, v)| {
                                    match v {
                                        crate::symbols::TypeMemberInfo::FunctionDef { params, return_type, exported, .. } => {
                                            if *exported {
                                                // Build function type for the method
                                                let param_types: Vec<Type> = params.iter()
                                                    .filter_map(|p| resolve_type_expr(&p.type_expr, env, symbols).ok())
                                                    .collect();
                                                let ret_type = resolve_type_expr(return_type, env, symbols).ok()?;
                                                Some((k.clone(), Type::Function {
                                                    params: param_types,
                                                    return_type: Box::new(ret_type),
                                                    variadic: None, // TODO: Handle variadic params
                                                }))
                                            } else {
                                                None
                                            }
                                        }
                                        _ => None,
                                    }
                                })
                                .collect();
                            
                            let ty = Type::Struct {
                                name: module_name.clone(),
                                members,
                                methods,
                                conversions: HashMap::new(), // TODO: load conversions from imported module
                            };
                            Some(Ok(TypedExpr::Untyped { expr: expr.clone(), ty }))
                        } else {
                            Some(Err(format!("Module type '{}' not found", module_name)))
                        }
                    }
                    ImportedBinding::TypeInfo { .. } => {
                        // This is a type import, it should evaluate to Type
                        Some(Ok(TypedExpr::Identifier { name: name.clone(), ty: Type::Type }))
                    }
                    ImportedBinding::DestructuredMember { member_info, .. } => {
                        // Get the type of the destructured member based on its kind
                        let ty = match member_info {
                            crate::symbols::TypeMemberInfo::LetBinding { type_expr, .. } => {
                                match resolve_member_type(type_expr, env, symbols) {
                                    Ok(ty) => ty,
                                    Err(e) => return Some(Err(e)),
                                }
                            }
                            crate::symbols::TypeMemberInfo::FunctionDef { params, return_type, .. } => {
                                // Create function type
                                let param_types: Result<Vec<Type>, String> = params.iter()
                                    .map(|p| resolve_type_expr(&p.type_expr, env, symbols))
                                    .collect();
                                let param_types = match param_types {
                                    Ok(types) => types,
                                    Err(e) => return Some(Err(e)),
                                };
                                let ret_type = match resolve_type_expr(return_type, env, symbols) {
                                    Ok(ty) => ty,
                                    Err(e) => return Some(Err(e)),
                                };
                                Type::Function {
                                    params: param_types,
                                    return_type: Box::new(ret_type),
                                    variadic: None, // TODO: Handle variadic params
                                }
                            }
                        };
                        Some(Ok(make_typed_expr(expr.clone(), ty)))
                    }
                }
            } else {
                // Not an import - let basic expressions handle it
                None
            }
        }
        
        Expr::TypeDef { name, members } => {
            // Create struct type
            let mut struct_members = HashMap::new();
            let mut struct_methods = HashMap::new();
            let mut struct_conversions = HashMap::new();
            
            // Create a new environment for the type
            let mut type_env = TypeEnv::child(env.clone());
            
            for member in members {
                match member {
                    TypeMember::LetBinding { name, type_expr, value: _, exported } => {
                        // Resolve member type
                        let member_type = match resolve_type_expr(type_expr, &mut type_env, symbols) {
                            Ok(ty) => ty,
                            Err(e) => return Some(Err(e)),
                        };
                        
                        struct_members.insert(
                            name.clone(),
                            MemberType {
                                ty: member_type.clone(),
                                exported: *exported,
                            }
                        );
                        
                        // Add to module environment
                        type_env.insert_var(name.clone(), member_type);
                    }
                    TypeMember::FunctionDef { name, params, return_type, body, exported } => {
                        // Check if last param is variadic
                        let _has_variadic = params.last().map(|p| p.variadic).unwrap_or(false);
                        
                        // Check that only last param is variadic
                        for (i, param) in params.iter().enumerate() {
                            if param.variadic && i != params.len() - 1 {
                                return Some(Err(format!(
                                    "Variadic parameter '{}' must be the last parameter",
                                    param.name
                                )));
                            }
                        }
                        
                        // Create function type
                        let mut param_types = Vec::new();
                        let mut variadic_type = None;
                        
                        for param in params {
                            let param_type = match resolve_type_expr(&param.type_expr, &mut type_env, symbols) {
                                Ok(ty) => ty,
                                Err(e) => return Some(Err(e)),
                            };
                            
                            if param.variadic {
                                // Variadic parameter - store its element type
                                variadic_type = Some(Box::new(param_type));
                            } else {
                                param_types.push(param_type);
                            }
                        }
                        
                        let ret_type = match resolve_type_expr(return_type, &mut type_env, symbols) {
                            Ok(ty) => ty,
                            Err(e) => return Some(Err(e)),
                        };
                        
                        let func_type = Type::Function {
                            params: param_types,
                            return_type: Box::new(ret_type.clone()),
                            variadic: variadic_type,
                        };
                        
                        struct_members.insert(
                            name.clone(),
                            MemberType {
                                ty: func_type.clone(),
                                exported: *exported,
                            }
                        );
                        
                        // Add to module environment
                        type_env.insert_func(name.clone(), func_type.clone());
                        
                        // Add to methods if exported
                        if *exported {
                            struct_methods.insert(name.clone(), func_type);
                        }
                        
                        // Type check the function body with proper parameter bindings
                        let mut func_env = type_env.clone();
                        
                        // Bind parameters in the function's type environment
                        for param in params {
                            let param_type = match resolve_type_expr(&param.type_expr, &type_env, symbols) {
                                Ok(ty) => ty,
                                Err(e) => return Some(Err(e)),
                            };
                            
                            if param.variadic {
                                // Variadic parameter should be bound as an array type
                                let array_type = Type::Array {
                                    element_type: Box::new(param_type),
                                    size: None, // Dynamic size
                                };
                                func_env.insert_var(param.name.clone(), array_type);
                            } else {
                                func_env.insert_var(param.name.clone(), param_type);
                            }
                        }
                        
                        // Type check the function body
                        let body_typed = match type_check_expr(body, &mut func_env, symbols) {
                            Ok(typed) => typed,
                            Err(e) => return Some(Err(e)),
                        };
                        let body_type = body_typed.get_type();
                        
                        // Check that the body type matches the declared return type
                        if !types_compatible(&body_type, &ret_type) {
                            return Some(Err(format!(
                                "Function '{}' body returns {} but is declared to return {}",
                                name, body_type.name(), ret_type.name()
                            )));
                        }
                    }
                    TypeMember::Conversion { target_type, body: _ } => {
                        // Resolve target type
                        let target = match resolve_type_expr(target_type, &type_env, symbols) {
                            Ok(ty) => ty,
                            Err(e) => return Some(Err(e)),
                        };
                        let target_name = target.name();
                        
                        // Create conversion function type (no params, returns target type)
                        let conversion_type = Type::Function {
                            params: vec![],
                            return_type: Box::new(target),
                            variadic: None,
                        };
                        
                        struct_conversions.insert(target_name, conversion_type);
                    }
                }
            }
            
            let struct_type = Type::Struct {
                name: name.clone(),
                members: struct_members,
                methods: struct_methods,
                conversions: struct_conversions,
            };
            
            // Add type to environment
            env.insert_type(name.clone(), struct_type.clone());
            
            // Module definitions evaluate to Void
            Some(Ok(TypedExpr::Untyped {
                expr: expr.clone(),
                ty: Type::Void,
            }))
        }
        
        Expr::FieldAccess { object, field } => {
            // Type check the object
            let object_typed = match type_check_expr(object, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Look up field type
            let field_type = match object_typed.get_type() {
                Type::Struct { members, .. } => {
                    if let Some(member) = members.get(field) {
                        Ok(member.ty.clone())
                    } else {
                        Err(format!("Module has no field '{}'", field))
                    }
                }
                _ => Err(format!("Field access on non-module type {}", object_typed.get_type().name()))
            };
            
            match field_type {
                Ok(ty) => Some(Ok(TypedExpr::FieldAccess {
                    object: Box::new(object_typed),
                    field: field.clone(),
                    ty,
                })),
                Err(e) => Some(Err(e)),
            }
        }
        
        Expr::FunctionCall { function, arguments } => {
            // Type check all arguments first
            let typed_args: Result<Vec<TypedExpr>, String> = arguments.iter()
                .map(|arg| type_check_expr(arg, env, symbols))
                .collect();
            let typed_args = match typed_args {
                Ok(args) => args,
                Err(e) => return Some(Err(e)),
            };
            
            // Get argument types for checking
            let arg_types: Vec<Type> = typed_args.iter()
                .map(|arg| arg.get_type().clone())
                .collect();
            
            // Check if this is a method call (function call on field access)
            if let Expr::FieldAccess { object, field } = function.as_ref() {
                // This is a method call: object.method(args)
                let object_typed = match type_check_expr(object, env, symbols) {
                    Ok(typed) => typed,
                    Err(e) => return Some(Err(e)),
                };
                
                // Check if the object type has this method
                if let Some(method_type) = object_typed.get_type().get_method(field) {
                    
                    match &method_type {
                        Type::Function { params, return_type, variadic } => {
                            // Check argument count
                            let min_args = params.len();
                            
                            if let Some(variadic_type) = variadic {
                                // Variadic function - need at least min_args
                                if arg_types.len() < min_args {
                                    return Some(Err(format!(
                                        "Method '{}' expects at least {} arguments, got {}",
                                        field, min_args, arg_types.len()
                                    )));
                                }
                                
                                // Check fixed argument types
                                for (i, (param_type, arg_type)) in params.iter().zip(arg_types.iter()).enumerate() {
                                    if !types_compatible(arg_type, param_type) {
                                        return Some(Err(format!(
                                            "Type mismatch in argument {}: expected {}, got {}",
                                            i + 1, param_type.name(), arg_type.name()
                                        )));
                                    }
                                }
                                
                                // Check variadic argument types
                                for (i, arg_type) in arg_types[min_args..].iter().enumerate() {
                                    if !types_compatible(arg_type, variadic_type) {
                                        return Some(Err(format!(
                                            "Type mismatch in variadic argument {}: expected {}, got {}",
                                            i + 1, variadic_type.name(), arg_type.name()
                                        )));
                                    }
                                }
                            } else {
                                // Non-variadic function - exact count
                                if params.len() != arg_types.len() {
                                    return Some(Err(format!(
                                        "Method '{}' expects {} arguments, got {}",
                                        field, params.len(), arg_types.len()
                                    )));
                                }
                                
                                // Check argument types
                                for (i, (param_type, arg_type)) in params.iter().zip(arg_types.iter()).enumerate() {
                                    if !types_compatible(arg_type, param_type) {
                                        return Some(Err(format!(
                                            "Type mismatch in argument {}: expected {}, got {}",
                                            i + 1, param_type.name(), arg_type.name()
                                        )));
                                    }
                                }
                            }
                            
                            // Return a proper method call
                            Some(Ok(TypedExpr::MethodCall {
                                object: Box::new(object_typed),
                                method: field.clone(),
                                arguments: typed_args,
                                ty: (**return_type).clone(),
                            }))
                        }
                        _ => unreachable!("Method type should always be a function")
                    }
                } else {
                    Some(Err(format!("Type '{}' does not have method '{}'", object_typed.get_type().name(), field)))
                }
            } else {
                // Regular function call
                let func_typed = match type_check_expr(function, env, symbols) {
                    Ok(typed) => typed,
                    Err(e) => return Some(Err(e)),
                };
                
                // Check function type and get return type
                let return_type = match func_typed.get_type() {
                    Type::Function { params, return_type, variadic } => {
                        // Check argument count
                        let min_args = params.len();
                        
                        if let Some(variadic_type) = variadic {
                            // Variadic function - need at least min_args
                            if arg_types.len() < min_args {
                                return Some(Err(format!(
                                    "Function expects at least {} arguments, got {}",
                                    min_args, arg_types.len()
                                )));
                            }
                            
                            // Check fixed argument types
                            for (i, (param_type, arg_type)) in params.iter().zip(arg_types.iter()).enumerate() {
                                if !types_compatible(arg_type, param_type) {
                                    return Some(Err(format!(
                                        "Type mismatch in argument {}: expected {}, got {}",
                                        i + 1, param_type.name(), arg_type.name()
                                    )));
                                }
                            }
                            
                            // Check variadic argument types
                            for (i, arg_type) in arg_types[min_args..].iter().enumerate() {
                                if !types_compatible(arg_type, variadic_type) {
                                    return Some(Err(format!(
                                        "Type mismatch in variadic argument {}: expected {}, got {}",
                                        i + 1, variadic_type.name(), arg_type.name()
                                    )));
                                }
                            }
                        } else {
                            // Non-variadic function - exact count
                            if params.len() != arg_types.len() {
                                return Some(Err(format!(
                                    "Function expects {} arguments, got {}",
                                    params.len(),
                                    arg_types.len()
                                )));
                            }
                            
                            // Check argument types
                            for (i, (param_type, arg_type)) in params.iter().zip(arg_types.iter()).enumerate() {
                                if !types_compatible(arg_type, param_type) {
                                    return Some(Err(format!(
                                        "Type mismatch in argument {}: expected {}, got {}",
                                        i + 1, param_type.name(), arg_type.name()
                                    )));
                                }
                            }
                        }
                        
                        Ok((**return_type).clone())
                    }
                    _ => Err(format!("Call on non-function type {}", func_typed.get_type().name()))
                };
                
                match return_type {
                    Ok(ty) => Some(Ok(TypedExpr::FunctionCall {
                        function: Box::new(func_typed),
                        arguments: typed_args,
                        ty,
                    })),
                    Err(e) => Some(Err(e)),
                }
            }
        }
        
        Expr::MethodCall { object, method, arguments } => {
            // Type check the object
            let object_typed = match type_check_expr(object, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Check if the object type has this method
            if let Some(method_type) = object_typed.get_type().clone().get_method(method) {
                // Type check arguments
                let typed_args: Result<Vec<TypedExpr>, String> = arguments.iter()
                    .map(|arg| type_check_expr(arg, env, symbols))
                    .collect();
                let typed_args = match typed_args {
                    Ok(args) => args,
                    Err(e) => return Some(Err(e)),
                };
                
                // Get argument types for checking
                let arg_types: Vec<Type> = typed_args.iter()
                    .map(|arg| arg.get_type().clone())
                    .collect();
                
                match &method_type {
                    Type::Function { params, return_type, variadic: _ } => {
                        // Check argument count
                        if params.len() != arg_types.len() {
                            return Some(Err(format!(
                                "Method '{}' expects {} arguments, got {}",
                                method, params.len(), arg_types.len()
                            )));
                        }
                        
                        // Check argument types
                        for (i, (param_type, arg_type)) in params.iter().zip(arg_types.iter()).enumerate() {
                            if !types_compatible(arg_type, param_type) {
                                return Some(Err(format!(
                                    "Type mismatch in argument {}: expected {}, got {}",
                                    i + 1, param_type.name(), arg_type.name()
                                )));
                            }
                        }
                        
                        Some(Ok(TypedExpr::MethodCall {
                            object: Box::new(object_typed),
                            method: method.clone(),
                            arguments: typed_args,
                            ty: (**return_type).clone(),
                        }))
                    }
                    _ => unreachable!("Method type should always be a function")
                }
            } else {
                Some(Err(format!("Type '{}' does not have method '{}'", object_typed.get_type().clone().name(), method)))
            }
        }
        
        Expr::ArrayLiteral { elements } => {
            if elements.is_empty() {
                // Empty array needs type annotation
                return Some(Err("Empty array literals need type annotation".to_string()));
            }
            
            // Type check all elements
            let element_types: Result<Vec<Type>, String> = elements.iter()
                .map(|elem| type_check_expr(elem, env, symbols).map(|t| t.get_type().clone()))
                .collect();
            let element_types = match element_types {
                Ok(types) => types,
                Err(e) => return Some(Err(e)),
            };
            
            // Check all elements have the same type
            let first_type = &element_types[0];
            for (i, elem_type) in element_types.iter().enumerate().skip(1) {
                if elem_type != first_type {
                    return Some(Err(format!(
                        "Array element type mismatch at index {}: expected {}, got {}",
                        i, first_type.name(), elem_type.name()
                    )));
                }
            }
            
            // Create array type
            let array_type = Type::Array {
                element_type: Box::new(first_type.clone()),
                size: Some(elements.len()),
            };
            
            Some(Ok(make_typed_expr(expr.clone(), array_type)))
        }
        
        Expr::ArrayIndex { array, index } => {
            // Type check array
            let array_typed = match type_check_expr(array, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Type check index (must be integer)
            let index_typed = match type_check_expr(index, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            if !index_typed.get_type().clone().is_numeric() {
                return Some(Err(format!(
                    "Array index must be numeric, got {}",
                    index_typed.get_type().clone().name()
                )));
            }
            
            match &array_typed.get_type().clone() {
                Type::Array { element_type, .. } => {
                    Some(Ok(make_typed_expr(expr.clone(), (**element_type).clone())))
                }
                _ => Some(Err(format!(
                    "Cannot index non-array type {}",
                    array_typed.get_type().clone().name()
                )))
            }
        }
        
        Expr::Conversion { expr, target_type } => {
            // Type check the expression to convert
            let expr_typed = match type_check_expr(expr, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Resolve the target type
            let target = match resolve_type_expr(target_type, env, symbols) {
                Ok(ty) => ty,
                Err(e) => return Some(Err(e)),
            };
            let target_name = target.name();
            
            // Check if conversion exists
            if let Some(_conversion_path) = expr_typed.get_type().clone().find_conversion_path(&target_name, env) {
                // For now, we just return the target type
                // In code generation, we'll actually call the conversion
                Some(Ok(make_typed_expr((**expr).clone(), target)))
            } else {
                Some(Err(format!(
                    "No conversion from {} to {}",
                    expr_typed.get_type().clone().name(),
                    target_name
                )))
            }
        }
        
        Expr::CanConvert { expr, target_type } => {
            // Type check the expression
            let expr_typed = match type_check_expr(expr, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Resolve the target type
            let target = match resolve_type_expr(target_type, env, symbols) {
                Ok(ty) => ty,
                Err(e) => return Some(Err(e)),
            };
            let target_name = target.name();
            
            // Check if conversion exists
            let can_convert = expr_typed.get_type().clone().find_conversion_path(&target_name, env).is_some();
            
            // Return a boolean value (treating it as Int8 for now)
            let bool_expr = if can_convert {
                Expr::Number(1)
            } else {
                Expr::Number(0)
            };
            Some(Ok(make_typed_expr(bool_expr, Type::int(8))))
        }
        
        Expr::TypeGuard { expr, type_expr } => {
            // Type check the expression
            let expr_typed = match type_check_expr(expr, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Resolve the target type
            let _target = match resolve_type_expr(type_expr, env, symbols) {
                Ok(ty) => ty,
                Err(e) => return Some(Err(e)),
            };
            
            // Type guards return boolean (Int8)
            Some(Ok(TypedExpr::TypeGuard {
                expr: Box::new(expr_typed),
                type_expr: type_expr.clone(),
                ty: Type::int(8),
            }))
        }
        
        Expr::TemplateLiteral { parts: _, expressions } => {
            // Type check all embedded expressions and ensure they can convert to String
            for expr in expressions {
                let expr_typed = match type_check_expr(expr, env, symbols) {
                    Ok(typed) => typed,
                    Err(e) => return Some(Err(e)),
                };
                
                // Check if expression can be converted to String
                if expr_typed.get_type().clone().find_conversion_path("String", env).is_none() {
                    return Some(Err(format!(
                        "Cannot convert {} to String in template literal",
                        expr_typed.get_type().clone().name()
                    )));
                }
            }
            // Template literals always evaluate to String
            Some(Ok(make_typed_expr(expr.clone(), Type::String)))
        }
        
        Expr::Import { .. } => {
            // Imports evaluate to Void
            Some(Ok(make_typed_expr(expr.clone(), Type::Void)))
        }
        
        Expr::Reference { expr: inner } => {
            // Type check inner expression
            let _inner_typed = match type_check_expr(inner, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // References are pointers (Int64 for now)
            Some(Ok(make_typed_expr(expr.clone(), Type::int(64))))
        }
        
        // Builtin functions
        Expr::Print { value } => {
            // Type check the value (should be Int64)
            let value_typed = match type_check_expr(value, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Print returns Void
            Some(Ok(TypedExpr::Print {
                value: Box::new(value_typed),
                ty: Type::Void,
            }))
        }
        
        Expr::PrintStr { value } => {
            // Type check the value (should be String/pointer)
            let value_typed = match type_check_expr(value, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // PrintStr returns Void
            Some(Ok(TypedExpr::PrintStr {
                value: Box::new(value_typed),
                ty: Type::Void,
            }))
        }
        
        Expr::Syscall { number, arguments } => {
            // Type check all arguments
            let _num_typed = match type_check_expr(number, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            for arg in arguments {
                let _arg_typed = match type_check_expr(arg, env, symbols) {
                    Ok(typed) => typed,
                    Err(e) => return Some(Err(e)),
                };
            }
            
            // Syscall returns Int64
            Some(Ok(make_typed_expr(expr.clone(), Type::int(64))))
        }
        
        Expr::Strlen { value } => {
            // Type check the value
            let _value_typed = match type_check_expr(value, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Strlen returns Int64
            Some(Ok(make_typed_expr(expr.clone(), Type::int(64))))
        }
        
        Expr::Cstr { value } => {
            // Type check the value (should be String)
            let _value_typed = match type_check_expr(value, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Cstr returns a pointer (Int64)
            Some(Ok(make_typed_expr(expr.clone(), Type::int(64))))
        }
        
        Expr::IntToString { value } => {
            // Type check the value
            let _value_typed = match type_check_expr(value, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // IntToString returns String (pointer)
            Some(Ok(make_typed_expr(expr.clone(), Type::String)))
        }
        
        // Return None for expressions that are not advanced and need other handling
        _ => None,
    }
}