//! Document and workspace symbols.
//!
//! A syntax-tree walk, nested by heading level, so `=` contains `==` — the
//! outline people actually want rather than a flat list of every binding.

use lsp_types::{
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, Location, SymbolKind,
    WorkspaceSymbolParams, WorkspaceSymbolResponse,
};
use typst::World;
use typst::syntax::{LinkedNode, Source, SyntaxKind, ast};

use crate::convert::range_to_lsp;
use crate::{Ports, Server};

/// A symbol found by the walk, before it becomes an LSP shape.
struct Found {
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    /// The symbol's full extent. For a heading this grows to cover the section.
    range: std::ops::Range<usize>,
    /// The part to reveal when the symbol is picked — always the declaration
    /// itself, so it stays inside `range` after the extension above.
    selection: std::ops::Range<usize>,
    /// Heading depth, or `None` for anything that is not a heading.
    depth: Option<usize>,
}

impl<Q: Ports> Server<Q> {
    /// `textDocument/documentSymbol`.
    pub fn document_symbols(
        &mut self,
        params: DocumentSymbolParams,
    ) -> Option<DocumentSymbolResponse> {
        // A bibliography's outline is its entries, not a typst syntax walk.
        if let Some((_, source, bib)) = self.bib_of(&params.text_document.uri) {
            return Some(DocumentSymbolResponse::Nested(self.bib_symbols(&source, &bib)));
        }

        let (_, source) = self.source_of(&params.text_document.uri)?;
        Some(DocumentSymbolResponse::Nested(nested_symbols(&source)))
    }

    /// `workspace/symbol`: the same walk over every file the host reported.
    pub fn workspace_symbols(
        &mut self,
        params: WorkspaceSymbolParams,
    ) -> Option<WorkspaceSymbolResponse> {
        let query = params.query.to_lowercase();
        let mut out = Vec::new();

        for id in self.workspace_files.clone() {
            let Ok(source) = self.session().world().source(id) else { continue };
            let Some(uri) = self.uris().to_uri(id) else { continue };

            // A citation key is a workspace symbol like any other — searching
            // for `knuth1984` should find the entry that defines it.
            for symbol in match super::bibtex::is_bib(id) {
                true => bib_symbols(&source),
                false => flat_symbols(&source),
            } {
                if !matches_query(&symbol.name, &query) {
                    continue;
                }
                #[allow(deprecated)]
                out.push(lsp_types::SymbolInformation {
                    name: symbol.name,
                    kind: symbol.kind,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: uri.clone(),
                        range: range_to_lsp(&source, symbol.range),
                    },
                    container_name: None,
                });
            }
        }

        Some(WorkspaceSymbolResponse::Flat(out))
    }
}

/// A bibliography's entries, in the same shape the typst walk produces.
fn bib_symbols(source: &Source) -> Vec<Found> {
    crate::bib::Bib::parse(source.text())
        .references()
        .filter_map(|entry| {
            let key = entry.key.clone()?;
            Some(Found {
                name: key,
                detail: Some(entry.summary()),
                kind: SymbolKind::CONSTANT,
                range: entry.key_range.clone(),
                selection: entry.key_range.clone(),
                depth: None,
            })
        })
        .collect()
}

/// A forgiving subsequence match, so `wsym` finds `workspace_symbols`.
fn matches_query(name: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let name = name.to_lowercase();
    let mut haystack = name.chars();
    query.chars().all(|needle| haystack.any(|candidate| candidate == needle))
}

/// The document's symbols, nested by heading level.
pub fn nested_symbols(source: &Source) -> Vec<DocumentSymbol> {
    let flat = extend_heading_ranges(flat_symbols(source), source.text().len());

    let mut roots: Vec<DocumentSymbol> = Vec::new();
    // The chain of headings currently open, innermost last.
    let mut open: Vec<(usize, DocumentSymbol)> = Vec::new();

    for found in flat {
        let symbol = to_lsp(source, &found);
        match found.depth {
            Some(depth) => {
                close_to(&mut roots, &mut open, depth);
                open.push((depth, symbol));
            }
            None => attach(&mut roots, &mut open, symbol),
        }
    }

    close_to(&mut roots, &mut open, 1);
    while let Some((_, done)) = open.pop() {
        attach(&mut roots, &mut open, done);
    }

    roots
}

