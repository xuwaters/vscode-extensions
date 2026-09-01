//! Reference-page prose → hover-ready markdown.
//!
//! The markup inside a `description` or `parameters` section is a small closed
//! set, counted in `research/docs-gl.md` §6, and every member of it has an
//! obvious markdown equivalent except two: MathML and the one embedded table,
//! which cannot be rendered honestly in a hover and are replaced by a note.
//!
//! Output is capped per entry. The reference pages run to 3.8 KB in the worst
//! case and a hover that long is a wall; whole paragraphs are kept until the
//! budget is spent and the rest is dropped with an ellipsis, so the text always
//! ends where a sentence does.

use crate::page::{collapse, raw_text};

/// The most markdown one entry may contribute to the pool.
const BUDGET: usize = 1200;

/// Appended when paragraphs were dropped for the budget.
const ELLIPSIS: &str = "\n\n…";

/// Appended when something unrenderable — a formula, a table — was dropped.
const OMITTED: &str = "\n\n*(formulas and tables omitted — see the reference page)*";

/// Render a section's children as markdown.
pub fn render(section: roxmltree::Node<'_, '_>) -> String {
    let mut blocks = Vec::new();
    let mut omitted = false;
    for child in section.children() {
        if !child.is_element() {
            continue;
        }
        match child.tag_name().name() {
            // The section's own `<h2>Description</h2>` heading.
            "h2" => {}
            "p" => {
                let text = inline(child, &mut omitted);
                if !text.trim().is_empty() {
                    blocks.push(text.trim().to_string());
                }
            }
            "pre" => {
                let code = dedent(raw_text(child).trim_matches('\n'));
                if !code.trim().is_empty() {
                    blocks.push(format!("```glsl\n{code}\n```"));
                }
            }
            "div" if child.attribute("class") == Some("itemizedlist") => {
                let items: Vec<String> = child
                    .descendants()
                    .filter(|n| n.is_element() && n.tag_name().name() == "li")
                    .map(|li| format!("- {}", inline(li, &mut omitted).trim()))
                    .collect();
                if !items.is_empty() {
                    blocks.push(items.join("\n"));
                }
            }
            // An embedded table, or anything else structural we will not try to
            // reproduce in a hover.
            _ => omitted = true,
        }
    }
    finish(blocks, omitted)
}

/// Render one paragraph-like element's inline content.
pub fn inline(node: roxmltree::Node<'_, '_>, omitted: &mut bool) -> String {
    let mut out = String::new();
    render_inline(node, &mut out, omitted);
    tex(&collapse(&out))
}

/// One paragraph rendered on its own, for a parameter's documentation.
pub fn render_paragraphs(node: roxmltree::Node<'_, '_>) -> String {
    let mut omitted = false;
    let blocks: Vec<String> = node
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
        .map(|p| inline(p, &mut omitted))
        .filter(|t| !t.trim().is_empty())
        .collect();
    finish(blocks, omitted)
}

/// Join blocks within the budget, marking what was dropped.
fn finish(blocks: Vec<String>, omitted: bool) -> String {
    let mut out = String::new();
    let mut dropped = false;
    for block in blocks {
        if out.is_empty() {
            out = block;
        } else if out.len() + 2 + block.len() <= BUDGET {
            out.push_str("\n\n");
            out.push_str(&block);
        } else {
            dropped = true;
        }
    }
    if out.len() > BUDGET {
        out = truncate(&out, BUDGET);
        dropped = true;
    }
    if dropped {
        out.push_str(ELLIPSIS);
    }
    if omitted {
        out.push_str(OMITTED);
    }
    out
}

fn render_inline(node: roxmltree::Node<'_, '_>, out: &mut String, omitted: &mut bool) {
    for child in node.children() {
        if child.is_text() {
            out.push_str(child.text().unwrap_or_default());
            continue;
        }
        if !child.is_element() {
            continue;
        }
        match child.tag_name().name() {
            // MathML. Nothing honest to render inline; note it and move on.
            "math" => {
                *omitted = true;
                out.push('…');
            }
            // `<code class="function">mix</code>`, `<em
            // class="parameter"><code>x</code></em>`, `<code
            // class="varname">gl_FragCoord</code>` — all one thing in markdown.
            "code" | "var" => {
                let text = collapse(&plain(child));
                if !text.is_empty() {
                    out.push('`');
                    out.push_str(&text);
                    out.push('`');
                }
            }
            "em" => {
                // `em.parameter` wraps a `<code>`; anything else is emphasis.
                if child.attribute("class") == Some("parameter") {
                    render_inline(child, out, omitted);
                } else {
                    let text = collapse(&plain(child));
                    if !text.is_empty() {
                        out.push('*');
                        out.push_str(&text);
                        out.push('*');
                    }
                }
            }
            "span" if child.attribute("class") == Some("emphasis") => {
                let text = collapse(&plain(child));
                if !text.is_empty() {
                    out.push('*');
                    out.push_str(&text);
                    out.push('*');
                }
            }
            // A cross-reference to another reference page. The link target is a
            // relative docs.gl URL and useless in a hover, so only the name of
            // the thing survives, as a code span.
            "a" if child.attribute("class") == Some("citerefentry") => {
                let text = collapse(&plain(child));
                if !text.is_empty() {
                    out.push('`');
                    out.push_str(&text);
                    out.push('`');
                }
            }
            _ => render_inline(child, out, omitted),
        }
    }
}

/// An element's text with no markup at all.
fn plain(node: roxmltree::Node<'_, '_>) -> String {
    raw_text(node)
}

/// The reference pages' three TeX spans (`sl4/mix.xhtml`, `dFdx`, `fwidth`):
/// `$x$` is a variable and `$x \times y$` is a product. Rendering them as code
/// spans with the operator spelled out is closer to the truth than leaving the
/// dollar signs in a hover.
fn tex(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('$') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let math = after[..end].replace("\\times", "*").replace("\\cdot", "*");
        out.push('`');
        out.push_str(&collapse(&math));
        out.push('`');
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Strip the common leading indentation from a code listing.
fn dedent(code: &str) -> String {
    let indent = code
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    code.lines()
        .map(|line| if line.len() >= indent { &line[indent..] } else { line.trim_start() })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Cut at the last sentence end before the limit, or the last space, so the
/// text never stops mid-word.
fn truncate(text: &str, limit: usize) -> String {
    let mut end = limit.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let head = &text[..end];
    let cut = head.rfind(". ").map(|i| i + 1).or_else(|| head.rfind(' ')).unwrap_or(end);
    text[..cut].trim_end().to_string()
}
