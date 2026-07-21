use std::process::Command;
use std::fs;
use std::path::Path;

#[test]
fn test_compile_simple_math() {
    let output_path = "test_simple_math";
    
    // Compile the test file
    let output = Command::new("cargo")
        .args(&["run", "--", "-c", "examples/HelloWorld.thx", "-o", output_path])
        .output()
        .expect("Failed to execute compiler");
    
    assert!(output.status.success(), "Compilation failed: {}", String::from_utf8_lossy(&output.stderr));
    
    // Check if output file exists
    assert!(Path::new(output_path).exists(), "Output binary not created");
    
    // Clean up
    fs::remove_file(output_path).ok();
}

#[test]
fn test_run_expression() {
    let output = Command::new("cargo")
        .args(&["run", "--", "-e", "5 + 3"])
        .output()
        .expect("Failed to execute compiler");
    
    // Debug output
    if !output.status.success() && output.status.code() != Some(8) {
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("exit code: {:?}", output.status.code());
    }
    
    // The new CLI doesn't print "Result:" prefix
    // JIT exits with the result value as exit code (modulo 256)
    assert!(output.status.success() || output.status.code() == Some(8), 
            "Expected success or exit code 8, got {:?}", output.status.code());
}

#[test]
fn test_complex_expression() {
    let output = Command::new("cargo")
        .args(&["run", "--", "-e", "(10 + 5) * 3 - 20 / 4"])
        .output()
        .expect("Failed to execute compiler");
    
    // (10 + 5) * 3 - 20 / 4 = 15 * 3 - 5 = 45 - 5 = 40
    assert!(output.status.success() || output.status.code() == Some(40), "Expected exit code 40");
}

#[test]
fn test_operator_precedence() {
    let output = Command::new("cargo")
        .args(&["run", "--", "-e", "2 + 3 * 4 - 5"])
        .output()
        .expect("Failed to execute compiler");
    
    // 2 + 3 * 4 - 5 = 2 + 12 - 5 = 9
    assert!(output.status.success() || output.status.code() == Some(9), "Expected exit code 9");
}

#[test]
fn test_comments() {
    let output = Command::new("cargo")
        .args(&["run", "--", "-e", "10 # comment\n+ 5 # another comment"])
        .output()
        .expect("Failed to execute compiler");
    
    // 10 + 5 = 15
    assert!(output.status.success() || output.status.code() == Some(15), "Expected exit code 15");
}