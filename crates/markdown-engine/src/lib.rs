//! Markdown rendering engine for the markdown-preview-ultra VSCode extension.
//!
//! Pipeline per render: parse (comrak, sourcepos) → sanitize document-supplied
//! HTML (ammonia) → transform (`[TOC]` markers) → per-top-level-block HTML
//! render (custom formatter: heading ids, mermaid containers) → hash + diff
//! against the previous render → compact patch script.
//!
//! The crate is pure Rust and testable natively; `wasm_api` exposes the same
//! surface as a `wasm-bindgen` module for the extension host.

pub mod diff;
pub mod sanitize;
pub mod toc;
pub mod transform;
pub mod wasm_api;

use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

use comrak::html::{self, ChildRendering, Context};
use comrak::nodes::{AstNode, Node, NodeValue};
use comrak::{Arena, Options, parse_document};
use rustc_hash::{FxHashMap, FxHasher};
use serde::{Deserialize, Serialize};

use diff::Patch;
use toc::TocEntry;

/// Render options, deserialized from the host's JSON. Field names mirror the
/// extension settings they are derived from.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderOptions {
    /// Render a single newline inside a paragraph as `<br>`.
    pub breaks: bool,
    /// Autoconvert bare URLs into links.
    pub linkify: bool,
    /// Smart punctuation (quotes, dashes, ellipses).
    pub typographer: bool,
    /// Pass document-supplied raw HTML through (sanitized); `false` escapes it.
    pub html: bool,
    /// Parse `$…$` / `$$…$$` into math spans for KaTeX.
    pub math: bool,
    /// Turn ```mermaid fences into webview-rendered diagram containers.
    pub mermaid: bool,
    /// GitHub `> [!NOTE]` style alerts.
    pub alerts: bool,
    /// `:shortcode:` emoji.
    pub emoji: bool,
    /// Render `[[wiki links]]` as plain relative links.
    pub wikilinks: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            breaks: false,
            linkify: true,
            typographer: false,
            html: true,
            math: true,
            mermaid: true,
            alerts: true,
            emoji: true,
            wikilinks: false,
        }
    }
}

/// Front matter extracted from the document head.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Frontmatter {
    /// The YAML text between the `---` delimiters.
    pub raw: String,
    /// Parsed YAML as JSON, or `null` if it failed to parse.
    pub data: serde_json::Value,
}

/// Non-essential diagnostics for logging.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderStats {
    pub block_count: usize,
}

/// One render's output: a patch script against the previous render plus the
/// document-level extractions (TOC, front matter).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    /// Monotonic per session; the webview drops stale sequence numbers.
    pub seq: u64,
    /// `true` → discard existing DOM and apply patches to an empty block list
    /// (first render, or options changed).
    pub reset: bool,
    pub patches: Vec<Patch>,
    pub toc: Vec<TocEntry>,
    pub frontmatter: Option<Frontmatter>,
    pub stats: RenderStats,
}

/// A per-document render session: retains the previous render's block hashes
/// so the next render can be expressed as a minimal patch script.
#[derive(Default)]
pub struct Session {
    prev_hashes: Vec<u64>,
    prev_options: Option<RenderOptions>,
    seq: u64,
}

impl Session {
    pub fn new() -> Session {
        Session::default()
    }

    /// Render `markdown`, returning patches relative to the previous call.
    pub fn render(&mut self, markdown: &str, options: &RenderOptions) -> RenderResult {
        let reset = self.prev_options.as_ref() != Some(options);
        let rendered = render_blocks(markdown, options);

        let hashes: Vec<u64> = rendered.blocks.iter().map(|b| hash_block(b)).collect();
        let patches = if reset {
            let mut p = Vec::new();
            if !rendered.blocks.is_empty() {
                p.push(Patch::Insert {
                    html: rendered.blocks.clone(),
                });
            }
            p
        } else {
            diff::diff(&self.prev_hashes, &hashes, &rendered.blocks)
        };

        self.prev_hashes = hashes;
        self.prev_options = Some(options.clone());
        self.seq += 1;

        RenderResult {
            seq: self.seq,
            reset,
            patches,
            toc: rendered.toc,
            frontmatter: rendered.frontmatter,
            stats: RenderStats {
                block_count: rendered.blocks.len(),
            },
        }
    }

