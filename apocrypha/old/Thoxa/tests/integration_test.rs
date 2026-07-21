//! Integration tests for the full compilation pipeline

use thoxa::parser::Parser;
use thoxa::codegen::CodeGen;

#[test]
fn test_minimal_program() {
    let source = r#"
fn main() -> Int {
    let x = 10;
    let y = x + 5;
    y
}
"#;
    
    // Parse
    let mut parser = Parser::new(source).expect("Failed to create parser");
    let module = parser.parse_module().expect("Failed to parse");
    
    // Generate code
    let codegen = CodeGen::new();
    let qbe_output = codegen.generate(&module).expect("Failed to generate code");
    
    println!("Generated QBE:");
    println!("{}", qbe_output);
    
    // Verify the output
    assert!(qbe_output.contains("export function l $main()"));
    assert!(qbe_output.contains("copy 10"));
    assert!(qbe_output.contains("copy 5"));
    assert!(qbe_output.contains("add"));
    assert!(qbe_output.contains("ret"));
}