use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;
use thoxa::type_checker::type_check;

#[test]
#[ignore = "Type guards not yet implemented"]
fn test_type_guard_int() {
    let input = r#"
        let x: Any = 42;
        if x is I64 { 1 } else { 0 }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 1); // x is indeed I64
}

#[test]
#[ignore = "Type guards not yet implemented"]
fn test_type_guard_string() {
    let input = r#"
        let x: Any = "Hello";
        if x is String { 1 } else { 0 }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 1); // x is indeed String
}

#[test]
#[ignore = "Type guards not yet implemented"]
fn test_type_guard_wrong_type() {
    let input = r#"
        let x: Any = 42;
        if x is String { 1 } else { 0 }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 0); // x is not String
}