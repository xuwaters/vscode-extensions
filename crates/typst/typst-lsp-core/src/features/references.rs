//! References.
//!
//! `typst-ide` gives the resolution primitives; the search is ours.
//!
//! * **Labels and references** (`<intro>` / `@intro`) are the common case and
//!   the reliable one: a syntax walk over every file in the compile graph.
//! * **Local bindings** (`#let x = …`) resolve through `named_items`, inverted:
//!   find the definition, then collect the identifiers in the file that resolve
//!   to the same definition span.
//!
//! Files outside the compile graph are not searched, and rename says so rather
//! than silently missing them.

use ecow::EcoString;
use lsp_types::{Location, ReferenceParams};
use typst::World;
use typst::syntax::ast::AstNode;
use typst::syntax::{FileId, LinkedNode, Side, Source, Span, SyntaxKind, ast};
use typst_ide::{Definition, IdeWorld, NamedItem, deref_target, named_items};

use crate::convert::range_to_lsp;
use crate::{Ports, Server};

/// What the cursor is sitting on, once resolved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Target {
    /// A label or a reference to one, by name (without `<`, `>`, or `@`).
    Label(String),
    /// A binding, identified by the span of its definition.
    Binding { name: String, definition: Span },
    /// A standard-library item. Referenced everywhere; renameable nowhere.
    Std(String),
}

impl<Q: Ports> Server<Q> {
    /// `textDocument/references`.
    pub fn references(&mut self, params: ReferenceParams) -> Option<Vec<Location>> {
        let position = params.text_document_position;
        let include_declaration = params.context.include_declaration;
        let (id, source, cursor) =
            self.locate(&position.text_document.uri, position.position)?;

        let target = self.resolve_target(&source, cursor)?;
        Some(self.find_references(id, &target, include_declaration))
    }

    /// Classify what the cursor is on.
    pub(crate) fn resolve_target(&self, source: &Source, cursor: usize) -> Option<Target> {
        let root = LinkedNode::new(source.root());
        let leaf = root
            .leaf_at(cursor, Side::Before)
            .or_else(|| root.leaf_at(cursor, Side::After))?;

        // A label or reference wins outright — they are their own namespace.
        if let Some(name) = enclosing_label_name(&leaf, source) {
            return Some(Target::Label(name));
        }

        match deref_target(leaf.clone())? {
            typst_ide::DerefTarget::VarAccess(node)
            | typst_ide::DerefTarget::Callee(node) => {
                let name = node.cast::<ast::Ident>()?.get().clone();
                let found = named_items(
                    self.session().world(),
                    node.clone(),
                    |item: NamedItem| {
                        let (item_name, span) = describe(&item);
                        (*item_name == name).then_some(span)
                    },
                );

                Some(match found {
                    Some(definition) => {
                        Target::Binding { name: name.to_string(), definition }
                    }
                    None => Target::Std(name.to_string()),
                })
            }
            _ => None,
        }
    }

    /// Collect every occurrence of a target across the compile graph.
    pub(crate) fn find_references(
        &self,
        current: FileId,
        target: &Target,
        include_declaration: bool,
    ) -> Vec<Location> {
        let mut out = Vec::new();

        match target {
            // Labels are document-wide, so every file in the graph is fair game.
            Target::Label(name) => {
                for id in self.graph_files(current) {
                    let Ok(source) = self.session().world().source(id) else { continue };
                    let Some(uri) = self.uris().to_uri(id) else { continue };

                    for range in label_occurrences(&source, name, include_declaration) {
                        out.push(Location {
                            uri: uri.clone(),
                            range: range_to_lsp(&source, range),
                        });
                    }
                }
            }

            // A binding's scope never leaves the file it is written in — an
            // imported item is a different binding at each import site.
            Target::Binding { name, definition } => {
                let Ok(source) = self.session().world().source(current) else {
                    return out;
                };
                let Some(uri) = self.uris().to_uri(current) else { return out };

                for range in
                    binding_occurrences(self, &source, name, *definition, include_declaration)
                {
                    out.push(Location {
                        uri: uri.clone(),
                        range: range_to_lsp(&source, range),
                    });
                }
            }

            // Finding every use of `#heading` would need a whole-universe index.
            Target::Std(_) => {}
        }

        out
    }

