use crate::ast::TypedExpr;
use crate::types::Type;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry};
use crate::codegen::tagged_values::tag_int_value;
use cranelift::prelude::*;
use cranelift_module::Module;

/// Handle literal expressions
pub fn translate_literals<M: Module>(
    typed_expr: &TypedExpr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable,
) -> Value {
    match typed_expr {
        TypedExpr::Number { value, ty: _ } => {
            let raw_value = builder.ins().iconst(types::I64, *value);
            // Tag the integer value
            tag_int_value(builder, raw_value)
        }
        
        TypedExpr::StringLiteral { value, ty: _ } => {
            // Create string with length prefix
            let string_bytes = value.as_bytes();
            let length = string_bytes.len() as u64;
            
            // Create data segment with length prefix + string data + null terminator
            let data_id = module
                .declare_anonymous_data(false, false)
                .expect("Failed to declare string data");
            
            let mut string_data = Vec::new();
            string_data.extend_from_slice(&length.to_le_bytes()); // 8 bytes length
            string_data.extend_from_slice(string_bytes);           // string bytes
            string_data.push(0);                                    // null terminator
            
            let mut data_desc = cranelift_module::DataDescription::new();
            data_desc.define(string_data.into_boxed_slice());
            module.define_data(data_id, &data_desc).expect("Failed to define string data");
            
            let local_id = module.declare_data_in_func(data_id, builder.func);
            let string_ptr = builder.ins().symbol_value(types::I64, local_id);
            
            // Strings are already pointers (8-byte aligned), low 3 bits are 000
            string_ptr
        }
        
        TypedExpr::Identifier { name, ty } => {
            // Check if this is a type name (for automatic module instantiation)
            if matches!(ty, Type::Struct { .. }) && var_env.get_variable(name).is_none() {
                // This is a reference to a type, return 0 for now
                // In the future, this could return a type descriptor
                builder.ins().iconst(types::I64, 0)
            } else {
                // Regular identifier lookup
                if let Some(var) = var_env.get_variable(name) {
                    builder.use_var(var)
                } else {
                    // Unknown variable - this should be caught by type checker
                    eprintln!("ERROR: Unknown variable '{}' in TypedExpr::Identifier", name);
                    builder.ins().iconst(types::I64, 0)
                }
            }
        }
        
        _ => panic!("Not a literal expression"),
    }
}