//! Hover: the JSON path under the cursor, plus the value's type and a
//! short preview.

use crate::ast::{Ast, Value, ValueKind};
use crate::spans::ByteSpan;
use crate::workspace::ParsedFile;
use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub struct HoverInfo {
    /// Markdown.
    pub contents: String,
    pub range: ByteSpan,
}

pub fn hover(pf: &ParsedFile, offset: u32) -> Option<HoverInfo> {
    let (value, path, range) = match &pf.ast {
        Ast::Single(root) => locate(root.value.as_ref()?, offset, String::from("$"))?,
        Ast::Lines(records) => {
            let record = records
                .iter()
                .find(|r| r.value.span.contains(offset))?;
            locate(&record.value, offset, String::from("$"))?
        }
    };
    let mut contents = format!("`{path}` — *{}*", value.type_name());
    match &value.kind {
        ValueKind::Object(o) => {
            let _ = write!(contents, ", {} members", o.members.len());
        }
        ValueKind::Array(a) => {
            let _ = write!(contents, ", {} items", a.elements.len());
        }
        ValueKind::Missing => return None,
        _ => {
            let raw = value.raw(&pf.source);
            if raw.len() <= 120 && !raw.contains('\n') {
                let _ = write!(contents, "\n\n```\n{raw}\n```");
            }
        }
    }
    Some(HoverInfo { contents, range })
}

/// Deepest node containing `offset`. Returns the node, its path, and
/// the span to highlight — the key span when the cursor sits on a key.
fn locate(value: &Value, offset: u32, path: String) -> Option<(&Value, String, ByteSpan)> {
    if !value.span.contains(offset) {
        return None;
    }
    match &value.kind {
        ValueKind::Object(object) => {
            for member in &object.members {
                let child_path = format!("{path}{}", segment(&member.key.name));
                if member.key.span.contains(offset) {
                    return Some((&member.value, child_path, member.key.span));
                }
                if let Some(hit) = locate(&member.value, offset, child_path) {
                    return Some(hit);
                }
            }
            Some((value, path, value.span))
        }
        ValueKind::Array(array) => {
            for (i, element) in array.elements.iter().enumerate() {
                if let Some(hit) = locate(&element.value, offset, format!("{path}[{i}]")) {
                    return Some(hit);
                }
            }
            Some((value, path, value.span))
        }
        _ => Some((value, path, value.span)),
    }
}

/// Path segments that are not identifier-shaped go through bracket
/// notation (separator included) so the path stays copy-pasteable.
fn segment(name: &str) -> String {
    let ident = !name.is_empty()
        && name
            .chars()
            .enumerate()
            .all(|(i, c)| c == '_' || c == '$' || c.is_alphabetic() || (i > 0 && c.is_numeric()));
    if ident {
        format!(".{name}")
    } else {
        format!("[\"{}\"]", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flavor::Flavor;
    use crate::workspace::{parse, FileUri};

    fn hover_at(src: &str, flavor: Flavor, offset: u32) -> Option<HoverInfo> {
        hover(&parse(FileUri::new("t"), src.to_string(), flavor), offset)
    }

    #[test]
    fn hover_on_nested_value_shows_path() {
        let src = r#"{"a": {"b": [10, 20]}}"#;
        let h = hover_at(src, Flavor::Json, 17).expect("hover"); // inside `20`
        assert!(h.contents.contains("$.a.b[1]"), "{}", h.contents);
        assert!(h.contents.contains("number"), "{}", h.contents);
    }

    #[test]
    fn hover_on_key_highlights_the_key() {
        let src = r#"{"alpha": 1}"#;
        let h = hover_at(src, Flavor::Json, 3).expect("hover");
        assert!(h.contents.contains("$.alpha"), "{}", h.contents);
        assert_eq!(h.range, ByteSpan::new(1, 8));
    }

    #[test]
    fn odd_keys_use_bracket_notation() {
        let src = r#"{"a key": 1}"#;
        let h = hover_at(src, Flavor::Json, 10).expect("hover");
        assert!(h.contents.contains(r#"$["a key"]"#), "{}", h.contents);
    }

    #[test]
    fn jsonl_hover_works_per_record() {
        let src = "{\"a\": 1}\n{\"b\": 2}\n";
        let h = hover_at(src, Flavor::Jsonl, 15).expect("hover"); // in second record
        assert!(h.contents.contains("$.b"), "{}", h.contents);
    }

    #[test]
    fn hover_outside_any_value_is_none() {
        assert!(hover_at("  42", Flavor::Json, 0).is_none());
    }
}
