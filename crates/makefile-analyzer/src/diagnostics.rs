//! Diagnostic types emitted by the lexer and parser. Severity and the
//! diagnostic carrier live in `analyzer-core`; this module only defines
//! the makefile-specific code enum.

pub use analyzer_core::diagnostics::Severity;

use analyzer_core::diagnostics::{Diagnostic, DiagnosticCode as DiagnosticCodeTrait};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    RecipeUsesSpaces,        // MAKE001
    RecipeOutsideRule,        // MAKE002
    DefineNotClosed,          // MAKE003
    ConditionalNotClosed,     // MAKE004
    EndefWithoutDefine,       // MAKE005
    EndifWithoutIf,           // MAKE006
    AssignToAutoVariable,     // MAKE007
    EmptyVariableName,        // MAKE008
    ExtraneousText,           // MAKE009
    ElseWithoutIf,            // MAKE010
    InvalidConditional,       // MAKE011
    MissingSeparator,         // MAKE012
}

impl DiagnosticCodeTrait for DiagnosticCode {
    fn as_str(self) -> &'static str {
        use DiagnosticCode::*;
        match self {
            RecipeUsesSpaces => "MAKE001",
            RecipeOutsideRule => "MAKE002",
            DefineNotClosed => "MAKE003",
            ConditionalNotClosed => "MAKE004",
            EndefWithoutDefine => "MAKE005",
            EndifWithoutIf => "MAKE006",
            AssignToAutoVariable => "MAKE007",
            EmptyVariableName => "MAKE008",
            ExtraneousText => "MAKE009",
            ElseWithoutIf => "MAKE010",
            InvalidConditional => "MAKE011",
            MissingSeparator => "MAKE012",
        }
    }
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        <Self as DiagnosticCodeTrait>::as_str(self)
    }
}

pub type MakeDiagnostic = Diagnostic<DiagnosticCode>;
