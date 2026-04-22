//! WASM entry points. The surface is intentionally flat and string-typed so
//! the TypeScript host can treat the module as "JSON in, JSON out".

use crate::diagnostics::{ProtoDiagnostic, StyleConfig};
use crate::features::{
    code_actions, completion, definition, document_symbols, folding, formatting, hover,
    inlay_hints, references, rename, semantic_tokens, workspace_symbols,
};
use crate::resolve::ReferenceIndex;
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

#[derive(Serialize)]
struct LspTextEdit {
    file: String,
    start: LineCol,
    end: LineCol,
    new_text: String,
}

#[derive(Serialize)]
struct LspRange {
    start: LineCol,
    end: LineCol,
}

#[derive(Serialize)]
struct LspInlayHint {
    line: u32,
    col: u32,
    label: String,
}

#[derive(Serialize)]
struct LspSemanticToken {
    line: u32,
    col: u32,
    length: u32,
    token_type: String,
}

#[derive(Serialize)]
struct LspCodeAction {
    title: String,
    kind: String,
    edits: Vec<LspTextEdit>,
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

    pub fn set_style_enabled(&self, enabled: bool) {
        self.0.borrow_mut().set_style_config(StyleConfig { enabled });
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

    pub fn references(&self, uri: &str, line: u32, col: u32, include_declaration: bool) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "[]".into() };
        let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line, col });
        let index = ws.build_index();
        let ref_index = ReferenceIndex::build(&ws, &index);
        let refs = references::references(&ws, &index, &ref_index, &uri, offset, include_declaration);
        let lsp: Vec<LspTextEdit> = refs
            .into_iter()
            .map(|r| {
                let target_pf = ws.file(&FileUri::new(&r.file));
                let (start, end) = to_line_col_range(target_pf, r.range);
                LspTextEdit { file: r.file, start, end, new_text: String::new() }
            })
            .collect();
        serde_json::to_string(&lsp).unwrap_or_else(|_| "[]".into())
    }

    pub fn prepare_rename(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "null".into() };
        let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line, col });
        let index = ws.build_index();
        match rename::prepare_rename(&ws, &index, &uri, offset) {
            Some(span) => {
                let start = pf.spans.offset_to_line_col(&pf.source, span.start);
                let end = pf.spans.offset_to_line_col(&pf.source, span.end);
                serde_json::to_string(&LspRange { start, end }).unwrap_or_else(|_| "null".into())
            }
            None => "null".into(),
        }
    }

    pub fn rename(&self, uri: &str, line: u32, col: u32, new_name: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "null".into() };
        let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line, col });
        let index = ws.build_index();
        let Some(edit) = rename::rename(&ws, &index, &uri, offset, new_name) else {
            return "null".into();
        };
        let mut out: Vec<LspTextEdit> = Vec::new();
        for (file, edits) in &edit.changes {
            let target_pf = ws.file(&FileUri::new(file));
            for e in edits {
                let (start, end) = to_line_col_range(target_pf, e.range);
                out.push(LspTextEdit {
                    file: file.clone(),
                    start,
                    end,
                    new_text: e.new_text.clone(),
                });
            }
        }
        serde_json::to_string(&out).unwrap_or_else(|_| "null".into())
    }

    pub fn formatting(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "null".into() };
        let Some(text) = formatting::format_file(pf) else { return "null".into() };
        let end = pf.spans.offset_to_line_col(&pf.source, pf.source.len() as u32);
        let lsp = LspTextEdit {
            file: uri.as_str().to_string(),
            start: LineCol { line: 0, col: 0 },
            end,
            new_text: text,
        };
        serde_json::to_string(&lsp).unwrap_or_else(|_| "null".into())
    }

    pub fn inlay_hints(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "[]".into() };
        let index = ws.build_index();
        let hints = inlay_hints::inlay_hints(&ws, &index, &uri);
        let lsp: Vec<LspInlayHint> = hints
            .into_iter()
            .map(|h| {
                let lc = pf.spans.offset_to_line_col(&pf.source, h.at.start);
                LspInlayHint { line: lc.line, col: lc.col, label: h.label }
            })
            .collect();
        serde_json::to_string(&lsp).unwrap_or_else(|_| "[]".into())
    }

    pub fn semantic_tokens(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "[]".into() };
        let index = ws.build_index();
        let tokens = semantic_tokens::semantic_tokens(&ws, &index, &uri);
        let lsp: Vec<LspSemanticToken> = tokens
            .into_iter()
            .map(|t| {
                let start = pf.spans.offset_to_line_col(&pf.source, t.span.start);
                LspSemanticToken {
                    line: start.line,
                    col: start.col,
                    length: t.span.len(),
                    token_type: format!("{:?}", t.ty),
                }
            })
            .collect();
        serde_json::to_string(&lsp).unwrap_or_else(|_| "[]".into())
    }

    pub fn code_actions(&self, uri: &str, line: u32, col: u32, diag_codes_json: &str) -> String {
        let ws = self.0.borrow();
        let uri = FileUri::new(uri);
        let Some(pf) = ws.file(&uri) else { return "[]".into() };
        let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line, col });
        let codes: Vec<String> = serde_json::from_str(diag_codes_json).unwrap_or_default();
        let index = ws.build_index();
        let actions = code_actions::code_actions(&ws, &index, &uri, offset, &codes);
        let lsp: Vec<LspCodeAction> = actions
            .into_iter()
            .map(|a| LspCodeAction {
                title: a.title,
                kind: a.kind,
                edits: a
                    .edits
                    .into_iter()
                    .map(|e| {
                        let target_pf = ws.file(&FileUri::new(&e.file));
                        let (start, end) = to_line_col_range(target_pf, e.range);
                        LspTextEdit { file: e.file, start, end, new_text: e.new_text }
                    })
                    .collect(),
            })
            .collect();
        serde_json::to_string(&lsp).unwrap_or_else(|_| "[]".into())
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
