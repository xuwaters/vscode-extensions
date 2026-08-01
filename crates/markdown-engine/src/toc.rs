//! Heading collection: GitHub-compatible slugs (via comrak's `Anchorizer`,
//! run document-wide so duplicates dedupe correctly across blocks), the TOC
//! tree returned to the host, and the inline `[TOC]` block HTML.

use comrak::Anchorizer;
use comrak::nodes::{Node, NodeValue};
use serde::Serialize;

/// A heading occurrence in document order.
pub struct HeadingInfo {
    pub level: u8,
    pub text: String,
    pub slug: String,
    /// 0-based source line, matching editor lines.
    pub line: usize,
    /// Sourcepos start (1-based line, column) — the formatter's lookup key.
    pub key: (usize, usize),
}

/// TOC entry shipped to the host/webview.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TocEntry {
    pub level: u8,
    pub text: String,
    pub slug: String,
    /// 0-based source line.
    pub line: usize,
    pub children: Vec<TocEntry>,
}

/// Collect every heading (at any depth) in document order, assigning
/// document-wide unique slugs.
pub fn collect<'a>(root: Node<'a>) -> Vec<HeadingInfo> {
    let mut anchorizer = Anchorizer::new();
    let mut headings = Vec::new();
    for node in root.descendants() {
        let level = match node.data().value {
            NodeValue::Heading(ref nh) => nh.level,
            _ => continue,
        };
        let text = node.collect_text();
        let slug = anchorizer.anchorize(&text);
        let sp = node.data().sourcepos;
        headings.push(HeadingInfo {
            level,
            text,
            slug,
            line: sp.start.line.saturating_sub(1),
            key: (sp.start.line, sp.start.column),
        });
    }
    headings
}

/// Fold the flat heading list into a tree by level: a heading owns every
/// following heading of a deeper level, up to the next one at its own or a
/// shallower level.
pub fn build_tree(headings: &[HeadingInfo]) -> Vec<TocEntry> {
    fn build(headings: &[HeadingInfo], pos: &mut usize, min_level: u8) -> Vec<TocEntry> {
        let mut out = Vec::new();
        while let Some(h) = headings.get(*pos) {
            if h.level < min_level {
                break;
            }
            *pos += 1;
            out.push(TocEntry {
                level: h.level,
                text: h.text.clone(),
                slug: h.slug.clone(),
                line: h.line,
                children: build(headings, pos, h.level + 1),
            });
        }
        out
    }
    build(headings, &mut 0, 1)
}

/// Render the heading tree as the inline `[TOC]` replacement block.
pub fn inline_toc_html(tree: &[TocEntry]) -> String {
    if tree.is_empty() {
        return "<nav class=\"inline-toc\"></nav>\n".to_string();
    }
    let mut out = String::from("<nav class=\"inline-toc\">");
    write_list(&mut out, tree);
    out.push_str("</nav>\n");
    out
}

fn write_list(out: &mut String, entries: &[TocEntry]) {
    out.push_str("<ul>");
    for e in entries {
        out.push_str("<li><a href=\"#");
        out.push_str(&e.slug);
        out.push_str("\">");
        out.push_str(&escape_html(&e.text));
        out.push_str("</a>");
        if !e.children.is_empty() {
            write_list(out, &e.children);
        }
        out.push_str("</li>");
    }
    out.push_str("</ul>");
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

    fn entry(level: u8, slug: &str) -> HeadingInfo {
        HeadingInfo {
            level,
            text: slug.to_string(),
            slug: slug.to_string(),
            line: 0,
            key: (1, 1),
        }
    }

    #[test]
    fn builds_nested_tree() {
        let flat = vec![
            entry(1, "a"),
            entry(2, "b"),
            entry(3, "c"),
            entry(2, "d"),
            entry(1, "e"),
        ];
        let tree = build_tree(&flat);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].slug, "a");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].slug, "b");
        assert_eq!(tree[0].children[0].children[0].slug, "c");
        assert_eq!(tree[0].children[1].slug, "d");
        assert_eq!(tree[1].slug, "e");
    }

    #[test]
    fn tolerates_level_jumps() {
        let flat = vec![entry(3, "deep"), entry(1, "top")];
        let tree = build_tree(&flat);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].slug, "deep");
        assert_eq!(tree[1].slug, "top");
    }
}
