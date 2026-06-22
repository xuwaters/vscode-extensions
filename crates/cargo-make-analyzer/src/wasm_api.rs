//! WASM entry points. The surface is intentionally flat and string-typed
//! so the TypeScript host can treat the module as "JSON in, JSON out".

use std::cell::RefCell;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::features::document_symbols::DocumentSymbol;
use crate::features::{completion, definition, document_symbols, folding, hover};
use crate::spans::{LineCol, SpanTable};
use crate::vfs::{FileUri, Workspace};
use analyzer_core::lsp::{
    span_to_line_cols, to_lsp_diagnostic, LspDiagnostic, LspDocumentSymbol, LspFoldingRange,
    LspHover,
};

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

#[derive(Serialize)]
struct LspLocation {
    start: LineCol,
    end: LineCol,
}

#[wasm_bindgen]
pub struct Analyzer(RefCell<Workspace>);

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Analyzer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Analyzer {
        Analyzer(RefCell::new(Workspace::new()))
    }

    pub fn update_file(&self, uri: &str, source: &str) {
        self.0
            .borrow_mut()
            .update_file(FileUri::new(uri), source.to_string());
    }

    pub fn remove_file(&self, uri: &str) {
        self.0.borrow_mut().remove_file(&FileUri::new(uri));
    }

    pub fn diagnostics(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "[]".into();
        };
        let items: Vec<LspDiagnostic> = ws
            .diagnostics_for(&uri)
            .into_iter()
            .map(|d| to_lsp_diagnostic(d, &pf.source, &pf.spans))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn document_symbols(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "[]".into();
        };
        let items: Vec<LspDocumentSymbol> = document_symbols::document_symbols(&pf.ast)
            .into_iter()
            .map(|s| to_lsp_symbol(s, &pf.source, &pf.spans))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn folding_ranges(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "[]".into();
        };
        let lsp: Vec<LspFoldingRange> = folding::folding_ranges(&pf.source)
            .into_iter()
            .map(|r| {
                let (start, end) = span_to_line_cols(r.span, &pf.source, &pf.spans);
                LspFoldingRange {
                    start_line: start.line,
                    end_line: end.line,
                    kind: format!("{:?}", r.kind),
                }
            })
            .collect();
        serde_json::to_string(&lsp).unwrap_or_else(|_| "[]".into())
    }

    pub fn complete(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "[]".into();
        };
        let items = completion::completions(pf, LineCol { line, col });
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn hover(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "null".into();
        };
        let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line, col });
        let Some(h) = hover::hover(&pf.ast, offset) else {
            return "null".into();
        };
        let (start, end) = span_to_line_cols(h.span, &pf.source, &pf.spans);
        let dto = LspHover { contents: h.markdown, start, end };
        serde_json::to_string(&dto).unwrap_or_else(|_| "null".into())
    }

    pub fn definition(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "null".into();
        };
        let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line, col });
        let Some(span) = definition::definition(&pf.ast, offset) else {
            return "null".into();
        };
        let (start, end) = span_to_line_cols(span, &pf.source, &pf.spans);
        serde_json::to_string(&LspLocation { start, end }).unwrap_or_else(|_| "null".into())
    }
}

fn to_lsp_symbol(s: DocumentSymbol, source: &str, spans: &SpanTable) -> LspDocumentSymbol {
    let (range_start, range_end) = span_to_line_cols(s.range, source, spans);
    let (selection_start, selection_end) = span_to_line_cols(s.selection_range, source, spans);
    LspDocumentSymbol {
        name: s.name,
        detail: s.detail,
        kind: format!("{:?}", s.kind),
        range_start,
        range_end,
        selection_start,
        selection_end,
        children: s
            .children
            .into_iter()
            .map(|c| to_lsp_symbol(c, source, spans))
            .collect(),
    }
}
