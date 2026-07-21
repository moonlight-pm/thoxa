use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;
use thoxa::type_checker::type_check;

#[test]
fn test_if_true_branch() {
    let input = "if 1 { 42 } else { 0 }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 42);
}

#[test]
fn test_if_false_branch() {
    let input = "if 0 { 42 } else { 99 }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 99);
}

#[test]
fn test_if_without_else() {
    let input = "if 1 { 42 }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // If without else should return 42 when true
    assert_eq!(result, 42);
}

#[test]
fn test_if_without_else_false() {
    let input = "if 0 { 42 }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // If without else should return void (0) when false
    assert_eq!(result, 0);
}

#[test]
fn test_if_with_comparison() {
    let input = "if 5 > 3 { 100 } else { 200 }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 100);
}

#[test]
fn test_nested_if() {
    let input = "if 1 { if 0 { 10 } else { 20 } } else { 30 }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Should take first branch, then inner else branch
    assert_eq!(result, 20);
}

#[test]
fn test_loop_with_break() {
    let input = "{ mut counter: I64 = 0; loop { counter = counter + 1; if counter > 5 { break; } }; counter }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 6);
}

#[test]
fn test_loop_with_continue() {
    let input = "{ mut sum: I64 = 0; mut i: I64 = 0; loop { i = i + 1; if i > 10 { break; }; if i == 5 { continue; }; sum = sum + i; }; sum }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Sum of 1-10 excluding 5 = 55 - 5 = 50
    assert_eq!(result, 50);
}

#[test]
fn test_nested_loops() {
    let input = "{ mut outer: I64 = 0; mut result: I64 = 0; loop { outer = outer + 1; if outer > 2 { break; }; mut inner: I64 = 0; loop { inner = inner + 1; if inner > 3 { break; }; result = result + 1; } }; result }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // 2 outer iterations * 3 inner iterations = 6
    assert_eq!(result, 6);
}