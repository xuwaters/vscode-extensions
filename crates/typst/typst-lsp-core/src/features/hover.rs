//! Hover — `typst_ide::tooltip`, plus one extension of ours.
//!
//! Upstream covers named-parameter docs, font information, label previews, and
//! import targets. We add the page number a label resolves to, which is the
//! thing you actually want to know when hovering `@intro` in a long document.

use lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind};
use typst::syntax::{LinkedNode, Side, SyntaxKind};
use typst_ide::Tooltip;

use crate::convert::range_to_lsp;
use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// `textDocument/hover`.
    pub fn hover(&mut self, params: HoverParams) -> Option<Hover> {
        let position = params.text_document_position_params;

        // A bibliography answers from its own parse.
        if let Some((_, source, bib)) = self.bib_of(&position.text_document.uri) {
            let cursor = crate::convert::position_to_offset(&source, position.position);
            return self.bib_hover(&source, &bib, cursor);
        }

        let (_, source, cursor) = self.locate(&position.text_document.uri, position.position)?;

        // Upstream's own tests probe both sides: which one carries the tooltip
        // depends on whether the cursor sits at a token boundary.
        let tooltip = typst_ide::tooltip(
            self.session().world(),
            self.last_good(),
            &source,
            cursor,
            Side::Before,
        )
        .or_else(|| {
            typst_ide::tooltip(
                self.session().world(),
                self.last_good(),
                &source,
                cursor,
                Side::After,
            )
        });

        let mut value = match tooltip {
            Some(Tooltip::Text(text)) => text.to_string(),
            Some(Tooltip::Code(code)) => format!("```typst\n{code}\n```"),
            None => String::new(),
        };

        if let Some(page) = self.label_page(&source, cursor) {
            if !value.is_empty() {
                value.push_str("\n\n");
            }
            value.push_str(&format!("On page {page}."));
        }

        // A citation upstream cannot place — the document has not compiled, or
        // `bibliography()` has not been written yet — is still a key we can look
        // up in the project's `.bib` files.
        if value.is_empty()
            && let Some((_, _, entry)) = self.cited_entry(&source, cursor)
        {
            value = entry.markdown();
        }

        if value.is_empty() {
            return None;
        }

        let leaf = LinkedNode::new(source.root()).leaf_at(cursor, Side::Before);
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: leaf.map(|leaf| range_to_lsp(&source, leaf.range())),
        })
    }

    /// The page a hovered label or reference resolves to, if a compiled
    /// document is available to ask.
    fn label_page(&self, source: &typst::syntax::Source, cursor: usize) -> Option<usize> {
        let document = self.last_good()?;
        let leaf = LinkedNode::new(source.root()).leaf_at(cursor, Side::Before)?;

        // Walk out to the enclosing `<label>` or `@ref`, if there is one.
        let mut node = Some(leaf);
        let range = loop {
            let current = node?;
            match current.kind() {
                SyntaxKind::Label | SyntaxKind::Ref => break current.range(),
                _ => node = current.parent().cloned(),
            }
        };

        // Both forms carry their delimiters in the source text.
        let name = source
            .text()
            .get(range)?
            .trim_start_matches(['<', '@'])
            .trim_end_matches('>');
        let label = typst::foundations::Label::new(typst::utils::PicoStr::intern(name))?;

        let introspector = document.introspector();
        let content = introspector.elements().query_label(label).ok()?;
        let position = introspector.position(content.location()?)?;
        Some(position.page.get())
    }
}