/// Close every open heading at `depth` or deeper, attaching each to its parent.
fn close_to(
    roots: &mut Vec<DocumentSymbol>,
    open: &mut Vec<(usize, DocumentSymbol)>,
    depth: usize,
) {
    while open.last().is_some_and(|(open_depth, _)| *open_depth >= depth) {
        let (_, done) = open.pop().expect("just checked");
        attach(roots, open, done);
    }
}

fn attach(
    roots: &mut Vec<DocumentSymbol>,
    open: &mut [(usize, DocumentSymbol)],
    symbol: DocumentSymbol,
) {
    match open.last_mut() {
        Some((_, parent)) => parent.children.get_or_insert_with(Vec::new).push(symbol),
        None => roots.push(symbol),
    }
}

/// A typst `Heading` node covers only its own line; the *section* runs to the
/// next heading of equal or lower level. Extending the range is what makes
/// "select enclosing symbol" and the breadcrumb bar behave.
fn extend_heading_ranges(mut found: Vec<Found>, end_of_file: usize) -> Vec<Found> {
    let headings: Vec<(usize, usize, usize)> = found
        .iter()
        .enumerate()
        .filter_map(|(index, item)| Some((index, item.depth?, item.range.start)))
        .collect();

    for (position, &(index, depth, _)) in headings.iter().enumerate() {
        let end = headings[position + 1..]
            .iter()
            .find(|(_, next_depth, _)| *next_depth <= depth)
            .map(|(_, _, start)| *start)
            .unwrap_or(end_of_file);
        found[index].range.end = end;
    }

    found
}

