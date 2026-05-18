//! LSP-shaped data transfer objects exchanged with the TypeScript host
//! and converter helpers that translate byte spans into the JSON-friendly
//! `{ start: LineCol, end: LineCol }` shape.
//!
//! The DTOs here intentionally use `String` for `code`, `severity`, and
//! `kind` so they can carry values from any per-language enum. Crates
//! whose JS-side schema differs (proto3, capnp use `markdown` instead of
//! `contents`; capnp uses `&'static str` codes) keep their own DTOs.

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::spans::{ByteSpan, LineCol, SpanTable};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LspDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub start: LineCol,
    pub end: LineCol,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspDocumentSymbol {
    pub name: String,
    pub detail: String,
    pub kind: String,
    pub range_start: LineCol,
    pub range_end: LineCol,
    pub selection_start: LineCol,
    pub selection_end: LineCol,
    pub children: Vec<LspDocumentSymbol>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspFoldingRange {
    pub start_line: u32,
    pub end_line: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspHover {
    pub contents: String,
    pub start: LineCol,
    pub end: LineCol,
}

/// Convert a [`Diagnostic`] into the wire-format [`LspDiagnostic`].
pub fn to_lsp_diagnostic<C: DiagnosticCode>(
    d: Diagnostic<C>,
    source: &str,
    spans: &SpanTable,
) -> LspDiagnostic {
    LspDiagnostic {
        code: d.code.as_str().to_string(),
        severity: d.severity.as_str().to_string(),
        message: d.message,
        start: spans.offset_to_line_col(source, d.span.start),
        end: spans.offset_to_line_col(source, d.span.end),
    }
}

/// Helper for translating a [`ByteSpan`] into the start/end [`LineCol`]
/// pair used by every LSP DTO above.
pub fn span_to_line_cols(span: ByteSpan, source: &str, spans: &SpanTable) -> (LineCol, LineCol) {
    (
        spans.offset_to_line_col(source, span.start),
        spans.offset_to_line_col(source, span.end),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostic;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct TestCode;
    impl DiagnosticCode for TestCode {
        fn as_str(self) -> &'static str {
            "TST001"
        }
    }

    #[test]
    fn diagnostic_translates_offsets_to_utf16_line_cols() {
        let src = "abc\ndef\nghi";
        let spans = SpanTable::new(src);
        let d = Diagnostic::error(TestCode, "bad", ByteSpan::new(4, 7)); // "def"
        let lsp = to_lsp_diagnostic(d, src, &spans);
        assert_eq!(lsp.code, "TST001");
        assert_eq!(lsp.severity, "error");
        assert_eq!(lsp.message, "bad");
        assert_eq!(lsp.start, LineCol { line: 1, col: 0 });
        assert_eq!(lsp.end, LineCol { line: 1, col: 3 });
    }

    #[test]
    fn span_to_line_cols_returns_both_ends() {
        let src = "ab\ncd";
        let spans = SpanTable::new(src);
        let (start, end) = span_to_line_cols(ByteSpan::new(0, 4), src, &spans);
        assert_eq!(start, LineCol { line: 0, col: 0 });
        assert_eq!(end, LineCol { line: 1, col: 1 });
    }

    #[test]
    fn diagnostic_serializes_with_string_severity_and_code() {
        let src = "x";
        let spans = SpanTable::new(src);
        let d = Diagnostic::warning(TestCode, "msg", ByteSpan::new(0, 1));
        let lsp = to_lsp_diagnostic(d, src, &spans);
        let json = serde_json::to_string(&lsp).unwrap();
        assert!(json.contains("\"code\":\"TST001\""), "{json}");
        assert!(json.contains("\"severity\":\"warning\""), "{json}");
    }
}
