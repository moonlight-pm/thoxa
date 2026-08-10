//! Compiler errors.

use crate::ast::Module;
use crate::span::Span;
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    pub message: String,
    pub span: Option<Span>,
    /// Additional independent diagnostics (e.g. other function body errors).
    /// Primary is `message`/`span`; each entry is a full follow-on message.
    pub more: Vec<AdditionalDiagnostic>,
    /// Best-effort AST when parse recovered (LSP multi-diag can still lower).
    pub recovered_module: Option<Box<Module>>,
}

/// Secondary diagnostic collected during multi-body lower (LSP multi-diag).
#[derive(Debug, Clone)]
pub struct AdditionalDiagnostic {
    pub message: String,
    pub span: Option<Span>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: None,
            more: Vec::new(),
            recovered_module: None,
        }
    }

    pub fn at(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span: Some(span),
            more: Vec::new(),
            recovered_module: None,
        }
    }

    pub fn with_more(mut self, more: Vec<AdditionalDiagnostic>) -> Self {
        self.more = more;
        self
    }

    pub fn with_recovered_module(mut self, module: Module) -> Self {
        self.recovered_module = Some(Box::new(module));
        self
    }

    /// Flatten this error into independent diagnostics (no recovered AST).
    pub fn into_diagnostics(self) -> Vec<Error> {
        let mut out = Vec::with_capacity(1 + self.more.len());
        out.push(Error {
            message: self.message,
            span: self.span,
            more: Vec::new(),
            recovered_module: None,
        });
        for m in self.more {
            out.push(Error {
                message: m.message,
                span: m.span,
                more: Vec::new(),
                recovered_module: None,
            });
        }
        out
    }

    /// Fold independent errors into one primary + `more` (LSP multi-diag).
    /// Nested `more` on any input error is flattened (block recovery inside
    /// a function, then top-level multi, must not drop inner diagnostics).
    /// Preserves the first non-empty `recovered_module`.
    pub fn multi(errors: Vec<Error>) -> Error {
        let mut recovered = None;
        let mut flat: Vec<(String, Option<Span>)> = Vec::new();
        for e in errors {
            if recovered.is_none() {
                recovered = e.recovered_module;
            }
            flat.push((e.message, e.span));
            for m in e.more {
                flat.push((m.message, m.span));
            }
        }
        let mut it = flat.into_iter();
        let (msg, span) = it.next().expect("Error::multi nonempty");
        let more = it
            .map(|(message, span)| AdditionalDiagnostic { message, span })
            .collect();
        Error {
            message: msg,
            span,
            more,
            recovered_module: recovered,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}
