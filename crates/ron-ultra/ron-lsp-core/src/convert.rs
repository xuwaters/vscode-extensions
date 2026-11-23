//! Byte offsets and analyzer shapes to LSP positions and types.

use analyzer_core::diagnostics::DiagnosticCode;
use lsp_types::{DiagnosticSeverity, NumberOrString, Position, Range, SymbolKind};
use ron_analyzer::diagnostics::{Diagnostic, Severity};
use ron_analyzer::spans::{ByteSpan, LineCol};
use ron_analyzer::workspace::ParsedFile;

pub fn position(lc: LineCol) -> Position {
    Position { line: lc.line, character: lc.col }
}

pub fn line_col(p: Position) -> LineCol {
    LineCol { line: p.line, col: p.character }
}

pub fn offset(pf: &ParsedFile, p: Position) -> u32 {
    pf.spans.line_col_to_offset(&pf.source, line_col(p))
}

pub fn range(pf: &ParsedFile, span: ByteSpan) -> Range {
    Range {
        start: position(pf.spans.offset_to_line_col(&pf.source, span.start)),
        end: position(pf.spans.offset_to_line_col(&pf.source, span.end)),
    }
}

pub fn diagnostic(pf: &ParsedFile, d: &Diagnostic) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: range(pf, d.span),
        severity: Some(match d.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Info => DiagnosticSeverity::INFORMATION,
            Severity::Hint => DiagnosticSeverity::HINT,
        }),
        code: Some(NumberOrString::String(d.code.as_str().to_string())),
        source: Some("ron".into()),
        message: d.message.clone(),
        ..lsp_types::Diagnostic::default()
    }
}

/// The analyzer names `vscode.SymbolKind` members; LSP wants numbers.
pub fn symbol_kind(name: &str) -> SymbolKind {
    match name {
        "Struct" => SymbolKind::STRUCT,
        "Field" => SymbolKind::FIELD,
        "Key" => SymbolKind::KEY,
        "Array" => SymbolKind::ARRAY,
        "Object" => SymbolKind::OBJECT,
        "Boolean" => SymbolKind::BOOLEAN,
        "Number" => SymbolKind::NUMBER,
        "String" => SymbolKind::STRING,
        "EnumMember" => SymbolKind::ENUM_MEMBER,
        "Null" | "None" => SymbolKind::NULL,
        _ => SymbolKind::VARIABLE,
    }
}
