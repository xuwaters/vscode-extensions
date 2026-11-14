//! Diagnostic codes for RON.

pub use analyzer_core::diagnostics::{Diagnostic as CoreDiagnostic, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    /// Generic syntax error: an unexpected token, a missing `:` or `,`, a
    /// missing value, an unclosed bracket.
    SyntaxError,
    /// A string literal that never sees its closing quote.
    UnterminatedString,
    /// A `/* ... */` comment that never closes (block comments nest).
    UnterminatedComment,
    /// A number RON cannot read: a bad digit for its radix, a dangling
    /// exponent, an unknown type suffix.
    InvalidNumber,
    /// An escape sequence RON does not understand.
    InvalidEscape,
    /// A character literal that is unterminated or holds other than one
    /// character.
    InvalidChar,
    /// Content after the top-level value.
    TrailingContent,
    /// Named fields and positional values mixed in one `( ... )`.
    MixedFields,
    /// The same field twice in one struct.
    DuplicateField,
    /// The same key twice in one map.
    DuplicateKey,
    /// `#![enable(...)]` naming an extension RON does not have.
    UnknownExtension,
}

impl analyzer_core::diagnostics::DiagnosticCode for DiagnosticCode {
    fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::SyntaxError => "RON001",
            DiagnosticCode::UnterminatedString => "RON002",
            DiagnosticCode::UnterminatedComment => "RON003",
            DiagnosticCode::InvalidNumber => "RON004",
            DiagnosticCode::InvalidEscape => "RON005",
            DiagnosticCode::InvalidChar => "RON006",
            DiagnosticCode::TrailingContent => "RON007",
            DiagnosticCode::MixedFields => "RON008",
            DiagnosticCode::DuplicateField => "RON009",
            DiagnosticCode::DuplicateKey => "RON010",
            DiagnosticCode::UnknownExtension => "RON011",
        }
    }
}

impl DiagnosticCode {
    /// Codes after which the AST has holes or untrustworthy raw slices, so
    /// the formatter must not touch the file. Duplicates and unknown
    /// extensions are real errors for a deserializer but leave the syntax
    /// tree intact, so they do not block formatting.
    pub fn blocks_formatting(self) -> bool {
        !matches!(
            self,
            DiagnosticCode::DuplicateField
                | DiagnosticCode::DuplicateKey
                | DiagnosticCode::UnknownExtension
        )
    }
}

pub type Diagnostic = CoreDiagnostic<DiagnosticCode>;
