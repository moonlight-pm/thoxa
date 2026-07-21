use thoxa::codegen_jit::JITCodegen;
use thoxa::lexer::lex;
use thoxa::parser::parse_with_symbols;
use thoxa::type_checker::type_check;

#[test]
fn test_module_definition() {
    let input = "type Point { export let x: I64 = 10; let y: I64 = 20; }";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Module definition should return a hash/ID of the module name
    assert!(result != 0);
}

#[test]
#[ignore = "Module instantiation not yet implemented with new TypedExpr"]
fn test_module_instantiation() {
    let input = "type Point { export let x: I64 = 10; export let y: I64 = 20; }; let p: Point; p";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Module instance should return a simulated address
    eprintln!("Module instance result: {}", result);
    assert!(result > 0x1000); // Should be greater than our base address
}

#[test]
fn test_field_access_x() {
    let input =
        "type Point { export let x: I64 = 10; export let y: I64 = 20; }; let p: Point; p.x";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 10);
}

#[test]
fn test_field_access_y() {
    let input =
        "type Point { export let x: I64 = 10; export let y: I64 = 20; }; let p: Point; p.y";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 20);
}

#[test]
fn test_field_arithmetic() {
    let input =
        "type Point { export let x: I64 = 10; export let y: I64 = 20; }; let p: Point; p.x + p.y";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 30);
}

#[test]
fn test_field_arithmetic_complex() {
    let input = "type Point { export let x: I64 = 10; export let y: I64 = 20; }; let p: Point; p.x * 2 + p.y";
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 40); // 10 * 2 + 20 = 40
}

#[test]
fn test_multiple_module_instances() {
    let input = r#"
        type Point { export let x: I64 = 10; export let y: I64 = 20; };
        let p1: Point;
        let p2: Point;
        p1.x + p2.y
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 30); // Both instances should have the same field values
}

#[test]
fn test_let_binding_with_module_field() {
    let input = r#"
        type Point { export let x: I64 = 10; export let y: I64 = 20; };
        let p: Point;
        let sum: I64 = p.x + p.y;
        sum
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    assert_eq!(result, 30);
}

#[test]
fn test_nested_field_operations() {
    let input = r#"
        type Point { export let x: I64 = 5; export let y: I64 = 3; };
        let p: Point;
        p.x * p.y + p.x - p.y
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Now using actual initialization values: x=5, y=3
    // So the calculation is 5 * 3 + 5 - 3 = 15 + 5 - 3 = 17
    assert_eq!(result, 17);
}

#[test]
fn test_module_with_mixed_exports() {
    let input = r#"
        type Data {
            export let publicval: I64 = 100;
            let privateval: I64 = 200;
        };
        let d: Data;
        d.publicval
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Should access the public field with value 100
    assert_eq!(result, 100);
}

#[test]
fn test_block_with_module_operations() {
    let input = r#"
        type Point { export let x: I64 = 8; export let y: I64 = 12; };
        {
            let p: Point;
            let result: I64 = p.x + p.y;
            result * 2
        }
    "#;
    let tokens = lex(input);
    let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();

    let typed_ast = type_check(&ast, &symbol_table).unwrap();
    let mut codegen = JITCodegen::new();
    let result = codegen.compile_expr(&typed_ast, &symbol_table);

    // Now using actual initialization values: x=8, y=12
    // So the calculation is (8 + 12) * 2 = 20 * 2 = 40
    assert_eq!(result, 40);
}
