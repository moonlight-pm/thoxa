//! Thoxa native compiler library (compile-to-C).

pub mod ast;
pub(crate) mod ast_build;
pub mod cgen;
pub mod conversion_desugar;
pub mod extend_desugar;
pub mod diagnostic;
pub mod driver;
pub mod error;
pub mod lexer;
pub mod loader;
pub mod lsp;
pub mod mir;
pub mod parser;
pub mod refine;
pub mod resolver;
pub mod route_desugar;
pub mod span;
pub mod trait_desugar;
pub mod validate_desugar;
pub mod version;

pub use diagnostic::{Diagnostic, Label, Severity};
pub use driver::{build, emit_c, run, BuildOptions};
pub use error::{Error, Result};
pub use lsp::{check_source, CheckDiagnostic};
