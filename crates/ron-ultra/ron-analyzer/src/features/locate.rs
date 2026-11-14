//! Walking from the root to whatever sits under the cursor, and naming the
//! way there as a path such as `window.size[0]`.

use crate::ast::{Entry, EntryKey, File, Value};
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Field(String),
    Key(String),
    Index(usize),
}

/// One level of the walk: the value reached, and the entry that holds it
/// (`None` for the root).
#[derive(Debug, Clone, Copy)]
pub struct Step<'a> {
    pub entry: Option<(&'a Entry, usize)>,
    pub value: &'a Value,
}

impl<'a> Step<'a> {
    pub fn segment(&self, source: &str) -> Option<Segment> {
        let (entry, index) = self.entry?;
        Some(match &entry.key {
            Some(EntryKey::Field(id)) => Segment::Field(id.name.clone()),
            Some(EntryKey::Map(k)) => Segment::Key(k.raw(source).to_string()),
            None => Segment::Index(index),
        })
    }
}

/// Root first, innermost last; empty when the offset is outside the value.
///
/// `strict` hit-tests half-open spans, for a character under the mouse
/// (hover); otherwise a span also claims the offset at its end, for a
/// cursor that sits between characters (smart select).
pub fn steps_to(file: &File, offset: u32, strict: bool) -> Vec<Step<'_>> {
    let hit = |s: crate::spans::ByteSpan| if strict { s.contains_strict(offset) } else { s.contains(offset) };
    let Some(root) = &file.value else { return Vec::new() };
    if !hit(root.span) {
        return Vec::new();
    }
    let mut steps = vec![Step { entry: None, value: root }];
    let mut current = root;
    'descend: while let Some(c) = current.container() {
        for (i, e) in c.entries.iter().enumerate() {
            if hit(e.span()) {
                steps.push(Step { entry: Some((e, i)), value: &e.value });
                current = &e.value;
                continue 'descend;
            }
        }
        break;
    }
    steps
}

pub fn path(steps: &[Step<'_>], source: &str) -> String {
    let mut out = String::new();
    for step in steps {
        match step.segment(source) {
            Some(Segment::Field(name)) if out.is_empty() => out.push_str(&name),
            Some(Segment::Field(name)) => {
                let _ = write!(out, ".{name}");
            }
            Some(Segment::Key(key)) => {
                let _ = write!(out, "[{key}]");
            }
            Some(Segment::Index(i)) => {
                let _ = write!(out, "[{i}]");
            }
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn path_at(src: &str, needle: &str) -> String {
        let (file, _) = parse(src);
        let offset = src.find(needle).expect("needle") as u32;
        path(&steps_to(&file, offset, true), src)
    }

    #[test]
    fn paths() {
        let src = "Config(window: (size: (800, 600)), keys: {\"up\": [Up, W]}, list: [(a: 1)])";
        assert_eq!(path_at(src, "600"), "window.size[1]");
        assert_eq!(path_at(src, "W]"), "keys[\"up\"][1]");
        assert_eq!(path_at(src, "a: 1"), "list[0].a");
        assert_eq!(path_at(src, "keys"), "keys");
        assert_eq!(path_at(src, "Config"), "");
    }

    #[test]
    fn outside_the_value_is_nowhere() {
        let (file, _) = parse("// c\n(a: 1)");
        assert!(steps_to(&file, 1, false).is_empty());
    }
}
