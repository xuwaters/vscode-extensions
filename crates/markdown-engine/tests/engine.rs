//! End-to-end engine tests: markdown in → blocks/patches/TOC/frontmatter out.

use markdown_engine::diff::Patch;
use markdown_engine::{RenderOptions, Session, render_blocks};

fn opts() -> RenderOptions {
    RenderOptions::default()
}

fn blocks(markdown: &str) -> Vec<String> {
    render_blocks(markdown, &opts()).blocks
}

#[test]
fn splits_top_level_blocks() {
    let b = blocks("# Title\n\nfirst\n\nsecond\n");
    assert_eq!(b.len(), 3);
    assert!(b[0].contains("<h1"));
    assert!(b[1].contains("<p"));
    assert!(b[2].contains("second"));
}

#[test]
fn blocks_carry_sourcepos() {
    let b = blocks("para one\n\npara two\n");
    assert!(b[0].contains("data-sourcepos=\"1:1-1:8\""), "{}", b[0]);
    assert!(b[1].contains("data-sourcepos=\"3:1-3:8\""), "{}", b[1]);
}

#[test]
fn headings_get_github_slugs_deduped_document_wide() {
    let b = blocks("# Intro\n\ntext\n\n# Intro\n\n## Fancy Stuff!\n");
    assert!(b[0].contains("id=\"intro\""), "{}", b[0]);
    assert!(b[2].contains("id=\"intro-1\""), "{}", b[2]);
    assert!(b[3].contains("id=\"fancy-stuff\""), "{}", b[3]);
}

#[test]
fn math_spans_survive() {
    let b = blocks("inline $x^2$ math\n\n$$\\int_0^1 f(x) dx$$\n");
    assert!(b[0].contains("data-math-style=\"inline\""), "{}", b[0]);
    assert!(b[1].contains("data-math-style=\"display\""), "{}", b[1]);
}

#[test]
fn math_disabled_leaves_dollars() {
    let opts = RenderOptions {
        math: false,
        ..opts()
    };
    let b = render_blocks("price $5 and $10\n", &opts).blocks;
    assert!(!b[0].contains("data-math-style"));
}

#[test]
fn mermaid_fence_becomes_container() {
    let b = blocks("```mermaid\ngraph TD; A-->B;\n```\n");
    assert_eq!(b.len(), 1);
    assert!(b[0].contains("class=\"mermaid-container\""), "{}", b[0]);
    assert!(
        b[0].contains("data-mermaid-source=\"graph%20TD%3B%20A--%3EB%3B%0A\""),
        "{}",
        b[0]
    );
    assert!(!b[0].contains("<pre"));
}

#[test]
fn mermaid_disabled_renders_code_fence() {
    let opts = RenderOptions {
        mermaid: false,
        ..opts()
    };
    let b = render_blocks("```mermaid\ngraph TD;\n```\n", &opts).blocks;
    assert!(b[0].contains("<pre"));
    assert!(!b[0].contains("mermaid-container"));
}

#[test]
fn code_fence_language_class() {
    let b = blocks("```rust\nfn main() {}\n```\n");
    assert!(b[0].contains("language-rust"), "{}", b[0]);
}

#[test]
fn alerts_render_github_style() {
    let b = blocks("> [!NOTE]\n> Useful info.\n");
    assert!(b[0].contains("markdown-alert-note"), "{}", b[0]);
    assert!(b[0].contains("markdown-alert-title"), "{}", b[0]);
}

#[test]
fn emoji_shortcodes_become_unicode() {
    let b = blocks("hello :smile:\n");
    assert!(b[0].contains('😄'), "{}", b[0]);
}

#[test]
fn task_lists_render_checkboxes_with_sourcepos() {
    let b = blocks("- [ ] todo\n- [x] done\n");
    assert_eq!(b.len(), 1);
    assert!(b[0].contains("type=\"checkbox\""));
    assert!(b[0].contains("checked=\"\""));
    assert!(b[0].contains("task-list-item"));
}

#[test]
fn footnotes_render_as_single_trailing_block() {
    let b = blocks("first[^a] use\n\nmiddle\n\n[^a]: the note\n\nlast\n");
    let last = b.last().unwrap();
    assert!(last.contains("<section"), "{}", last);
    assert!(last.contains("class=\"footnotes\""), "{}", last);
    assert!(last.contains("</section>"), "{}", last);
    assert!(b[0].contains("footnote-ref"), "{}", b[0]);
    // The footnote block is exactly one block even with other content after
    // the definition in source order.
    assert_eq!(b.iter().filter(|x| x.contains("footnotes")).count(), 1);
}

