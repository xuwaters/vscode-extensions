//! Parse pipeline — parse a single source string into a [`ParsedFile`] and
//! then run cross-cutting resolution diagnostics (duplicate tables,
//! unknown joinable references, etc.).

use crate::ast::SchemaFile;
use crate::diagnostics::{DiagnosticCode, SchemaDiagnostic};
use crate::parser;
use crate::spans::SpanTable;
use crate::vfs::FileUri;
use std::collections::{HashMap, HashSet};

pub use crate::vfs::ParsedFile;

pub fn parse(uri: FileUri, source: String) -> ParsedFile {
    let spans = SpanTable::new(&source);
    let (ast, mut diagnostics) = parser::parse_schema_file(&source);
    resolve(&ast, &mut diagnostics);
    ParsedFile { uri, source, ast, spans, diagnostics }
}

fn resolve(file: &SchemaFile, diags: &mut Vec<SchemaDiagnostic>) {
    // Index tables by name; report duplicates.
    let mut tables_by_name: HashMap<&str, &crate::ast::Table> = HashMap::new();
    for t in &file.tables {
        if let Some(_prev) = tables_by_name.get(t.name.name.as_str()) {
            diags.push(SchemaDiagnostic::warning(
                DiagnosticCode::DuplicateTable,
                format!("duplicate table definition for `{}`", t.name.name),
                t.name.span,
            ));
        } else {
            tables_by_name.insert(t.name.name.as_str(), t);
        }
    }

    // Per-table column duplicate + unknown primary-key column.
    for t in &file.tables {
        let mut seen: HashMap<&str, ()> = HashMap::new();
        for c in &t.columns {
            if seen.contains_key(c.name.name.as_str()) {
                diags.push(SchemaDiagnostic::warning(
                    DiagnosticCode::DuplicateColumn,
                    format!("duplicate column `{}` in table `{}`", c.name.name, t.name.name),
                    c.name.span,
                ));
            } else {
                seen.insert(c.name.name.as_str(), ());
            }
        }
        for pk in &t.primary_keys {
            if !seen.contains_key(pk.name.as_str()) {
                diags.push(SchemaDiagnostic::warning(
                    DiagnosticCode::UnknownPrimaryKeyColumn,
                    format!(
                        "primary-key column `{}` is not declared in table `{}`",
                        pk.name, t.name.name
                    ),
                    pk.span,
                ));
            }
        }
    }

    // joinables — child/parent must exist; fk_column must be on child.
    for j in &file.joinables {
        let child = tables_by_name.get(j.child.name.as_str()).copied();
        let parent = tables_by_name.get(j.parent.name.as_str()).copied();
        if child.is_none() {
            diags.push(SchemaDiagnostic::warning(
                DiagnosticCode::UnknownJoinableTable,
                format!("unknown table `{}` in `diesel::joinable!`", j.child.name),
                j.child.span,
            ));
        }
        if parent.is_none() {
            diags.push(SchemaDiagnostic::warning(
                DiagnosticCode::UnknownJoinableTable,
                format!("unknown table `{}` in `diesel::joinable!`", j.parent.name),
                j.parent.span,
            ));
        }
        if let Some(c) = child {
            let has_col = c.columns.iter().any(|col| col.name.name == j.fk_column.name);
            if !has_col {
                diags.push(SchemaDiagnostic::warning(
                    DiagnosticCode::UnknownJoinableColumn,
                    format!(
                        "column `{}` not found on table `{}` (referenced by `diesel::joinable!`)",
                        j.fk_column.name, c.name.name
                    ),
                    j.fk_column.span,
                ));
            }
        }
    }

    // allow_tables_to_appear_in_same_query — entries must exist; warn on duplicates.
    for g in &file.allow_groups {
        let mut seen: HashSet<&str> = HashSet::new();
        for t in &g.tables {
            if !tables_by_name.contains_key(t.name.as_str()) {
                diags.push(SchemaDiagnostic::warning(
                    DiagnosticCode::UnknownAllowTable,
                    format!(
                        "table `{}` is not defined in this file (referenced by `diesel::allow_tables_to_appear_in_same_query!`)",
                        t.name
                    ),
                    t.span,
                ));
            }
            if !seen.insert(t.name.as_str()) {
                diags.push(SchemaDiagnostic::info(
                    DiagnosticCode::DuplicateAllowTable,
                    format!("table `{}` listed more than once", t.name),
                    t.span,
                ));
            }
        }
    }

    // joinables — both endpoints should appear together in some allow group.
    // Only fires when at least one allow group exists (avoids noise on
    // schema files that don't bother with the macro).
    if !file.allow_groups.is_empty() {
        for j in &file.joinables {
            let mut ok = false;
            for g in &file.allow_groups {
                let names: HashSet<&str> = g.tables.iter().map(|t| t.name.as_str()).collect();
                if names.contains(j.child.name.as_str()) && names.contains(j.parent.name.as_str()) {
                    ok = true;
                    break;
                }
            }
            if !ok {
                diags.push(SchemaDiagnostic::info(
                    DiagnosticCode::JoinableNotAllowedTogether,
                    format!(
                        "`{}` and `{}` are joinable but do not appear together in any `allow_tables_to_appear_in_same_query!` group",
                        j.child.name, j.parent.name
                    ),
                    j.span,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::DiagnosticCode;

    fn parse_src(src: &str) -> ParsedFile {
        parse(FileUri::new("t"), src.to_string())
    }

    #[test]
    fn flags_unknown_joinable_table() {
        let src = r#"
diesel::table! { users (id) { id -> Text, } }
diesel::joinable!(messages -> users (author_id));
"#;
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownJoinableTable
                && d.message.contains("messages")));
    }

    #[test]
    fn flags_unknown_joinable_column() {
        let src = r#"
diesel::table! { messages (id) { id -> Text, } }
diesel::table! { users (id) { id -> Text, } }
diesel::joinable!(messages -> users (author_id));
"#;
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownJoinableColumn));
    }

    #[test]
    fn flags_duplicate_table() {
        let src = r#"
diesel::table! { users (id) { id -> Text, } }
diesel::table! { users (id) { id -> Text, } }
"#;
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::DuplicateTable));
    }

    #[test]
    fn flags_unknown_primary_key_column() {
        let src = r#"
diesel::table! { users (uid) { id -> Text, } }
"#;
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownPrimaryKeyColumn));
    }

    #[test]
    fn flags_unknown_allow_table() {
        let src = r#"
diesel::table! { users (id) { id -> Text, } }
diesel::allow_tables_to_appear_in_same_query!(users, gone);
"#;
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownAllowTable
                && d.message.contains("gone")));
    }

    #[test]
    fn flags_joinable_not_allowed_together() {
        let src = r#"
diesel::table! { messages (id) { id -> Text, author_id -> Text, } }
diesel::table! { users (id) { id -> Text, } }
diesel::table! { other (id) { id -> Text, } }
diesel::joinable!(messages -> users (author_id));
diesel::allow_tables_to_appear_in_same_query!(messages, other);
"#;
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::JoinableNotAllowedTogether));
    }
}
