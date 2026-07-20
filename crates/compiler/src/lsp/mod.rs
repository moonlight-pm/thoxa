//! Language server support (`thoxa lsp`).
//!
//! Phase 1: diagnostics + semantic tokens over stdio.
//! Phase 2: resolver + index + navigation (PR5–PR6); cross-module + buffer overlay (PR7).

pub mod analysis;
pub mod convert;
pub mod crossmodule;
pub mod index;
pub mod protocol;
pub mod server;

pub use analysis::{check_source, CheckDiagnostic};
pub use server::run;
