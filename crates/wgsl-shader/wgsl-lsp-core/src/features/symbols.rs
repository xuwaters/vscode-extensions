//! `textDocument/documentSymbol` and `workspace/symbol`.

use lsp_types::{
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, Location, OneOf,
    SymbolInformation, WorkspaceSymbol, WorkspaceSymbolParams, WorkspaceSymbolResponse,
};
use wgsl_syntax::{Parsed, SymbolKind};

use crate::Server;
use crate::features::lsp_symbol_kind;
use crate::state::Document;

impl Server {
    pub fn document_symbols(
        &mut self,
        params: DocumentSymbolParams,
    ) -> Option<DocumentSymbolResponse> {
        let document = self.document(&params.text_document.uri)?;
        let parsed = document.parsed();
        let symbols = parsed
            .roots
            .iter()
            .map(|&index| build(document, parsed, index))
            .collect();
        Some(DocumentSymbolResponse::Nested(symbols))
    }

    pub fn workspace_symbols(
        &mut self,
        params: WorkspaceSymbolParams,
    ) -> Option<WorkspaceSymbolResponse> {
        let query = &params.query;
        let mut symbols: Vec<WorkspaceSymbol> = Vec::new();

        // Open documents first: they are newer than anything the index holds,
        // and `did_open` already dropped their indexed copy.
        for document in self.documents() {
            let parsed = document.parsed();
            for symbol in &parsed.symbols {
                if symbol.kind.is_local() || crate::index::score(&symbol.name, query).is_none()
                {
                    continue;
                }
                symbols.push(WorkspaceSymbol {
                    name: symbol.name.clone(),
                    kind: lsp_symbol_kind(symbol.kind),
                    tags: None,
                    container_name: symbol
                        .parent
                        .map(|parent| parsed.symbols[parent].name.clone()),
                    location: OneOf::Left(Location {
                        uri: document.uri.clone(),
                        range: document.range(symbol.name_span),
                    }),
                    data: None,
                });
            }
        }

        for entry in self.index().search(query) {
            symbols.push(WorkspaceSymbol {
                name: entry.name.clone(),
                kind: lsp_symbol_kind(entry.kind),
                tags: None,
                container_name: entry.container.clone(),
                location: OneOf::Left(Location {
                    uri: entry.uri.clone(),
                    range: entry.selection_range,
                }),
                data: None,
            });
        }

        Some(WorkspaceSymbolResponse::Nested(symbols))
    }
}

/// One outline entry and its children.
fn build(document: &Document, parsed: &Parsed, index: usize) -> DocumentSymbol {
    let symbol = &parsed.symbols[index];
    let children: Vec<DocumentSymbol> = symbol
        .children
        .iter()
        // Locals belong to the reader of the function, not the reader of the
        // outline; parameters and struct fields do belong there.
        .filter(|&&child| parsed.symbols[child].kind != SymbolKind::Local)
        .map(|&child| build(document, parsed, child))
        .collect();

    #[allow(deprecated)] // `DocumentSymbol::deprecated` is required by the struct.
    DocumentSymbol {
        name: symbol.name.clone(),
        detail: (!symbol.detail.is_empty()).then(|| symbol.detail.clone()),
        kind: lsp_symbol_kind(symbol.kind),
        tags: None,
        deprecated: None,
        range: document.range(symbol.full_span),
        selection_range: document.range(symbol.name_span),
        children: (!children.is_empty()).then_some(children),
    }
}

/// Kept so a client that only understands the flat response shape still gets
/// something useful. `SymbolInformation` is deprecated upstream but remains
/// the only thing some editors accept.
#[allow(deprecated)]
pub fn flatten(document: &Document, parsed: &Parsed) -> Vec<SymbolInformation> {
    parsed
        .symbols
        .iter()
        .filter(|symbol| !symbol.kind.is_local())
        .map(|symbol| SymbolInformation {
            name: symbol.name.clone(),
            kind: lsp_symbol_kind(symbol.kind),
            tags: None,
            deprecated: None,
            location: Location {
                uri: document.uri.clone(),
                range: document.range(symbol.name_span),
            },
            container_name: symbol
                .parent
                .map(|parent| parsed.symbols[parent].name.clone()),
        })
        .collect()
}
