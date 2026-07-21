use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;

#[test]
fn test_function_compilation_creates_pointers() {
    let input = r#"
        module Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b } 
        }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let _result = codegen.compile_expr(&ast, &symbol_table);

    // Check that function pointer exists
    let func_ptr = codegen.get_function_ptr("Math.add");
    assert!(func_ptr.is_some());
    assert!(!func_ptr.unwrap().is_null());
}

#[test]
fn test_multiple_functions_get_different_pointers() {
    let input = r#"
        module Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b };
            export fn sub(a: I64, b: I64) -> I64 { a - b };
        }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let _result = codegen.compile_expr(&ast, &symbol_table);

    // Check that both functions have different pointers
    let add_ptr = codegen.get_function_ptr("Math.add").unwrap();
    let sub_ptr = codegen.get_function_ptr("Math.sub").unwrap();
    
    assert_ne!(add_ptr, sub_ptr);
}