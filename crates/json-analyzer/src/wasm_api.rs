//! WASM entry points. The surface is intentionally flat and string-typed
//! so the TypeScript host can treat the module as "JSON in, JSON out".
//! Mirrored by `extensions/json-ultra/src/types.ts`.

use crate::features::document_symbols::{self, DocSymbol};
use crate::features::folding::{self, FoldKind};
use crate::features::formatting::{self, FormatOptions};
use crate::features::hover;
use crate::features::table;
use crate::flavor::Flavor;
use crate::spans::LineCol;
use crate::workspace::{FileUri, Workspace};
use analyzer_core::lsp::{
    span_to_line_cols, to_lsp_diagnostic, LspDiagnostic, LspDocumentSymbol, LspFoldingRange,
    LspHover,
};
use serde::Serialize;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

/// A whole-range replacement. The formatter only ever produces one.
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

    /// `language_id` is the VSCode language id (`json`, `jsonc`,
    /// `json5`, `jsonl`); anything unknown parses as strict JSON.
    pub fn update_file(&self, uri: &str, source: &str, language_id: &str) {
        self.0.borrow_mut().update_file(
            FileUri::new(uri),
            source.to_string(),
            Flavor::from_language_id(language_id),
        );
    }

    pub fn remove_file(&self, uri: &str) {
        self.0.borrow_mut().remove_file(&FileUri::new(uri));
    }

    pub fn diagnostics(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let Some(pf) = ws.file(&FileUri::new(uri)) else {
            return "[]".into();
        };
        let items: Vec<LspDiagnostic> = pf
            .diagnostics
            .iter()
            .cloned()
            .map(|d| to_lsp_diagnostic(d, &pf.source, &pf.spans))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn document_symbols(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let Some(pf) = ws.file(&FileUri::new(uri)) else {
            return "[]".into();
        };
        let items: Vec<LspDocumentSymbol> = document_symbols::document_symbols(pf)
            .into_iter()
            .map(|s| to_lsp_symbol(s, &pf.source, &pf.spans))
            .collect();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
    }

    pub fn folding_ranges(&self, uri: &str) -> String {
        let ws = self.0.borrow();
        let Some(pf) = ws.file(&FileUri::new(uri)) else {
            return "[]".into();
        };
        let lsp: Vec<LspFoldingRange> = folding::folding_ranges(pf)
            .into_iter()
            .filter_map(|r| {
                let (start, end) = span_to_line_cols(r.span, &pf.source, &pf.spans);
                // A fold that starts and ends on one line is noise.
                if start.line == end.line {
                    return None;
                }
                let kind = match r.kind {
                    FoldKind::Region => "Region",
                    FoldKind::Comment => "Comment",
                };
                Some(LspFoldingRange {
                    start_line: start.line,
                    end_line: end.line,
                    kind: kind.to_string(),
                })
            })
            .collect();
        serde_json::to_string(&lsp).unwrap_or_else(|_| "[]".into())
    }

    pub fn hover(&self, uri: &str, line: u32, col: u32) -> String {
        let ws = self.0.borrow();
        let Some(pf) = ws.file(&FileUri::new(uri)) else {
            return "null".into();
        };
        let offset = pf.spans.line_col_to_offset(&pf.source, LineCol { line, col });
        let Some(h) = hover::hover(pf, offset) else {
            return "null".into();
        };
        let (start, end) = span_to_line_cols(h.range, &pf.source, &pf.spans);
        let lsp = LspHover { contents: h.contents, start, end };
        serde_json::to_string(&lsp).unwrap_or_else(|_| "null".into())
    }

    /// `options_json` is a [`FormatOptions`] object; unparseable or
    /// partial JSON falls back to the defaults. Returns `null` when the
    /// file is already formatted or the formatter declined.
    pub fn formatting(&self, uri: &str, options_json: &str) -> String {
        self.format_impl(uri, options_json, false)
    }

    /// Formatting with `sort_keys` forced on — the "Sort Object Keys"
    /// command, independent of the user's format-time sort setting.
    pub fn sort_keys(&self, uri: &str, options_json: &str) -> String {
        self.format_impl(uri, options_json, true)
    }

    fn format_impl(&self, uri: &str, options_json: &str, force_sort: bool) -> String {
        let ws = self.0.borrow();
        let Some(pf) = ws.file(&FileUri::new(uri)) else {
            return "null".into();
        };
        let mut opts: FormatOptions = serde_json::from_str(options_json).unwrap_or_default();
        if force_sort {
            opts.sort_keys = true;
        }
        let Some(text) = formatting::format_file(pf, &opts) else {
            return "null".into();
        };
        let end = pf.spans.offset_to_line_col(&pf.source, pf.source.len() as u32);
        let lsp = LspTextEdit { start: LineCol { line: 0, col: 0 }, end, new_text: text };
        serde_json::to_string(&lsp).unwrap_or_else(|_| "null".into())
    }

    /// Table extraction for the JSON Lines preview. Returns a
    /// `Table` object; `null` for a file that is not loaded.
    pub fn jsonl_table(&self, uri: &str, max_rows: u32) -> String {
        let ws = self.0.borrow();
        let Some(pf) = ws.file(&FileUri::new(uri)) else {
            return "null".into();
        };
        let t = table::jsonl_table(pf, max_rows as usize);
        serde_json::to_string(&t).unwrap_or_else(|_| "null".into())
    }
}

