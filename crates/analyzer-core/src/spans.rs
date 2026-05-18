//! Byte-span and line/column conversion utilities.
//!
//! A [`ByteSpan`] is a half-open `[start, end)` range into the source. A
//! [`SpanTable`] amortises line/col ↔ offset lookups so feature providers
//! can translate VSCode `Position`s without rescanning the source.
//!
//! `LineCol` columns are UTF-16 code units, matching the LSP spec.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ByteSpan {
    pub start: u32,
    pub end: u32,
}

impl ByteSpan {
    pub const EMPTY: ByteSpan = ByteSpan { start: 0, end: 0 };

    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end);
        ByteSpan { start, end }
    }

    pub fn from_usize(start: usize, end: usize) -> Self {
        Self::new(start as u32, end as u32)
    }

    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Inclusive hit-test: `start <= offset <= end`.
    ///
    /// Suits cursor-based editor queries where a cursor positioned exactly
    /// at the trailing edge should still count as "inside" the span (you
    /// just typed the last character of an identifier and ask for hover).
    pub fn contains(&self, offset: u32) -> bool {
        offset >= self.start && offset <= self.end
    }

    /// Strict half-open hit-test: `start <= offset < end`.
    ///
    /// Matches the formal `[start, end)` definition. Use this when the
    /// trailing edge belongs to the *next* token (e.g. textproto field
    /// resolution where adjoining values must not both match).
    pub fn contains_strict(&self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    pub fn join(self, other: ByteSpan) -> ByteSpan {
        ByteSpan::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// Zero-indexed line/column pair (UTF-16 column, matching LSP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone)]
pub struct SpanTable {
    line_starts: Vec<u32>,
    source_len: u32,
}

impl SpanTable {
    pub fn new(source: &str) -> Self {
        let mut line_starts = Vec::with_capacity(source.len() / 40 + 1);
        line_starts.push(0);
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        SpanTable { line_starts, source_len: source.len() as u32 }
    }

    pub fn source_len(&self) -> u32 {
        self.source_len
    }

    pub fn offset_to_line_col(&self, source: &str, offset: u32) -> LineCol {
        let offset = offset.min(self.source_len);
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let line_start = self.line_starts[line_idx] as usize;
        let end = (offset as usize).min(source.len());
        let col = utf16_len(&source[line_start..end]);
        LineCol { line: line_idx as u32, col }
    }

    pub fn line_col_to_offset(&self, source: &str, pos: LineCol) -> u32 {
        if pos.line as usize >= self.line_starts.len() {
            return self.source_len;
        }
        let line_start = self.line_starts[pos.line as usize] as usize;
        let next_line_start = self
            .line_starts
            .get(pos.line as usize + 1)
            .copied()
            .unwrap_or(self.source_len) as usize;
        let slice = &source[line_start..next_line_start];
        let mut offset = line_start;
        let mut remaining_u16 = pos.col;
        for ch in slice.chars() {
            if remaining_u16 == 0 {
                break;
            }
            let u16_len = ch.len_utf16() as u32;
            if u16_len > remaining_u16 {
                break;
            }
            remaining_u16 -= u16_len;
            offset += ch.len_utf8();
        }
        offset as u32
    }
}

fn utf16_len(s: &str) -> u32 {
    s.chars().map(|c| c.len_utf16() as u32).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_roundtrip_ascii() {
        let src = "abc\ndef\nghi";
        let t = SpanTable::new(src);
        for i in 0..=src.len() as u32 {
            let pos = t.offset_to_line_col(src, i);
            let back = t.line_col_to_offset(src, pos);
            assert_eq!(back, i);
        }
    }

    #[test]
    fn offset_roundtrip_multibyte() {
        // "α" is 2 bytes UTF-8 / 1 unit UTF-16; "🦀" is 4 bytes UTF-8 / 2 units UTF-16.
        let src = "α\n🦀x\nz";
        let t = SpanTable::new(src);
        let mut i = 0u32;
        while (i as usize) <= src.len() {
            if src.is_char_boundary(i as usize) {
                let pos = t.offset_to_line_col(src, i);
                let back = t.line_col_to_offset(src, pos);
                assert_eq!(back, i, "roundtrip failed at offset {i}");
            }
            i += 1;
        }
    }

    #[test]
    fn join_extends_both_ends() {
        let a = ByteSpan::new(3, 7);
        let b = ByteSpan::new(5, 10);
        assert_eq!(a.join(b), ByteSpan::new(3, 10));
    }

    #[test]
    fn contains_is_inclusive_at_both_edges() {
        let s = ByteSpan::new(3, 7);
        assert!(s.contains(3));
        assert!(s.contains(5));
        assert!(s.contains(7));
        assert!(!s.contains(2));
        assert!(!s.contains(8));
    }

    #[test]
    fn contains_strict_excludes_trailing_edge() {
        let s = ByteSpan::new(3, 7);
        assert!(s.contains_strict(3));
        assert!(s.contains_strict(6));
        assert!(!s.contains_strict(7));
        assert!(!s.contains_strict(2));
    }

    #[test]
    fn len_matches_byte_distance() {
        assert_eq!(ByteSpan::new(3, 10).len(), 7);
        assert_eq!(ByteSpan::EMPTY.len(), 0);
    }

    #[test]
    fn source_len_matches_input() {
        let src = "hello\nworld";
        assert_eq!(SpanTable::new(src).source_len(), src.len() as u32);
    }

    #[test]
    fn line_col_table_basic_offsets() {
        let src = "a\nbb\nccc";
        let t = SpanTable::new(src);
        assert_eq!(t.offset_to_line_col(src, 0), LineCol { line: 0, col: 0 });
        assert_eq!(t.offset_to_line_col(src, 2), LineCol { line: 1, col: 0 });
        assert_eq!(t.offset_to_line_col(src, 5), LineCol { line: 2, col: 0 });
        assert_eq!(t.offset_to_line_col(src, 7), LineCol { line: 2, col: 2 });
    }
}
