//! Diagnostic types emitted by the dotenv parser. The diagnostic shape
//! (severity, message, span) lives in `analyzer-core`; this module only
//! defines the dotenv-specific code enum.

pub use analyzer_core::diagnostics::Severity;

use analyzer_core::diagnostics::{Diagnostic, DiagnosticCode as DiagnosticCodeTrait};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    InvalidKey,          // ENV001 — key not a valid identifier
    MissingEquals,       // ENV002 — non-blank, non-comment line without `=`
    SpacesAroundEquals,  // ENV003 — `KEY = value` (rejected by many loaders)
    UnclosedQuote,       // ENV004 — quote opened but never closed
    DuplicateKey,        // ENV005 — same key assigned twice
    UnknownVariableRef,  // ENV006 — `$FOO` / `${FOO}` references a key not in this file
    EmptyKey,            // ENV007 — `=value` with no name
}

impl DiagnosticCodeTrait for DiagnosticCode {
    fn as_str(self) -> &'static str {
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

impl DiagnosticCode {
    /// Inherent shadow of the trait method so existing `code.as_str()`
    /// callers don't have to import the trait.
    pub fn as_str(self) -> &'static str {
        <Self as DiagnosticCodeTrait>::as_str(self)
    }
}

pub type DotenvDiagnostic = Diagnostic<DiagnosticCode>;
