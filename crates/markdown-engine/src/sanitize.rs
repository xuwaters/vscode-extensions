//! Sanitization of document-supplied raw HTML, run on the AST before block
//! rendering so engine-generated markup (mermaid containers, inline TOC) can
//! bypass it.
//!
//! - `HtmlBlock` literals go through ammonia with an allowlist profile.
//! - `HtmlInline` literals are single tags per the CommonMark spec, and their
//!   open/close pairing spans multiple nodes (`<kbd>` … `</kbd>` around a text
//!   node). Running ammonia per node would auto-balance each fragment and
//!   break the pairing, so inline tags use a small single-tag scanner with the
//!   same tag/attribute allowlist instead.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::sync::OnceLock;

use comrak::nodes::{Node, NodeValue};

/// Tags allowed in document HTML, on top of structural/inline defaults.
/// (Ammonia's defaults already cover most of these; the set is spelled out so
/// the block and inline paths agree.)
const ALLOWED_TAGS: &[&str] = &[
    // Structure
    "div",
    "p",
    "blockquote",
    "pre",
    "hr",
    "br",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "caption",
    "colgroup",
    "col",
    "figure",
    "figcaption",
    "details",
    "summary",
    // Inline
    "a",
    "abbr",
    "b",
    "bdi",
    "bdo",
    "cite",
    "code",
    "data",
    "dfn",
    "em",
    "i",
    "ins",
    "del",
    "kbd",
    "mark",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "small",
    "span",
    "strong",
    "sub",
    "sup",
    "time",
    "u",
    "var",
    "wbr",
    // Media
    "img",
    "picture",
    "source",
];

/// Attributes allowed on any tag (URL-bearing ones get a scheme check).
const ALLOWED_ATTRS: &[&str] = &[
    "href", "src", "srcset", "title", "alt", "width", "height", "align", "open", "dir", "lang",
    "start", "type", "colspan", "rowspan", "datetime", "cite",
];

const ALLOWED_SCHEMES: &[&str] = &["http", "https", "mailto", "data"];

fn ammonia_builder() -> &'static ammonia::Builder<'static> {
    static BUILDER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    BUILDER.get_or_init(|| {
        let mut b = ammonia::Builder::default();
        b.tags(HashSet::from_iter(ALLOWED_TAGS.iter().copied()))
            .generic_attributes(HashSet::from_iter(ALLOWED_ATTRS.iter().copied()))
            .url_schemes(HashSet::from_iter(ALLOWED_SCHEMES.iter().copied()));
        b
    })
}

/// Sanitize every raw-HTML node in the document, in place.
pub fn sanitize_document<'a>(root: Node<'a>) {
    for node in root.descendants() {
        let replacement = {
            let data = node.data();
            match data.value {
                NodeValue::HtmlBlock(ref nhb) => {
                    Some(NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock {
                        literal: sanitize_block(&nhb.literal),
                        block_type: nhb.block_type,
                    }))
                }
                NodeValue::HtmlInline(ref literal) => {
                    Some(NodeValue::HtmlInline(sanitize_inline(literal)))
                }
                _ => None,
            }
        };
        if let Some(value) = replacement {
            node.data_mut().value = value;
        }
    }
}

/// Sanitize a raw HTML block with ammonia. Note that a block containing only
/// an opening wrapper tag (`<div align="center">`) gets auto-closed, so
/// markdown wrapped in raw-HTML wrappers loses the wrapper — same degradation
/// GitHub's sanitizer applies.
pub fn sanitize_block(html: &str) -> String {
    ammonia_builder().clean(html).to_string()
}

/// Sanitize a CommonMark *inline* HTML fragment: exactly one tag, comment, or
/// declaration. Allowed tags survive with allowlisted attributes only;
/// everything else is dropped (comments) or HTML-escaped.
pub fn sanitize_inline(fragment: &str) -> String {
    let trimmed = fragment.trim();
    // Comments, processing instructions, declarations, CDATA: drop.
    if trimmed.starts_with("<!") || trimmed.starts_with("<?") {
        return String::new();
    }
    match parse_single_tag(trimmed) {
        Some(tag) => tag,
        None => escape_html(fragment),
    }
}

