//! JSON Lines → table, for the preview editor.
//!
//! Columns are the union of the records' top-level object keys in
//! first-seen order. Records that are not objects (a bare array or
//! scalar on a line) land in a synthetic `(value)` column appended
//! after the real keys, so mixed files still render.

use crate::ast::{Ast, Value, ValueKind};
use crate::features::formatting::{FormatOptions, Renderer};
use crate::workspace::ParsedFile;
use serde::Serialize;

/// Cells longer than this are cut with an ellipsis; the preview is a
/// scanner, not an editor.
const CELL_BUDGET: usize = 2000;

pub const VALUE_COLUMN: &str = "(value)";

#[derive(Debug, Clone, Serialize)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    /// Total parseable records in the file, before `max_rows`.
    pub total: u32,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    /// Zero-based source line, for row headers and jump-to-text.
    pub line: u32,
    pub cells: Vec<String>,
}

pub fn jsonl_table(pf: &ParsedFile, max_rows: usize) -> Table {
    let Ast::Lines(records) = &pf.ast else {
        return Table { columns: Vec::new(), rows: Vec::new(), total: 0, truncated: false };
    };
    let taken = &records[..records.len().min(max_rows)];

    let mut columns: Vec<String> = Vec::new();
    let mut needs_value_column = false;
    for record in taken {
        match &record.value.kind {
            ValueKind::Object(object) => {
                for member in &object.members {
                    if !columns.iter().any(|c| c == &member.key.name) {
                        columns.push(member.key.name.clone());
                    }
                }
            }
            _ => needs_value_column = true,
        }
    }
    let value_column = columns.len();
    if needs_value_column || columns.is_empty() {
        columns.push(VALUE_COLUMN.to_string());
    }

    let opts = FormatOptions::default();
    let renderer = Renderer { source: &pf.source, opts: &opts };
    let mut rows = Vec::with_capacity(taken.len());
    for record in taken {
        let mut cells = vec![String::new(); columns.len()];
        match &record.value.kind {
            ValueKind::Object(object) => {
                for member in &object.members {
                    let Some(idx) = columns.iter().position(|c| c == &member.key.name) else {
                        continue;
                    };
                    cells[idx] = cell_text(&renderer, &member.value);
                }
            }
            _ => cells[value_column] = cell_text(&renderer, &record.value),
        }
        rows.push(Row { line: record.line, cells });
    }

    Table {
        columns,
        rows,
        total: records.len() as u32,
        truncated: records.len() > max_rows,
    }
}

fn cell_text(renderer: &Renderer<'_>, value: &Value) -> String {
    let mut out = String::new();
    match &value.kind {
        // Strings show their decoded text — the preview reads like data,
        // not like source.
        ValueKind::String(s) => {
            out = s.value.clone();
            if out.len() > CELL_BUDGET {
                let mut cut = CELL_BUDGET;
                while !out.is_char_boundary(cut) {
                    cut -= 1;
                }
                out.truncate(cut);
                out.push('…');
            }
        }
        _ => renderer.compact(&mut out, value, CELL_BUDGET),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flavor::Flavor;
    use crate::workspace::{parse, FileUri};
    use pretty_assertions::assert_eq;

    fn table(src: &str, max_rows: usize) -> Table {
        jsonl_table(&parse(FileUri::new("t"), src.to_string(), Flavor::Jsonl), max_rows)
    }

    #[test]
    fn columns_union_in_first_seen_order() {
        let t = table(
            "{\"b\": 1, \"a\": 2}\n{\"a\": 3, \"c\": 4}\n",
            100,
        );
        assert_eq!(t.columns, vec!["b", "a", "c"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].cells, vec!["1", "2", ""]);
        assert_eq!(t.rows[1].cells, vec!["", "3", "4"]);
    }

    #[test]
    fn non_object_records_use_value_column() {
        let t = table("{\"a\": 1}\n[1, 2]\n\"hello\"\n", 100);
        assert_eq!(t.columns, vec!["a", "(value)"]);
        assert_eq!(t.rows[1].cells, vec!["", "[1, 2]"]);
        assert_eq!(t.rows[2].cells, vec!["", "hello"]);
    }

    #[test]
    fn nested_values_render_compact() {
        let t = table("{\"a\": {\"x\": [1, 2]}}\n", 100);
        assert_eq!(t.rows[0].cells, vec!["{\"x\": [1, 2]}"]);
    }

    #[test]
    fn strings_are_decoded() {
        let t = table("{\"a\": \"line\\nbreak \\u00e9\"}\n", 100);
        assert_eq!(t.rows[0].cells, vec!["line\nbreak é"]);
    }

    #[test]
    fn rows_carry_source_lines_and_truncation() {
        let t = table("{\"a\": 1}\n\n{\"a\": 2}\n{\"a\": 3}\n", 2);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[1].line, 2);
        assert_eq!(t.total, 3);
        assert!(t.truncated);
    }

    #[test]
    fn broken_lines_are_skipped() {
        let t = table("{\"a\": 1}\nnot json\n{\"a\": 2}\n", 100);
        assert_eq!(t.rows.len(), 2);
    }

    #[test]
    fn empty_file_gives_empty_table() {
        let t = table("", 100);
        assert_eq!(t.rows.len(), 0);
        assert_eq!(t.total, 0);
    }
}
