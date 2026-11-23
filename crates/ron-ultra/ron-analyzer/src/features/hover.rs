//! Hover: where a value sits (`window.size[0]`) and what it is; on an
//! attribute, what each extension does; and, when the file names its Rust
//! type, the field, struct or variant behind what is under the cursor,
//! with its documentation.

use crate::ast::{Container, ContainerKind, EntryKey, ValueKind};
use crate::extensions;
use crate::features::locate::{path, steps_to};
use crate::rust::world::ItemRef;
use crate::schema::check::variant_of;
use crate::schema::Schema;
use crate::spans::{ByteSpan, LineCol};
use crate::workspace::ParsedFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    pub contents: String,
    pub range: ByteSpan,
}

pub fn hover(pf: &ParsedFile, schema: Option<&Schema>, pos: LineCol) -> Option<Hover> {
    let offset = pf.spans.line_col_to_offset(&pf.source, pos);
    if let Some(h) = schema.and_then(|s| annotation_hover(s, offset)) {
        return Some(h);
    }
    let base = attribute_hover(pf, offset).or_else(|| value_hover(pf, offset))?;
    match schema.and_then(|s| rust_info(s, offset, base.range)) {
        Some(info) => Some(Hover { contents: format!("{info}\n\n---\n\n{}", base.contents), range: base.range }),
        None => Some(base),
    }
}

fn annotation_hover(schema: &Schema, offset: u32) -> Option<Hover> {
    let ann = schema.annotation.as_ref().filter(|a| a.span.contains_strict(offset))?;
    let contents = match (&schema.target, &schema.root) {
        (Some(target), _) => item_info(target, schema),
        (None, Some(root)) => format!("```rust\n{root}\n```"),
        (None, None) => return None,
    };
    Some(Hover { contents, range: ann.span })
}

/// The Rust side of what sits at `offset`: a field, a struct or variant
/// name, or a value's expected type.
fn rust_info(schema: &Schema, offset: u32, range: ByteSpan) -> Option<String> {
    if let Some(k) = schema.facts.keys.iter().find(|k| k.span.contains_strict(offset)) {
        let field = k.expected.field();
        let owner = k.expected.owner.item().name();
        let owner = match k.expected.variant.and_then(|v| variant_of(&k.expected.owner, v)) {
            Some(var) => format!("{owner}::{}", var.name.name),
            None => owner.to_string(),
        };
        let name = field.name.as_ref().map_or("", |n| n.name.as_str());
        return Some(with_docs(format!("```rust\n{owner}.{name}: {}\n```", field.ty_text), &field.docs));
    }
    if let Some(n) = schema.facts.names.iter().find(|n| n.span.contains_strict(offset)) {
        return Some(match n.variant.and_then(|v| variant_of(&n.owner, v)) {
            Some(var) => with_docs(format!("```rust\n{}::{}\n```", n.owner.item().name(), var.name.name), &var.docs),
            None => item_info(&n.owner, schema),
        });
    }
    // A scalar or container: its expected type.
    let (_, ty) = schema.facts.values.iter().rev().find(|(span, _)| *span == range || span.contains_strict(offset) && span.len() <= range.len())?;
    if matches!(ty, crate::schema::ty::Ty::Any) {
        return None;
    }
    Some(format!("```rust\n{ty}\n```"))
}

fn item_info(item: &ItemRef, schema: &Schema) -> String {
    let def = item.item();
    let module = item.module.display(schema.crates.first().map(|c| c.as_ref()));
    with_docs(format!("```rust\n{module}\n{} {}\n```", def.keyword(), def.name()), &def.docs)
}

fn with_docs(code: String, docs: &str) -> String {
    if docs.is_empty() { code } else { format!("{code}\n\n{docs}") }
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
        hover(&pf, None, pos).map(|h| h.contents)
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

    fn schema_hover_at(src: &str, needle: &str) -> Option<String> {
        let (pf, schema, _) = crate::schema::tests::analyzed(src);
        let offset = src.find(needle).expect("needle") as u32 + 1;
        let pos = pf.spans.offset_to_line_col(src, offset);
        hover(&pf, Some(&schema), pos).map(|h| h.contents)
    }

    const TYPED: &str = "/* @[crate::config::GameConfig] */\nGameConfig(window_size: (8, 6), max_fps: 60, difficulty: (start: Easy, adaptive: true))";

    #[test]
    fn rust_types_and_docs() {
        assert_eq!(
            schema_hover_at(TYPED, "window_size").unwrap(),
            "```rust\nGameConfig.window_size: (u32, u32)\n```\n\nWindow size in pixels.\n\n---\n\n`window_size` · tuple · 2 items"
        );
        assert_eq!(schema_hover_at(TYPED, "60").unwrap(), "```rust\nu8\n```\n\n---\n\n`max_fps` · integer");
        assert_eq!(schema_hover_at(TYPED, "Easy").unwrap(), "```rust\nLevel::Easy\n```\n\n---\n\n`difficulty.start` · unit struct or enum variant `Easy`");
        assert_eq!(
            schema_hover_at(TYPED, "config::Game").unwrap(),
            "```rust\ncrate::config\nstruct GameConfig\n```\n\nThe whole game's settings."
        );
    }
}
