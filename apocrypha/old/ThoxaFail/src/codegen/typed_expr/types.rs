use crate::ast::{Expr, TypedExpr};
use crate::types::Type;
use crate::runtime::BuiltinTypeId;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry, translate_typed_expr, translate_expr};
use crate::codegen::tagged_values::extract_value;
use cranelift::prelude::*;
use cranelift_module::Module;

/// Handle type-related expressions
pub fn translate_types<M: Module>(
    typed_expr: &TypedExpr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable,
) -> Value {
    match typed_expr {
        TypedExpr::TypeGuard { expr, type_expr, ty } => {
            // Evaluate the expression to check
            let expr_tagged = translate_typed_expr(expr, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // For tagged pointers, we need to check the type:
            // - Integers have tag 0x1
            // - Booleans have tags 0x2 (false) or 0x6 (true)
            // - Pointers have tag 0x0 and type ID in object header
            
            // Get the tag bits
            let tag_mask = builder.ins().iconst(types::I64, 0x7);
            let tag = builder.ins().band(expr_tagged, tag_mask);
            
            // Check if it's a pointer (tag == 0)
            let is_ptr = builder.ins().icmp_imm(IntCC::Equal, tag, 0);
            
            // For pointers, load type ID from object header
            // For non-pointers, use a simple type based on tag
            let ptr_block = builder.create_block();
            let non_ptr_block = builder.create_block();
            let merge_block = builder.create_block();
            builder.append_block_param(merge_block, types::I64);
            
            builder.ins().brif(is_ptr, ptr_block, &[], non_ptr_block, &[]);
            
            // Pointer path - load type ID from header
            builder.switch_to_block(ptr_block);
            builder.seal_block(ptr_block);
            let type_id_ptr = expr_tagged; // The pointer itself
            let type_id = builder.ins().load(types::I32, MemFlags::new(), type_id_ptr, 0);
            let type_id_64 = builder.ins().uextend(types::I64, type_id);
            builder.ins().jump(merge_block, &[type_id_64]);
            
            // Non-pointer path - determine type from tag
            builder.switch_to_block(non_ptr_block);
            builder.seal_block(non_ptr_block);
            // Check if integer (tag == 1)
            let is_int = builder.ins().icmp_imm(IntCC::Equal, tag, 0x1);
            // Check if boolean (tag == 2 or tag == 6)
            let is_false = builder.ins().icmp_imm(IntCC::Equal, tag, 0x2);
            let is_true = builder.ins().icmp_imm(IntCC::Equal, tag, 0x6);
            let is_bool = builder.ins().bor(is_false, is_true);
            
            // Select type ID based on tag
            let int_type = builder.ins().iconst(types::I64, BuiltinTypeId::Int64 as i64);
            // Booleans don't have a BuiltinTypeId, use a special value
            let bool_type = builder.ins().iconst(types::I64, 0xFFFF); // Special marker for bool
            let unknown_type = builder.ins().iconst(types::I64, 0);
            
            let type_for_tag = builder.ins().select(is_int, int_type, unknown_type);
            let type_for_tag2 = builder.ins().select(is_bool, bool_type, type_for_tag);
            builder.ins().jump(merge_block, &[type_for_tag2]);
            
            // Merge
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            let type_tag = builder.block_params(merge_block)[0];
            
            // Resolve the target type to get its type tag
            let target_type_tag = match type_expr.as_ref() {
                Expr::Identifier(name) => {
                    match name.as_str() {
                        "Int64" => builder.ins().iconst(types::I64, BuiltinTypeId::Int64 as i64),
                        "String" => builder.ins().iconst(types::I64, BuiltinTypeId::String as i64),
                        "Any" => builder.ins().iconst(types::I64, BuiltinTypeId::Any as i64),
                        "Type" => builder.ins().iconst(types::I64, BuiltinTypeId::Type as i64),
                        // TODO: Handle custom types
                        _ => {
                            eprintln!("WARNING: Type guard for custom type '{}' not yet implemented", name);
                            builder.ins().iconst(types::I64, 0)
                        }
                    }
                }
                _ => {
                    eprintln!("WARNING: Complex type expressions in type guards not yet supported");
                    builder.ins().iconst(types::I64, 0)
                }
            };
            
            // Compare the type tags
            let is_equal = builder.ins().icmp(IntCC::Equal, type_tag, target_type_tag);
            let result = builder.ins().uextend(types::I64, is_equal);
            
            result
        }
        
        TypedExpr::Untyped { expr, ty } => {
            // Fall back to untyped expression translation
            let result = translate_expr(expr, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Return result as-is (should already be properly tagged by translate_expr)
            result
        }
        
        TypedExpr::Conversion { expr, target_type, ty: _ } => {
            let source_val = translate_typed_expr(expr, builder, var_env, func_env, module_registry, module, symbol_table);
            let source_type = expr.get_type();
            
            match (source_type, target_type) {
                // Identity conversion
                (src, tgt) if src == tgt => source_val,
                
                // Any to concrete type conversion (runtime type check)
                (Type::Any, ref target) => {
                    // Runtime type checking for tagged pointers
                    // We need to check if the actual runtime type matches the target type
                    
                    // Get the actual type of the value (reuse logic from TypeGuard above)
                    let tag_mask = builder.ins().iconst(types::I64, 0x7);
                    let tag = builder.ins().band(source_val, tag_mask);
                    let is_ptr = builder.ins().icmp_imm(IntCC::Equal, tag, 0);
                    
                    // Determine the target type ID
                    let target_type_id = match target {
                        Type::Int { signed: true, bits: 64 } => BuiltinTypeId::Int64 as i64,
                        Type::String => BuiltinTypeId::String as i64,
                        // Booleans don't have a BuiltinTypeId, use a special marker
                        Type::Void => 0xFFFF, // Bool is represented as Void type for now
                        _ => {
                            eprintln!("WARNING: Conversion from Any to {:?} not supported", target);
                            return source_val;
                        }
                    };
                    
                    // For now, we'll do a simple check and trap if types don't match
                    // In the future, this should return an Option or Result type
                    
                    // Create blocks for type checking
                    let ok_block = builder.create_block();
                    let fail_block = builder.create_block();
                    
                    // For pointers, load and check type ID from header
                    // For non-pointers, check based on tag
                    if target_type_id == BuiltinTypeId::String as i64 {
                        // String must be a pointer
                        builder.ins().brif(is_ptr, ok_block, &[], fail_block, &[]);
                        
                        // Additional check: verify type ID in header
                        builder.switch_to_block(ok_block);
                        let type_id = builder.ins().load(types::I32, MemFlags::new(), source_val, 0);
                        let type_id_64 = builder.ins().uextend(types::I64, type_id);
                        let is_string = builder.ins().icmp_imm(IntCC::Equal, type_id_64, BuiltinTypeId::String as i64);
                        builder.ins().brif(is_string, ok_block, &[], fail_block, &[]);
                    } else if target_type_id == BuiltinTypeId::Int64 as i64 {
                        // Int must have tag 0x1
                        let is_int = builder.ins().icmp_imm(IntCC::Equal, tag, 0x1);
                        builder.ins().brif(is_int, ok_block, &[], fail_block, &[]);
                    } else if target_type_id == 0xFFFF {
                        // Bool must have tag 0x2 or 0x6
                        let is_false = builder.ins().icmp_imm(IntCC::Equal, tag, 0x2);
                        let is_true = builder.ins().icmp_imm(IntCC::Equal, tag, 0x6);
                        let is_bool = builder.ins().bor(is_false, is_true);
                        builder.ins().brif(is_bool, ok_block, &[], fail_block, &[]);
                    }
                    
                    // Fail path - trap with type mismatch
                    builder.switch_to_block(fail_block);
                    builder.seal_block(fail_block);
                    builder.ins().trap(TrapCode::User(0));
                    
                    // Success path - return the value
                    builder.switch_to_block(ok_block);
                    builder.seal_block(ok_block);
                    
                    source_val
                }
                
                // Int to String conversion
                (Type::Int { signed: true, bits: 64 }, Type::String) => {
                    // Extract the actual value from tagged pointer
                    let int_val = extract_value(builder, source_val);
                    
                    // Call int_to_string function
                    if let Some(func_id) = func_env.get_function("int_to_string") {
                        let func_ref = module.declare_func_in_func(func_id, builder.func);
                        let args = vec![int_val];
                        let call = builder.ins().call(func_ref, &args);
                        let results = builder.inst_results(call);
                        
                        let string_ptr = if results.is_empty() {
                            builder.ins().iconst(types::I64, 0)
                        } else {
                            results[0]
                        };
                        
                        // String pointers are already tagged (pointer tag 000)
                        string_ptr
                    } else {
                        eprintln!("ERROR: int_to_string function not found");
                        builder.ins().iconst(types::I64, 0)
                    }
                }
                
                // Unsupported conversion
                _ => {
                    eprintln!("ERROR: Unsupported conversion from {:?} to {:?}", expr.get_type(), target_type);
                    builder.ins().iconst(types::I64, 0)
                }
            }
        }
        
        _ => panic!("Not a type expression"),
    }
}