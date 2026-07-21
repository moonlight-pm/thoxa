use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;

#[test]
fn test_direct_function_call_no_params() {
    let input = r#"
        module Math { 
            export fn getValue() -> I64 { 42 } 
        }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let _result = codegen.compile_expr(&ast, &symbol_table);

    // Get the function pointer and call it directly
    if let Some(func_ptr) = codegen.get_function_ptr("Math.getValue") {
        // Cast to a function that takes no params and returns i64
        let func = unsafe { std::mem::transmute::<_, fn() -> i64>(func_ptr) };
        let result = func();
        assert_eq!(result, 42);
    } else {
        panic!("Function not found");
    }
}

#[test]
fn test_direct_function_call_with_params() {
    let input = r#"
        module Math { 
            export fn add(a: I64, b: I64) -> I64 { a + b } 
        }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let _result = codegen.compile_expr(&ast, &symbol_table);

    // Get the function pointer and call it directly
    if let Some(func_ptr) = codegen.get_function_ptr("Math.add") {
        // This function takes 2 params and returns their sum
        let func = unsafe { std::mem::transmute::<_, fn(i64, i64) -> i64>(func_ptr) };
        let result = func(5, 7);
        assert_eq!(result, 12);
    } else {
        panic!("Function not found");
    }
}