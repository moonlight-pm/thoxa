use clap::Parser;
use std::fs;
use std::path::Path;
use thoxa::ast::Expr;
use thoxa::lexer::lex;
use thoxa::codegen::jit::JITCodegen;
use thoxa::codegen::aot::compile_to_binary;
use thoxa::modules::ModuleCompiler;
use thoxa::type_checker::type_check;

#[derive(Parser)]
#[command(name = "thoxa")]
#[command(version = "0.1.0")]
#[command(about = "The Thoxa programming language compiler", long_about = None)]
#[command(arg_required_else_help = true)]
struct Cli {
    /// Source file to run or compile (.thx)
    file: Option<String>,
    
    /// Evaluate an expression directly
    #[arg(short = 'e', long = "eval", conflicts_with = "file")]
    expression: Option<String>,
    
    /// Compile to binary instead of running in JIT mode
    #[arg(short = 'c', long = "compile")]
    compile: bool,
    
    /// Output file name (defaults to 'a.out' when compiling)
    #[arg(short = 'o', long = "output")]
    output: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    
    // Handle expression evaluation
    if let Some(expression) = cli.expression {
        if cli.compile {
            eprintln!("Error: Cannot compile an expression. Use -c with a .thx file instead.");
            std::process::exit(1);
        }
        
        // Evaluate the expression
        let tokens = lex(&expression);
        let (ast, symbol_table) = match thoxa::parser::parse_with_symbols(tokens) {
            Ok((ast, symbols)) => (ast, symbols),
            Err(errors) => {
                eprintln!("Parse error: {}", errors);
                std::process::exit(1);
            }
        };
        
        // Type check the AST
        let typed_ast = match type_check(&ast, &symbol_table) {
            Ok(typed) => typed,
            Err(e) => {
                eprintln!("Type error: {}", e);
                std::process::exit(1);
            }
        };
        
        let mut codegen = JITCodegen::new();
        let result = codegen.compile_expr(&typed_ast, &symbol_table);
        std::process::exit(result as i32);
    }
    
    // Handle file operations
    let Some(file) = cli.file else {
        // This shouldn't happen due to arg_required_else_help
        return;
    };
    
    // Check if file exists and has .thx extension
    let path = Path::new(&file);
    if !path.exists() {
        eprintln!("Error: File '{}' not found", file);
        std::process::exit(1);
    }
    
    if path.extension().and_then(|s| s.to_str()) != Some("thx") {
        eprintln!("Error: File must have .thx extension");
        std::process::exit(1);
    }
    
    // Check if we need to use the module compiler (if file has imports)
    // For now, we'll do a quick check by reading the file
    let source = match fs::read_to_string(&file) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            std::process::exit(1);
        }
    };
    
    // Quick check for imports
    let has_imports = source.contains("import ");
    
    let (ast, symbol_table) = if has_imports {
        use thoxa::import_resolver::ImportResolver;
        
        // Use module compiler for multi-file compilation
        let mut module_compiler = ModuleCompiler::new();
        let modules = match module_compiler.compile_module_tree(path) {
            Ok(modules) => modules,
            Err(e) => {
                eprintln!("Module compilation error: {}", e);
                std::process::exit(1);
            }
        };
        
        // Create a symbol table and populate it with all modules
        let mut symbol_table = thoxa::symbols::SymbolTable::new();
        let mut import_resolver = ImportResolver::new();
        
        // First pass: register all modules and their exports
        for module in &modules {
            // Extract the actual module definition from the AST
            let module_def = match &module.ast {
                Expr::TypeDef { .. } => &module.ast,
                Expr::Sequence(exprs) => {
                    // Find the module definition in the sequence
                    exprs.iter().find(|e| matches!(e, Expr::TypeDef { .. }))
                        .unwrap_or(&module.ast)
                }
                _ => &module.ast,
            };
            
            if let Expr::TypeDef { name, members } = module_def {
                symbol_table.register_type(name, members).unwrap_or_else(|e| {
                    eprintln!("Symbol table error: {}", e);
                    std::process::exit(1);
                });
            }
            
            // Register the module with the import resolver
            let path_str = module.path.to_str().unwrap_or("").to_string();
            import_resolver.register_type(path_str, module.clone());
        }
        
        // Second pass: resolve imports for each module
        for module in &modules {
            import_resolver.resolve_imports(module, &mut symbol_table).unwrap_or_else(|e| {
                eprintln!("Import resolution error: {}", e);
                std::process::exit(1);
            });
        }
        
        // For now, just use the last module (the entry point)
        // In a full implementation, we'd need to merge all modules
        if let Some(entry_module) = modules.last() {
            (entry_module.ast.clone(), symbol_table)
        } else {
            eprintln!("No modules found");
            std::process::exit(1);
        }
    } else {
        // Simple single-file compilation
        let tokens = lex(&source);
        match thoxa::parser::parse_with_symbols(tokens) {
            Ok((ast, symbols)) => (ast, symbols),
            Err(errors) => {
                eprintln!("Parse error: {}", errors);
                std::process::exit(1);
            }
        }
    };
    
    // Type check the AST before compilation
    let typed_ast = match type_check(&ast, &symbol_table) {
        Ok(typed) => typed,
        Err(e) => {
            eprintln!("Type error: {}", e);
            std::process::exit(1);
        }
    };
    
    if cli.compile {
        // Compile mode
        let output = cli.output.unwrap_or_else(|| "a.out".to_string());
        
        println!("Compiling {} -> {}", file, output);
        
        match compile_to_binary(&typed_ast, &output, &symbol_table) {
            Ok(()) => {
                // Make the output executable on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = fs::metadata(&output).unwrap().permissions();
                    perms.set_mode(0o755);
                    fs::set_permissions(&output, perms).unwrap();
                }
                
                println!("Successfully compiled to {}", output);
            }
            Err(e) => {
                eprintln!("Compilation error: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        // JIT mode
        let mut codegen = JITCodegen::new();
        let result = codegen.compile_expr(&typed_ast, &symbol_table);
        
        // The result is already the exit code from the expression
        std::process::exit(result as i32);
    }
}