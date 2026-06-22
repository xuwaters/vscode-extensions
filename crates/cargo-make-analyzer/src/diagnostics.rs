//! Diagnostic types emitted by the parser and structural lints. Severity
//! and the diagnostic carrier live in `analyzer-core`; this module only
//! defines the cargo-make-specific code enum.

pub use analyzer_core::diagnostics::Severity;

use analyzer_core::diagnostics::{Diagnostic, DiagnosticCode as DiagnosticCodeTrait};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    /// The TOML document failed to parse.
    ParseError, // CARGOMAKE001
    /// A `[tasks.X]` table declares more than one action
    /// (`command` / `script` / `run_task`).
    ConflictingAction, // CARGOMAKE002
    /// A key under `[config]` is not a recognised cargo-make config key.
    UnknownConfigKey, // CARGOMAKE003
    /// A key inside a `[tasks.X]` table is not a recognised task field.
    UnknownTaskField, // CARGOMAKE004
    /// A key inside a task `condition` table is not a recognised condition
    /// criterion.
    UnknownConditionKey, // CARGOMAKE005
    /// A task lists itself in its own `dependencies`.
    SelfDependency, // CARGOMAKE006
}

impl DiagnosticCodeTrait for DiagnosticCode {
    fn as_str(self) -> &'static str {
        use DiagnosticCode::*;
        match self {
            ParseError => "CARGOMAKE001",
            ConflictingAction => "CARGOMAKE002",
            UnknownConfigKey => "CARGOMAKE003",
            UnknownTaskField => "CARGOMAKE004",
            UnknownConditionKey => "CARGOMAKE005",
            SelfDependency => "CARGOMAKE006",
        }
    }
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        <Self as DiagnosticCodeTrait>::as_str(self)
    }
}

pub type CargoMakeDiagnostic = Diagnostic<DiagnosticCode>;
