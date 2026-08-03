//! WASM entry points. The surface is intentionally flat and string-typed
//! so the TypeScript host can treat the module as "JSON in, JSON out".

use crate::features::formatting::FormatOptions;
use crate::features::{completion, document_symbols, folding, formatting, hover};
use crate::spans::LineCol;
use crate::vfs::{FileUri, Workspace};
use analyzer_core::lsp::{
    span_to_line_cols, to_lsp_diagnostic, LspDiagnostic, LspDocumentSymbol, LspFoldingRange,
    LspHover,
};
use serde::Serialize;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

/// A whole-range replacement. The dotenv formatter only ever produces
/// one, so there is no batch variant.
#[derive(Serialize)]
struct LspTextEdit {
    start: LineCol,
    end: LineCol,
    new_text: String,
}

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct Analyzer(RefCell<Workspace>);

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
        let symbols = document_symbols::document_symbols(&pf.ast, &pf.source);
        let items: Vec<LspDocumentSymbol> = symbols
            .into_iter()
            .map(|s| {
                let (range_start, range_end) = span_to_line_cols(s.range, &pf.source, &pf.spans);
                let (selection_start, selection_end) =
                    span_to_line_cols(s.selection_range, &pf.source, &pf.spans);
                LspDocumentSymbol {
                    name: s.name,
                    detail: s.detail,
                    kind: format!("{:?}", s.kind),
                    range_start,
                    range_end,
                    selection_start,
                    selection_end,
                    children: Vec::new(),
                }
            })
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn folding_ranges(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "[]".into();
        };
        let ranges = folding::folding_ranges(&pf.ast, &pf.source);
        let lsp: Vec<LspFoldingRange> = ranges
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

    /// `options_json` is a [`FormatOptions`] object; unparseable or
    /// partial JSON falls back to the defaults. Returns `null` when the
    /// file is already formatted or the formatter declined.
    pub fn formatting(&self, uri: &str, options_json: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "null".into();
        };
        let opts: FormatOptions = serde_json::from_str(options_json).unwrap_or_default();
        let Some(text) = formatting::format_file(pf, &opts) else {
            return "null".into();
        };
        let end = pf.spans.offset_to_line_col(&pf.source, pf.source.len() as u32);
        let lsp = LspTextEdit { start: LineCol { line: 0, col: 0 }, end, new_text: text };
        serde_json::to_string(&lsp).unwrap_or_else(|_| "null".into())
    }

    /// Formats the entries overlapping the inclusive line range
    /// `[start_line, end_line]`.
    pub fn formatting_range(
        &self,
        uri: &str,
        start_line: u32,
        end_line: u32,
        options_json: &str,
    ) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "null".into();
        };
        let opts: FormatOptions = serde_json::from_str(options_json).unwrap_or_default();
        let Some((span, text)) = formatting::format_range(pf, start_line, end_line, &opts) else {
            return "null".into();
        };
        let (start, end) = span_to_line_cols(span, &pf.source, &pf.spans);
        let lsp = LspTextEdit { start, end, new_text: text };
        serde_json::to_string(&lsp).unwrap_or_else(|_| "null".into())
    }

    pub fn hover(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "null".into();
        };
        let Some(h) = hover::hover(pf, LineCol { line, col }) else {
            return "null".into();
        };
        let (start, end) = span_to_line_cols(h.range, &pf.source, &pf.spans);
        let lsp = LspHover { contents: h.contents, start, end };
        serde_json::to_string(&lsp).unwrap_or_else(|_| "null".into())
    }
}
