use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;
use thoxa::type_checker::type_check;

#[test]
fn test_simple_number_with_fat_pointers() {
    let input = "42";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // With fat pointers disabled, should return 42
    assert_eq!(result, 42);
}

#[test]
fn test_arithmetic_with_fat_pointers() {
    let input = "10 + 32";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 42);
}

#[test]
fn test_variable_with_fat_pointers() {
    let input = "{ let x: I64 = 42; x }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 42);
}