//! Diagnostic types emitted by the diesel-schema parser and resolver.
//! Severity and the diagnostic carrier live in `analyzer-core`; this
//! module only defines the diesel-specific code enum.

pub use analyzer_core::diagnostics::Severity;

use analyzer_core::diagnostics::{Diagnostic, DiagnosticCode as DiagnosticCodeTrait};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

impl DiagnosticCodeTrait for DiagnosticCode {
    fn as_str(self) -> &'static str {
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

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        <Self as DiagnosticCodeTrait>::as_str(self)
    }
}

pub type SchemaDiagnostic = Diagnostic<DiagnosticCode>;
