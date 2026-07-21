pub mod ast;
pub mod codegen;
pub mod import_instantiation;
pub mod import_resolver;
pub mod lexer;
pub mod modules;
pub mod parser;
pub mod runtime;
pub mod symbols;
pub mod tagged_value;
pub mod type_checker;
pub mod types;

// Re-export for backward compatibility with tests
pub mod codegen_jit {
    pub use crate::codegen::jit::JITCodegen;
}
