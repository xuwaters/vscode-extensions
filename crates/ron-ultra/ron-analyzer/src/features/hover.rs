//! Hover: where a value sits (`window.size[0]`) and what it is; and, on an
//! attribute, what each extension does.

use crate::ast::{Container, ContainerKind, EntryKey, ValueKind};
use crate::extensions;
use crate::features::locate::{path, steps_to};
use crate::spans::{ByteSpan, LineCol};
use crate::workspace::ParsedFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    pub contents: String,
    pub range: ByteSpan,
}

pub fn hover(pf: &ParsedFile, pos: LineCol) -> Option<Hover> {
    let offset = pf.spans.line_col_to_offset(&pf.source, pos);
    attribute_hover(pf, offset).or_else(|| value_hover(pf, offset))
}

fn attribute_hover(pf: &ParsedFile, offset: u32) -> Option<Hover> {
    let attr = pf.ast.attributes.iter().find(|a| a.span.contains_strict(offset))?;
    if let Some(ext) = attr.extensions.iter().find(|e| e.span.contains_strict(offset)) {
        let contents = match extensions::find(&ext.name) {
            Some(known) => format!("**{}** — RON extension\n\n{}", known.name, known.summary),
            None => format!("**{}** — not a RON extension", ext.name),
        };
        return Some(Hover { contents, range: ext.span });
    }
    let name = attr.name.as_ref().filter(|n| n.span.contains_strict(offset))?;
    let contents = if name.name == "enable" {
        "**enable** — switches on RON extensions for this file".to_string()
    } else {
        format!("**{}** — attribute", name.name)
    };
    Some(Hover { contents, range: name.span })
}

fn value_hover(pf: &ParsedFile, offset: u32) -> Option<Hover> {
    let steps = steps_to(&pf.ast, offset, true);
    let last = steps.last()?;
    let value = last.value;
    let key_span = last.entry.and_then(|(e, _)| match &e.key {
        Some(EntryKey::Field(id)) => Some(id.span),
        Some(EntryKey::Map(k)) => Some(k.span),
        None => None,
    });
    // Over a key, a scalar, a name or a bracket — not the whitespace and
    // commas between a container's entries.
    let range = if let Some(span) = key_span.filter(|s| s.contains_strict(offset)) {
        span
    } else if let Some(c) = value.container() {
        let on_name = value.name().map(|n| n.span).filter(|s| s.contains_strict(offset));
        let on_bracket = [Some(c.open), c.close].into_iter().flatten().find(|s| s.contains_strict(offset));
        on_name.or(on_bracket)?
    } else if matches!(value.kind, ValueKind::Missing) {
        return None;
    } else {
        value.span
    };
    let mut description = value.describe();
    if let Some(c) = value.container() {
        if let Some(count) = count(c) {
            description.push_str(" · ");
            description.push_str(&count);
        }
    }
    let path = path(&steps, &pf.source);
    let contents = if path.is_empty() { description } else { format!("`{path}` · {description}") };
    Some(Hover { contents, range })
}

fn count(c: &Container) -> Option<String> {
    let n = c.entries.len();
    let noun = match c.kind {
        ContainerKind::Tuple if n == 0 => return None,
        ContainerKind::Struct => "field",
        ContainerKind::Map => "entry",
        ContainerKind::List | ContainerKind::Tuple => "item",
    };
    let plural = match (n, noun) {
        (1, _) => noun.to_string(),
        (_, "entry") => "entries".to_string(),
        _ => format!("{noun}s"),
    };
    Some(format!("{n} {plural}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{parse, FileUri};
    use pretty_assertions::assert_eq;

    fn hover_at(src: &str, needle: &str, delta: u32) -> Option<String> {
        let pf = parse(FileUri::new("t"), src.to_string());
        let offset = src.find(needle).expect("needle") as u32 + delta;
        let pos = pf.spans.offset_to_line_col(src, offset);
        hover(&pf, pos).map(|h| h.contents)
    }

    const SRC: &str = "#![enable(implicit_some)]\nConfig(\n    size: (800, 600),\n    keys: {\"up\": Up},\n    tags: [],\n    name: None,\n)";

    #[test]
    fn scalars_show_path_and_type() {
        assert_eq!(hover_at(SRC, "600", 1).unwrap(), "`size[1]` · integer");
        assert_eq!(hover_at(SRC, "Up}", 0).unwrap(), "`keys[\"up\"]` · unit struct or enum variant `Up`");
        assert_eq!(hover_at(SRC, "None", 0).unwrap(), "`name` · option `None`");
    }

    #[test]
    fn keys_describe_their_value() {
        assert_eq!(hover_at(SRC, "size", 0).unwrap(), "`size` · tuple · 2 items");
        assert_eq!(hover_at(SRC, "keys", 0).unwrap(), "`keys` · map · 1 entry");
        assert_eq!(hover_at(SRC, "tags", 0).unwrap(), "`tags` · list · 0 items");
    }

    #[test]
    fn names_and_brackets() {
        assert_eq!(hover_at(SRC, "Config", 2).unwrap(), "struct `Config` · 4 fields");
        assert_eq!(hover_at(SRC, "(800", 0).unwrap(), "`size` · tuple · 2 items");
    }

    #[test]
    fn whitespace_between_entries_is_quiet() {
        assert_eq!(hover_at(SRC, ",\n    keys", 0), None);
        assert_eq!(hover_at(SRC, "\n    keys", 2), None);
    }

    #[test]
    fn extensions() {
        let h = hover_at(SRC, "implicit_some", 3).unwrap();
        assert!(h.starts_with("**implicit_some** — RON extension"), "{h}");
        assert!(hover_at(SRC, "enable", 1).unwrap().contains("switches on"));
        assert_eq!(hover_at("#![enable(nope)] 1", "nope", 0).unwrap(), "**nope** — not a RON extension");
    }
}
