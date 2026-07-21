use crate::ast::Expr;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry, value_type};
use crate::codegen::tagged_values::extract_value;
use crate::codegen::helpers::{LOOP_EXIT_BLOCKS, LOOP_CONTINUE_BLOCKS, LOOP_VARIABLE_INFO};
use cranelift::prelude::*;
use cranelift_module::Module;

pub fn translate_control_flow<M: Module>(
    expr: &Expr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable,
    translate_expr: &dyn Fn(&Expr, &mut FunctionBuilder, &mut VarEnv, &FuncEnv, &mut ModuleRegistry, &mut M, &crate::symbols::SymbolTable) -> Value,
) -> Option<Value> {
    match expr {
        Expr::If { condition, then_expr, else_expr } => {
            // Evaluate the condition
            let cond_val = translate_expr(condition, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Create blocks for the then and else branches, and a merge block
            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let merge_block = builder.create_block();
            
            // Add a block parameter to the merge block to receive the result
            builder.append_block_param(merge_block, value_type());
            
            // Branch based on condition (0 is false, non-zero is true)
            // Extract the actual value from the fat/tagged pointer for the condition
            let cond_value = if true {
                extract_value(builder, cond_val)
            } else {
                cond_val
            };
            builder.ins().brif(cond_value, then_block, &[], else_block, &[]);
            
            // Generate then branch
            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            let then_val = translate_expr(then_expr, builder, var_env, func_env, module_registry, module, symbol_table);
            builder.ins().jump(merge_block, &[then_val]);
            
            // Generate else branch
            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            let else_val = if let Some(else_expr) = else_expr {
                translate_expr(else_expr, builder, var_env, func_env, module_registry, module, symbol_table)
            } else {
                // No else branch - default to void
                builder.ins().iconst(types::I64, 0)
            };
            builder.ins().jump(merge_block, &[else_val]);
            
            // Continue with merge block
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            
            // The result is the block parameter of the merge block
            Some(builder.block_params(merge_block)[0])
        }
        Expr::Loop { body } => {
            // Create blocks for the loop with proper variable handling
            let loop_block = builder.create_block();
            let exit_block = builder.create_block();
            
            // Add exit block parameter for the break value
            builder.append_block_param(exit_block, value_type());
            
            // Collect all variables that exist before the loop
            // IMPORTANT: We must maintain consistent ordering, so sort by variable name
            let mut loop_vars = Vec::new();
            let mut sorted_vars: Vec<_> = var_env.get_all_variables().iter().collect();
            sorted_vars.sort_by_key(|(name, _)| name.as_str());
            
            for (var_name, (var, _)) in sorted_vars {
                // Add a block parameter for this variable
                builder.append_block_param(loop_block, value_type());
                loop_vars.push((var_name.clone(), *var));
            }
            
            // Get initial values for all variables using the same order as loop_vars
            let mut initial_values = Vec::new();
            for (_, var) in &loop_vars {
                initial_values.push(builder.use_var(*var));
            }
            builder.ins().jump(loop_block, &initial_values);
            
            // Build the loop block
            builder.switch_to_block(loop_block);
            
            // Set up variable mappings from block parameters
            let block_params: Vec<_> = builder.block_params(loop_block).to_vec();
            eprintln!("DEBUG: Setting up {} loop variables from block params", loop_vars.len());
            for (i, (var_name, var)) in loop_vars.iter().enumerate() {
                eprintln!("DEBUG: Defining loop variable '{}' (var{:?}) from block param {}", var_name, var, i);
                builder.def_var(*var, block_params[i]);
            }
            
            // Push the control blocks onto the stacks
            LOOP_EXIT_BLOCKS.with(|blocks| {
                blocks.borrow_mut().push(exit_block);
            });
            LOOP_CONTINUE_BLOCKS.with(|blocks| {
                blocks.borrow_mut().push(loop_block);
            });
            LOOP_VARIABLE_INFO.with(|vars| {
                vars.borrow_mut().push(loop_vars.clone());
            });
            
            // Translate the loop body
            let _body_result = translate_expr(body, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Pop the control blocks from the stacks
            LOOP_EXIT_BLOCKS.with(|blocks| {
                blocks.borrow_mut().pop();
            });
            LOOP_CONTINUE_BLOCKS.with(|blocks| {
                blocks.borrow_mut().pop();
            });
            LOOP_VARIABLE_INFO.with(|vars| {
                vars.borrow_mut().pop();
            });
            
            // Create the back edge with updated variable values
            if builder.current_block().is_some() {
                let mut updated_values = Vec::new();
                eprintln!("DEBUG: Loop back edge - gathering {} variables", loop_vars.len());
                for (var_name, var) in &loop_vars {
                    let val = builder.use_var(*var);
                    eprintln!("DEBUG: Loop variable '{}' has value at back edge", var_name);
                    updated_values.push(val);
                }
                builder.ins().jump(loop_block, &updated_values);
            }
            
            // Now seal the loop block after all predecessors have been added
            builder.seal_block(loop_block);
            
            // Switch to the exit block
            builder.switch_to_block(exit_block);
            builder.seal_block(exit_block);
            
            // The result is the block parameter (break value)
            Some(builder.block_params(exit_block)[0])
        }
        Expr::Break { value } => {
            // Get the current loop's exit block first
            let exit_block = LOOP_EXIT_BLOCKS.with(|blocks| {
                blocks.borrow().last().copied()
            });
            
            if let Some(exit_block) = exit_block {
                // Evaluate the break value (default to 0 if none)
                let break_val = if let Some(val) = value {
                    translate_expr(val, builder, var_env, func_env, module_registry, module, symbol_table)
                } else {
                    // Default to void value
                    builder.ins().iconst(types::I64, 0)
                };
                
                // Jump to the exit block with the break value
                builder.ins().jump(exit_block, &[break_val]);
                
                // After a jump, the current block is terminated.
                // We need to create a new unreachable block for cranelift's sake
                let unreachable_block = builder.create_block();
                builder.switch_to_block(unreachable_block);
                // Seal the unreachable block immediately - it has no predecessors
                builder.seal_block(unreachable_block);
                
                // Return a dummy value that will never be used
                Some(builder.ins().iconst(types::I64, 0))
            } else {
                // Break outside of loop - this is an error
                // In a proper implementation, this would be caught during parsing/validation
                Some(builder.ins().iconst(types::I64, 0))
            }
        }
        Expr::Continue => {
            // Get the current loop's continue block and variable info
            let continue_block = LOOP_CONTINUE_BLOCKS.with(|blocks| {
                blocks.borrow().last().copied()
            });
            let loop_vars = LOOP_VARIABLE_INFO.with(|vars| {
                vars.borrow().last().cloned()
            });
            
            if let (Some(continue_block), Some(loop_vars)) = (continue_block, loop_vars) {
                // Gather current values for all loop variables
                let mut updated_values = Vec::new();
                for (_, var) in &loop_vars {
                    updated_values.push(builder.use_var(*var));
                }
                
                // Jump to the loop block with updated values
                builder.ins().jump(continue_block, &updated_values);
                
                // After a jump, the current block is terminated.
                // We need to create a new unreachable block for cranelift's sake
                let unreachable_block = builder.create_block();
                builder.switch_to_block(unreachable_block);
                // Seal the unreachable block immediately - it has no predecessors
                builder.seal_block(unreachable_block);
                
                // Return a dummy value that will never be used
                Some(builder.ins().iconst(types::I64, 0))
            } else {
                // Continue outside of loop - this is an error
                // In a proper implementation, this would be caught during parsing/validation
                Some(builder.ins().iconst(types::I64, 0))
            }
        }
        _ => None, // Not a control flow expression
    }
}