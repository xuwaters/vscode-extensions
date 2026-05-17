//! Completion provider — suggests table names inside `diesel::joinable!`
//! and `diesel::allow_tables_to_appear_in_same_query!`, and column names
//! for the joinable's parenthesised FK position.
//!
//! Strategy is deliberately simple: based on the cursor position
//! (relative to parsed macro invocations), decide whether the user is
//! inside one of those slots and emit completions accordingly. This
//! avoids any LSP-level grammar tracking.

use crate::ast::{Joinable, SchemaFile};
use crate::parse::ParsedFile;
use crate::spans::LineCol;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub kind: CompletionKind,
    pub insert_text: String,
    pub replace_length: u32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum CompletionKind {
    Table,
    Column,
}

pub fn completions(parsed: &ParsedFile, pos: LineCol) -> Vec<CompletionItem> {
    let cursor = parsed.spans.line_col_to_offset(&parsed.source, pos);
    let file = &parsed.ast;
    let bytes = parsed.source.as_bytes();
    let prefix = ident_prefix(bytes, cursor as usize);

    // Joinable slots.
    for j in &file.joinables {
        if !j.span.contains(cursor) {
            continue;
        }
        return complete_in_joinable(file, j, cursor, prefix);
    }
    // Allow group slot.
    for g in &file.allow_groups {
        if g.span.contains(cursor) {
            let already: std::collections::HashSet<&str> =
                g.tables.iter().map(|t| t.name.as_str()).collect();
            return file
                .tables
                .iter()
                .filter(|t| t.name.name.starts_with(prefix.0))
                .filter(|t| !already.contains(t.name.name.as_str()))
                .map(|t| table_item(&t.name.name, prefix.1))
                .collect();
        }
    }
    Vec::new()
}

fn complete_in_joinable(
    file: &SchemaFile,
    j: &Joinable,
    cursor: u32,
    prefix: (&str, u32),
) -> Vec<CompletionItem> {
    // If cursor sits inside the inner `(fk_col)` parens, suggest the
    // child table's columns. Heuristic: the inner span is anything after
    // the parent ident but before the joinable span's end, and it's
    // anchored by parentheses we don't track directly. We approximate
    // by saying "after parent.span.end".
    if cursor > j.parent.span.end {
        // Default: column slot for the child table.
        if let Some(t) = file.tables.iter().find(|t| t.name.name == j.child.name) {
            return t
                .columns
                .iter()
                .filter(|c| c.name.name.starts_with(prefix.0))
                .map(|c| CompletionItem {
                    label: c.name.name.clone(),
                    detail: Some(c.sql_type.display.clone()),
                    kind: CompletionKind::Column,
                    insert_text: c.name.name.clone(),
                    replace_length: prefix.1,
                })
                .collect();
        }
        return Vec::new();
    }
    // Otherwise the cursor is in the `child -> parent` portion; suggest
    // table names.
    file.tables
        .iter()
        .filter(|t| t.name.name.starts_with(prefix.0))
        .map(|t| table_item(&t.name.name, prefix.1))
        .collect()
}

fn table_item(name: &str, replace_length: u32) -> CompletionItem {
    CompletionItem {
        label: name.to_string(),
        detail: Some("diesel table".to_string()),
        kind: CompletionKind::Table,
        insert_text: name.to_string(),
        replace_length,
    }
}

fn ident_prefix(bytes: &[u8], cursor: usize) -> (&str, u32) {
    let cursor = cursor.min(bytes.len());
    let mut i = cursor;
    while i > 0 && is_ident_cont(bytes[i - 1]) {
        i -= 1;
    }
    let slice = &bytes[i..cursor];
    let s = std::str::from_utf8(slice).unwrap_or("");
    (s, (cursor - i) as u32)
}

fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    fn complete_at(src: &str, cursor_byte: usize) -> Vec<String> {
        let pf = parse(FileUri::new("t"), src.to_string());
        let pos = pf.spans.offset_to_line_col(&pf.source, cursor_byte as u32);
        completions(&pf, pos).into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn completes_tables_in_joinable_head() {
        let src = "diesel::table! { users (id) { id -> Text, } }\n\
                   diesel::table! { messages (id) { id -> Text, author_id -> Text, } }\n\
                   diesel::joinable!(m -> users (author_id));";
        let cursor = src.find("(m -> ").unwrap() + 2; // right after `m`
        let labels = complete_at(src, cursor);
        assert!(labels.iter().any(|l| l == "messages"));
    }

    #[test]
    fn completes_columns_for_fk() {
        let src = "diesel::table! { users (id) { id -> Text, } }\n\
                   diesel::table! { messages (id) { id -> Text, author_id -> Text, } }\n\
                   diesel::joinable!(messages -> users (au));";
        let cursor = src.rfind("au").unwrap() + 2;
        let labels = complete_at(src, cursor);
        assert!(labels.iter().any(|l| l == "author_id"));
    }

    #[test]
    fn completes_tables_in_allow_group() {
        let src = "diesel::table! { users (id) { id -> Text, } }\n\
                   diesel::table! { messages (id) { id -> Text, } }\n\
                   diesel::allow_tables_to_appear_in_same_query!(users, );";
        let cursor = src.rfind(", ").unwrap() + 2;
        let labels = complete_at(src, cursor);
        assert!(labels.iter().any(|l| l == "messages"));
        // `users` is already listed — should be filtered out.
        assert!(!labels.iter().any(|l| l == "users"));
    }
}
