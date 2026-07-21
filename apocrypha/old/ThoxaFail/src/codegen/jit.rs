use crate::ast::{Expr, TypedExpr, TypeMember, Parameter};
use crate::codegen::{translate_expr, translate_typed_expr, VarEnv, FuncEnv, ModuleRegistry, value_type};
use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::collections::HashMap;
use std::mem;

// Helper function that can be called from JIT code
extern "C" fn print_i64(value: i64) -> i64 {
    println!("{}", value);
    0 // Return 0 for void
}

// Helper function to print string pointer
extern "C" fn print_string(ptr: i64) -> i64 {
    if ptr == 0 {
        println!("<null>");
        return 0;
    }
    
    // Our string structure:
    // [0..8]   length (u64)
    // [8..8+N] data (bytes)
    // [8+N]    null terminator
    
    unsafe {
        // Read the length first
        let length_ptr = ptr as *const u64;
        let length = *length_ptr;
        
        // Get pointer to the data (skip the 8-byte length field)
        let data_ptr = (ptr + 8) as *const u8;
        
        // Create a slice from the data
        let slice = std::slice::from_raw_parts(data_ptr, length as usize);
        
        // Convert to string and print
        if let Ok(str_slice) = std::str::from_utf8(slice) {
            println!("{}", str_slice);
        } else {
            println!("<invalid utf-8>");
        }
    }
    0 // Return 0 for void
}

// Helper function to perform syscalls
// On macOS ARM64, syscall numbers are different from Linux
extern "C" fn do_syscall(num: i64, arg0: i64, arg1: i64, arg2: i64, _arg3: i64, _arg4: i64, _arg5: i64) -> i64 {
    // macOS syscall numbers (from sys/syscall.h)
    const SYS_EXIT: i64 = 1;
    const SYS_WRITE: i64 = 4;
    
    match num {
        SYS_EXIT => {
            std::process::exit(arg0 as i32);
        }
        SYS_WRITE => {
            // fd, buffer, count
            let fd = arg0;
            let buffer = arg1 as *const u8;
            let count = arg2 as usize;
            
            if buffer.is_null() {
                return -1;
            }
            
            unsafe {
                let slice = std::slice::from_raw_parts(buffer, count);
                match fd {
                    1 => { // stdout
                        std::io::Write::write_all(&mut std::io::stdout(), slice).ok();
                    }
                    2 => { // stderr
                        std::io::Write::write_all(&mut std::io::stderr(), slice).ok();
                    }
                    _ => {
                        // Other file descriptors not supported yet
                        return -1;
                    }
                }
            }
            
            count as i64
        }
        _ => {
            // Unsupported syscall
            eprintln!("Unsupported syscall: {}", num);
            -1
        }
    }
}

// Helper function to calculate string length
extern "C" fn strlen(ptr: i64) -> i64 {
    if ptr == 0 {
        return 0;
    }
    
    unsafe {
        let mut len = 0;
        let mut p = ptr as *const u8;
        while *p != 0 {
            len += 1;
            p = p.add(1);
        }
        len
    }
}

// Helper function to convert integer to string
// Returns a pointer to a static buffer (not thread-safe, but simple for now)
extern "C" fn int_to_string(value: i64) -> i64 {
    // Use a thread-local buffer to avoid allocation
    thread_local! {
        static BUFFER: std::cell::RefCell<[u8; 32]> = std::cell::RefCell::new([0; 32]);
    }
    
    BUFFER.with(|buf| {
        let mut buffer = buf.borrow_mut();
        
        // Convert integer to string manually
        let is_negative = value < 0;
        let mut num = if is_negative { -value } else { value } as u64;
        let mut idx = 0;
        
        // Handle special case of 0
        if num == 0 {
            buffer[0] = b'0';
            buffer[1] = 0;
            return buffer.as_ptr() as i64;
        }
        
        // Convert digits (reversed)
        let mut digits = [0u8; 20]; // max digits for i64
        let mut digit_count = 0;
        while num > 0 {
            digits[digit_count] = (num % 10) as u8 + b'0';
            num /= 10;
            digit_count += 1;
        }
        
        // Add negative sign if needed
        if is_negative {
            buffer[idx] = b'-';
            idx += 1;
        }
        
        // Copy digits in correct order
        for i in (0..digit_count).rev() {
            buffer[idx] = digits[i];
            idx += 1;
        }
        
        // Null terminate
        buffer[idx] = 0;
        
        // Return pointer to the buffer
        buffer.as_ptr() as i64
    })
}

