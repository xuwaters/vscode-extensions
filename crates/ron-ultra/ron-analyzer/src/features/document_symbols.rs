//! Hierarchical outline: struct fields and map entries by name, list and
//! tuple items by index — the latter only when they hold structure, so a
//! list of a thousand numbers does not flood the outline.

use crate::ast::{Container, ContainerKind, Entry, EntryKey, File, Value, ValueKind};
use crate::spans::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSymbol {
    pub name: String,
    pub detail: String,
    /// A VS Code `SymbolKind` member name.
    pub kind: &'static str,
    pub range: ByteSpan,
    pub selection_range: ByteSpan,
    pub children: Vec<DocumentSymbol>,
}

pub fn document_symbols(file: &File, source: &str) -> Vec<DocumentSymbol> {
    let Some(root) = &file.value else { return Vec::new() };
    let Some(body) = root.container() else { return Vec::new() };
    let children = entries(body, source);
    match root.name() {
        // A named root is the document's one top-level symbol.
        Some(name) => vec![DocumentSymbol {
            name: name.name.clone(),
            detail: String::new(),
            kind: kind_of(root),
            range: root.span,
            selection_range: name.span,
            children,
        }],
        None => children,
    }
}

fn entries(c: &Container, source: &str) -> Vec<DocumentSymbol> {
    c.entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.key.is_some() || e.value.container().is_some())
        .map(|(i, e)| entry(e, i, source))
        .collect()
}

fn entry(e: &Entry, index: usize, source: &str) -> DocumentSymbol {
    let (name, selection_range, kind) = match &e.key {
        Some(EntryKey::Field(id)) => (id.name.clone(), id.span, field_kind(&e.value)),
        Some(EntryKey::Map(k)) => (preview(k.raw(source), 40), k.span, map_kind(&e.value)),
        None => (format!("[{index}]"), e.value.name().map_or(e.value.span, |n| n.span), kind_of(&e.value)),
    };
    DocumentSymbol {
        name,
        detail: detail(&e.value, source),
        kind,
        range: e.span(),
        selection_range,
        children: e.value.container().map(|c| entries(c, source)).unwrap_or_default(),
    }
}

fn field_kind(v: &Value) -> &'static str {
    if v.container().is_some() { kind_of(v) } else { "Field" }
}

fn map_kind(v: &Value) -> &'static str {
    if v.container().is_some() { kind_of(v) } else { "Key" }
}

fn kind_of(v: &Value) -> &'static str {
    match &v.kind {
        ValueKind::Bool(_) => "Boolean",
        ValueKind::Integer | ValueKind::Float => "Number",
        ValueKind::String { .. } | ValueKind::ByteString | ValueKind::Char => "String",
        ValueKind::Named { name, body: None } if name.name == "None" => "Null",
        ValueKind::Named { body: None, .. } => "EnumMember",
        ValueKind::Missing => "Null",
        ValueKind::Named { body: Some(c), .. } | ValueKind::Container(c) => match c.kind {
            ContainerKind::Struct => "Struct",
            ContainerKind::Map => "Object",
            ContainerKind::List | ContainerKind::Tuple => "Array",
        },
    }
}

fn detail(v: &Value, source: &str) -> String {
    match &v.kind {
        ValueKind::Named { name, body: Some(_) } => name.name.clone(),
        ValueKind::Container(c) => match c.kind {
            ContainerKind::Tuple if c.entries.is_empty() => "()".into(),
            _ => String::new(),
        },
        _ => preview(v.raw(source), 60),
    }
}

fn preview(s: &str, max: usize) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let mut out: String = collapsed.chars().take(max).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use pretty_assertions::assert_eq;

    fn outline(src: &str) -> Vec<String> {
        fn walk(syms: &[DocumentSymbol], depth: usize, out: &mut Vec<String>) {
            for s in syms {
                out.push(format!("{}{} {} [{}]", "  ".repeat(depth), s.kind, s.name, s.detail));
                walk(&s.children, depth + 1, out);
            }
        }
        let (file, _) = parse(src);
        let mut out = Vec::new();
        walk(&document_symbols(&file, src), 0, &mut out);
        out
    }

    #[test]
    fn named_root_nests_everything() {
        let src = "Config(size: (800, 600), title: \"hi\", keys: {\"up\": Up}, mode: Window(w: 1), list: [1, 2, (a: 1)], none: None)";
        assert_eq!(
            outline(src),
            vec![
                "Struct Config []",
                "  Array size []",
                "  Field title [\"hi\"]",
                "  Object keys []",
                "    Key \"up\" [Up]",
                "  Struct mode [Window]",
                "    Field w [1]",
                "  Array list []",
                "    Struct [2] []",
                "      Field a [1]",
                "  Field none [None]",
            ]
        );
    }

    #[test]
    fn anonymous_root_lists_its_entries() {
        assert_eq!(outline("(a: 1, b: [])"), vec!["Field a [1]", "Array b []"]);
        assert!(outline("42").is_empty());
        assert!(outline("").is_empty());
    }
}
