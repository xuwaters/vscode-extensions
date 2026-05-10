mod ansi;
mod filter;

use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn init() {
    console_error_panic_hook::set_once();
}

#[derive(Serialize)]
struct LinesPayload<'a> {
    html: Vec<&'a str>,
    text: Vec<&'a str>,
}

/// Owned, ANSI-parsed log file. The instance keeps line data in Rust memory;
/// the JS side queries it via the methods below to avoid copying every line
/// string across the WASM boundary unless needed.
#[wasm_bindgen]
pub struct LogIndex {
    lines: Vec<ansi::Line>,
}

#[wasm_bindgen]
impl LogIndex {
    /// Parse the input text into ANSI-rendered lines.
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

    /// Return a JSON payload `{ html: string[], text: string[] }` for all
    /// lines. JS calls this once after construction to populate its in-memory
    /// model. (Per-range fetching is also offered via `render_range`, but
    /// for the typical 8 MB cap a single bulk transfer is simpler and fast
    /// enough.)
    #[wasm_bindgen(js_name = allLinesJson)]
    pub fn all_lines_json(&self) -> Result<String, JsError> {
        self.range_json(0, self.lines.len())
    }

    /// Return JSON for a half-open range `[start, end)` of lines.
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

    /// Match each filter rule (provided as a JSON array of `Rule` shape) against
    /// every line. Returns a `Uint8Array` of length `line_count`, where each
    /// byte is `0` (no match) or `rule_index + 1` (1-based, first matching rule).
    /// Caller decides ordering / priority by rule order.
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

    /// Search the plain text of each line. Returns indices of matching lines.
    /// Caller chooses regex/case-sensitive semantics.
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

/// Strip every ANSI escape sequence (handy for logging / clipboard).
#[wasm_bindgen(js_name = stripAnsi)]
pub fn strip_ansi(input: &str) -> String {
    ansi::strip_ansi(input)
}
