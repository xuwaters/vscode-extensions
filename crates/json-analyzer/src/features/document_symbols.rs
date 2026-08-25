//! Outline tree: object members and array elements down to scalars.

use crate::ast::{Ast, Value, ValueKind};
use crate::spans::ByteSpan;
use crate::workspace::ParsedFile;

/// Overall node budget so a machine-generated file cannot melt the
/// outline view. Depth-first, so the beginning of the file wins.
const MAX_SYMBOLS: usize = 20_000;

#[derive(Debug, Clone)]
pub struct DocSymbol {
    pub name: String,
    pub detail: String,
    /// One of `object array string number boolean null` — mapped to
    /// `vscode.SymbolKind` on the TypeScript side.
    pub kind: &'static str,
    pub range: ByteSpan,
    pub selection: ByteSpan,
    pub children: Vec<DocSymbol>,
}

pub fn document_symbols(pf: &ParsedFile) -> Vec<DocSymbol> {
    let mut budget = MAX_SYMBOLS;
    match &pf.ast {
        Ast::Single(root) => match &root.value {
            Some(value) => children_of(value, &pf.source, &mut budget),
            None => Vec::new(),
        },
        Ast::Lines(records) => {
            let mut out = Vec::new();
            for record in records {
                if budget == 0 {
                    break;
                }
                budget -= 1;
                out.push(DocSymbol {
                    name: format!("Line {}", record.line + 1),
                    detail: preview(&record.value, &pf.source),
                    kind: kind_of(&record.value),
                    range: record.value.span,
                    selection: record.value.span,
                    children: children_of(&record.value, &pf.source, &mut budget),
                });
            }
            out
        }
    }
}

fn kind_of(value: &Value) -> &'static str {
    match &value.kind {
        ValueKind::Object(_) => "object",
        ValueKind::Array(_) => "array",
        ValueKind::String(_) => "string",
        ValueKind::Number => "number",
        ValueKind::Bool(_) => "boolean",
        _ => "null",
    }
}

/// A short scalar preview for the outline's detail column.
fn preview(value: &Value, source: &str) -> String {
    match &value.kind {
        ValueKind::Object(o) => format!("{{…}} {} members", o.members.len()),
        ValueKind::Array(a) => format!("[…] {} items", a.elements.len()),
        ValueKind::Missing => String::new(),
        _ => {
            let raw = value.raw(source);
            if raw.chars().count() > 40 {
                let cut: String = raw.chars().take(39).collect();
                format!("{cut}…")
            } else {
                raw.to_string()
            }
        }
    }
}

fn children_of(value: &Value, source: &str, budget: &mut usize) -> Vec<DocSymbol> {
    let mut out = Vec::new();
    match &value.kind {
        ValueKind::Object(object) => {
            for member in &object.members {
                if *budget == 0 {
                    break;
                }
                *budget -= 1;
                let range = ByteSpan::new(member.key.span.start, member.value.span.end);
                out.push(DocSymbol {
                    name: member.key.name.clone(),
                    detail: preview(&member.value, source),
                    kind: kind_of(&member.value),
                    range,
                    selection: member.key.span,
                    children: children_of(&member.value, source, budget),
                });
            }
        }
        ValueKind::Array(array) => {
            for (i, element) in array.elements.iter().enumerate() {
                if *budget == 0 {
                    break;
                }
                *budget -= 1;
                out.push(DocSymbol {
                    name: format!("[{i}]"),
                    detail: preview(&element.value, source),
                    kind: kind_of(&element.value),
                    range: element.value.span,
                    selection: element.value.span,
                    children: children_of(&element.value, source, budget),
                });
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flavor::Flavor;
    use crate::workspace::{parse, FileUri};

    fn symbols(src: &str, flavor: Flavor) -> Vec<DocSymbol> {
        document_symbols(&parse(FileUri::new("t"), src.to_string(), flavor))
    }

    #[test]
    fn nested_members_become_a_tree() {
        let s = symbols(r#"{"a": {"b": [1, 2]}, "c": "x"}"#, Flavor::Json);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "a");
        assert_eq!(s[0].kind, "object");
        assert_eq!(s[0].children.len(), 1);
        assert_eq!(s[0].children[0].name, "b");
        assert_eq!(s[0].children[0].children.len(), 2);
        assert_eq!(s[0].children[0].children[0].name, "[0]");
        assert_eq!(s[1].kind, "string");
    }

    #[test]
    fn jsonl_records_become_lines() {
        let s = symbols("{\"a\": 1}\n[2]\n", Flavor::Jsonl);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "Line 1");
        assert_eq!(s[1].name, "Line 2");
        assert_eq!(s[1].kind, "array");
    }

    #[test]
    fn scalar_root_has_no_symbols() {
        assert!(symbols("42", Flavor::Json).is_empty());
    }
}
