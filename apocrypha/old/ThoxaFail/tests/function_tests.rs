use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;
use thoxa::type_checker::type_check;

#[test]
fn test_module_function_definition() {
    let input = "type Math { export fn add(a: I64, b: I64) -> I64 { a + b } }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Module definition returns a hash of the module name
    assert!(result != 0);
}

#[test]
fn test_module_function_call_placeholder() {
    let input = r#"
        type Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b } 
        };
        let m: Math;
        m.add(5, 7)
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Function should return 5 + 7 = 12
    assert_eq!(result, 12);
}

#[test]
fn test_method_style_call() {
    let input = r#"
        type Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b } 
        };
        let m: Math;
        m.add(10, 20)
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Method should return 10 + 20 = 30
    assert_eq!(result, 30);
}

#[test]
fn test_nested_module_function_calls() {
    let input = r#"
        type Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b } 
        };
        let m: Math;
        m.add(m.add(1, 2), 3)
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Nested calls: m.add(m.add(1, 2), 3) = m.add(3, 3) = 6
    assert_eq!(result, 6);
}

#[test]
fn test_module_function_call_with_expression_args() {
    let input = r#"
        type Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b } 
        };
        let m: Math;
        m.add(2 * 3, 4 + 5)
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // m.add(2 * 3, 4 + 5) = m.add(6, 9) = 15
    assert_eq!(result, 15);
}