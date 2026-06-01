//! WASM entry points — `Analyzer` wraps a [`Workspace`] and exposes a flat,
//! string-typed JSON surface for the TypeScript host.
//!
//! Positions crossing the boundary are in LSP-style line/col (UTF-16).

use crate::diagnostics::{workspace_diagnostics, MojomDiagnostic, Severity};
use crate::features::completion::{completion, CompletionItem};
use crate::features::definition::definition;
use crate::features::hover::hover;
use crate::features::symbols::{document_symbols, folding_ranges, FoldingRange, Symbol, SymbolKind};
use crate::features::workspace_symbols::{workspace_symbols, WorkspaceSymbolItem};
use crate::resolve::WorkspaceIndex;
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

#[derive(Serialize)]
struct LspHover {
    markdown: String,
    start: LineCol,
    end: LineCol,
}

#[derive(Serialize)]
struct LspLocation {
    file: String,
    start: LineCol,
    end: LineCol,
}

#[derive(Serialize)]
struct LspCompletionItem {
    label: String,
    insert_text: String,
    kind: &'static str,
    detail: String,
}

#[derive(Serialize)]
struct LspWorkspaceSymbol {
    name: String,
    fqn: String,
    kind: &'static str,
    file: String,
    start: LineCol,
    end: LineCol,
    detail: Option<String>,
}

#[wasm_bindgen]
impl Analyzer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Analyzer(RefCell::new(Workspace::new()))
    }

    pub fn set_include_paths(&self, json: &str) {
        let paths: Vec<String> = serde_json::from_str(json).unwrap_or_default();
        self.0.borrow_mut().set_include_paths(paths);
    }

    pub fn update_file(&self, uri: &str, source: &str) {
        self.0.borrow_mut().update(uri, source.to_string());
    }

    pub fn preload_file(&self, uri: &str, source: &str) {
        self.0.borrow_mut().preload(uri, source.to_string());
    }

    pub fn remove_file(&self, uri: &str) {
        self.0.borrow_mut().remove(uri);
    }

    pub fn diagnostics(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let Some(state) = ws.get(uri) else { return "[]".into() };
        let table = SpanTable::new(&state.source);
        let mut items: Vec<LspDiagnostic> = state
            .analysis
            .diagnostics
            .iter()
            .map(|d| diag_to_lsp(d, &state.source, &table))
            .collect();
        let index = WorkspaceIndex::build(&ws);
        let file_uri = FileUri(uri.to_string());
        for d in workspace_diagnostics(&ws, &index, &file_uri) {
            items.push(diag_to_lsp(&d, &state.source, &table));
        }
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn document_symbols(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let Some(state) = ws.get(uri) else { return "[]".into() };
        let table = SpanTable::new(&state.source);
        let items: Vec<LspSymbol> = document_symbols(&state.analysis.file)
            .into_iter()
            .map(|s| symbol_to_lsp(s, &state.source, &table))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn folding_ranges(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let Some(state) = ws.get(uri) else { return "[]".into() };
        let table = SpanTable::new(&state.source);
        let items: Vec<LspFoldingRange> = folding_ranges(&state.analysis.file)
            .into_iter()
            .map(|f| folding_to_lsp(f, &state.source, &table))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn hover(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let Some(state) = ws.get(uri) else { return "null".into() };
        let table = SpanTable::new(&state.source);
        let offset = table.line_col_to_offset(&state.source, LineCol { line, col });
        let file_uri = FileUri(uri.to_string());
        let index = WorkspaceIndex::build(&ws);
        let Some(h) = hover(&ws, &index, &file_uri, offset) else { return "null".into() };
        let (start, end) = span_to_linecol(h.range, &state.source, &table);
        serde_json::to_string(&LspHover { markdown: h.markdown, start, end })
            .unwrap_or_else(|_| "null".into())
    }

    pub fn definition(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let Some(state) = ws.get(uri) else { return "null".into() };
        let table = SpanTable::new(&state.source);
        let offset = table.line_col_to_offset(&state.source, LineCol { line, col });
        let file_uri = FileUri(uri.to_string());
        let index = WorkspaceIndex::build(&ws);
        let Some(loc) = definition(&ws, &index, &file_uri, offset) else { return "null".into() };
        let (start, end) = if let Some(ts) = ws.get(&loc.file) {
            let t_table = SpanTable::new(&ts.source);
            span_to_linecol(loc.range, &ts.source, &t_table)
        } else {
            let lc = LineCol { line: 0, col: 0 };
            (lc, lc)
        };
        serde_json::to_string(&LspLocation { file: loc.file, start, end })
            .unwrap_or_else(|_| "null".into())
    }

    pub fn completion(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let Some(state) = ws.get(uri) else { return "[]".into() };
        let table = SpanTable::new(&state.source);
        let offset = table.line_col_to_offset(&state.source, LineCol { line, col });
        let file_uri = FileUri(uri.to_string());
        let index = WorkspaceIndex::build(&ws);
        let items: Vec<LspCompletionItem> = completion(&ws, &index, &file_uri, offset)
            .into_iter()
            .map(completion_to_lsp)
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn workspace_symbols(&self, query: &str) -> String {
        let ws = self.0.borrow();
        let index = WorkspaceIndex::build(&ws);
        let items: Vec<LspWorkspaceSymbol> = workspace_symbols(&index, query)
            .into_iter()
            .filter_map(|s| workspace_symbol_to_lsp(s, &ws))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn diag_to_lsp(d: &MojomDiagnostic, source: &str, table: &SpanTable) -> LspDiagnostic {
    let (start, end) = span_to_linecol(d.span, source, table);
    LspDiagnostic {
        code: d.code,
        severity: match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
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
        children: s.children.into_iter().map(|c| symbol_to_lsp(c, source, table)).collect(),
    }
}

fn folding_to_lsp(f: FoldingRange, source: &str, table: &SpanTable) -> LspFoldingRange {
    let (start, end) = span_to_linecol(f.start.join(f.end), source, table);
    LspFoldingRange { start_line: start.line, end_line: end.line, kind: f.kind }
}

fn completion_to_lsp(c: CompletionItem) -> LspCompletionItem {
    LspCompletionItem { label: c.label, insert_text: c.insert_text, kind: c.kind, detail: c.detail }
}

fn workspace_symbol_to_lsp(s: WorkspaceSymbolItem, ws: &Workspace) -> Option<LspWorkspaceSymbol> {
    let state = ws.get(&s.file)?;
    let table = SpanTable::new(&state.source);
    let (start, end) = span_to_linecol(s.range, &state.source, &table);
    Some(LspWorkspaceSymbol {
        name: s.name,
        fqn: s.fqn,
        kind: s.kind,
        file: s.file,
        start,
        end,
        detail: s.detail,
    })
}

fn span_to_linecol(span: ByteSpan, source: &str, table: &SpanTable) -> (LineCol, LineCol) {
    (table.offset_to_line_col(source, span.start), table.offset_to_line_col(source, span.end))
}

fn symbol_kind_name(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Module => "module",
        SymbolKind::Struct => "struct",
        SymbolKind::Union => "union",
        SymbolKind::Interface => "interface",
        SymbolKind::Enum => "enum",
        SymbolKind::EnumMember => "enumMember",
        SymbolKind::Method => "method",
        SymbolKind::Field => "field",
        SymbolKind::Constant => "constant",
    }
}
