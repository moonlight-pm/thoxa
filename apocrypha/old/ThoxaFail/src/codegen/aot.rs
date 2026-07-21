use crate::ast::TypedExpr;
use crate::codegen::{translate_typed_expr, VarEnv, FuncEnv, ModuleRegistry};
use cranelift::prelude::*;
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::fs::File;
use std::io::Write;

pub fn compile_to_binary(typed_expr: &TypedExpr, output_path: &str, symbol_table: &crate::symbols::SymbolTable) -> Result<(), String> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "false").unwrap();
    // Enable debug output
    flag_builder.set("enable_verifier", "true").unwrap();
    flag_builder.set("opt_level", "none").unwrap();
    
    let isa_builder = cranelift_native::builder().unwrap_or_else(|msg| {
        panic!("host machine is not supported: {}", msg);
    });
    
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
        
    let builder = ObjectBuilder::new(
        isa,
        "thoxa_output",
        cranelift_module::default_libcall_names(),
    ).unwrap();
    
    let mut module = ObjectModule::new(builder);
    let mut ctx = module.make_context();
    let mut builder_context = FunctionBuilderContext::new();
    
    // Create main function signature
    ctx.func.signature.params.clear();
    ctx.func.signature.returns.clear();
    ctx.func.signature.returns.push(AbiParam::new(types::I32));
    
    let mut func_builder = FunctionBuilder::new(&mut ctx.func, &mut builder_context);
    let entry_block = func_builder.create_block();
    
    func_builder.append_block_params_for_function_params(entry_block);
    func_builder.switch_to_block(entry_block);
    func_builder.seal_block(entry_block);
    
    // Create variable environment for let bindings
    let mut var_env = VarEnv::new();
    
    // Create function environment (empty for AOT for now)
    let func_env = FuncEnv::new();
    
    // Create module registry for module types
    let mut module_registry = ModuleRegistry::new();
    
    // Translate the expression
    let result = translate_typed_expr(typed_expr, &mut func_builder, &mut var_env, &func_env, &mut module_registry, &mut module, symbol_table);
    
    // Convert the i64 result to i32 for the exit code (truncate to fit)
    let exit_code = func_builder.ins().ireduce(types::I32, result);
    func_builder.ins().return_(&[exit_code]);
    
    func_builder.finalize();
    
    // Print the Cranelift IR for debugging
    println!("=== CRANELIFT IR ===");
    println!("{}", ctx.func);
    println!("==================");
    
    // Define the main function
    let id = module
        .declare_function("main", Linkage::Export, &ctx.func.signature)
        .map_err(|e| format!("Failed to declare function: {}", e))?;
        
    module.define_function(id, &mut ctx)
        .map_err(|e| format!("Failed to define function: {}", e))?;
        
    module.clear_context(&mut ctx);
    
    // Finish the module and get the object bytes
    let product = module.finish();
    let bytes = product.emit()
        .map_err(|e| format!("Failed to emit object code: {}", e))?;
        
    // Write object file
    let object_path = format!("{}.o", output_path);
    let mut file = File::create(&object_path)
        .map_err(|e| format!("Failed to create object file: {}", e))?;
    file.write_all(&bytes)
        .map_err(|e| format!("Failed to write object file: {}", e))?;
        
    // Link the object file to create an executable
    link_object_file(&object_path, output_path)?;
    
    // Clean up object file
    std::fs::remove_file(&object_path).ok();
    
    Ok(())
}

fn link_object_file(object_path: &str, output_path: &str) -> Result<(), String> {
    use std::process::Command;
    
    #[cfg(target_os = "macos")]
    {
        Command::new("cc")
            .args(&[
                object_path,
                "-o", output_path,
                "-Wl,-e,_main", // Entry point
                "-nostartfiles", // Don't link standard startup files
                "-lc", // Link with libc for printf
            ])
            .status()
            .map_err(|e| format!("Failed to run linker: {}", e))?
            .success()
            .then_some(())
            .ok_or_else(|| "Linking failed".to_string())
    }
    
    #[cfg(target_os = "linux")]
    {
        Command::new("ld")
            .args(&[
                object_path,
                "-o", output_path,
                "-e", "main",
            ])
            .status()
            .map_err(|e| format!("Failed to run linker: {}", e))?
            .success()
            .then_some(())
            .ok_or_else(|| "Linking failed".to_string())
    }
    
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err("Unsupported platform for linking".to_string())
    }
}