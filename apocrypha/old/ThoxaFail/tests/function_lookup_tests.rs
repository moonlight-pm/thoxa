use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;

#[test]
fn test_function_lookup_returns_12() {
    // Test that function calls return 12 when function is found
    let input = r#"
        module Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b } 
        };
        let m: Math;
        m.add(5, 7)
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&ast, &symbol_table);

    // Should return 12 indicating function was found
    assert_eq!(result, 12);
}

#[test]
fn test_missing_function_returns_0() {
    // Test that function calls return 0 when function is not found
    let input = r#"
        module Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b } 
        };
        let m: Math;
        m.subtract(5, 7)
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&ast, &symbol_table);

    // Should return 0 indicating function was not found
    assert_eq!(result, 0);
}

#[test]
fn test_module_not_found_returns_0() {
    // Test that function calls return 0 when module instance is not found
    let input = r#"
        module Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b } 
        };
        undefined.add(5, 7)
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&ast, &symbol_table);

    // Should return 0 indicating module instance was not found
    assert_eq!(result, 0);
}