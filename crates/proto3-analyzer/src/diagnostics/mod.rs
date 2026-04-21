//! Diagnostic types and the diagnostic-engine driver.

use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

mod checks;
pub use checks::run_all_checks;

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
    ParseUnexpectedToken,     // PROTO0001
    ParseExpected,             // PROTO0002
    LexUnterminatedString,     // PROTO0003
    LexUnterminatedComment,    // PROTO0004
    LexInvalidEscape,          // PROTO0005

    ImportUnresolved,          // PROTO0010
    ImportUnused,              // PROTO0011
    ImportCircular,            // PROTO0012

    UnknownType,               // PROTO0020
    NotAType,                  // PROTO0021

    DuplicateFieldNumber,      // PROTO0030
    FieldNumberOutOfRange,     // PROTO0031
    FieldNumberReservedRange,  // PROTO0032
    FieldNumberReserved,       // PROTO0033
    FieldNameReserved,         // PROTO0034

    DuplicateName,             // PROTO0040
    DuplicateEnumValue,        // PROTO0041
    Proto3EnumFirstValueZero,  // PROTO0042

    OneofRepeatedField,        // PROTO0050
    OneofMapField,             // PROTO0051
    OneofInvalidField,         // PROTO0052

    PackedInvalid,             // PROTO0060
    MapKeyTypeInvalid,         // PROTO0061

    Proto3RequiredForbidden,   // PROTO0090 internal
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        use DiagnosticCode::*;
        match self {
            ParseUnexpectedToken => "PROTO0001",
            ParseExpected => "PROTO0002",
            LexUnterminatedString => "PROTO0003",
            LexUnterminatedComment => "PROTO0004",
            LexInvalidEscape => "PROTO0005",
            ImportUnresolved => "PROTO0010",
            ImportUnused => "PROTO0011",
            ImportCircular => "PROTO0012",
            UnknownType => "PROTO0020",
            NotAType => "PROTO0021",
            DuplicateFieldNumber => "PROTO0030",
            FieldNumberOutOfRange => "PROTO0031",
            FieldNumberReservedRange => "PROTO0032",
            FieldNumberReserved => "PROTO0033",
            FieldNameReserved => "PROTO0034",
            DuplicateName => "PROTO0040",
            DuplicateEnumValue => "PROTO0041",
            Proto3EnumFirstValueZero => "PROTO0042",
            OneofRepeatedField => "PROTO0050",
            OneofMapField => "PROTO0051",
            OneofInvalidField => "PROTO0052",
            PackedInvalid => "PROTO0060",
            MapKeyTypeInvalid => "PROTO0061",
            Proto3RequiredForbidden => "PROTO0090",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtoDiagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub span: ByteSpan,
}

impl ProtoDiagnostic {
    pub fn new(code: DiagnosticCode, severity: Severity, message: String, span: ByteSpan) -> Self {
        ProtoDiagnostic { code, severity, message, span }
    }
}
