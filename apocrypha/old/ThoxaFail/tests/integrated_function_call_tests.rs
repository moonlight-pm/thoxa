use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;

#[test] 
#[ignore] // Ignore until full implementation is ready
fn test_integrated_function_call() {
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

    // Should return 12 (the actual sum)
    assert_eq!(result, 12);
}

#[test]
#[ignore] // Ignore until full implementation is ready  
fn test_function_with_self_param() {
    let input = r#"
        module Counter { 
            export let value: I64 = 100;
            export fn getValue(self: &Counter) -> I64 { self.value }
        };
        let c: Counter;
        c.getValue()
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&ast, &symbol_table);

    // Should return 100 (the value field)
    assert_eq!(result, 100);
}