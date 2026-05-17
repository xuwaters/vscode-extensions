//! Diagnostic types emitted by the diesel-schema parser and resolver.

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
    /// DS001 — `diesel::table!` body did not parse.
    MalformedTable,
    /// DS002 — same table name declared by two `diesel::table!` blocks.
    DuplicateTable,
    /// DS003 — same column appears twice inside one table body.
    DuplicateColumn,
    /// DS004 — primary-key column is not listed in the column block.
    UnknownPrimaryKeyColumn,
    /// DS005 — `diesel::joinable!` references a table not defined in this file.
    UnknownJoinableTable,
    /// DS006 — `diesel::joinable!` references a column not present on the child table.
    UnknownJoinableColumn,
    /// DS007 — `diesel::allow_tables_to_appear_in_same_query!` references an unknown table.
    UnknownAllowTable,
    /// DS008 — same table listed twice inside one `allow_tables_to_appear_in_same_query!`.
    DuplicateAllowTable,
    /// DS009 — `diesel::joinable!` references a pair of tables that are not in any
    /// `allow_tables_to_appear_in_same_query!` group together.
    JoinableNotAllowedTogether,
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        use DiagnosticCode::*;
        match self {
            MalformedTable => "DS001",
            DuplicateTable => "DS002",
            DuplicateColumn => "DS003",
            UnknownPrimaryKeyColumn => "DS004",
            UnknownJoinableTable => "DS005",
            UnknownJoinableColumn => "DS006",
            UnknownAllowTable => "DS007",
            DuplicateAllowTable => "DS008",
            JoinableNotAllowedTogether => "DS009",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SchemaDiagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub span: ByteSpan,
}

impl SchemaDiagnostic {
    pub fn error(code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) -> Self {
        SchemaDiagnostic { code, severity: Severity::Error, message: message.into(), span }
    }

    pub fn warning(code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) -> Self {
        SchemaDiagnostic { code, severity: Severity::Warning, message: message.into(), span }
    }

    pub fn info(code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) -> Self {
        SchemaDiagnostic { code, severity: Severity::Info, message: message.into(), span }
    }
}
