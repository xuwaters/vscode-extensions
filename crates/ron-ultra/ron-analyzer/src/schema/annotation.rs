//! The `@[path::to::Type]` comment that ties a RON file to a Rust type.
//!
//! It may sit in any comment before the top-level value — line or block,
//! before or after `#![enable(...)]` — and the first one wins:
//!
//! ```ron
//! /* @[crate::config::GameConfig] */
//! GameConfig(...)
//! ```
//!
//! What is inside the brackets is a Rust type, so `@[Vec<crate::Level>]`
//! works too. Paths resolve from the root of the crate whose `Cargo.toml`
//! is nearest above the RON file.

use crate::ast::File;
use crate::lexer::{lex, TokenKind};
use crate::spans::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// The text between `@[` and `]`, trimmed.
    pub text: String,
    /// Where that text sits in the RON file.
    pub span: ByteSpan,
    /// Where the editable region ends: the `]`, or the end of the comment
    /// while the annotation is still being typed. Completion works up to
    /// here.
    pub end: u32,
    /// The `]` is there.
    pub closed: bool,
}

pub fn find(source: &str, file: &File) -> Option<Annotation> {
    let limit = file.value.as_ref().map_or(source.len() as u32, |v| v.span.start);
    for tok in lex(source).tokens {
        if tok.span.start >= limit {
            break;
        }
        if !tok.kind.is_comment() {
            continue;
        }
        let comment = &source[tok.span.start as usize..tok.span.end as usize];
        let Some(at) = comment.find("@[") else { continue };
        let body_start = at + 2;
        let body_end_limit = match tok.kind {
            TokenKind::BlockComment if comment.ends_with("*/") && comment.len() >= 4 => comment.len() - 2,
            _ => comment.len(),
        }
        .max(body_start);
        let (body_end, closed) = match comment[body_start..body_end_limit].find(']') {
            Some(i) => (body_start + nested_close(&comment[body_start..body_end_limit]).unwrap_or(i), true),
            None => (body_end_limit, false),
        };
        let raw = &comment[body_start..body_end];
        let lead = raw.len() - raw.trim_start().len();
        let text = raw.trim();
        let start = tok.span.start + (body_start + lead) as u32;
        return Some(Annotation {
            text: text.to_string(),
            span: ByteSpan::new(start, start + text.len() as u32),
            end: tok.span.start + body_end as u32,
            closed,
        });
    }
    None
}

/// The `]` closing the annotation, skipping any inside the type itself,
/// as in `@[[u8; 4]]`.
fn nested_close(body: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in body.char_indices() {
        match c {
            '[' => depth += 1,
            ']' if depth == 0 => return Some(i),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn ann(src: &str) -> Option<Annotation> {
        let (file, _) = parse(src);
        find(src, &file)
    }

    #[test]
    fn block_and_line_comments() {
        let src = "/* @[crate::config::GameConfig] */\nGameConfig()";
        let a = ann(src).unwrap();
        assert_eq!(a.text, "crate::config::GameConfig");
        assert_eq!(&src[a.span.start as usize..a.span.end as usize], a.text);
        assert!(a.closed);
        assert_eq!(ann("#![enable(implicit_some)]\n// schema: @[ Config ]\n()").unwrap().text, "Config");
    }

    #[test]
    fn only_before_the_value() {
        assert_eq!(ann("(a: 1) // @[Config]"), None);
        assert_eq!(ann("(a: /* @[Config] */ 1)"), None);
    }

    #[test]
    fn nested_brackets_and_unfinished() {
        assert_eq!(ann("// @[[u8; 4]]\n()").unwrap().text, "[u8; 4]");
        let a = ann("/* @[crate::con").unwrap();
        assert_eq!(a.text, "crate::con");
        assert!(!a.closed);
        assert_eq!(a.end, 15);
    }
}
