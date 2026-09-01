//! `textDocument/formatting` and `textDocument/rangeFormatting`.
//!
//! A re-indenter, not a pretty-printer. It changes the leading whitespace of a
//! line and nothing else — line breaks, spacing inside a line, and the order of
//! anything are all left exactly as written.
//!
//! That is a deliberate limit rather than an unfinished one. There is no
//! `rustfmt` for shaders and no house style to appeal to, so a formatter that
//! rewrote statements would be imposing one; a formatter that only fixes the
//! indentation is doing the part everybody agrees on. It is also off by
//! default, so "format on save" cannot restyle a file by surprise.

use lsp_types::{
    DocumentFormattingParams, DocumentRangeFormattingParams, Position, Range, TextEdit,
};
use wgsl_syntax::lexer::TokenKind;

use crate::Server;
use crate::state::Document;

impl Server {
    pub fn formatting(&mut self, params: DocumentFormattingParams) -> Option<Vec<TextEdit>> {
        let document = self.document(&params.text_document.uri)?;
        self.enabled(document)?;
        Some(reindent(document, self.indent_width(document), None))
    }

    pub fn range_formatting(
        &mut self,
        params: DocumentRangeFormattingParams,
    ) -> Option<Vec<TextEdit>> {
        let document = self.document(&params.text_document.uri)?;
        self.enabled(document)?;
        let lines = params.range.start.line..=params.range.end.line;
        Some(reindent(document, self.indent_width(document), Some(lines)))
    }

    fn enabled(&self, document: &Document) -> Option<()> {
        self.settings().for_language(document.language).format.enable.then_some(())
    }

    fn indent_width(&self, document: &Document) -> usize {
        self.settings().for_language(document.language).format.indent_width as usize
    }
}

/// One edit per line whose leading whitespace is wrong.
///
/// Per-line rather than a whole-document replacement: an editor applying a
/// single giant edit loses the cursor, folds and selection, and a formatter
/// that does that gets switched off.
pub fn reindent(
    document: &Document,
    indent_width: usize,
    only: Option<std::ops::RangeInclusive<u32>>,
) -> Vec<TextEdit> {
    let text = document.text();
    let inside_comment = comment_lines(document);

    let mut edits = Vec::new();
    let mut depth = 0i32;

    for (number, line) in text.lines().enumerate() {
        let number = number as u32;
        let trimmed = line.trim_start();

        // A line continuing a block comment keeps whatever alignment its
        // author chose; re-indenting the middle of a comment mangles ASCII art.
        if trimmed.is_empty() || inside_comment.contains(&number) {
            depth += delta(document, number, trimmed);
            continue;
        }

        // A line that *starts* by closing a block belongs one level out.
        let closes_first = trimmed.starts_with(['}', ')', ']']);
        let indent = (depth - i32::from(closes_first)).max(0) as usize * indent_width;

        let current = line.len() - trimmed.len();
        if only.as_ref().is_none_or(|lines| lines.contains(&number))
            && (current != indent || line[..current].contains('\t'))
        {
            edits.push(TextEdit {
                range: Range {
                    start: Position { line: number, character: 0 },
                    end: Position { line: number, character: leading_utf16(line) },
                },
                new_text: " ".repeat(indent),
            });
        }

        depth += delta(document, number, trimmed);
    }

    edits
}

/// The net change in nesting a line makes, counting only real delimiters —
/// braces inside a comment or a string must not move the indentation.
fn delta(document: &Document, line: u32, _trimmed: &str) -> i32 {
    document
        .parsed()
        .tokens
        .iter()
        .filter(|token| token.line == line && token.kind == TokenKind::Punct)
        .map(|token| match document.slice(token.span) {
            "{" | "(" | "[" => 1,
            "}" | ")" | "]" => -1,
            _ => 0,
        })
        .sum()
}

/// Lines that are the continuation of a multi-line comment.
///
/// The line a comment *starts* on is not one of them: its indentation is the
/// statement's, and belongs to the formatter.
fn comment_lines(document: &Document) -> std::collections::HashSet<u32> {
    let mut lines = std::collections::HashSet::new();
    for token in &document.parsed().tokens {
        if token.kind != TokenKind::Comment {
            continue;
        }
        let end = document.position(token.span.end).line;
        for line in (token.line + 1)..=end {
            lines.insert(line);
        }
    }
    lines
}

/// The UTF-16 length of a line's leading whitespace.
fn leading_utf16(line: &str) -> u32 {
    line.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| c.len_utf16() as u32)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_whitespace_is_measured_in_utf16_units() {
        assert_eq!(leading_utf16("    x"), 4);
        assert_eq!(leading_utf16("\t\tx"), 2);
        assert_eq!(leading_utf16("x"), 0);
    }
}
