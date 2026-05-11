//! WASM adapter over `log-engine`.
//!
//! Two surfaces are exported:
//!
//! - `LogIndex`: the original whole-document API. Kept for the small-file
//!   fast path (logs under `logViewer.streamingThresholdBytes`); the host
//!   reads `TextDocument.getText()`, constructs one of these, and queries
//!   it for rendered lines, filter matches, or search hits — unchanged
//!   from earlier releases.
//! - Stateless `render_lines` / `match_lines` / `search_lines` /
//!   `find_newlines`: the streaming path's interface. The host owns the
//!   file (reads byte ranges from disk), calls these on byte slabs, and
//!   never asks WASM to hold the file in memory.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use log_engine::{ansi, filter, index as eng_index, render};

#[wasm_bindgen]
pub fn init() {
    console_error_panic_hook::set_once();
}

#[derive(Serialize)]
struct LinesPayload<'a> {
    html: Vec<&'a str>,
    text: Vec<&'a str>,
}

/// Owned, ANSI-parsed log file. Kept for the small-file path. For files
/// above the streaming threshold the host bypasses this and uses the
/// stateless byte-slab API below.
#[wasm_bindgen]
pub struct LogIndex {
    lines: Vec<ansi::Line>,
}

#[wasm_bindgen]
impl LogIndex {
    #[wasm_bindgen(constructor)]
    pub fn new(text: &str) -> LogIndex {
        LogIndex {
            lines: ansi::parse_lines(text),
        }
    }

    #[wasm_bindgen(getter, js_name = lineCount)]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    #[wasm_bindgen(js_name = allLinesJson)]
    pub fn all_lines_json(&self) -> Result<String, JsError> {
        self.range_json(0, self.lines.len())
    }

    #[wasm_bindgen(js_name = renderRange)]
    pub fn render_range(&self, start: usize, end: usize) -> Result<String, JsError> {
        self.range_json(start, end)
    }

    fn range_json(&self, start: usize, end: usize) -> Result<String, JsError> {
        let end = end.min(self.lines.len());
        let start = start.min(end);
        let slice = &self.lines[start..end];
        let payload = LinesPayload {
            html: slice.iter().map(|l| l.html.as_str()).collect(),
            text: slice.iter().map(|l| l.text.as_str()).collect(),
        };
        serde_json::to_string(&payload)
            .map_err(|e| JsError::new(&format!("serialize lines: {}", e)))
    }

    #[wasm_bindgen(js_name = matchFilters)]
    pub fn match_filters(&self, rules_json: &str) -> Result<Vec<u8>, JsError> {
        let rules: Vec<filter::Rule> = serde_json::from_str(rules_json)
            .map_err(|e| JsError::new(&format!("parse rules: {}", e)))?;
        let matchers = filter::compile(&rules).map_err(|e| JsError::new(&e))?;
        let mut out = vec![0u8; self.lines.len()];
        for (i, line) in self.lines.iter().enumerate() {
            for (j, m) in matchers.iter().enumerate() {
                if m.is_match(&line.text) {
                    out[i] = (j as u8).saturating_add(1);
                    break;
                }
            }
        }
        Ok(out)
    }

    #[wasm_bindgen]
    pub fn search(
        &self,
        query: &str,
        regex: bool,
        case_sensitive: bool,
    ) -> Result<Vec<u32>, JsError> {
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let m = filter::build_search(query, regex, case_sensitive).map_err(|e| JsError::new(&e))?;
        let mut out = Vec::new();
        for (i, line) in self.lines.iter().enumerate() {
            if m.is_match(&line.text) {
                out.push(i as u32);
            }
        }
        Ok(out)
    }
}

#[wasm_bindgen(js_name = stripAnsi)]
pub fn strip_ansi(input: &str) -> String {
    ansi::strip_ansi(input)
}

// ===== Stateless API for the streaming path =====

/// Render a byte slab to `{ html, text }` JSON for `lines_count` lines.
/// Slab must contain whole lines back-to-back, with no trailing newline
/// requirement.
#[wasm_bindgen(js_name = renderLines)]
pub fn render_lines_export(bytes: &[u8]) -> Result<String, JsError> {
    render::render_lines_json(bytes).map_err(|e| JsError::new(&e))
}

/// Match each line in the slab against the rules. Returns a `Uint8Array`
/// of per-line tags (0 = no match; otherwise the 1-based index of the
/// first matching rule).
#[wasm_bindgen(js_name = matchLines)]
pub fn match_lines_export(bytes: &[u8], rules_json: &str) -> Result<Vec<u8>, JsError> {
    let rules: Vec<filter::Rule> = serde_json::from_str(rules_json)
        .map_err(|e| JsError::new(&format!("parse rules: {}", e)))?;
    render::match_lines(bytes, &rules).map_err(|e| JsError::new(&e))
}

/// Search the slab. Returns local line indices that match.
#[wasm_bindgen(js_name = searchLines)]
pub fn search_lines_export(
    bytes: &[u8],
    query: &str,
    regex: bool,
    case_sensitive: bool,
) -> Result<Vec<u32>, JsError> {
    render::search_lines(bytes, query, regex, case_sensitive).map_err(|e| JsError::new(&e))
}

/// Return the byte offsets of every `\n` in the slab. Mirrors
/// `Buffer.indexOf` on the JS side, but uses SIMD memchr and avoids a JS
/// loop when scanning large chunks.
#[wasm_bindgen(js_name = findNewlines)]
pub fn find_newlines_export(bytes: &[u8]) -> Vec<u32> {
    eng_index::find_newlines(bytes)
}
