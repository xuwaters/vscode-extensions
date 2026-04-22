//! Diagnostic types and the diagnostic-engine driver.

use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

mod checks;
mod resolve_checks;
mod style;

pub use checks::run_all_checks;
pub use resolve_checks::run_resolve_checks;
pub use style::{run_style_checks, StyleConfig};

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

    StyleUpperCamel,           // PROTO0070 (messages/enums/services/rpcs)
    StyleLowerSnake,           // PROTO0071 (field names)
    StyleScreamingSnake,       // PROTO0072 (enum values)
    StyleEmptyMessage,         // PROTO0073

    Proto3RequiredForbidden,   // PROTO0090 internal

    // Textproto (text format) parsing and schema validation. The textproto
    // analyzer reuses this enum so the LSP diagnostic stream is uniform.
    TextprotoParseError,           // PROTO0100
    TextprotoHeaderUnknown,        // PROTO0101
    TextprotoHeaderDuplicate,      // PROTO0102
    TextprotoSchemaFileUnresolved, // PROTO0110
    TextprotoSchemaMessageUnknown, // PROTO0111
    TextprotoFieldUnknown,         // PROTO0112
    TextprotoFieldTypeMismatch,    // PROTO0113
    TextprotoEnumValueUnknown,     // PROTO0114
    TextprotoDuplicateSingular,    // PROTO0115
    TextprotoOneofConflict,        // PROTO0116
    TextprotoAnyUnsupported,       // PROTO0117
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
            StyleUpperCamel => "PROTO0070",
            StyleLowerSnake => "PROTO0071",
            StyleScreamingSnake => "PROTO0072",
            StyleEmptyMessage => "PROTO0073",
            Proto3RequiredForbidden => "PROTO0090",
            TextprotoParseError => "PROTO0100",
            TextprotoHeaderUnknown => "PROTO0101",
            TextprotoHeaderDuplicate => "PROTO0102",
            TextprotoSchemaFileUnresolved => "PROTO0110",
            TextprotoSchemaMessageUnknown => "PROTO0111",
            TextprotoFieldUnknown => "PROTO0112",
            TextprotoFieldTypeMismatch => "PROTO0113",
            TextprotoEnumValueUnknown => "PROTO0114",
            TextprotoDuplicateSingular => "PROTO0115",
            TextprotoOneofConflict => "PROTO0116",
            TextprotoAnyUnsupported => "PROTO0117",
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