    /// JSON-in/JSON-out wrapper used by the WASM surface.
    pub fn render_json(&mut self, markdown: &str, options_json: &str) -> String {
        let options: RenderOptions = serde_json::from_str(options_json).unwrap_or_default();
        let result = self.render(markdown, &options);
        serde_json::to_string(&result).unwrap_or_else(|_| "{\"error\":\"serialize\"}".into())
    }
}

/// A full render before diffing: block HTML plus document-level extractions.
pub struct RenderedDocument {
    pub blocks: Vec<String>,
    pub toc: Vec<TocEntry>,
    pub frontmatter: Option<Frontmatter>,
}

/// Parse and render `markdown` into per-top-level-block HTML strings.
pub fn render_blocks(markdown: &str, options: &RenderOptions) -> RenderedDocument {
    let comrak_options = build_comrak_options(options);
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &comrak_options);

    if options.html {
        sanitize::sanitize_document(root);
    }

    let headings = toc::collect(root);
    let toc_tree = toc::build_tree(&headings);
    transform::replace_toc_markers(root, &toc::inline_toc_html(&toc_tree));

    let frontmatter = extract_frontmatter(root);

    let state = FmtState {
        heading_ids: headings.iter().map(|h| (h.key, h.slug.clone())).collect(),
        mermaid: options.mermaid,
    };
    let blocks = render_top_level_blocks(&arena, root, &comrak_options, state);

    RenderedDocument {
        blocks,
        toc: toc_tree,
        frontmatter,
    }
}

fn build_comrak_options(o: &RenderOptions) -> Options<'static> {
    let mut c = Options::default();
    c.extension.table = true;
    c.extension.strikethrough = true;
    c.extension.autolink = o.linkify;
    c.extension.tasklist = true;
    c.extension.footnotes = true;
    c.extension.front_matter_delimiter = Some("---".into());
    c.extension.multiline_block_quotes = true;
    c.extension.description_lists = true;
    c.extension.alerts = o.alerts;
    c.extension.math_dollars = o.math;
    c.extension.shortcodes = o.emoji;
    if o.wikilinks {
        c.extension.wikilinks_title_after_pipe = true;
    }
    // Heading ids are assigned by our own formatter (document-wide anchorizer
    // shared across per-block renders), not comrak's.
    c.extension.header_id_prefix = None;

    c.parse.smart = o.typographer;
    c.parse.relaxed_tasklist_matching = true;

    c.render.sourcepos = true;
    c.render.hardbreaks = o.breaks;
    // Document HTML is sanitized in the AST before rendering (when enabled),
    // so the formatter can emit it as-is; disabled → escape instead.
    c.render.r#unsafe = o.html;
    c.render.escape = !o.html;
    c.render.tasklist_classes = true;
    c.render.gfm_quirks = true;
    c
}

/// Per-render state threaded through the block formatter.
struct FmtState {
    /// Heading start position → document-wide unique GitHub-style slug.
    heading_ids: FxHashMap<(usize, usize), String>,
    mermaid: bool,
}

/// Render each top-level node to its own HTML string. Consecutive trailing
/// footnote definitions (comrak moves them to the document tail) are grouped
/// into one synthetic block so the `<section class="footnotes">` wrapper stays
/// well-formed.
fn render_top_level_blocks<'a>(
    arena: &'a Arena<'a>,
    root: Node<'a>,
    options: &Options,
    mut state: FmtState,
) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut footnote_defs: Vec<Node<'a>> = Vec::new();

    let children: Vec<Node<'a>> = root.children().collect();
    for node in children {
        let is_frontmatter = matches!(node.data().value, NodeValue::FrontMatter(_));
        let is_footnote_def = matches!(node.data().value, NodeValue::FootnoteDefinition(_));
        if is_frontmatter {
            continue;
        }
        if is_footnote_def {
            footnote_defs.push(node);
            continue;
        }
        let mut out = String::new();
        state = html::format_document_with_formatter(
            node,
            options,
            &mut out,
            &comrak::options::Plugins::default(),
            block_formatter,
            state,
        )
        .expect("writing to String cannot fail");
        if !out.trim().is_empty() {
            blocks.push(out);
        }
    }

    if !footnote_defs.is_empty() {
        // Reparent the definitions under a synthetic document so the section
        // wrapper opens and closes within a single formatted block.
        let doc = arena.alloc(AstNode::from(NodeValue::Document));
        for def in footnote_defs {
            def.detach();
            doc.append(def);
        }
        let mut out = String::new();
        let _ = html::format_document_with_formatter(
            doc,
            options,
            &mut out,
            &comrak::options::Plugins::default(),
            block_formatter,
            state,
        );
        if !out.trim().is_empty() {
            blocks.push(out);
        }
    }

    blocks
}

