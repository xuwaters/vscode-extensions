//! Diagnostic codes for the JSON family.

pub use analyzer_core::diagnostics::{Diagnostic as CoreDiagnostic, DiagnosticCode as Code, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    /// Generic syntax error: unexpected token, missing `:`/`,`, missing value.
    SyntaxError,
    /// A string literal never sees its closing quote.
    UnterminatedString,
    /// An escape sequence the flavor does not understand.
    InvalidEscape,
    /// A raw control character (U+0000..U+001F) inside a string.
    ControlCharacterInString,
    /// A number that no flavor of JSON can read.
    InvalidNumber,
    /// Content after the first top-level value.
    MultipleTopLevelValues,
    /// The same key appears twice in one object.
    DuplicateKey,
    /// A comment in a flavor without comments.
    CommentNotAllowed,
    /// A trailing comma in a flavor without trailing commas.
    TrailingCommaNotAllowed,
    /// A single-quoted string outside JSON5.
    SingleQuoteNotAllowed,
    /// An unquoted object key outside JSON5.
    UnquotedKeyNotAllowed,
    /// Hex literals, leading `+`, `.5`, `5.`, `Infinity`, `NaN` outside JSON5.
    NonStandardNumber,
    /// A `/* ... */` comment that never closes.
    UnterminatedComment,
}

impl analyzer_core::diagnostics::DiagnosticCode for DiagnosticCode {
    fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::SyntaxError => "JSON001",
            DiagnosticCode::UnterminatedString => "JSON002",
            DiagnosticCode::InvalidEscape => "JSON003",
            DiagnosticCode::ControlCharacterInString => "JSON004",
            DiagnosticCode::InvalidNumber => "JSON005",
            DiagnosticCode::MultipleTopLevelValues => "JSON006",
            DiagnosticCode::DuplicateKey => "JSON007",
            DiagnosticCode::CommentNotAllowed => "JSON008",
            DiagnosticCode::TrailingCommaNotAllowed => "JSON009",
            DiagnosticCode::SingleQuoteNotAllowed => "JSON010",
            DiagnosticCode::UnquotedKeyNotAllowed => "JSON011",
            DiagnosticCode::NonStandardNumber => "JSON012",
            DiagnosticCode::UnterminatedComment => "JSON013",
        }
    }
}

pub type Diagnostic = CoreDiagnostic<DiagnosticCode>;
