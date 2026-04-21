//! WASM entry points. The surface is intentionally flat and string-typed so
//! the TypeScript host can treat the module as "JSON in, JSON out".

use crate::diagnostics::ProtoDiagnostic;
use crate::features::{completion, definition, document_symbols, folding, hover, workspace_symbols};
use crate::spans::{ByteSpan, LineCol, SpanTable};
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
struct LspLocation {
    file: String,
    start: LineCol,
    end: LineCol,
}

#[derive(Serialize)]
struct LspHover {
    markdown: String,
    start: LineCol,
    end: LineCol,
}

#[derive(Serialize)]
struct LspCompletionItem {
    label: String,
    insert_text: String,
    kind: String,
    detail: String,
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
        Analyzer(RefCell::new(Workspace::with_bundled_well_known_types()))
    }

    pub fn set_include_paths(&self, paths_json: &str) {
        let paths: Vec<String> = serde_json::from_str(paths_json).unwrap_or_default();
        self.0.borrow_mut().set_include_paths(paths);
    }

    pub fn update_file(&self, uri: &str, source: &str) -> String {
        let changed = self
            .0
            .borrow_mut()
            .update_file(FileUri::new(uri), source.to_string());
        serde_json::to_string(&changed).unwrap_or_else(|_| "{}".into())
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
        let diags = ws.diagnostics_for(&uri);
        let source = &pf.source;
        let spans = &pf.spans;
        let items: Vec<LspDiagnostic> =
            diags.into_iter().map(|d| to_lsp_diag(d, source, spans)).collect();
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

    pub fn workspace_symbols(&self, _query: &str) -> String {
        let ws = self.0.borrow();
        let items = workspace_symbols::collect(&ws);
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn definition(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "null".into() };
        let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line, col });
        let index = ws.build_index();
        match definition::definition(&ws, &index, &uri, offset) {
            Some(loc) => {
                let target_pf = ws.file(&FileUri::new(&loc.file));
                let (start, end) = to_line_col_range(target_pf, loc.range);
                let lsp = LspLocation { file: loc.file, start, end };
                serde_json::to_string(&lsp).unwrap_or_else(|_| "null".into())
            }
            None => "null".into(),
        }
    }

    pub fn hover(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "null".into() };
        let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line, col });
        let index = ws.build_index();
        match hover::hover(&ws, &index, &uri, offset) {
            Some(h) => {
                let start = pf.spans.offset_to_line_col(&pf.source, h.range.start);
                let end = pf.spans.offset_to_line_col(&pf.source, h.range.end);
                let lsp = LspHover { markdown: h.markdown, start, end };
                serde_json::to_string(&lsp).unwrap_or_else(|_| "null".into())
            }
            None => "null".into(),
        }
    }

    pub fn completion(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "[]".into() };
        let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line, col });
        let index = ws.build_index();
        let items = completion::completion(&ws, &index, &uri, offset);
        let lsp: Vec<LspCompletionItem> = items
            .into_iter()
            .map(|c| LspCompletionItem {
                label: c.label,
                insert_text: c.insert_text,
                kind: format!("{:?}", c.kind),
                detail: c.detail,
            })
            .collect();
        serde_json::to_string(&lsp).unwrap_or_else(|_| "[]".into())
    }

    pub fn folding_ranges(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "[]".into() };
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

    // Phase-3 stubs — stable boundary, empty payloads.
    pub fn references(&self, _uri: &str, _line: u32, _col: u32) -> String {
        "[]".into()
    }

    pub fn rename(&self, _uri: &str, _line: u32, _col: u32, _new_name: &str) -> String {
        "null".into()
    }
}

fn severity_str(s: crate::diagnostics::Severity) -> &'static str {
    use crate::diagnostics::Severity::*;
    match s {
        Error => "error",
        Warning => "warning",
        Info => "info",
        Hint => "hint",
    }
}

fn to_lsp_diag(d: ProtoDiagnostic, source: &str, spans: &SpanTable) -> LspDiagnostic {
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

fn to_line_col_range(pf: Option<&crate::parse::ParsedFile>, span: ByteSpan) -> (LineCol, LineCol) {
    match pf {
        Some(f) => (
            f.spans.offset_to_line_col(&f.source, span.start),
            f.spans.offset_to_line_col(&f.source, span.end),
        ),
        None => (LineCol { line: 0, col: 0 }, LineCol { line: 0, col: 0 }),
    }
}
