//! Go to definition: from a RON key, struct name or variant — or the
//! `@[...]` annotation — to its declaration in the Rust source.

use crate::rust::syntax::Item;
use crate::rust::world::{ItemRef, RustFile};
use crate::schema::check::variant_of;
use crate::schema::Schema;
use crate::spans::{ByteSpan, LineCol};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub uri: String,
    pub start: LineCol,
    pub end: LineCol,
}

pub fn definition(schema: &Schema, offset: u32) -> Option<Location> {
    if let (Some(ann), Some(target)) = (&schema.annotation, &schema.target) {
        if ann.span.contains(offset) {
            return Some(item_location(target));
        }
    }
    if let Some(k) = schema.facts.keys.iter().find(|k| k.span.contains(offset)) {
        let field = k.expected.field();
        let span = field.name.as_ref().map_or(field.span, |n| n.span);
        return Some(location(&k.expected.owner.file, span));
    }
    let n = schema.facts.names.iter().find(|n| n.span.contains(offset))?;
    match n.variant.and_then(|v| variant_of(&n.owner, v)) {
        Some(var) => Some(location(&n.owner.file, var.name.span)),
        None => Some(item_location(&n.owner)),
    }
}

fn item_location(item: &ItemRef) -> Location {
    let def: &Item = item.item();
    location(&item.file, def.name.as_ref().map_or(def.span, |n| n.span))
}

fn location(file: &Rc<RustFile>, span: ByteSpan) -> Location {
    Location {
        uri: file.uri.clone(),
        start: file.spans.offset_to_line_col(&file.source, span.start),
        end: file.spans.offset_to_line_col(&file.source, span.end),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::{analyzed, CONFIG};

    fn def_at(src: &str, needle: &str) -> Option<(String, String)> {
        let (_, schema, _) = analyzed(src);
        let offset = src.find(needle).expect("needle") as u32 + 1;
        let loc = definition(&schema, offset)?;
        let line = CONFIG.lines().nth(loc.start.line as usize).unwrap_or("").trim().to_string();
        let word = CONFIG.lines().nth(loc.start.line as usize).unwrap_or("")[loc.start.col as usize..loc.end.col as usize].to_string();
        Some((word, line))
    }

    const SRC: &str = "/* @[crate::config::GameConfig] */\nGameConfig(fullscreen: true, difficulty: (start: Hard, adaptive: true))";

    #[test]
    fn keys_names_variants_and_the_annotation() {
        assert_eq!(def_at(SRC, "fullscreen").unwrap(), ("fullscreen".into(), "pub fullscreen: bool,".into()));
        assert_eq!(def_at(SRC, "Hard").unwrap().0, "Hard");
        assert_eq!(def_at(SRC, "GameConfig(").unwrap().0, "GameConfig");
        assert_eq!(def_at(SRC, "config::Game").unwrap().0, "GameConfig");
        assert_eq!(def_at(SRC, "start").unwrap().0, "start");
        assert_eq!(def_at(SRC, "true"), None);
    }
}
