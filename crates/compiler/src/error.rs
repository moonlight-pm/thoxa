//! Compiler errors (internal) + conversion into structured diagnostics.

use crate::ast::Module;
use crate::diagnostic::{self, Diagnostic, Label};
use crate::span::Span;
use std::fmt;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    pub message: String,
    pub span: Option<Span>,
    /// Source file this span is relative to (multi-file packages / LSP).
    pub path: Option<PathBuf>,
    /// Stable diagnostic code when known at emit time (e.g. `THX001`).
    pub code: Option<&'static str>,
    pub help: Vec<String>,
    pub labels: Vec<Label>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    /// Additional independent diagnostics (e.g. other function body errors).
    pub more: Vec<AdditionalDiagnostic>,
    /// Best-effort AST when parse recovered (LSP multi-diag can still lower).
    pub recovered_module: Option<Box<Module>>,
}

/// Secondary diagnostic collected during multi-body lower (LSP multi-diag).
#[derive(Debug, Clone)]
pub struct AdditionalDiagnostic {
    pub message: String,
    pub span: Option<Span>,
    pub path: Option<PathBuf>,
    pub code: Option<&'static str>,
    pub help: Vec<String>,
    pub labels: Vec<Label>,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

impl AdditionalDiagnostic {
    pub fn new(message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            message: message.into(),
            span,
            path: None,
            code: None,
            help: Vec::new(),
            labels: Vec::new(),
            expected: None,
            actual: None,
        }
    }

    pub fn from_error_parts(e: Error) -> Self {
        Self {
            message: e.message,
            span: e.span,
            path: e.path,
            code: e.code,
            help: e.help,
            labels: e.labels,
            expected: e.expected,
            actual: e.actual,
        }
    }

    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: None,
            path: None,
            code: None,
            help: Vec::new(),
            labels: Vec::new(),
            expected: None,
            actual: None,
            more: Vec::new(),
            recovered_module: None,
        }
    }

    pub fn at(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span: Some(span),
            path: None,
            code: None,
            help: Vec::new(),
            labels: Vec::new(),
            expected: None,
            actual: None,
            more: Vec::new(),
            recovered_module: None,
        }
    }

    pub fn coded(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: None,
            path: None,
            code: Some(code),
            help: Vec::new(),
            labels: Vec::new(),
            expected: None,
            actual: None,
            more: Vec::new(),
            recovered_module: None,
        }
    }

    pub fn coded_at(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span: Some(span),
            path: None,
            code: Some(code),
            help: Vec::new(),
            labels: Vec::new(),
            expected: None,
            actual: None,
            more: Vec::new(),
            recovered_module: None,
        }
    }

    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    /// Attach the source file this error's span is measured in.
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help.push(help.into());
        self
    }

    pub fn with_label(mut self, label: Label) -> Self {
        self.labels.push(label);
        self
    }

    pub fn with_expected_actual(
        mut self,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        self.expected = Some(expected.into());
        self.actual = Some(actual.into());
        self
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
    ///
    /// Preserves structured fields. Prefer [`into_structured`] for presentation.
    pub fn into_diagnostics(self) -> Vec<Error> {
        let mut out = Vec::with_capacity(1 + self.more.len());
        out.push(Error {
            message: self.message,
            span: self.span,
            path: self.path,
            code: self.code,
            help: self.help,
            labels: self.labels,
            expected: self.expected,
            actual: self.actual,
            more: Vec::new(),
            recovered_module: None,
        });
        for m in self.more {
            out.push(Error {
                message: m.message,
                span: m.span,
                path: m.path,
                code: m.code,
                help: m.help,
                labels: m.labels,
                expected: m.expected,
                actual: m.actual,
                more: Vec::new(),
                recovered_module: None,
            });
        }
        out
    }

    /// Convert to structured [`Diagnostic`] list (enriched codes/help).
    pub fn into_structured(self, path: Option<&Path>) -> Vec<Diagnostic> {
        diagnostic::from_error(self, path)
    }

    /// Fold independent errors into one primary + `more` (LSP multi-diag).
    /// Nested `more` on any input error is flattened (block recovery inside
    /// a function, then top-level multi, must not drop inner diagnostics).
    /// Preserves the first non-empty `recovered_module` and structured fields.
    pub fn multi(errors: Vec<Error>) -> Error {
        let mut recovered = None;
        let mut flat: Vec<AdditionalDiagnostic> = Vec::new();
        for e in errors {
            if recovered.is_none() {
                recovered = e.recovered_module;
            }
            flat.push(AdditionalDiagnostic {
                message: e.message,
                span: e.span,
                path: e.path,
                code: e.code,
                help: e.help,
                labels: e.labels,
                expected: e.expected,
                actual: e.actual,
            });
            for m in e.more {
                flat.push(m);
            }
        }
        let mut it = flat.into_iter();
        let first = it.next().expect("Error::multi nonempty");
        let more = it.collect();
        Error {
            message: first.message,
            span: first.span,
            path: first.path,
            code: first.code,
            help: first.help,
            labels: first.labels,
            expected: first.expected,
            actual: first.actual,
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
