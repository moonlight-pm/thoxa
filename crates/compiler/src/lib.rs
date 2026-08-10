//! Thoxa native compiler library (compile-to-C).

pub mod ast;
pub mod cgen;
pub mod command;
pub mod driver;
pub mod error;
pub mod lexer;
pub mod loader;
pub mod lsp;
pub mod mir;
pub mod parser;
pub mod repl;
pub mod resolver;
pub mod session;
pub mod span;
pub mod version;

#[cfg(thoxa_libtcc)]
pub mod tcc;

pub use driver::{build, emit_c, run, BuildOptions};
pub use error::{Error, Result};
pub use lsp::{check_source, CheckDiagnostic};
