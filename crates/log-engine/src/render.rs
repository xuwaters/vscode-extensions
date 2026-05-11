//! Stateless byte-slab → line-record renderer.
//!
//! The streaming path hands the engine contiguous byte slabs covering whole
//! lines (located on the host side via the line-index). The engine decodes
//! the bytes as UTF-8 (lossy) and runs the existing ANSI line parser.
//!
//! SGR state does not carry **across** slabs — the host owns where slab
//! boundaries fall, and on a freshly-jumped window we cannot know what
//! state the file would have produced earlier. Within a slab, state still
//! carries from line to line exactly as in the small-file path.

use serde::Serialize;

use crate::ansi;
use crate::filter::{self, Rule};

#[derive(Serialize)]
pub struct LinesPayload<'a> {
    pub html: Vec<&'a str>,
    pub text: Vec<&'a str>,
}

/// Decode a byte slab as UTF-8 (lossy on invalid bytes) and parse it into
/// per-line (html, text) records.
pub fn render_lines(bytes: &[u8]) -> Vec<ansi::Line> {
    let s = String::from_utf8_lossy(bytes);
    ansi::parse_lines(&s)
}

/// Same as `render_lines`, but serialized to `{ html, text }` JSON for the
/// WASM boundary.
pub fn render_lines_json(bytes: &[u8]) -> Result<String, String> {
    let lines = render_lines(bytes);
    let payload = LinesPayload {
        html: lines.iter().map(|l| l.html.as_str()).collect(),
        text: lines.iter().map(|l| l.text.as_str()).collect(),
    };
    serde_json::to_string(&payload).map_err(|e| format!("serialize lines: {}", e))
}

/// For each line in the slab, return the 1-based index of the first matching
/// rule, or 0 if no rule matched. Disabled rules occupy their slot and never
/// match so caller-side indices stay aligned with the rules array.
pub fn match_lines(bytes: &[u8], rules: &[Rule]) -> Result<Vec<u8>, String> {
    let matchers = filter::compile(rules)?;
    let lines = render_lines(bytes);
    let mut out = vec![0u8; lines.len()];
    for (i, line) in lines.iter().enumerate() {
        for (j, m) in matchers.iter().enumerate() {
            if m.is_match(&line.text) {
                out[i] = (j as u8).saturating_add(1);
                break;
            }
        }
    }
    Ok(out)
}

/// Return local indices of lines whose plain text matches `query`.
pub fn search_lines(
    bytes: &[u8],
    query: &str,
    regex: bool,
    case_sensitive: bool,
) -> Result<Vec<u32>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let m = filter::build_search(query, regex, case_sensitive)?;
    let lines = render_lines(bytes);
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if m.is_match(&line.text) {
            out.push(i as u32);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const ESC: &str = "\x1b";

    #[test]
    fn render_lines_round_trips_text() {
        let slab = format!("{ESC}[31merror\nokay{ESC}[0m");
        let lines = render_lines(slab.as_bytes());
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "error");
        assert_eq!(lines[1].text, "okay");
    }

    #[test]
    fn render_lines_json_payload_shape() {
        let s = render_lines_json(b"hello\nworld").unwrap();
        assert!(s.contains("\"text\""));
        assert!(s.contains("hello"));
        assert!(s.contains("world"));
    }

    #[test]
    fn match_lines_returns_one_based_rule_indices() {
        let rules = vec![
            Rule {
                pattern: "ERROR".into(),
                regex: false,
                case_sensitive: false,
                enabled: true,
            },
            Rule {
                pattern: "INFO".into(),
                regex: false,
                case_sensitive: false,
                enabled: true,
            },
        ];
        let slab = b"first ERROR line\nsecond INFO line\nthird";
        let out = match_lines(slab, &rules).unwrap();
        assert_eq!(out, vec![1, 2, 0]);
    }

    #[test]
    fn search_lines_returns_local_indices() {
        let slab = b"alpha\nbeta\nALPHA";
        let ci = search_lines(slab, "alpha", false, false).unwrap();
        assert_eq!(ci, vec![0, 2]);
        let cs = search_lines(slab, "alpha", false, true).unwrap();
        assert_eq!(cs, vec![0]);
    }

    #[test]
    fn empty_query_returns_empty() {
        let out = search_lines(b"anything", "", false, false).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn lossy_utf8_does_not_panic() {
        let slab = vec![0xFF, b'a', b'\n', b'b'];
        let lines = render_lines(&slab);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].text, "b");
    }
}
