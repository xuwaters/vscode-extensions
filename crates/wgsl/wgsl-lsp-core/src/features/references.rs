//! `textDocument/references` and `textDocument/documentHighlight`.
//!
//! Both answer the same question — where else does this name appear — and both
//! answer it by name rather than by resolved symbol.
//!
//! That is a deliberate imprecision. Scope-resolving every occurrence would
//! drop a use that a shadowing declaration hides, and in a single-file shader
//! the cost of showing one extra homonym (which the user can see) is far lower
//! than the cost of silently missing a use (which they cannot).

use lsp_types::{
    DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams, Location, ReferenceParams,
};

use crate::Server;
use crate::state::Document;

impl Server {
    pub fn references(&mut self, params: ReferenceParams) -> Option<Vec<Location>> {
        let position = params.text_document_position;
        let (document, offset) = self.locate(&position.text_document.uri, position.position)?;
        let name = name_at(document, offset)?;
        let include_declaration = params.context.include_declaration;

        Some(
            document
                .parsed()
                .occurrences(document.text(), name)
                .filter(|reference| include_declaration || !reference.is_declaration)
                .map(|reference| Location {
                    uri: document.uri.clone(),
                    range: document.range(reference.span),
                })
                .collect(),
        )
    }

    pub fn document_highlights(
        &mut self,
        params: DocumentHighlightParams,
    ) -> Option<Vec<DocumentHighlight>> {
        let position = params.text_document_position_params;
        let (document, offset) = self.locate(&position.text_document.uri, position.position)?;
        let name = name_at(document, offset)?;

        Some(
            document
                .parsed()
                .occurrences(document.text(), name)
                .map(|reference| DocumentHighlight {
                    range: document.range(reference.span),
                    kind: Some(if reference.is_declaration {
                        DocumentHighlightKind::WRITE
                    } else {
                        DocumentHighlightKind::READ
                    }),
                })
                .collect(),
        )
    }
}

/// The identifier under the cursor.
pub(crate) fn name_at(document: &Document, offset: u32) -> Option<&str> {
    let reference = document.parsed().reference_at(offset)?;
    Some(document.slice(reference.span))
}
