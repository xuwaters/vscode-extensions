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
    /// A `@[path::to::Type]` annotation that names nothing the schema can
    /// find.
    UnresolvedType,
    /// A value of the wrong kind for its Rust type.
    TypeMismatch,
    /// A field the Rust struct does not declare.
    UnknownField,
    /// A required field of the Rust struct is absent.
    MissingField,
    /// A name that is not a variant of the Rust enum.
    UnknownVariant,
    /// A tuple, tuple struct or array with the wrong number of values.
    WrongLength,
    /// A struct written with someone else's name, or without one under
    /// `explicit_struct_names`.
    WrongName,
    /// An integer too large or too small for its Rust type.
    OutOfRange,
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
            DiagnosticCode::UnresolvedType => "RON012",
            DiagnosticCode::TypeMismatch => "RON013",
            DiagnosticCode::UnknownField => "RON014",
            DiagnosticCode::MissingField => "RON015",
            DiagnosticCode::UnknownVariant => "RON016",
            DiagnosticCode::WrongLength => "RON017",
            DiagnosticCode::WrongName => "RON018",
            DiagnosticCode::OutOfRange => "RON019",
        }
    }
}

impl DiagnosticCode {
    /// Codes after which the AST has holes or untrustworthy raw slices, so
    /// the formatter must not touch the file. Duplicates, unknown
    /// extensions and schema findings are real errors for a deserializer
    /// but leave the syntax tree intact, so they do not block formatting.
    pub fn blocks_formatting(self) -> bool {
        matches!(
            self,
            DiagnosticCode::SyntaxError
                | DiagnosticCode::UnterminatedString
                | DiagnosticCode::UnterminatedComment
                | DiagnosticCode::InvalidNumber
                | DiagnosticCode::InvalidEscape
                | DiagnosticCode::InvalidChar
                | DiagnosticCode::TrailingContent
                | DiagnosticCode::MixedFields
        )
    }
}

pub type Diagnostic = CoreDiagnostic<DiagnosticCode>;