#[test]
fn frontmatter_extracted_and_hidden() {
    let doc = "---\ntitle: Hello\ntags: [a, b]\n---\n\nbody\n";
    let r = render_blocks(doc, &opts());
    let fm = r.frontmatter.expect("frontmatter");
    assert_eq!(fm.data["title"], "Hello");
    assert_eq!(fm.data["tags"][0], "a");
    assert!(!r.blocks.iter().any(|b| b.contains("title: Hello")));
}

#[test]
fn toc_extracted_with_nesting() {
    let r = render_blocks("# One\n\n## Two\n\ncontent\n\n# Three\n", &opts());
    assert_eq!(r.toc.len(), 2);
    assert_eq!(r.toc[0].slug, "one");
    assert_eq!(r.toc[0].children[0].slug, "two");
    assert_eq!(r.toc[0].children[0].line, 2);
    assert_eq!(r.toc[1].slug, "three");
}

#[test]
fn toc_marker_becomes_inline_nav() {
    let b = blocks("[TOC]\n\n# Section\n");
    assert!(b[0].contains("<nav class=\"inline-toc\""), "{}", b[0]);
    assert!(b[0].contains("href=\"#section\""), "{}", b[0]);
}

#[test]
fn raw_html_is_sanitized_when_enabled() {
    let b =
        blocks("<div onclick=\"evil()\">\n\n<script>alert(1)</script>\n\ntext <kbd>K</kbd> in\n");
    let joined = b.join("");
    assert!(!joined.contains("onclick"));
    assert!(!joined.contains("<script>"));
    assert!(joined.contains("<kbd>K</kbd>"), "{}", joined);
}

#[test]
fn raw_html_escaped_when_disabled() {
    let opts = RenderOptions {
        html: false,
        ..opts()
    };
    let b = render_blocks("<b>bold</b> move\n", &opts).blocks;
    let joined = b.join("");
    assert!(joined.contains("&lt;b&gt;"), "{}", joined);
}

#[test]
fn html_disabled_still_renders_inline_toc() {
    let opts = RenderOptions {
        html: false,
        ..opts()
    };
    let b = render_blocks("[TOC]\n\n# S\n", &opts).blocks;
    assert!(b[0].contains("<nav class=\"inline-toc\""), "{}", b[0]);
}

#[test]
fn session_keystroke_produces_minimal_patch() {
    let mut session = Session::new();
    let o = opts();
    let first = session.render("# Title\n\nfirst\n\nsecond\n", &o);
    assert!(first.reset);
    assert_eq!(first.seq, 1);

    let second = session.render("# Title\n\nfirst!\n\nsecond\n", &o);
    assert!(!second.reset);
    assert_eq!(second.seq, 2);
    assert_eq!(
        second.patches,
        vec![
            Patch::Keep { count: 1 },
            Patch::Replace {
                count: 1,
                html: vec![second_html(&second.patches)]
            },
            Patch::Keep { count: 1 },
        ]
    );
}

fn second_html(patches: &[Patch]) -> String {
    match &patches[1] {
        Patch::Replace { html, .. } => html[0].clone(),
        other => panic!("expected replace, got {other:?}"),
    }
}

#[test]
fn session_option_change_forces_reset() {
    let mut session = Session::new();
    session.render("hi\n", &opts());
    let changed = RenderOptions {
        breaks: true,
        ..opts()
    };
    let r = session.render("hi\n", &changed);
    assert!(r.reset);
}

#[test]
fn linkify_toggle() {
    let on = render_blocks("visit https://example.com now\n", &opts()).blocks;
    assert!(on[0].contains("href=\"https://example.com\""), "{}", on[0]);
    let off = RenderOptions {
        linkify: false,
        ..opts()
    };
    let off_b = render_blocks("visit https://example.com now\n", &off).blocks;
    assert!(!off_b[0].contains("<a "), "{}", off_b[0]);
}

#[test]
fn breaks_toggle() {
    let on = RenderOptions {
        breaks: true,
        ..opts()
    };
    assert!(render_blocks("a\nb\n", &on).blocks[0].contains("<br"));
    assert!(!render_blocks("a\nb\n", &opts()).blocks[0].contains("<br"));
}

#[test]
fn render_json_roundtrip() {
    let mut session = Session::new();
    let json = session.render_json("# Hi\n", "{}");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["seq"], 1);
    assert_eq!(v["reset"], true);
    assert_eq!(v["stats"]["blockCount"], 1);
    assert_eq!(v["patches"][0]["op"], "insert");
}