/// Information about a compiled function
#[derive(Debug, Clone)]
struct FunctionInfo {
    /// The cranelift function ID
    func_id: FuncId,
    /// The function signature
    signature: Signature,
    /// Whether this function takes &self as first parameter
    _has_self_param: bool,
    /// Number of parameters (including self if present)
    _param_count: usize,
    /// Whether this function has a variadic parameter
    is_variadic: bool,
    /// For variadic functions, the number of fixed parameters
    fixed_param_count: usize,
}

/// Registry for tracking compiled functions
#[derive(Debug)]
struct FunctionRegistry {
    /// Maps module_name.function_name to function info
    functions: HashMap<String, FunctionInfo>,
}

pub struct JITCodegen {
    ctx: codegen::Context,
    module: JITModule,
    function_registry: FunctionRegistry,
    /// Stores raw function pointers for direct calling
    function_pointers: HashMap<String, *const u8>,
}

impl JITCodegen {
    pub fn new() -> Self {
        // Create a mutable settings builder to configure compilation flags
        let mut flag_builder = settings::builder();

        // Disable colocated libcalls - keeps library function calls separate from generated code
        flag_builder.set("use_colocated_libcalls", "false").unwrap();

        // Disable position-independent code generation for simpler, faster code
        flag_builder.set("is_pic", "false").unwrap();

        // Get a builder for the host machine's instruction set architecture (ISA)
        // Panics if the current CPU architecture isn't supported by Cranelift
        let isa_builder = cranelift_native::builder().unwrap_or_else(|msg| {
            panic!("host machine is not supported: {}", msg);
        });

        // Build the ISA with our custom flags, creating the target architecture configuration
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .unwrap();

        // Create a JIT builder with the ISA and default library call names mapping
        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        
        // Register our print functions as symbols
        builder.symbol("print_i64", print_i64 as *const u8);
        builder.symbol("print_string", print_string as *const u8);
        builder.symbol("do_syscall", do_syscall as *const u8);
        builder.symbol("strlen", strlen as *const u8);
        builder.symbol("int_to_string", int_to_string as *const u8);

        // Initialize the JIT module that will manage compiled functions
        let module = JITModule::new(builder);

        // Return a new instance with initialized components
        Self {
            ctx: module.make_context(), // Context for current function being compiled
            module,                     // The JIT module for code generation
            function_registry: FunctionRegistry {
                functions: HashMap::new(),
            },
            function_pointers: HashMap::new(),
        }
    }

    /// Compile a module function
    #[allow(dead_code)]
    fn compile_module_function(
        &mut self,
        module_name: &str,
        func_name: &str,
        params: &[Parameter],
        _return_type: &Expr, // TODO: Use this when we have type checking
        body: &Expr,
        exported: bool,
    ) -> Result<FuncId, String> {
        // Clear the context for a new function
        self.ctx.clear();
        
        // Create a fresh FunctionBuilderContext for this function to avoid variable conflicts
        let mut func_builder_context = FunctionBuilderContext::new();
        
        // Check if first parameter is &self or self
        let has_self_param = params.first()
            .map(|p| p.name == "self")
            .unwrap_or(false);
        
        // Build the function signature
        let mut sig = self.module.make_signature();
        
        // Add parameters - use value_type() for tagged pointers
        for _param in params {
            sig.params.push(AbiParam::new(value_type()));
        }
        
        // Add return value - use value_type() for tagged pointers
        sig.returns.push(AbiParam::new(value_type()));
        
        // Create the function name (module.function)
        let full_name = format!("{}.{}", module_name, func_name);
        
        // Declare the function in the module
        let linkage = if exported { Linkage::Export } else { Linkage::Local };
        let func_id = self.module
            .declare_function(&full_name, linkage, &sig)
            .map_err(|e| format!("Failed to declare function: {}", e))?;
        
        // Check if function is variadic and count fixed parameters
        let mut is_variadic = false;
        let mut fixed_param_count = 0;
        for param in params {
            if param.variadic {
                is_variadic = true;
            } else {
                fixed_param_count += 1;
            }
        }
        
        // Store function info
        self.function_registry.functions.insert(
            full_name.clone(),
            FunctionInfo {
                func_id,
                signature: sig.clone(),
                _has_self_param: has_self_param,
                _param_count: params.len(),
                is_variadic,
                fixed_param_count,
            },
        );
        
        // Set up the function context
        self.ctx.func.signature = sig;
        
        // Build the function body
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut func_builder_context);
            let entry_block = builder.create_block();
            
