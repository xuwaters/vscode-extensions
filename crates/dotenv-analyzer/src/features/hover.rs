//! Hover provider — shows the assigned value when hovering a key
//! definition, and the referenced key's value when hovering inside
//! `$NAME` / `${NAME}`.

use crate::ast::{Entry, File, VarRef};
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
    for entry in &parsed.ast.entries {
        let Entry::Assignment(a) = entry else { continue };
        if a.name.span.contains(cursor) {
            return Some(definition_hover(&a.name.name, &parsed.ast, &parsed.source, a.name.span));
        }
        for r in &a.references {
            if r.span.contains(cursor) {
                return Some(reference_hover(r, &parsed.ast, &parsed.source));
            }
        }
    }
    None
}

fn definition_hover(name: &str, file: &File, source: &str, range: ByteSpan) -> Hover {
    let value = find_value(file, source, name).unwrap_or("").to_string();
    Hover {
        contents: format!("**{name}** = `{value}`"),
        range,
    }
}

fn reference_hover(r: &VarRef, file: &File, source: &str) -> Hover {
    let value = find_value(file, source, &r.name);
    let body = match value {
        Some(v) => format!("**{}** = `{}`", r.name, v),
        None => format!("**{}** — not defined in this file", r.name),
    };
    Hover { contents: body, range: r.span }
}

fn find_value<'a>(file: &File, source: &'a str, name: &str) -> Option<&'a str> {
    for entry in &file.entries {
        if let Entry::Assignment(a) = entry {
            if a.name.name == name {
                return Some(&source[a.value_span.start as usize..a.value_span.end as usize]);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    #[test]
    fn hover_on_definition() {
        let pf = parse(FileUri::new("t"), "FOO=bar\n".to_string());
        let h = hover(&pf, LineCol { line: 0, col: 1 }).unwrap();
        assert!(h.contents.contains("FOO"));
        assert!(h.contents.contains("bar"));
    }

    #[test]
    fn hover_on_reference_resolves_value() {
        let pf = parse(FileUri::new("t"), "FOO=bar\nB=$FOO\n".to_string());
        // Cursor inside `$FOO` on line 1 (col 3 — right after `$`).
        let h = hover(&pf, LineCol { line: 1, col: 3 }).unwrap();
        assert!(h.contents.contains("FOO"));
        assert!(h.contents.contains("bar"));
    }
}
