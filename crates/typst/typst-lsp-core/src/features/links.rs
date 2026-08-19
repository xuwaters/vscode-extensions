//! Document links.
//!
//! `#import`, `#include`, `image()`, `read()`, `bibliography()`, and `link()`.
//! Relative paths resolve through the same `VirtualPath` logic the compiler
//! uses, so a link is only offered when the target actually resolves — a
//! ctrl-click that lands nowhere is worse than no link at all.

use lsp_types::{DocumentLink, DocumentLinkParams, Uri};
use typst::World;
use typst::syntax::{
    FileId, LinkedNode, RootedPath, Side, Source, SyntaxKind, VirtualPath, ast,
};

use crate::convert::range_to_lsp;
use crate::{Ports, Server};

/// Functions whose first string argument is a path into the project.
const PATH_FUNCTIONS: &[&str] = &["image", "read", "bibliography", "csv", "json", "xml", "yaml", "cbor", "toml"];

/// URL schemes the host will open. Anything else is not offered as a link.
pub const ALLOWED_SCHEMES: &[&str] = &["https", "http", "mailto"];

impl<Q: Ports> Server<Q> {
    /// `textDocument/documentLink`.
    pub fn document_links(&mut self, params: DocumentLinkParams) -> Option<Vec<DocumentLink>> {
        // In a bibliography the links are `url` and `doi` fields.
        if let Some((_, source, bib)) = self.bib_of(&params.text_document.uri) {
            return Some(self.bib_links(&source, &bib));
        }

        let (id, source) = self.source_of(&params.text_document.uri)?;
        let mut out = Vec::new();

        collect(&LinkedNode::new(source.root()), &source, &mut |range, text, kind| {
            let target = match kind {
                LinkKind::Path => self.resolve_path(id, &text)?,
                LinkKind::Url => {
                    let scheme = text.split(':').next().unwrap_or_default();
                    if !ALLOWED_SCHEMES.contains(&scheme) {
                        return None;
                    }
                    text.parse::<Uri>().ok()?
                }
            };

            Some(DocumentLink {
                range: range_to_lsp(&source, range),
                target: Some(target),
                tooltip: None,
                data: None,
            })
        }, &mut out);

        Some(out)
    }

    /// Resolve a document-relative path the way the compiler would, and only
    /// return a file if it is really there.
    pub(crate) fn resolve_path_id(&self, from: FileId, path: &str) -> Option<FileId> {
        let rooted = from.get();
        let vpath = if path.starts_with('/') {
            VirtualPath::new(path).ok()?
        } else {
            rooted.vpath().parent()?.join(path).ok()?
        };

        let id = FileId::new(RootedPath::new(rooted.root().clone(), vpath));
        // Reading is how we know it resolves; the result is cached for the rest
        // of the compile anyway.
        self.session().world().file(id).ok()?;
        Some(id)
    }

    /// The same, as a URI.
    fn resolve_path(&self, from: FileId, path: &str) -> Option<Uri> {
        self.uris().to_uri(self.resolve_path_id(from, path)?)
    }
}

/// The document-relative path the cursor sits inside, if it sits in one.
///
/// Goto-definition should land wherever a ctrl-click on the document link
/// would, and `typst-ide` only knows `#import` and `#include` — every other
/// path-taking function is ours to answer.
pub(crate) fn path_at(source: &Source, cursor: usize) -> Option<String> {
    let root = LinkedNode::new(source.root());
    let leaf = root
        .leaf_at(cursor, Side::Before)
        .or_else(|| root.leaf_at(cursor, Side::After))?;
    if leaf.kind() != SyntaxKind::Str {
        return None;
    }

    let (_, text) = inner_string(&leaf, source)?;
    // A package spec is not a file path.
    if text.starts_with('@') {
        return None;
    }

    // Walk out of any wrapping: `bibliography(("a.bib", "b.bib"))` puts the
    // string two levels below the call it belongs to.
    let mut node = leaf.parent()?;
    while matches!(
        node.kind(),
        SyntaxKind::Array | SyntaxKind::Parenthesized | SyntaxKind::Named
    ) {
        node = node.parent()?;
    }

    match node.kind() {
        SyntaxKind::ModuleImport | SyntaxKind::ModuleInclude => Some(text),
        SyntaxKind::Args => {
            let call = node.parent()?.cast::<ast::FuncCall>()?;
            let ast::Expr::Ident(name) = call.callee() else {
                return None;
            };
            PATH_FUNCTIONS.contains(&name.get().as_str()).then_some(text)
        }
        _ => None,
    }
}

enum LinkKind {
    Path,
    Url,
}

fn collect(
    node: &LinkedNode,
    source: &Source,
    make: &mut impl FnMut(std::ops::Range<usize>, String, LinkKind) -> Option<DocumentLink>,
    out: &mut Vec<DocumentLink>,
) {
    match node.kind() {
        // A bare URL in markup: `https://typst.app`.
        SyntaxKind::Link => {
            let text = source.text().get(node.range()).unwrap_or_default().to_string();
            if let Some(link) = make(node.range(), text, LinkKind::Url) {
                out.push(link);
            }
        }

        SyntaxKind::ModuleImport | SyntaxKind::ModuleInclude => {
            if let Some((range, text)) = first_string_child(node, source) {
                // Package imports (`@preview/…`) are not file paths.
                if !text.starts_with('@')
                    && let Some(link) = make(range, text, LinkKind::Path)
                {
                    out.push(link);
                }
            }
        }

        SyntaxKind::FuncCall => {
            if let Some(call) = node.cast::<ast::FuncCall>()
                && let ast::Expr::Ident(name) = call.callee()
            {
                let name = name.get().as_str();
                if PATH_FUNCTIONS.contains(&name)
                    && let Some((range, text)) = first_string_in_args(node, source)
                    && let Some(link) = make(range, text, LinkKind::Path)
                {
                    out.push(link);
                } else if name == "link"
                    && let Some((range, text)) = first_string_in_args(node, source)
                    && let Some(link) = make(range, text, LinkKind::Url)
                {
                    out.push(link);
                }
            }
        }

        _ => {}
    }

    for child in node.children() {
        collect(&child, source, make, out);
    }
}

/// The first `Str` child of a node, with the quotes trimmed off the range.
fn first_string_child(
    node: &LinkedNode,
    source: &Source,
) -> Option<(std::ops::Range<usize>, String)> {
    let child = node.children().find(|child| child.kind() == SyntaxKind::Str)?;
    inner_string(&child, source)
}

fn first_string_in_args(
    node: &LinkedNode,
    source: &Source,
) -> Option<(std::ops::Range<usize>, String)> {
    let args = node.children().find(|child| child.kind() == SyntaxKind::Args)?;
    let child = args.children().find(|child| child.kind() == SyntaxKind::Str)?;
    inner_string(&child, source)
}

/// The range and text inside a string literal's quotes.
fn inner_string(
    node: &LinkedNode,
    source: &Source,
) -> Option<(std::ops::Range<usize>, String)> {
    let range = node.range();
    let raw = source.text().get(range.clone())?;
    if raw.len() < 2 {
        return None;
    }
    let inner = range.start + 1..range.end - 1;
    Some((inner.clone(), source.text().get(inner)?.to_string()))
}
