//! Line-based text buffer mirroring a VSCode `TextDocument`.
//!
//! All columns are in UTF-16 code units so positions round-trip with the
//! VSCode API without conversion on the TypeScript side. Lines are stored
//! without terminators; the document EOL flavor is the host's concern.

use serde::{Deserialize, Serialize};

/// A position in the buffer. `col` is in UTF-16 code units.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(line: usize, col: usize) -> Self {
        Pos { line, col }
    }
}

/// UTF-16 length of a string.
pub fn utf16_len(s: &str) -> usize {
    if s.is_ascii() {
        return s.len();
    }
    s.chars().map(char::len_utf16).sum()
}

/// Byte offset for a UTF-16 column, clamped to the end of the string.
/// A column landing inside a surrogate pair snaps to the pair's start.
pub fn utf16_to_byte(s: &str, col: usize) -> usize {
    // Byte, char and UTF-16 unit are the same index in ASCII, which most
    // lines are; checking is a vector compare against a walk of the line.
    if s.is_ascii() {
        return col.min(s.len());
    }
    let mut u16s = 0;
    for (byte, ch) in s.char_indices() {
        if col < u16s + ch.len_utf16() {
            return byte;
        }
        u16s += ch.len_utf16();
    }
    s.len()
}

/// UTF-16 column for a byte offset (must lie on a char boundary or past end).
pub fn byte_to_utf16(s: &str, byte: usize) -> usize {
    utf16_len(&s[..byte.min(s.len())])
}

/// Chars of `s` paired with their starting UTF-16 column.
pub fn chars_with_cols(s: &str) -> Vec<(usize, char)> {
    let mut out = Vec::with_capacity(s.len());
    let mut col = 0;
    for ch in s.chars() {
        out.push((col, ch));
        col += ch.len_utf16();
    }
    out
}

#[derive(Clone, Debug)]
pub struct Buffer {
    lines: Vec<String>,
}

impl Buffer {
    pub fn from_text(text: &str) -> Self {
        let lines = text.replace("\r\n", "\n").replace('\r', "\n");
        Buffer {
            lines: lines.split('\n').map(str::to_string).collect(),
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, i: usize) -> &str {
        self.lines.get(i).map(String::as_str).unwrap_or("")
    }

    /// UTF-16 length of line `i`.
    pub fn line_len(&self, i: usize) -> usize {
        utf16_len(self.line(i))
    }

    pub fn last_line(&self) -> usize {
        self.lines.len().saturating_sub(1)
    }

    /// Column of the first non-blank char of line `i` (0 for a blank line).
    pub fn first_non_blank(&self, i: usize) -> usize {
        let line = self.line(i);
        let mut col = 0;
        for ch in line.chars() {
            if !ch.is_whitespace() {
                return col;
            }
            col += ch.len_utf16();
        }
        0
    }

    /// The char at `pos`, if any.
    pub fn char_at(&self, pos: Pos) -> Option<char> {
        let line = self.line(pos.line);
        let byte = utf16_to_byte(line, pos.col);
        line[byte..].chars().next()
    }

    /// Text in `[start, end)`, lines joined with `\n`.
    pub fn slice(&self, start: Pos, end: Pos) -> String {
        if start >= end {
            return String::new();
        }
        if start.line == end.line {
            let line = self.line(start.line);
            let a = utf16_to_byte(line, start.col);
            let b = utf16_to_byte(line, end.col);
            return line[a..b.max(a)].to_string();
        }
        let mut out = String::new();
        let first = self.line(start.line);
        out.push_str(&first[utf16_to_byte(first, start.col)..]);
        for i in start.line + 1..end.line.min(self.line_count()) {
            out.push('\n');
            out.push_str(self.line(i));
        }
        if end.line < self.line_count() {
            let last = self.line(end.line);
            out.push('\n');
            out.push_str(&last[..utf16_to_byte(last, end.col)]);
        }
        out
    }

    /// Full text, `\n`-joined.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Replace `[start, end)` with `text` (may contain newlines), mirroring a
    /// VSCode `TextDocumentContentChangeEvent`.
    pub fn apply_change(&mut self, start: Pos, end: Pos, text: &str) {
        // A change inside one line that adds no line break — a keystroke in
        // insert mode, one line's worth of `:s` — rewrites that line's bytes
        // in place. The general path below splices the line vector and builds
        // several strings to do the same thing, which a `:%s` over a large
        // buffer pays for once per line.
        if start.line == end.line && !text.contains(['\n', '\r']) {
            if let Some(line) = self.lines.get_mut(start.line) {
                let a = utf16_to_byte(line, start.col);
                let b = utf16_to_byte(line, end.col).max(a);
                line.replace_range(a..b, text);
                return;
            }
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let start_line = start.line.min(self.last_line());
        let end_line = end.line.min(self.last_line());
        let prefix = {
            let line = self.line(start_line);
            line[..utf16_to_byte(line, start.col)].to_string()
        };
        let suffix = {
            let line = self.line(end_line);
            line[utf16_to_byte(line, end.col)..].to_string()
        };
        let mut new_lines: Vec<String> = Vec::new();
        let mut parts = text.split('\n').peekable();
        let mut current = prefix;
        while let Some(part) = parts.next() {
            current.push_str(part);
            if parts.peek().is_some() {
                new_lines.push(std::mem::take(&mut current));
            }
        }
        current.push_str(&suffix);
        new_lines.push(current);
        self.lines.splice(start_line..=end_line, new_lines);
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn from_text_and_slice() {
        let b = Buffer::from_text("hello\nworld");
        assert_eq!(b.line_count(), 2);
        assert_eq!(b.slice(Pos::new(0, 1), Pos::new(1, 3)), "ello\nwor");
        assert_eq!(b.slice(Pos::new(0, 0), Pos::new(0, 5)), "hello");
    }

    #[test]
    fn apply_change_single_line() {
        let mut b = Buffer::from_text("hello world");
        b.apply_change(Pos::new(0, 5), Pos::new(0, 11), " there");
        assert_eq!(b.text(), "hello there");
    }

    #[test]
    fn apply_change_insert_newline() {
        let mut b = Buffer::from_text("ab");
        b.apply_change(Pos::new(0, 1), Pos::new(0, 1), "x\ny");
        assert_eq!(b.text(), "ax\nyb");
    }

    #[test]
    fn apply_change_delete_lines() {
        let mut b = Buffer::from_text("one\ntwo\nthree");
        b.apply_change(Pos::new(0, 3), Pos::new(2, 0), "");
        assert_eq!(b.text(), "onethree");
    }

    #[test]
    fn utf16_columns() {
        // '😀' is 2 UTF-16 units, 4 bytes.
        let s = "a😀b";
        assert_eq!(utf16_len(s), 4);
        assert_eq!(utf16_to_byte(s, 1), 1);
        assert_eq!(utf16_to_byte(s, 3), 5);
        assert_eq!(byte_to_utf16(s, 5), 3);
        // Column inside the surrogate pair snaps to its start.
        assert_eq!(utf16_to_byte(s, 2), 1);
    }
}
