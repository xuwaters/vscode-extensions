//! WASM entry points. The surface is intentionally flat and string-typed
//! so the TypeScript host can treat the module as "JSON in, JSON out".

use crate::diagnostics::{MakeDiagnostic, Severity};
use crate::features::{document_symbols, folding};
use crate::spans::{LineCol, SpanTable};
use crate::vfs::{FileUri, Workspace};
use serde::Serialize;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct Analyzer(RefCell<Workspace>);

#[derive(Serialize)]
struct LspDiagnostic {
    code: String,
    severity: String,
    message: String,
    start: LineCol,
    end: LineCol,
}

#[derive(Serialize)]
struct LspDocumentSymbol {
    name: String,
    detail: String,
    kind: String,
    range_start: LineCol,
    range_end: LineCol,
    selection_start: LineCol,
    selection_end: LineCol,
    children: Vec<LspDocumentSymbol>,
}

#[derive(Serialize)]
struct LspFoldingRange {
    start_line: u32,
    end_line: u32,
    kind: String,
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
            .map(|d| to_lsp_diag(d, &pf.source, &pf.spans))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn document_symbols(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else {
            return "[]".into();
        };
        let symbols = document_symbols::document_symbols(&pf.ast);
        let items: Vec<LspDocumentSymbol> = symbols
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
        let ranges = folding::folding_ranges(&pf.ast);
        let lsp: Vec<LspFoldingRange> = ranges
            .into_iter()
            .map(|r| {
                let start = pf.spans.offset_to_line_col(&pf.source, r.span.start);
                let end = pf.spans.offset_to_line_col(&pf.source, r.span.end);
                LspFoldingRange {
                    start_line: start.line,
                    end_line: end.line,
                    kind: format!("{:?}", r.kind),
                }
            })
            .collect();
        serde_json::to_string(&lsp).unwrap_or_else(|_| "[]".into())
    }
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

fn to_lsp_diag(d: MakeDiagnostic, source: &str, spans: &SpanTable) -> LspDiagnostic {
    LspDiagnostic {
        code: d.code.as_str().to_string(),
        severity: severity_str(d.severity).to_string(),
        message: d.message,
        start: spans.offset_to_line_col(source, d.span.start),
        end: spans.offset_to_line_col(source, d.span.end),
    }
}

fn to_lsp_symbol(
    s: document_symbols::DocumentSymbol,
    source: &str,
    spans: &SpanTable,
) -> LspDocumentSymbol {
    let children = s
        .children
        .into_iter()
        .map(|c| to_lsp_symbol(c, source, spans))
        .collect();
    LspDocumentSymbol {
        name: s.name,
        detail: s.detail,
        kind: format!("{:?}", s.kind),
        range_start: spans.offset_to_line_col(source, s.range.start),
        range_end: spans.offset_to_line_col(source, s.range.end),
        selection_start: spans.offset_to_line_col(source, s.selection_range.start),
        selection_end: spans.offset_to_line_col(source, s.selection_range.end),
        children,
    }
}
