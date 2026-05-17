//! Byte-span and line/column conversion utilities. Same shape as the
//! dotenv-analyzer copy — kept local so the crate has no internal
//! workspace dependency.

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

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, offset: u32) -> bool {
        offset >= self.start && offset <= self.end
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
}
