//! WASM entry points — `Analyzer` wraps a [`Workspace`] and exposes a flat,
//! string-typed JSON surface for the TypeScript host.
//!
//! Positions crossing the boundary are in LSP-style line/col (UTF-16).

use crate::diagnostics::CapnpDiagnostic;
use crate::features::{document_symbols, folding_ranges, FoldingRange, Symbol};
use crate::spans::{ByteSpan, LineCol, SpanTable};
use crate::vfs::Workspace;
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
    code: &'static str,
    severity: &'static str,
    message: String,
    start: LineCol,
    end: LineCol,
}

#[derive(Serialize)]
struct LspSymbol {
    name: String,
    detail: String,
    kind: &'static str,
    range_start: LineCol,
    range_end: LineCol,
    selection_start: LineCol,
    selection_end: LineCol,
    children: Vec<LspSymbol>,
}

#[derive(Serialize)]
struct LspFoldingRange {
    start_line: u32,
    end_line: u32,
    kind: &'static str,
}

#[wasm_bindgen]
impl Analyzer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Analyzer(RefCell::new(Workspace::new()))
    }

    pub fn update_file(&self, uri: &str, source: &str) {
        self.0.borrow_mut().update(uri, source.to_string());
    }

    pub fn remove_file(&self, uri: &str) {
        self.0.borrow_mut().remove(uri);
    }

    pub fn diagnostics(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let Some(state) = ws.get(uri) else { return "[]".into(); };
        let table = SpanTable::new(&state.source);
        let items: Vec<LspDiagnostic> = state
            .analysis
            .diagnostics
            .iter()
            .map(|d| diag_to_lsp(d, &state.source, &table))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn document_symbols(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let Some(state) = ws.get(uri) else { return "[]".into(); };
        let table = SpanTable::new(&state.source);
        let symbols = document_symbols(&state.analysis.file);
        let items: Vec<LspSymbol> = symbols
            .into_iter()
            .map(|s| symbol_to_lsp(s, &state.source, &table))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn folding_ranges(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let Some(state) = ws.get(uri) else { return "[]".into(); };
        let table = SpanTable::new(&state.source);
        let items: Vec<LspFoldingRange> = folding_ranges(&state.analysis.file)
            .into_iter()
            .map(|f| folding_to_lsp(f, &state.source, &table))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }
}

fn diag_to_lsp(d: &CapnpDiagnostic, source: &str, table: &SpanTable) -> LspDiagnostic {
    let (start, end) = span_to_linecol(d.span, source, table);
    LspDiagnostic {
        code: d.code,
        severity: match d.severity {
            crate::diagnostics::Severity::Error => "error",
            crate::diagnostics::Severity::Warning => "warning",
        },
        message: d.message.clone(),
        start,
        end,
    }
}

fn symbol_to_lsp(s: Symbol, source: &str, table: &SpanTable) -> LspSymbol {
    let (range_start, range_end) = span_to_linecol(s.range, source, table);
    let (selection_start, selection_end) = span_to_linecol(s.selection_range, source, table);
    LspSymbol {
        name: s.name,
        detail: s.detail,
        kind: symbol_kind_name(s.kind),
        range_start,
        range_end,
        selection_start,
        selection_end,
        children: s
            .children
            .into_iter()
            .map(|c| symbol_to_lsp(c, source, table))
            .collect(),
    }
}

fn folding_to_lsp(f: FoldingRange, source: &str, table: &SpanTable) -> LspFoldingRange {
    let (start, end) = span_to_linecol(f.start.join(f.end), source, table);
    LspFoldingRange {
        start_line: start.line,
        end_line: end.line,
        kind: f.kind,
    }
}

fn span_to_linecol(span: ByteSpan, source: &str, table: &SpanTable) -> (LineCol, LineCol) {
    (
        table.offset_to_line_col(source, span.start),
        table.offset_to_line_col(source, span.end),
    )
}

fn symbol_kind_name(k: crate::features::SymbolKind) -> &'static str {
    use crate::features::SymbolKind::*;
    match k {
        Struct => "struct",
        Enum => "enum",
        EnumMember => "enumMember",
        Interface => "interface",
        Method => "method",
        Field => "field",
        Union => "union",
        Group => "group",
        Constant => "constant",
        Annotation => "annotation",
        Namespace => "namespace",
    }
}
