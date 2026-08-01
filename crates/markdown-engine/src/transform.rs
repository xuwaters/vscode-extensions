//! AST transformations that run between sanitization and block rendering.
//!
//! Currently one pass: a top-level paragraph consisting solely of `[TOC]`
//! becomes an engine-generated inline TOC block. (Mermaid fences and heading
//! ids are handled directly in the block formatter; relative URLs resolve in
//! the webview via a `<base>` element.)

use comrak::nodes::{Node, NodeValue};

/// Replace `[TOC]` marker paragraphs with the pre-rendered inline TOC HTML.
/// Uses `NodeValue::Raw` so the trusted engine output bypasses both escaping
/// and the sanitizer regardless of the `html` option.
pub fn replace_toc_markers<'a>(root: Node<'a>, inline_toc_html: &str) {
    let markers: Vec<Node<'a>> = root.children().filter(|node| is_toc_marker(node)).collect();
    for node in markers {
        for child in node.children().collect::<Vec<_>>() {
            child.detach();
        }
        node.data_mut().value = NodeValue::Raw(inline_toc_html.to_string());
    }
}

fn is_toc_marker(node: Node<'_>) -> bool {
    if !matches!(node.data().value, NodeValue::Paragraph) {
        return false;
    }
    node.collect_text().trim().eq_ignore_ascii_case("[toc]")
}
