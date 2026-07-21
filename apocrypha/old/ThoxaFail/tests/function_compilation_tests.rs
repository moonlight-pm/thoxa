use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;

#[test]
fn test_module_function_compiles() {
    let input = r#"
        module Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b } 
        }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&ast, &symbol_table);

    // Module definition returns hash - verify it's non-zero
    assert!(result != 0);
}

#[test]
fn test_module_with_multiple_functions() {
    let input = r#"
        module Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b };
            export fn sub(a: I64, b: I64) -> I64 { a - b };
            fn mul(a: I64, b: I64) -> I64 { a * b };
        }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&ast, &symbol_table);

    // Module definition returns hash
    assert!(result != 0);
}

#[test]
fn test_module_function_with_self_param() {
    let input = r#"
        module Counter { 
            export let count: I64 = 0;
            export fn increment(self: &Counter) -> I64 { self.count + 1 }
        }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&ast, &symbol_table);

    // Module definition returns hash
    assert!(result != 0);
}

#[test]
fn test_function_body_uses_parameters() {
    let input = r#"
        module Calculator {
            export fn calculate(x: I64, y: I64, z: I64) -> I64 { 
                x + y * z - x
            }
        }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&ast, &symbol_table);

    // Module definition returns hash
    assert!(result != 0);
}