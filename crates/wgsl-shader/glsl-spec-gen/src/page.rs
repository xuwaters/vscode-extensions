//! Reading a docs.gl reference page.
//!
//! Every page is one `<div class="refentry">` holding a name, a declaration, an
//! optional parameters list, a description, a version table and a copyright —
//! see `research/docs-gl.md` §2. They are well-formed XML fragments, so
//! `roxmltree` reads them as documents with no wrapping.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Which set of reference pages a page came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Profile {
    /// `sl4/` — desktop GLSL.
    Desktop,
    /// `el3/` — GLSL ES.
    Es,
}

impl Profile {
    pub const ALL: [Profile; 2] = [Profile::Desktop, Profile::Es];

    pub const fn directory(self) -> &'static str {
        match self {
            Profile::Desktop => "sl4",
            Profile::Es => "el3",
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.directory())
    }
}

/// A page's raw text, kept alive so `roxmltree`'s borrowed document can be
/// built from it by the caller.
pub struct Source {
    pub profile: Profile,
    /// The file's stem: `mix`, `gl_FragCoord`. Provenance only — entries are
    /// keyed by the name a prototype declares, which is not always this.
    pub stem: String,
    pub text: String,
}

/// Why a page was not read.
#[derive(Debug)]
pub enum Skipped {
    /// A `<script>window.location.replace(…)</script>` stub. `sl4/dFdy.xhtml`
    /// and `el3/dFdy.xhtml`, and nothing else — the functions they point at are
    /// declared on `dFdx.xhtml`.
    Redirect,
}

/// Every reference page in one profile's directory, in name order.
///
/// Ordering is by file stem so a run is reproducible regardless of what the
/// filesystem feels like returning.
pub fn load_profile(root: &Path, profile: Profile) -> Result<Vec<Source>, String> {
    let dir = root.join(profile.directory());
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    entries.retain(|p| p.extension().is_some_and(|e| e == "xhtml"));
    entries.sort();

    let mut sources = Vec::with_capacity(entries.len());
    for path in entries {
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        sources.push(Source { profile, stem, text });
    }
    Ok(sources)
}

impl Source {
    /// Whether this is a redirect stub rather than a reference page.
    pub fn skipped(&self) -> Option<Skipped> {
        self.text.trim_start().starts_with("<script").then_some(Skipped::Redirect)
    }

    /// A location prefix for an error message: `sl4/mix.xhtml`.
    pub fn where_(&self) -> String {
        format!("{}/{}.xhtml", self.profile, self.stem)
    }
}

/// All of an element's text, exactly as written.
///
/// Only text *nodes* — `roxmltree::Node::text` also answers for an element
/// whose first child is text, so a plain `filter_map` over descendants counts
/// `<span><strong>1.10</strong></span>` twice.
pub fn raw_text(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants().filter(roxmltree::Node::is_text).filter_map(|n| n.text()).collect()
}

/// All of an element's text, whitespace collapsed to single spaces.
pub fn flat_text(node: roxmltree::Node<'_, '_>) -> String {
    collapse(&raw_text(node))
}

/// Whitespace collapsed to single spaces and trimmed — how the reference pages'
/// heavily wrapped prose has to be read.
pub fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            space = !out.is_empty();
        } else {
            if space {
                out.push(' ');
            }
            space = false;
            out.push(ch);
        }
    }
    out
}

/// The `refsect1` with that id — `parameters`, `description`, `versions`.
pub fn section<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    id: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    root.descendants().find(|n| {
        n.is_element() && n.tag_name().name() == "div" && n.attribute("id") == Some(id)
    })
}

/// Every descendant element carrying `class="…"` exactly.
pub fn by_class<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    tag: &str,
    class: &str,
) -> Vec<roxmltree::Node<'a, 'input>> {
    root.descendants()
        .filter(|n| {
            n.is_element() && n.tag_name().name() == tag && n.attribute("class") == Some(class)
        })
        .collect()
}
