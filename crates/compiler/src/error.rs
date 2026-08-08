//! Compiler errors.

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
        }
    }

    pub fn at(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span: Some(span),
            more: Vec::new(),
        }
    }

    pub fn with_more(mut self, more: Vec<AdditionalDiagnostic>) -> Self {
        self.more = more;
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}
