//! Test utilities for inspecting QBE output

use super::CodeGen;

/// Print QBE output with a header (only when SHOW_QBE env var is set)
pub fn show_qbe(codegen: &CodeGen, title: &str) {
    if std::env::var("SHOW_QBE").is_ok() {
        println!("\n=== {} ===", title);
        let output = codegen.current_qbe();
        for (i, line) in output.lines().enumerate() {
            println!("{:3}: {}", i + 1, line);
        }
        println!();
    }
}

/// Always print QBE output (useful for debugging specific tests)
pub fn debug_qbe(codegen: &CodeGen, title: &str) {
    println!("\n=== {} ===", title);
    let output = codegen.current_qbe();
    for (i, line) in output.lines().enumerate() {
        println!("{:3}: {}", i + 1, line);
    }
    println!();
}