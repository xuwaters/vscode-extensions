//! Hover provider — shows:
//!
//! * **Table definitions** — a markdown summary of the table's primary key
//!   and column list when hovering the table's name (in any context).
//! * **Columns** — `column: SqlType` plus the owning table name when
//!   hovering a column inside its `diesel::table!` block, or inside a
//!   `diesel::joinable!(child -> parent (fk_col))` invocation.

use crate::ast::{SchemaFile, Table};
use crate::parse::ParsedFile;
use crate::spans::{ByteSpan, LineCol};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Hover {
    pub contents: String,
    pub range: ByteSpan,
}

pub fn hover(parsed: &ParsedFile, pos: LineCol) -> Option<Hover> {
    let cursor = parsed.spans.line_col_to_offset(&parsed.source, pos);
    let file = &parsed.ast;

    // Column inside its own table body.
    for t in &file.tables {
        for c in &t.columns {
            if c.name.span.contains(cursor) {
                return Some(Hover {
                    contents: format!(
                        "**{}.{}**: `{}`",
                        t.name.name, c.name.name, c.sql_type.display
                    ),
                    range: c.name.span,
                });
            }
            if c.sql_type.span.contains(cursor) {
                return Some(Hover {
                    contents: format!("`{}`", c.sql_type.display),
                    range: c.sql_type.span,
                });
            }
        }
        // Schema qualifier.
        if let Some(s) = &t.schema {
            if s.span.contains(cursor) {
                return Some(Hover {
                    contents: format!("schema `{}`", s.name),
                    range: s.span,
                });
            }
        }
        // Table name in the table! header.
        if t.name.span.contains(cursor) {
            return Some(Hover { contents: render_table_summary(t), range: t.name.span });
        }
        // Primary-key list.
        for pk in &t.primary_keys {
            if pk.span.contains(cursor) {
                let detail = column_type(t, &pk.name)
                    .map(|d| format!("primary key `{}.{}`: `{}`", t.name.name, pk.name, d))
                    .unwrap_or_else(|| {
                        format!("primary-key reference `{}` (not declared)", pk.name)
                    });
                return Some(Hover { contents: detail, range: pk.span });
            }
        }
    }

    // Joinable references.
    for j in &file.joinables {
        if j.child.span.contains(cursor) {
            return table_summary_hover(file, &j.child.name, j.child.span);
        }
        if j.parent.span.contains(cursor) {
            return table_summary_hover(file, &j.parent.name, j.parent.span);
        }
        if j.fk_column.span.contains(cursor) {
            let detail = file
                .tables
                .iter()
                .find(|t| t.name.name == j.child.name)
                .and_then(|t| column_type(t, &j.fk_column.name).map(|ty| (t, ty)))
                .map(|(t, ty)| format!("**{}.{}**: `{}`", t.name.name, j.fk_column.name, ty))
                .unwrap_or_else(|| format!("`{}` — column not found", j.fk_column.name));
            return Some(Hover { contents: detail, range: j.fk_column.span });
        }
    }

    // Allow-group entries.
    for g in &file.allow_groups {
        for t in &g.tables {
            if t.span.contains(cursor) {
                return table_summary_hover(file, &t.name, t.span);
            }
        }
    }

    None
}

fn table_summary_hover(file: &SchemaFile, name: &str, span: ByteSpan) -> Option<Hover> {
    let t = file.tables.iter().find(|t| t.name.name == name)?;
    Some(Hover { contents: render_table_summary(t), range: span })
}

fn render_table_summary(t: &Table) -> String {
    let mut out = String::new();
    let qualified = match &t.schema {
        Some(s) => format!("{}.{}", s.name, t.name.name),
        None => t.name.name.clone(),
    };
    out.push_str(&format!("**table `{qualified}`**"));
    if !t.primary_keys.is_empty() {
        let names: Vec<&str> = t.primary_keys.iter().map(|p| p.name.as_str()).collect();
        out.push_str(&format!("\n\nprimary key: `{}`", names.join(", ")));
    }
    if !t.columns.is_empty() {
        out.push_str("\n\n| column | type |\n|---|---|");
        for c in &t.columns {
            out.push_str(&format!("\n| `{}` | `{}` |", c.name.name, c.sql_type.display));
        }
    }
    out
}

fn column_type<'a>(t: &'a Table, name: &str) -> Option<&'a str> {
    t.columns
        .iter()
        .find(|c| c.name.name == name)
        .map(|c| c.sql_type.display.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    #[test]
    fn hover_on_column_name() {
        let src = "diesel::table! { users (id) { id -> Text, email -> Citext, } }\n";
        let pf = parse(FileUri::new("t"), src.to_string());
        let off = src.find("email").unwrap() as u32;
        let pos = pf.spans.offset_to_line_col(&pf.source, off + 1);
        let h = hover(&pf, pos).unwrap();
        assert!(h.contents.contains("users.email"));
        assert!(h.contents.contains("Citext"));
    }

    #[test]
    fn hover_on_table_name_shows_summary() {
        let src = "diesel::table! { users (id) { id -> Text, } }\n";
        let pf = parse(FileUri::new("t"), src.to_string());
        let off = src.find("users").unwrap() as u32;
        let pos = pf.spans.offset_to_line_col(&pf.source, off + 1);
        let h = hover(&pf, pos).unwrap();
        assert!(h.contents.contains("table `users`"));
        assert!(h.contents.contains("primary key"));
    }

    #[test]
    fn hover_on_joinable_fk_column_resolves_type() {
        let src = r#"
diesel::table! { messages (id) { id -> Text, author_id -> Text, } }
diesel::table! { users (id) { id -> Text, } }
diesel::joinable!(messages -> users (author_id));
"#;
        let pf = parse(FileUri::new("t"), src.to_string());
        let off = src.rfind("author_id").unwrap() as u32;
        let pos = pf.spans.offset_to_line_col(&pf.source, off + 1);
        let h = hover(&pf, pos).unwrap();
        assert!(h.contents.contains("messages.author_id"));
        assert!(h.contents.contains("Text"));
    }
}