fn to_lsp(source: &Source, found: &Found) -> DocumentSymbol {
    let range = range_to_lsp(source, found.range.clone());
    // The selection range must be inside the full range; for an extended
    // heading that means the heading line rather than the whole section.
    let selection_range = range_to_lsp(source, found.selection.clone());
    #[allow(deprecated)]
    DocumentSymbol {
        name: found.name.clone(),
        detail: found.detail.clone(),
        kind: found.kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}

/// Walk the tree collecting symbols in document order.
///
/// A nameless symbol is an error on the client — `vscode-languageclient`
/// rejects the whole response with "name must not be falsy" — so a half-typed
/// construct that yields no name is dropped here rather than taking the
/// document's outline down with it.
fn flat_symbols(source: &Source) -> Vec<Found> {
    let mut out = Vec::new();
    walk(&LinkedNode::new(source.root()), source, &mut out);
    out.retain(|found| !found.name.is_empty());
    out
}

fn walk(node: &LinkedNode, source: &Source, out: &mut Vec<Found>) {
    if let Some(found) = classify(node, source) {
        out.push(found);
    }
    for child in node.children() {
        walk(&child, source, out);
    }
}

fn classify(node: &LinkedNode, source: &Source) -> Option<Found> {
    let range = node.range();
    let text = |range: std::ops::Range<usize>| -> String {
        source.text().get(range).unwrap_or_default().trim().to_string()
    };

    let simple = |name: String, kind: SymbolKind| Found {
        name,
        detail: None,
        kind,
        range: range.clone(),
        selection: range.clone(),
        depth: None,
    };

    match node.kind() {
        SyntaxKind::Heading => {
            let heading = node.cast::<ast::Heading>()?;
            let depth = heading.depth().get();
            let title = text(range.clone()).trim_start_matches('=').trim().to_string();
            Some(Found {
                // A line is a heading the moment its marker is typed; the title
                // arrives a keystroke later. The marker stands in until then, so
                // the outline has an entry to grow rather than a nameless symbol
                // the client rejects outright.
                name: match title.is_empty() {
                    true => "=".repeat(depth),
                    false => title,
                },
                detail: None,
                kind: SymbolKind::STRING,
                range: range.clone(),
                selection: range,
                depth: Some(depth),
            })
        }

        SyntaxKind::LetBinding => {
            let binding = node.cast::<ast::LetBinding>()?;
            let kind = match binding.kind() {
                ast::LetBindingKind::Closure(_) => SymbolKind::FUNCTION,
                ast::LetBindingKind::Normal(_) => SymbolKind::VARIABLE,
            };
            let names: Vec<String> = binding
                .kind()
                .bindings()
                .iter()
                .map(|ident| ident.get().to_string())
                .collect();
            let name = names.join(", ");
            (!name.is_empty()).then(|| simple(name, kind))
        }

        SyntaxKind::ShowRule | SyntaxKind::SetRule => {
            Some(simple(first_line(&text(range.clone())), SymbolKind::EVENT))
        }

        SyntaxKind::Label => Some(simple(text(range.clone()), SymbolKind::KEY)),

        SyntaxKind::ModuleImport | SyntaxKind::ModuleInclude => {
            Some(simple(first_line(&text(range.clone())), SymbolKind::MODULE))
        }

        _ => None,
    }
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.len() > 80 { format!("{}…", &line[..77]) } else { line.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbols(text: &str) -> Vec<DocumentSymbol> {
        nested_symbols(&Source::detached(text))
    }

    #[test]
    fn headings_nest_by_level() {
        let tree = symbols("= One\n\n== One A\n\n== One B\n\n= Two\n");

        assert_eq!(tree.len(), 2, "two top-level headings");
        assert_eq!(tree[0].name, "One");
        let children = tree[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, "One A");
        assert_eq!(children[1].name, "One B");
        assert_eq!(tree[1].name, "Two");
    }

    #[test]
    fn a_deeper_heading_nests_further() {
        let tree = symbols("= A\n\n== B\n\n=== C\n");
        let b = &tree[0].children.as_ref().unwrap()[0];
        assert_eq!(b.name, "B");
        assert_eq!(b.children.as_ref().unwrap()[0].name, "C");
    }

    #[test]
    fn bindings_land_under_the_enclosing_heading() {
        let tree = symbols("= Section\n\n#let helper(x) = x\n\n#let value = 1\n");
        let children = tree[0].children.as_ref().unwrap();

        let helper = children.iter().find(|s| s.name == "helper").unwrap();
        assert_eq!(helper.kind, SymbolKind::FUNCTION);
        let value = children.iter().find(|s| s.name == "value").unwrap();
        assert_eq!(value.kind, SymbolKind::VARIABLE);
    }

    #[test]
    fn imports_and_labels_are_reported() {
        let tree = symbols("#import \"a.typ\": x\n\n= Head <intro>\n");
        assert!(tree.iter().any(|s| s.kind == SymbolKind::MODULE));

        let head = tree.iter().find(|s| s.kind == SymbolKind::STRING).unwrap();
        let label = head
            .children
            .as_ref()
            .map(|children| children.iter().any(|s| s.kind == SymbolKind::KEY))
            .unwrap_or(false);
        assert!(label, "the label belongs under its heading");
    }

    #[test]
    fn a_heading_still_being_typed_keeps_a_name() {
        // "= Background", one keystroke at a time. The client throws away the
        // whole response over a single empty name, so every prefix must be safe.
        let typed = "= Background\n";
        for end in 0..=typed.len() {
            let tree = symbols(&typed[..end]);
            assert!(
                names(&tree).iter().all(|name| !name.is_empty()),
                "empty symbol name for prefix {:?}",
                &typed[..end],
            );
        }

        assert_eq!(names(&symbols("= ")), ["="], "the marker stands in");
        assert_eq!(names(&symbols("== ")), ["=="]);
        assert_eq!(names(&symbols("= B")), ["B"], "and gives way to the title");
    }

    /// Every name in the tree, parents before children.
    fn names(tree: &[DocumentSymbol]) -> Vec<String> {
        tree.iter()
            .flat_map(|symbol| {
                std::iter::once(symbol.name.clone())
                    .chain(symbol.children.as_deref().map(names).unwrap_or_default())
            })
            .collect()
    }

    #[test]
    fn the_fuzzy_filter_matches_subsequences() {
        assert!(matches_query("workspace_symbols", "wsym"));
        assert!(matches_query("Heading", ""));
        assert!(!matches_query("Heading", "xyz"));
    }
}