fn to_lsp_symbol(
    s: DocSymbol,
    source: &str,
    spans: &crate::spans::SpanTable,
) -> LspDocumentSymbol {
    let (range_start, range_end) = span_to_line_cols(s.range, source, spans);
    let (selection_start, selection_end) = span_to_line_cols(s.selection, source, spans);
    LspDocumentSymbol {
        name: s.name,
        detail: s.detail,
        kind: s.kind.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn analyzer_with(source: &str, language: &str) -> Analyzer {
        let a = Analyzer::new();
        a.update_file("test://f", source, language);
        a
    }

    #[test]
    fn json_in_json_out_roundtrip() {
        let a = analyzer_with("{\"b\": 1, \"a\": 2}", "json");
        assert_eq!(a.diagnostics("test://f"), "[]");
        let symbols: serde_json::Value =
            serde_json::from_str(&a.document_symbols("test://f")).unwrap();
        assert_eq!(symbols.as_array().unwrap().len(), 2);
        let edit: serde_json::Value = serde_json::from_str(&a.sort_keys("test://f", "{}")).unwrap();
        assert_eq!(edit["new_text"], "{\n  \"a\": 2,\n  \"b\": 1\n}\n");
    }

    #[test]
    fn missing_file_returns_fallbacks() {
        let a = Analyzer::new();
        assert_eq!(a.diagnostics("nope"), "[]");
        assert_eq!(a.document_symbols("nope"), "[]");
        assert_eq!(a.folding_ranges("nope"), "[]");
        assert_eq!(a.hover("nope", 0, 0), "null");
        assert_eq!(a.formatting("nope", "{}"), "null");
        assert_eq!(a.jsonl_table("nope", 10), "null");
    }

    #[test]
    fn garbage_options_fall_back_to_defaults() {
        let a = analyzer_with("{\"a\":1}", "json");
        let edit: serde_json::Value =
            serde_json::from_str(&a.formatting("test://f", "not json")).unwrap();
        assert_eq!(edit["new_text"], "{\n  \"a\": 1\n}\n");
    }

    #[test]
    fn jsonl_table_serializes() {
        let a = analyzer_with("{\"a\": 1}\n{\"a\": 2, \"b\": 3}\n", "jsonl");
        let t: serde_json::Value = serde_json::from_str(&a.jsonl_table("test://f", 100)).unwrap();
        assert_eq!(t["columns"], serde_json::json!(["a", "b"]));
        assert_eq!(t["total"], 2);
        assert_eq!(t["rows"][1]["cells"], serde_json::json!(["2", "3"]));
    }

    #[test]
    fn single_line_containers_do_not_fold() {
        let a = analyzer_with("{\"a\": [1, 2]}", "json");
        assert_eq!(a.folding_ranges("test://f"), "[]");
    }

    #[test]
    fn remove_file_forgets() {
        let a = analyzer_with("{}", "json");
        a.remove_file("test://f");
        assert_eq!(a.diagnostics("test://f"), "[]");
    }
}
