//! Diagnostic types emitted by the dotenv parser.

use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum DiagnosticCode {
    InvalidKey,          // ENV001 — key not a valid identifier
    MissingEquals,       // ENV002 — non-blank, non-comment line without `=`
    SpacesAroundEquals,  // ENV003 — `KEY = value` (rejected by many loaders)
    UnclosedQuote,       // ENV004 — quote opened but never closed
    DuplicateKey,        // ENV005 — same key assigned twice
    UnknownVariableRef,  // ENV006 — `$FOO` / `${FOO}` references a key not in this file
    EmptyKey,            // ENV007 — `=value` with no name
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        use DiagnosticCode::*;
        match self {
            InvalidKey => "ENV001",
            MissingEquals => "ENV002",
            SpacesAroundEquals => "ENV003",
            UnclosedQuote => "ENV004",
            DuplicateKey => "ENV005",
            UnknownVariableRef => "ENV006",
            EmptyKey => "ENV007",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DotenvDiagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub span: ByteSpan,
}

impl DotenvDiagnostic {
    pub fn error(code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) -> Self {
        DotenvDiagnostic { code, severity: Severity::Error, message: message.into(), span }
    }

    pub fn warning(code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) -> Self {
        DotenvDiagnostic { code, severity: Severity::Warning, message: message.into(), span }
    }

    pub fn info(code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) -> Self {
        DotenvDiagnostic { code, severity: Severity::Info, message: message.into(), span }
    }
}