/// Parse `<name attrs…>`, `</name>`, or `<name attrs…/>`; re-serialize with
/// only allowlisted attributes. Returns `None` if the fragment isn't a single
/// well-formed allowlisted tag.
fn parse_single_tag(s: &str) -> Option<String> {
    let inner = s.strip_prefix('<')?.strip_suffix('>')?;
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let (self_closing, inner) = match inner.strip_suffix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };

    let mut chars = inner.char_indices();
    let name_end = chars
        .find(|(_, c)| !c.is_ascii_alphanumeric())
        .map(|(i, _)| i)
        .unwrap_or(inner.len());
    let name = inner[..name_end].to_ascii_lowercase();
    if name.is_empty() || !ALLOWED_TAGS.contains(&name.as_str()) {
        return None;
    }
    let rest = &inner[name_end..];
    if closing {
        if !rest.trim().is_empty() {
            return None;
        }
        return Some(format!("</{}>", name));
    }

    let attrs = parse_attributes(rest)?;
    let mut out = format!("<{}", name);
    for (attr_name, attr_value) in attrs {
        let attr_lower = attr_name.to_ascii_lowercase();
        if !ALLOWED_ATTRS.contains(&attr_lower.as_str()) {
            continue;
        }
        match attr_value {
            Some(value) => {
                if matches!(attr_lower.as_str(), "href" | "src" | "srcset" | "cite")
                    && !is_safe_url(&value)
                {
                    continue;
                }
                let _ = write!(out, " {}=\"{}\"", attr_lower, escape_html(&value));
            }
            None => {
                let _ = write!(out, " {}", attr_lower);
            }
        }
    }
    if self_closing {
        out.push_str(" /");
    }
    out.push('>');
    Some(out)
}

/// Parse an attribute list; `None` if it doesn't scan cleanly.
fn parse_attributes(mut rest: &str) -> Option<Vec<(String, Option<String>)>> {
    let mut attrs = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return Some(attrs);
        }
        let name_len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':'))
            .unwrap_or(rest.len());
        if name_len == 0 {
            return None;
        }
        let name = rest[..name_len].to_string();
        rest = &rest[name_len..];
        let after = rest.trim_start();
        if let Some(value_part) = after.strip_prefix('=') {
            let value_part = value_part.trim_start();
            let (value, remaining) = if let Some(q) = value_part.strip_prefix('"') {
                let end = q.find('"')?;
                (q[..end].to_string(), &q[end + 1..])
            } else if let Some(q) = value_part.strip_prefix('\'') {
                let end = q.find('\'')?;
                (q[..end].to_string(), &q[end + 1..])
            } else {
                let end = value_part
                    .find(|c: char| c.is_whitespace())
                    .unwrap_or(value_part.len());
                if end == 0 {
                    return None;
                }
                (value_part[..end].to_string(), &value_part[end..])
            };
            attrs.push((name, Some(value)));
            rest = remaining;
        } else {
            attrs.push((name, None));
            rest = after;
        }
    }
}

/// A URL is safe if it is relative, fragment, or uses an allowlisted scheme.
fn is_safe_url(url: &str) -> bool {
    let url = url.trim();
    let Some(colon) = url.find(':') else {
        return true; // relative or fragment
    };
    // A '/', '?', or '#' before the ':' means the colon is part of the path.
    if url[..colon].contains(['/', '?', '#']) {
        return true;
    }
    let scheme = url[..colon].to_ascii_lowercase();
    ALLOWED_SCHEMES.contains(&scheme.as_str())
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_strips_scripts_and_handlers() {
        let out = sanitize_block("<div onclick=\"evil()\"><script>alert(1)</script>hi</div>");
        assert!(!out.contains("script"));
        assert!(!out.contains("onclick"));
        assert!(out.contains("hi"));
    }

    #[test]
    fn block_keeps_details() {
        let out = sanitize_block("<details open><summary>More</summary>body</details>");
        assert!(out.contains("<details"));
        assert!(out.contains("<summary>More</summary>"));
    }

    #[test]
    fn inline_keeps_allowed_pairs() {
        assert_eq!(sanitize_inline("<kbd>"), "<kbd>");
        assert_eq!(sanitize_inline("</kbd>"), "</kbd>");
    }

    #[test]
    fn inline_strips_event_handlers() {
        assert_eq!(sanitize_inline("<span onclick=\"x()\">"), "<span>");
    }

    #[test]
    fn inline_escapes_disallowed_tags() {
        assert_eq!(sanitize_inline("<script>"), "&lt;script&gt;");
        assert_eq!(
            sanitize_inline("<iframe src=\"x\">"),
            "&lt;iframe src=&quot;x&quot;&gt;"
        );
    }

    #[test]
    fn inline_blocks_javascript_urls() {
        assert_eq!(sanitize_inline("<a href=\"javascript:alert(1)\">"), "<a>");
        assert_eq!(
            sanitize_inline("<a href=\"https://example.com\">"),
            "<a href=\"https://example.com\">"
        );
        assert_eq!(
            sanitize_inline("<img src=\"./cat.png\">"),
            "<img src=\"./cat.png\">"
        );
    }

    #[test]
    fn inline_drops_comments() {
        assert_eq!(sanitize_inline("<!-- note -->"), "");
    }
}