            // Add block parameters for function parameters
            builder.append_block_params_for_function_params(entry_block);
            
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);
            
            // Create variable environment and bind parameters
            let mut var_env = VarEnv::new();
            
            // Get block params after all setup is done
            let block_params: Vec<Value> = builder.block_params(entry_block).to_vec();
            
            for (i, param) in params.iter().enumerate() {
                let var = Variable::new(i);
                builder.declare_var(var, value_type());
                builder.def_var(var, block_params[i]);
                var_env.declare_variable(param.name.clone(), var, false); // Function parameters are immutable
            }
            
            // Set next_var to start after parameters to avoid conflicts
            var_env.set_next_var(params.len());
            
            // Create function environment with all previously compiled functions
            let mut func_env = FuncEnv::new();
            for (name, info) in &self.function_registry.functions {
                func_env.declare_variadic_function(name.clone(), info.func_id, info.is_variadic, info.fixed_param_count);
            }
            // Set the current module context for this function
            func_env.set_current_module(Some(module_name.to_string()));
            
            // Create module registry (empty for now - will be passed in later)
            let mut module_registry = ModuleRegistry::new();
            
            // Translate the function body
            // TODO: Pass the actual symbol table once functions support imports
            let empty_symbol_table = crate::symbols::SymbolTable::new();
            let result = translate_expr(body, &mut builder, &mut var_env, &func_env, &mut module_registry, &mut self.module, &empty_symbol_table);
            
            // Return the result
            builder.ins().return_(&[result]);
            builder.finalize();
        }
        
        // Define the function
        self.module.define_function(func_id, &mut self.ctx)
            .map_err(|e| format!("Failed to define function: {}", e))?;
        self.module.clear_context(&mut self.ctx);
        
        Ok(func_id)
    }
    
    /// Compile a module function body with full context (all functions already declared)
    fn compile_module_function_body(
        &mut self,
        module_name: &str,
        func_name: &str,
        params: &[Parameter],
        _return_type: &Expr,
        body: &Expr,
        _exported: bool,
        symbol_table: &crate::symbols::SymbolTable,
    ) -> Result<(), String> {
        let full_name = format!("{}.{}", module_name, func_name);
        let func_info = self.function_registry.functions.get(&full_name)
            .ok_or_else(|| format!("Function {} not found in registry", full_name))?;
        let func_id = func_info.func_id;
        let sig = func_info.signature.clone();
        
        // Clear context for new function
        self.ctx.clear();
        self.ctx.func.signature = sig;
        
        // Create a fresh function builder context
        let mut func_builder_context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut func_builder_context);
        
        // Create entry block
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        
        // Set up variable environment
        let mut var_env = VarEnv::new();
        let block_params: Vec<Value> = builder.block_params(entry_block).to_vec();
        
        for (i, param) in params.iter().enumerate() {
            let var = Variable::new(i);
            builder.declare_var(var, value_type());
            builder.def_var(var, block_params[i]);
            var_env.declare_variable(param.name.clone(), var, false);
        }
        
        var_env.set_next_var(params.len());
        
        // Create function environment with ALL declared functions
        let mut func_env = FuncEnv::new();
        for (name, info) in &self.function_registry.functions {
            func_env.declare_variadic_function(name.clone(), info.func_id, info.is_variadic, info.fixed_param_count);
        }
        // Set current module for proper name resolution
        func_env.set_current_module(Some(module_name.to_string()));
        
        // Create module registry
        let mut module_registry = ModuleRegistry::new();
        // TODO: Populate from symbol table if needed
        
        // Translate the function body with full context
        let result = translate_expr(
            body,
            &mut builder,
            &mut var_env,
            &func_env,
            &mut module_registry,
            &mut self.module,
            symbol_table,  // Pass the actual symbol table!
        );
        
        builder.ins().return_(&[result]);
        builder.finalize();
        
        // Verify the function before defining
        if let Err(errors) = self.ctx.verify(self.module.isa()) {
            eprintln!("Verification errors for function {}:", full_name);
            eprintln!("{}", errors);
            eprintln!("Function text:");
            eprintln!("{}", self.ctx.func.display());
            eprintln!("Function signature: {:?}", self.ctx.func.signature);
        }
        
        // Define the function
        self.module.define_function(func_id, &mut self.ctx)
            .map_err(|e| format!("Failed to define function {}: {}", full_name, e))?;
        self.module.clear_context(&mut self.ctx);
        
        Ok(())
    }

    /// Extract and compile all module functions from the AST
    #[allow(dead_code)]
    fn compile_all_module_functions(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::TypeDef { name, members } => {
                // Compile each function in this module
                for member in members {
                    if let TypeMember::FunctionDef { 
                        name: func_name, 
                        params, 
                        return_type, 
                        body, 
                        exported 
                    } = member {
                        self.compile_module_function(
                            name,
                            func_name,
                            params,
                            return_type,
                            body,
                            *exported,
                        )?;
                    }
                }
                Ok(())
            }
            Expr::Sequence(exprs) => {
                // Recursively process each expression in the sequence
                for expr in exprs {
                    self.compile_all_module_functions(expr)?;
                }
                Ok(())
            }
            _ => Ok(()), // Other expressions don't contain module definitions
        }
    }

    /// Get the compiled function pointer for a function
    pub fn get_function_ptr(&self, func_name: &str) -> Option<*const u8> {
        if let Some(info) = self.function_registry.functions.get(func_name) {
            Some(self.module.get_finalized_function(info.func_id))
        } else {
            None
        }
    }
    
    /// Compile functions from all modules in the symbol table
    fn compile_functions_from_symbol_table(&mut self, symbol_table: &crate::symbols::SymbolTable) -> Result<(), String> {
        use crate::symbols::TypeMemberInfo;
        
        // Phase 1: Declare ALL functions first so they can reference each other
        for (module_name, module_type) in &symbol_table.type_definitions {
            for (member_name, member_info) in &module_type.members {
                if let TypeMemberInfo::FunctionDef { params, exported, .. } = member_info {
                    // Create function signature
                    let mut sig = self.module.make_signature();
                    for _param in params {
                        // All parameters use tagged pointers (i64)
                        // Variadic parameters are passed as array pointers
                        sig.params.push(AbiParam::new(value_type()));
                    }
                    sig.returns.push(AbiParam::new(value_type()));
                    
                    // Create the function name (module.function)
                    let full_name = format!("{}.{}", module_name, member_name);
                    
                    // Declare the function
                    let linkage = if *exported { Linkage::Export } else { Linkage::Local };
                    let func_id = self.module
                        .declare_function(&full_name, linkage, &sig)
                        .map_err(|e| format!("Failed to declare function {}: {}", full_name, e))?;
                    
                    // Check if function is variadic and count fixed parameters
                    let mut is_variadic = false;
                    let mut fixed_param_count = 0;
                    for param in params {
                        if param.variadic {
                            is_variadic = true;
                        } else {
                            fixed_param_count += 1;
                        }
                    }
                    
                    // Store in function registry
                    self.function_registry.functions.insert(
                        full_name.clone(),
                        FunctionInfo {
                            func_id,
                            signature: sig,
                            _has_self_param: false,
                            _param_count: params.len(),
                            is_variadic,
                            fixed_param_count,
                        },
                    );
                }
            }
        }
        
        // Phase 2: Now compile all function bodies with full context
        for (module_name, module_type) in &symbol_table.type_definitions {
            for (member_name, member_info) in &module_type.members {
                if let TypeMemberInfo::FunctionDef { params, return_type, body, exported } = member_info {
                    // We already have the function declared, now compile its body
                    self.compile_module_function_body(
                        module_name,
                        member_name,
                        params,
                        return_type,
                        body,
                        *exported,
                        symbol_table,
                    )?;
                }
            }
        }
        Ok(())
    }
    
    pub fn compile_expr(&mut self, typed_expr: &TypedExpr, symbol_table: &crate::symbols::SymbolTable) -> i64 {
        // Compile all module functions from the symbol table
        // This handles all functions that have been properly registered during parsing
        if let Err(e) = self.compile_functions_from_symbol_table(symbol_table) {
            eprintln!("Error compiling module functions from symbol table: {}", e);
            return -1;
        }
        
        // Note: We don't need to call compile_all_module_functions here anymore
        // because all functions should already be in the symbol table
        
        // Finalize function definitions so they can be called
        self.module.finalize_definitions().unwrap();
        
        // Store function pointers for all compiled functions
        for (name, info) in &self.function_registry.functions {
            let ptr = self.module.get_finalized_function(info.func_id);
            self.function_pointers.insert(name.clone(), ptr);
        }
        // Add a return parameter to the function signature
        self.ctx
            .func
            .signature
            .returns
            .push(AbiParam::new(value_type()));

        // Create a fresh function builder context for the main function
        let mut main_builder_context = FunctionBuilderContext::new();
        
        // Create a function builder to construct the IR (intermediate representation) code
        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut main_builder_context);

        // Create the entry basic block where execution will start
        let entry_block = builder.create_block();

        // Set up parameters for the entry block based on function signature (none in this case)
        builder.append_block_params_for_function_params(entry_block);

        // Switch code generation to target the entry block
        builder.switch_to_block(entry_block);

        // Mark the entry block as sealed (no more predecessors will be added)
        builder.seal_block(entry_block);

        // Create variable environment for let bindings
        let mut var_env = VarEnv::new();
        
        // Create function environment with all compiled functions
        let mut func_env = FuncEnv::new();
        
        // Declare builtin functions
        // print_i64
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module
                .declare_function("print_i64", Linkage::Import, &sig)
                .unwrap();
            func_env.declare_function("print_i64".to_string(), func_id);
        }
        
        // print_string (mapped to print_str in TypedExpr)
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module
                .declare_function("print_string", Linkage::Import, &sig)
                .unwrap();
            func_env.declare_function("print_string".to_string(), func_id);
        }
        
        // do_syscall (mapped to syscall in TypedExpr)
        {
            let mut sig = self.module.make_signature();
            // 7 parameters: syscall number + 6 arguments
            for _ in 0..7 {
                sig.params.push(AbiParam::new(types::I64));
            }
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module
                .declare_function("do_syscall", Linkage::Import, &sig)
                .unwrap();
            func_env.declare_function("syscall".to_string(), func_id);
        }
        
        // strlen
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module
                .declare_function("strlen", Linkage::Import, &sig)
                .unwrap();
            func_env.declare_function("strlen".to_string(), func_id);
        }
        
        // int_to_string
        {
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self.module
                .declare_function("int_to_string", Linkage::Import, &sig)
                .unwrap();
            func_env.declare_function("int_to_string".to_string(), func_id);
        }
        
        for (name, info) in &self.function_registry.functions {
            func_env.declare_variadic_function(name.clone(), info.func_id, info.is_variadic, info.fixed_param_count);
        }
        
        // Create module registry for module types
        let mut module_registry = ModuleRegistry::new();
        
        // Register all modules from the symbol table into the module registry
        for (module_name, module_type) in &symbol_table.type_definitions {
            let mut module_info = crate::codegen::ModuleInfo::new(module_name.clone());
            let mut offset = 0;
            
            for (field_name, member_info) in &module_type.members {
                if let crate::symbols::TypeMemberInfo::LetBinding { type_expr: _, value, exported } = member_info {
                    if *exported {
                        module_info.fields.push((field_name.clone(), *exported, value.clone()));
                        module_info.field_offsets.insert(field_name.clone(), offset);
                        offset += 1;
                    }
                }
            }
            
            module_info.size = offset * 8; // Each field is 8 bytes
            if !module_info.fields.is_empty() {
                module_registry.register_module(module_name.clone(), module_info);
            }
        }
        
        // Instantiate all imported modules
        use crate::import_instantiation::instantiate_imports;
        if let Err(e) = instantiate_imports(&mut builder, &mut var_env, &func_env, &mut module_registry, &mut self.module, symbol_table) {
            eprintln!("Error instantiating imports: {}", e);
            let error_val = builder.ins().iconst(types::I64, -1);
            builder.ins().return_(&[error_val]);
            builder.finalize();
            let id = self.module.declare_function("expr", Linkage::Export, &self.ctx.func.signature).unwrap();
            self.module.define_function(id, &mut self.ctx).unwrap();
            self.module.clear_context(&mut self.ctx);
            self.module.finalize_definitions().unwrap();
            let code = self.module.get_finalized_function(id);
            // All values are 64-bit tagged pointers
            let code_fn = unsafe { std::mem::transmute::<_, fn() -> i64>(code) };
            return code_fn();
        }

        // Recursively translate the expression AST into Cranelift IR values
        let result = translate_typed_expr(typed_expr, &mut builder, &mut var_env, &func_env, &mut module_registry, &mut self.module, symbol_table);

        // Generate a return instruction that returns the computed result value
        builder.ins().return_(&[result]);

        // Complete the function building process and validate the IR
        builder.finalize();

        // Declare the function in the module with name "expr", export linkage, and the signature
        let id = self
            .module
            .declare_function("expr", Linkage::Export, &self.ctx.func.signature)
            .unwrap();

        // Define the function implementation using the IR we just built
        self.module.define_function(id, &mut self.ctx).unwrap();

        // Clear the context to prepare for potential future compilations
        self.module.clear_context(&mut self.ctx);

        // Finalize all function definitions and prepare for execution
        self.module.finalize_definitions().unwrap();

        // Get a pointer to the compiled machine code for our function
        let code = self.module.get_finalized_function(id);

        // All values are 64-bit tagged pointers
        let code_fn = unsafe { mem::transmute::<_, fn() -> i64>(code) };
        code_fn()
    }
}