//! The workspace symbol index.
//!
//! `workspace/symbol` has to answer for files the editor never opened, and the
//! server has no filesystem — it runs inside WASM. So the host walks the tree
//! and pushes what it finds over `wgsl/workspaceFiles`; this is where that
//! lands.
//!
//! Only the outline is kept, not the text. A workspace of a thousand shaders
//! is then a few hundred kilobytes of names rather than tens of megabytes of
//! source, and nothing here has to be invalidated when a file changes — the
//! host re-pushes it.
//!
//! Both languages produce the same [`wgsl_syntax::Symbol`] shape, so the code
//! below is language-blind: WGSL parses with `wgsl-syntax` and GLSL projects
//! its own CST outline onto the same arena
//! ([`crate::glsl::GlslDocument::project`]).

use lsp_types::{Range, Uri};
use wgsl_syntax::{Language, SymbolKind};

/// One symbol from an unopened file.
#[derive(Debug, Clone)]
pub struct Entry {
    pub uri: Uri,
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    pub detail: String,
    /// The struct or function the symbol is declared in, for the "in …"
    /// suffix a symbol picker shows.
    pub container: Option<String>,
}

#[derive(Debug, Default)]
pub struct WorkspaceIndex {
    entries: Vec<Entry>,
}

impl WorkspaceIndex {
    /// Index one file, replacing whatever was held for that URI.
    pub fn set_file(&mut self, uri: &Uri, language: Language, text: &str) {
        self.remove(uri);

        let parsed = match language {
            Language::Wgsl => wgsl_syntax::parse(text, language),
            Language::Glsl => crate::glsl::GlslDocument::project(text),
        };
        let lines = analyzer_core::spans::SpanTable::new(text);
        for symbol in &parsed.symbols {
            // Locals and parameters are noise in a workspace-wide picker.
            if symbol.kind.is_local() {
                continue;
            }
            self.entries.push(Entry {
                uri: uri.clone(),
                name: symbol.name.clone(),
                kind: symbol.kind,
                range: crate::convert::span_to_range(text, &lines, symbol.full_span),
                selection_range: crate::convert::span_to_range(text, &lines, symbol.name_span),
                detail: symbol.detail.clone(),
                container: symbol
                    .parent
                    .map(|parent| parsed.symbols[parent].name.clone()),
            });
        }
    }

    /// Forget a file — it was deleted, or it has just been opened and the
    /// live document is now the better source.
    pub fn remove(&mut self, uri: &Uri) {
        self.entries.retain(|entry| entry.uri.as_str() != uri.as_str());
    }

    /// Whether anything is held for a URI.
    pub fn contains(&self, uri: &Uri) -> bool {
        self.entries.iter().any(|entry| entry.uri.as_str() == uri.as_str())
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Entries matching `query`, best first.
    ///
    /// Matching is a case-insensitive subsequence, the convention every symbol
    /// picker uses: `ftm` finds `fragmentMain`. An empty query matches
    /// everything, which is what a client sends to populate the list before
    /// the user types.
    pub fn search(&self, query: &str) -> Vec<&Entry> {
        let mut matched: Vec<(u32, &Entry)> = self
            .entries
            .iter()
            .filter_map(|entry| score(&entry.name, query).map(|score| (score, entry)))
            .collect();
        matched.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
        matched.into_iter().map(|(_, entry)| entry).collect()
    }
}

/// How well `name` matches `query`, lower being better. `None` is no match.
///
/// A prefix match beats a subsequence match, and a shorter name beats a longer
/// one — so `dot` ranks above `dotProduct` for the query `dot`.
pub fn score(name: &str, query: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(name.len() as u32);
    }
    let lower = name.to_ascii_lowercase();
    let query = query.to_ascii_lowercase();

    if lower.starts_with(&query) {
        return Some(name.len() as u32);
    }
    if !is_subsequence(&lower, &query) {
        return None;
    }
    // Ranked behind every prefix match, however long.
    Some(10_000 + name.len() as u32)
}

fn is_subsequence(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|wanted| chars.any(|c| c == wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(text: &str) -> Uri {
        text.parse().unwrap()
    }

    fn index() -> WorkspaceIndex {
        let mut index = WorkspaceIndex::default();
        index.set_file(
            &uri("file:///a.wgsl"),
            Language::Wgsl,
            "struct Camera { view: mat4x4f }\nfn fragmentMain() {}\nfn dot2() {}\n",
        );
        index
    }

    #[test]
    fn locals_are_left_out_of_the_workspace_picker() {
        let mut index = WorkspaceIndex::default();
        index.set_file(
            &uri("file:///a.wgsl"),
            Language::Wgsl,
            "fn f(a: f32) { let b = a; }\n",
        );
        let names: Vec<&str> =
            index.entries().iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["f"]);
    }

    #[test]
    fn struct_members_carry_their_container() {
        let index = index();
        let view = index.entries().iter().find(|e| e.name == "view").unwrap();
        assert_eq!(view.container.as_deref(), Some("Camera"));
    }

    #[test]
    fn a_subsequence_query_finds_a_camel_case_name() {
        let index = index();
        let names: Vec<&str> =
            index.search("ftm").iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["fragmentMain"]);
    }

    #[test]
    fn a_prefix_match_outranks_a_subsequence_match() {
        assert!(score("dot", "dot") < score("dotProduct", "dot"));
        assert!(score("dotProduct", "dot") < score("aDelightfulOtter", "dot"));
        assert_eq!(score("nope", "xyz"), None);
    }

    #[test]
    fn an_empty_query_returns_everything() {
        assert_eq!(index().search("").len(), index().entries().len());
    }

    #[test]
    fn reindexing_a_file_replaces_its_entries() {
        let mut index = index();
        index.set_file(&uri("file:///a.wgsl"), Language::Wgsl, "fn onlyThis() {}\n");
        let names: Vec<&str> =
            index.entries().iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["onlyThis"]);
    }

    #[test]
    fn removing_a_file_forgets_it() {
        let mut index = index();
        assert!(index.contains(&uri("file:///a.wgsl")));
        index.remove(&uri("file:///a.wgsl"));
        assert!(!index.contains(&uri("file:///a.wgsl")));
        assert!(index.entries().is_empty());
    }
}
