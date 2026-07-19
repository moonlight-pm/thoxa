//! Thoxa native compiler library (compile-to-C).

pub mod ast;
pub mod cgen;
pub mod driver;
pub mod error;
pub mod lexer;
pub mod mir;
pub mod parser;
pub mod span;

pub use driver::{build, emit_c, run, BuildOptions};
pub use error::{Error, Result};
