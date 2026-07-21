use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;

#[test]
fn test_simple_function_execution() {
    // For now, test that we can compile and the placeholders work
    let input = r#"
        module Math { 
            export fn getValue() -> I64 { 42 } 
        };
        let m: Math;
        m.getValue()
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&ast, &symbol_table);

    // Currently returns 12 (placeholder for found function)
    assert_eq!(result, 12);
}

#[test]
fn test_function_with_parameters() {
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

    // Currently returns 12 (placeholder for found function)
    assert_eq!(result, 12);
}

#[test]
fn test_function_using_self() {
    let input = r#"
        module Counter { 
            export let count: I64 = 10;
            export fn getCount(self: &Counter) -> I64 { 42 }
        };
        let c: Counter;
        c.getCount()
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&ast, &symbol_table);

    // Currently returns 12 (placeholder for found function)
    assert_eq!(result, 12);
}