    /// Every file the compile graph touched, plus the workspace files the host
    /// reported, deduplicated.
    pub(crate) fn graph_files(&self, current: FileId) -> Vec<FileId> {
        let mut ids = self.session().world().files();
        for id in &self.workspace_files {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        if !ids.contains(&current) {
            ids.push(current);
        }
        ids
    }
}

/// The label name the cursor is inside, if any.
fn enclosing_label_name(leaf: &LinkedNode, source: &Source) -> Option<String> {
    let mut node = Some(leaf.clone());
    while let Some(current) = node {
        if matches!(current.kind(), SyntaxKind::Label | SyntaxKind::Ref) {
            let text = source.text().get(current.range())?;
            return Some(
                text.trim_start_matches(['<', '@']).trim_end_matches('>').to_string(),
            );
        }
        node = current.parent().cloned();
    }
    None
}

/// Every `<name>` and `@name` in a file.
fn label_occurrences(
    source: &Source,
    name: &str,
    include_declaration: bool,
) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    walk(&LinkedNode::new(source.root()), &mut |node| {
        let range = node.range();
        let Some(text) = source.text().get(range.clone()) else { return };

        let matches = match node.kind() {
            SyntaxKind::Label => {
                include_declaration && text.trim_matches(['<', '>']) == name
            }
            SyntaxKind::Ref => text.trim_start_matches('@') == name,
            _ => false,
        };

        if matches {
            out.push(range);
        }
    });
    out
}

/// Every identifier in a file that resolves to the same definition.
fn binding_occurrences<Q: Ports>(
    server: &Server<Q>,
    source: &Source,
    name: &str,
    definition: Span,
    include_declaration: bool,
) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();

    walk(&LinkedNode::new(source.root()), &mut |node| {
        if node.kind() != SyntaxKind::Ident {
            return;
        }
        let Some(ident) = node.cast::<ast::Ident>() else { return };
        if ident.get().as_str() != name {
            return;
        }

        let is_declaration = node.span() == definition;
        if is_declaration {
            if include_declaration {
                out.push(node.range());
            }
            return;
        }

        // Resolve this occurrence and keep it only if it points at the same
        // definition — that is what makes shadowing behave.
        let resolved = named_items(server.session().world(), node.clone(), |item: NamedItem| {
            let (item_name, span) = describe(&item);
            (item_name.as_str() == name).then_some(span)
        });

        if resolved == Some(definition) {
            out.push(node.range());
        }
    });

    out
}

fn walk(node: &LinkedNode, visit: &mut impl FnMut(&LinkedNode)) {
    visit(node);
    for child in node.children() {
        walk(&child, visit);
    }
}

/// A `NamedItem`'s name and defining span.
///
/// `NamedItem::name` and `::span` are `pub(crate)` upstream, so we read the
/// public variants directly. Rebuilding an accessor from what *is* exported is
/// option (a) of decision 0001, and it costs four lines.
fn describe<'a>(item: &NamedItem<'a>) -> (&'a EcoString, Span) {
    match item {
        NamedItem::Var(ident) | NamedItem::Fn(ident) => (ident.get(), ident.span()),
        NamedItem::Module(name, span, _) | NamedItem::Import(name, span, _) => (name, *span),
    }
}

/// Whether a definition sits in a package, the standard library, or outside the
/// compile graph — the three cases rename refuses.
pub(crate) fn definition_origin<Q: Ports>(
    server: &Server<Q>,
    source: &Source,
    cursor: usize,
) -> Option<Definition> {
    typst_ide::definition(
        server.session().world(),
        server.session().last_good(),
        source,
        cursor,
        Side::Before,
    )
}