/// Custom node formatter: default rendering, except headings get our
/// document-wide ids and mermaid fences become webview-rendered containers.
fn block_formatter<'a>(
    context: &mut Context<FmtState>,
    node: Node<'a>,
    entering: bool,
) -> Result<ChildRendering, std::fmt::Error> {
    enum Special {
        Heading(u8),
        Mermaid(String),
    }
    let special = {
        let data = node.data();
        match data.value {
            NodeValue::Heading(ref nh) => Some(Special::Heading(nh.level)),
            NodeValue::CodeBlock(ref ncb)
                if context.user.mermaid && fence_lang(&ncb.info) == "mermaid" =>
            {
                Some(Special::Mermaid(ncb.literal.clone()))
            }
            _ => None,
        }
    };

    match special {
        Some(Special::Heading(level)) => {
            if entering {
                context.cr()?;
                write!(context, "<h{}", level)?;
                let key = {
                    let sp = node.data().sourcepos;
                    (sp.start.line, sp.start.column)
                };
                if let Some(id) = context.user.heading_ids.get(&key).cloned() {
                    write!(context, " id=\"{}\"", id)?;
                }
                html::render_sourcepos(context, node)?;
                context.write_str(">")?;
            } else {
                write!(context, "</h{}>", level)?;
                context.lf()?;
            }
            Ok(ChildRendering::HTML)
        }
        Some(Special::Mermaid(source)) => {
            if entering {
                context.cr()?;
                context.write_str("<div class=\"mermaid-container\"")?;
                html::render_sourcepos(context, node)?;
                write!(
                    context,
                    " data-mermaid-source=\"{}\"></div>",
                    percent_encode(&source)
                )?;
                context.lf()?;
            }
            Ok(ChildRendering::HTML)
        }
        None => html::format_node_default(context, node, entering),
    }
}

/// First whitespace-delimited token of a fence info string.
fn fence_lang(info: &str) -> String {
    info.split_whitespace().next().unwrap_or("").to_lowercase()
}

/// `encodeURIComponent`-compatible percent encoding: the webview decodes the
/// mermaid source with `decodeURIComponent`.
pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(*byte as char),
            _ => {
                let _ = write!(out, "%{:02X}", byte);
            }
        }
    }
    out
}

fn extract_frontmatter<'a>(root: Node<'a>) -> Option<Frontmatter> {
    for node in root.children() {
        if let NodeValue::FrontMatter(ref raw) = node.data().value {
            let trimmed = strip_frontmatter_delimiters(raw);
            let data: serde_json::Value =
                serde_yaml::from_str(&trimmed).unwrap_or(serde_json::Value::Null);
            return Some(Frontmatter { raw: trimmed, data });
        }
    }
    None
}

/// Remove the surrounding `---` fence lines from comrak's raw front matter
/// (which includes the delimiters and trailing blank line).
fn strip_frontmatter_delimiters(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.trim().lines().collect();
    if lines.first().is_some_and(|l| l.trim() == "---") {
        lines.remove(0);
    }
    if lines.last().is_some_and(|l| l.trim() == "---") {
        lines.pop();
    }
    lines.join("\n")
}

fn hash_block(html: &str) -> u64 {
    let mut hasher = FxHasher::default();
    html.hash(&mut hasher);
    hasher.finish()
}
