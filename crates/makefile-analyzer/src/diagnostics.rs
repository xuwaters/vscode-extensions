//! Diagnostic types emitted by the lexer and parser.

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
    RecipeUsesSpaces,        // MAKE001
    RecipeOutsideRule,        // MAKE002
    DefineNotClosed,          // MAKE003
    ConditionalNotClosed,     // MAKE004
    EndefWithoutDefine,       // MAKE005
    EndifWithoutIf,           // MAKE006
    AssignToAutoVariable,     // MAKE007
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        use DiagnosticCode::*;
        match self {
            RecipeUsesSpaces => "MAKE001",
            RecipeOutsideRule => "MAKE002",
            DefineNotClosed => "MAKE003",
            ConditionalNotClosed => "MAKE004",
            EndefWithoutDefine => "MAKE005",
            EndifWithoutIf => "MAKE006",
            AssignToAutoVariable => "MAKE007",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MakeDiagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub span: ByteSpan,
}

impl MakeDiagnostic {
    pub fn error(code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) -> Self {
        MakeDiagnostic { code, severity: Severity::Error, message: message.into(), span }
    }

    pub fn warning(code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) -> Self {
        MakeDiagnostic { code, severity: Severity::Warning, message: message.into(), span }
    }
}
