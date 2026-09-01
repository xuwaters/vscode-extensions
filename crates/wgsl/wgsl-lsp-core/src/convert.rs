//! Byte offsets ⇄ LSP positions.
//!
//! The whole crate works in [`ByteSpan`]s; LSP speaks lines and UTF-16 code
//! units. This is the only place that translates, so an off-by-one has one
//! place to be, and one place to be tested.
//!
//! [`SpanTable`] does the arithmetic — it is shared with every other analyzer
//! in the repo, and its round-trip is property-tested there.

use analyzer_core::spans::{ByteSpan, LineCol, SpanTable};
use lsp_types::{Position, Range};

pub fn offset_to_position(source: &str, lines: &SpanTable, offset: u32) -> Position {
    let LineCol { line, col } = lines.offset_to_line_col(source, offset);
    Position { line, character: col }
}

pub fn position_to_offset(source: &str, lines: &SpanTable, position: Position) -> u32 {
    lines.line_col_to_offset(
        source,
        LineCol { line: position.line, col: position.character },
    )
}

pub fn span_to_range(source: &str, lines: &SpanTable, span: ByteSpan) -> Range {
    Range {
        start: offset_to_position(source, lines, span.start),
        end: offset_to_position(source, lines, span.end),
    }
}

pub fn range_to_span(source: &str, lines: &SpanTable, range: Range) -> ByteSpan {
    let start = position_to_offset(source, lines, range.start);
    let end = position_to_offset(source, lines, range.end);
    ByteSpan::new(start.min(end), start.max(end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_round_trip_through_offsets() {
        let source = "fn main() {\n    let 🦀 = vec2f(1.0);\n}\n";
        let lines = SpanTable::new(source);
        for offset in 0..=source.len() as u32 {
            if !source.is_char_boundary(offset as usize) {
                continue;
            }
            let position = offset_to_position(source, &lines, offset);
            assert_eq!(position_to_offset(source, &lines, position), offset);
        }
    }

    /// A crab is one scalar but two UTF-16 units, and LSP counts the latter.
    #[test]
    fn columns_are_utf16_code_units() {
        let source = "let 🦀 = 1;";
        let lines = SpanTable::new(source);
        let after = source.find(" =").unwrap() as u32;
        assert_eq!(
            offset_to_position(source, &lines, after),
            Position { line: 0, character: 6 }
        );
    }

    /// VS Code sends a character index past the end of the line when the
    /// cursor is in virtual space; clamping beats failing.
    #[test]
    fn a_position_past_the_end_clamps() {
        let source = "abc\ndef";
        let lines = SpanTable::new(source);
        let offset = position_to_offset(source, &lines, Position { line: 0, character: 99 });
        assert!(offset <= source.len() as u32);
        let offset = position_to_offset(source, &lines, Position { line: 99, character: 0 });
        assert_eq!(offset, source.len() as u32);
    }

    #[test]
    fn a_reversed_range_is_normalised() {
        let source = "abcdef";
        let lines = SpanTable::new(source);
        let range = Range {
            start: Position { line: 0, character: 4 },
            end: Position { line: 0, character: 1 },
        };
        assert_eq!(range_to_span(source, &lines, range), ByteSpan::new(1, 4));
    }
}
