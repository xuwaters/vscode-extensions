//! WASM entry points. The surface is flat and string-typed so the
//! TypeScript host treats the module as "JSON in, JSON out". This is the
//! only module that converts byte offsets to UTF-16 line/column.

use crate::features::document_symbols::{document_symbols, DocumentSymbol};
use crate::features::folding::folding_ranges;
use crate::features::formatting::{format_file, FormatOptions};
use crate::features::hover::hover;
use crate::features::selection::selection_ranges;
use crate::spans::{LineCol, SpanTable};
use crate::workspace::{FileUri, ParsedFile, Workspace};
use analyzer_core::lsp::{
    span_to_line_cols, to_lsp_diagnostic, LspDiagnostic, LspDocumentSymbol, LspFoldingRange, LspHover,
};
use serde::Serialize;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct LspTextEdit {
    start: LineCol,
    end: LineCol,
    new_text: String,
}

#[derive(Serialize)]
struct LspRange {
    start: LineCol,
    end: LineCol,
}

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
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
        self.0.borrow_mut().update_file(FileUri::new(uri), source.to_string());
    }

    pub fn remove_file(&self, uri: &str) {
        self.0.borrow_mut().remove_file(&FileUri::new(uri));
    }

    pub fn diagnostics(&self, uri: &str) -> String {
        self.with_file(uri, "[]", |pf| {
            let items: Vec<LspDiagnostic> = pf
                .diagnostics
                .iter()
                .cloned()
                .map(|d| to_lsp_diagnostic(d, &pf.source, &pf.spans))
                .collect();
            serde_json::to_string(&items).ok()
        })
    }

    pub fn document_symbols(&self, uri: &str) -> String {
        self.with_file(uri, "[]", |pf| {
            let items: Vec<LspDocumentSymbol> = document_symbols(&pf.ast, &pf.source)
                .into_iter()
                .map(|s| to_lsp_symbol(s, &pf.source, &pf.spans))
                .collect();
            serde_json::to_string(&items).ok()
        })
    }

    pub fn folding_ranges(&self, uri: &str) -> String {
        self.with_file(uri, "[]", |pf| {
            let items: Vec<LspFoldingRange> = folding_ranges(&pf.ast, &pf.source, &pf.spans)
                .into_iter()
                .map(|r| LspFoldingRange { start_line: r.start_line, end_line: r.end_line, kind: format!("{:?}", r.kind) })
                .collect();
            serde_json::to_string(&items).ok()
        })
    }

    pub fn hover(&self, uri: &str, line: u32, col: u32) -> String {
        self.with_file(uri, "null", |pf| {
            let h = hover(pf, LineCol { line, col })?;
            let (start, end) = span_to_line_cols(h.range, &pf.source, &pf.spans);
            serde_json::to_string(&LspHover { contents: h.contents, start, end }).ok()
        })
    }

    /// `positions_json` is `[{line, col}, ...]`; the result holds one
    /// innermost-first chain of ranges per position.
    pub fn selection_ranges(&self, uri: &str, positions_json: &str) -> String {
        self.with_file(uri, "[]", |pf| {
            let positions: Vec<LineCol> = serde_json::from_str(positions_json).ok()?;
            let chains: Vec<Vec<LspRange>> = positions
                .into_iter()
                .map(|pos| {
                    selection_ranges(pf, pos)
                        .into_iter()
                        .map(|span| {
                            let (start, end) = span_to_line_cols(span, &pf.source, &pf.spans);
                            LspRange { start, end }
                        })
                        .collect()
                })
                .collect();
            serde_json::to_string(&chains).ok()
        })
    }

    /// `options_json` is a [`FormatOptions`]; missing fields take their
    /// defaults. Returns `null` when the file is already formatted or the
    /// formatter declined.
    pub fn formatting(&self, uri: &str, options_json: &str) -> String {
        self.with_file(uri, "null", |pf| {
            let opts: FormatOptions = serde_json::from_str(options_json).unwrap_or_default();
            let text = format_file(pf, &opts)?;
            let end = pf.spans.offset_to_line_col(&pf.source, pf.source.len() as u32);
            serde_json::to_string(&LspTextEdit { start: LineCol { line: 0, col: 0 }, end, new_text: text }).ok()
        })
    }
}

impl Analyzer {
    fn with_file(&self, uri: &str, empty: &str, f: impl FnOnce(&ParsedFile) -> Option<String>) -> String {
        let ws = self.0.borrow();
        ws.file(&FileUri::new(uri)).and_then(f).unwrap_or_else(|| empty.to_string())
    }
}

fn to_lsp_symbol(s: DocumentSymbol, source: &str, spans: &SpanTable) -> LspDocumentSymbol {
    let (range_start, range_end) = span_to_line_cols(s.range, source, spans);
    let (selection_start, selection_end) = span_to_line_cols(s.selection_range, source, spans);
    LspDocumentSymbol {
        name: s.name,
        detail: s.detail,
        kind: s.kind.to_string(),
        range_start,
        range_end,
        selection_start,
        selection_end,
        children: s.children.into_iter().map(|c| to_lsp_symbol(c, source, spans)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URI: &str = "file:///t.ron";

    fn loaded(src: &str) -> Analyzer {
        let a = Analyzer::new();
        a.update_file(URI, src);
        a
    }

    #[test]
    fn unknown_files_answer_empty() {
        let a = Analyzer::new();
        assert_eq!(a.diagnostics(URI), "[]");
        assert_eq!(a.hover(URI, 0, 0), "null");
        assert_eq!(a.formatting(URI, "{}"), "null");
        assert_eq!(a.selection_ranges(URI, "[]"), "[]");
    }

    #[test]
    fn diagnostics_are_line_col() {
        let json = loaded("(\n  a: 1,\n  a: 2,\n)").diagnostics(URI);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["code"], "RON009");
        assert_eq!(v[0]["severity"], "error");
        assert_eq!(v[0]["start"], serde_json::json!({"line": 2, "col": 2}));
    }

    #[test]
    fn formatting_replaces_the_whole_file() {
        let json = loaded("(a:1,\nb:2)").formatting(URI, r#"{"tab_size": 2}"#);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["new_text"], "(a: 1, b: 2)\n");
        assert_eq!(v["end"], serde_json::json!({"line": 1, "col": 4}));
    }

    #[test]
    fn symbols_nest() {
        let json = loaded("Config(window: (w: 1))").document_symbols(URI);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["name"], "Config");
        assert_eq!(v[0]["children"][0]["name"], "window");
        assert_eq!(v[0]["children"][0]["children"][0]["kind"], "Field");
    }

    #[test]
    fn selection_ranges_per_position() {
        let json = loaded("(a: [1])").selection_ranges(URI, r#"[{"line":0,"col":5}]"#);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0].as_array().unwrap().len(), 4);
        assert_eq!(v[0][0]["start"], serde_json::json!({"line": 0, "col": 5}));
    }

    #[test]
    fn folding_kinds_are_named() {
        let json = loaded("[\n1,\n2,\n]").folding_ranges(URI);
        assert_eq!(json, r#"[{"start_line":0,"end_line":2,"kind":"Region"}]"#);
    }
}
