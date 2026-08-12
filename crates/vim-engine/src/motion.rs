//! Cursor motions. Every function returns a target position from a starting
//! position; it never mutates state. Columns are UTF-16 units (see buffer.rs).
//!
//! Semantics follow Vim (reference: vim/src/normal.c, search.c, textobject.c
//! in the original source), independently reimplemented.

use crate::buffer::{Buffer, Pos, chars_with_cols, utf16_len};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharClass {
    Ws,
    Word,
    Punct,
}

/// Vim's char classes: whitespace / keyword chars / other printable chars.
/// `big` collapses Word and Punct (WORD motions).
pub fn class(ch: char, big: bool) -> CharClass {
    if ch.is_whitespace() {
        CharClass::Ws
    } else if big || ch.is_alphanumeric() || ch == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

/// Char-by-char walker over the buffer. A position at the end of a line sits
/// on a virtual `'\n'`, which keeps cross-line word motions uniform.
pub(crate) struct Walker<'a> {
    buf: &'a Buffer,
    pub(crate) pos: Pos,
}

impl<'a> Walker<'a> {
    pub(crate) fn new(buf: &'a Buffer, pos: Pos) -> Self {
        Walker { buf, pos }
    }

    pub(crate) fn char(&self) -> char {
        self.buf.char_at(self.pos).unwrap_or('\n')
    }

    pub(crate) fn next(&mut self) -> bool {
        let len = self.buf.line_len(self.pos.line);
        if self.pos.col < len {
            let w = self.buf.char_at(self.pos).map_or(1, char::len_utf16);
            self.pos.col += w;
            true
        } else if self.pos.line + 1 < self.buf.line_count() {
            self.pos = Pos::new(self.pos.line + 1, 0);
            true
        } else {
            false
        }
    }

    pub(crate) fn prev(&mut self) -> bool {
        if self.pos.col > 0 {
            let line = self.buf.line(self.pos.line);
            let cols = chars_with_cols(line);
            let idx = cols.partition_point(|&(c, _)| c < self.pos.col);
            self.pos.col = cols.get(idx.wrapping_sub(1)).map_or(0, |&(c, _)| c);
            true
        } else if self.pos.line > 0 {
            self.pos = Pos::new(self.pos.line - 1, self.buf.line_len(self.pos.line - 1));
            true
        } else {
            false
        }
    }

    fn on_empty_line(&self) -> bool {
        self.pos.col == 0 && self.buf.line_len(self.pos.line) == 0
    }
}

/// The char position immediately before `pos`, crossing line boundaries
/// (where it lands on the previous line's end). Start of buffer is returned
/// unchanged.
pub fn char_before(buf: &Buffer, pos: Pos) -> Pos {
    let mut w = Walker::new(buf, pos);
    w.prev();
    w.pos
}

/// `h` / `l` targets. `right` may return the column one past the last char;
/// normal-mode movement clamps, operator ranges use it as an exclusive end.
pub fn left(pos: Pos, count: usize) -> Pos {
    Pos::new(pos.line, pos.col.saturating_sub(count))
}

pub fn right(buf: &Buffer, pos: Pos, count: usize) -> Pos {
    let line = buf.line(pos.line);
    let cols = chars_with_cols(line);
    let idx = cols.partition_point(|&(c, _)| c <= pos.col);
    // idx is the char index just past the cursor's char.
    let target = (idx.saturating_sub(1)).saturating_add(count);
    match cols.get(target) {
        Some(&(c, _)) => Pos::new(pos.line, c),
        None => Pos::new(pos.line, utf16_len(line)),
    }
}

/// `w` / `W`: start of the next word. Empty lines count as words.
pub fn word_forward(buf: &Buffer, pos: Pos, big: bool, count: usize) -> Pos {
    let mut w = Walker::new(buf, pos);
    for _ in 0..count {
        let start = class(w.char(), big);
        if start != CharClass::Ws {
            while class(w.char(), big) == start {
                if !w.next() {
                    return w.pos;
                }
            }
        }
        while class(w.char(), big) == CharClass::Ws {
            if w.on_empty_line() && w.pos != pos {
                break;
            }
            if !w.next() {
                return w.pos;
            }
        }
    }
    w.pos
}

/// `b` / `B`: start of the previous word.
pub fn word_back(buf: &Buffer, pos: Pos, big: bool, count: usize) -> Pos {
    let mut w = Walker::new(buf, pos);
    for _ in 0..count {
        if !w.prev() {
            return w.pos;
        }
        while class(w.char(), big) == CharClass::Ws {
            if w.on_empty_line() {
                break;
            }
            if !w.prev() {
                return w.pos;
            }
        }
        if w.on_empty_line() {
            continue;
        }
        let cls = class(w.char(), big);
        loop {
            let mut back = Walker::new(buf, w.pos);
            if !back.prev() || class(back.char(), big) != cls || back.pos.line != w.pos.line {
                break;
            }
            w.pos = back.pos;
        }
    }
    w.pos
}

/// `e` / `E`: end of the current/next word.
pub fn word_end(buf: &Buffer, pos: Pos, big: bool, count: usize) -> Pos {
    let mut w = Walker::new(buf, pos);
    for _ in 0..count {
        if !w.next() {
            return w.pos;
        }
        while class(w.char(), big) == CharClass::Ws {
            if !w.next() {
                return w.pos;
            }
        }
        let cls = class(w.char(), big);
        loop {
            let mut ahead = Walker::new(buf, w.pos);
            if !ahead.next() || class(ahead.char(), big) != cls {
                break;
            }
            w.pos = ahead.pos;
        }
    }
    w.pos
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FindKind {
    /// `f` — onto the char, forward.
    To,
    /// `F` — onto the char, backward.
    ToBack,
    /// `t` — till before the char, forward.
    Till,
    /// `T` — till after the char, backward.
    TillBack,
}

impl FindKind {
    pub fn forward(self) -> bool {
        matches!(self, FindKind::To | FindKind::Till)
    }

    /// `;` repeats as-is, `,` repeats with reversed direction.
    pub fn reversed(self) -> FindKind {
        match self {
            FindKind::To => FindKind::ToBack,
            FindKind::ToBack => FindKind::To,
            FindKind::Till => FindKind::TillBack,
            FindKind::TillBack => FindKind::Till,
        }
    }
}

/// `f F t T` on the current line. Returns None when the char is not found
/// `count` times (the whole motion then fails, per Vim).
pub fn find_char(buf: &Buffer, pos: Pos, kind: FindKind, target: char, count: usize) -> Option<Pos> {
    let cols = chars_with_cols(buf.line(pos.line));
    let idx = cols.partition_point(|&(c, _)| c <= pos.col).saturating_sub(1);
    let mut remaining = count.max(1);
    if kind.forward() {
        for i in idx + 1..cols.len() {
            if cols[i].1 == target {
                remaining -= 1;
                if remaining == 0 {
                    let hit = match kind {
                        FindKind::Till => i.checked_sub(1)?,
                        _ => i,
                    };
                    if kind == FindKind::Till && hit <= idx {
                        return None;
                    }
                    return Some(Pos::new(pos.line, cols[hit].0));
                }
            }
        }
    } else {
        for i in (0..idx.min(cols.len())).rev() {
            if cols[i].1 == target {
                remaining -= 1;
                if remaining == 0 {
                    let hit = match kind {
                        FindKind::TillBack => i + 1,
                        _ => i,
                    };
                    if kind == FindKind::TillBack && hit >= idx {
                        return None;
                    }
                    return Some(Pos::new(pos.line, cols[hit].0));
                }
            }
        }
    }
    None
}

/// `{` / `}`: previous/next empty line (buffer edges act as boundaries).
pub fn paragraph(buf: &Buffer, pos: Pos, forward: bool, count: usize) -> Pos {
    let mut line = pos.line;
    for _ in 0..count {
        if forward {
            let mut l = line + 1;
            while l < buf.line_count() && buf.line_len(l) != 0 {
                l += 1;
            }
            line = l.min(buf.last_line());
        } else {
            let mut l = line;
            while l > 0 {
                l -= 1;
                if buf.line_len(l) == 0 {
                    break;
                }
            }
            line = l;
        }
    }
    if forward && line == buf.last_line() && buf.line_len(line) != 0 {
        // Past the final paragraph `}` lands on the last char, per Vim.
        let cols = chars_with_cols(buf.line(line));
        return Pos::new(line, cols.last().map_or(0, |&(c, _)| c));
    }
    Pos::new(line, 0)
}

const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

/// `%`: match the bracket under (or after, on the same line) the cursor.
pub fn matching_pair(buf: &Buffer, pos: Pos) -> Option<Pos> {
    // Find the first bracket at or after the cursor's char on this line.
    let cols = chars_with_cols(buf.line(pos.line));
    let idx = cols.partition_point(|&(c, _)| c <= pos.col).saturating_sub(1);
    let (col, ch) = cols[idx.min(cols.len())..]
        .iter()
        .copied()
        .find(|&(_, ch)| is_bracket(ch))?;
    let start = Pos::new(pos.line, col);
    if let Some(&(open, close)) = PAIRS.iter().find(|&&(o, _)| o == ch) {
        scan_match(buf, start, open, close, true)
    } else {
        let &(open, close) = PAIRS.iter().find(|&&(_, c)| c == ch)?;
        scan_match(buf, start, open, close, false)
    }
}

fn is_bracket(ch: char) -> bool {
    PAIRS.iter().any(|&(o, c)| ch == o || ch == c)
}

pub(crate) fn scan_match(buf: &Buffer, start: Pos, open: char, close: char, forward: bool) -> Option<Pos> {
    let mut w = Walker::new(buf, start);
    let mut depth = 0i32;
    loop {
        let ch = w.char();
        if ch == open {
            depth += if forward { 1 } else { -1 };
        } else if ch == close {
            depth += if forward { -1 } else { 1 };
        }
        if depth == 0 && (ch == open || ch == close) {
            return Some(w.pos);
        }
        let moved = if forward { w.next() } else { w.prev() };
        if !moved {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn buf(s: &str) -> Buffer {
        Buffer::from_text(s)
    }

    #[test]
    fn word_forward_basic() {
        let b = buf("foo bar+baz  qux");
        assert_eq!(word_forward(&b, Pos::new(0, 0), false, 1), Pos::new(0, 4));
        assert_eq!(word_forward(&b, Pos::new(0, 4), false, 1), Pos::new(0, 7));
        assert_eq!(word_forward(&b, Pos::new(0, 7), false, 1), Pos::new(0, 8));
        assert_eq!(word_forward(&b, Pos::new(0, 8), false, 1), Pos::new(0, 13));
        assert_eq!(word_forward(&b, Pos::new(0, 0), false, 2), Pos::new(0, 7));
        // WORD skips punctuation runs.
        assert_eq!(word_forward(&b, Pos::new(0, 4), true, 1), Pos::new(0, 13));
    }

    #[test]
    fn word_forward_across_lines_and_empty() {
        let b = buf("foo\n\nbar");
        assert_eq!(word_forward(&b, Pos::new(0, 0), false, 1), Pos::new(1, 0));
        assert_eq!(word_forward(&b, Pos::new(1, 0), false, 1), Pos::new(2, 0));
    }

    #[test]
    fn word_back_basic() {
        let b = buf("foo bar+baz");
        assert_eq!(word_back(&b, Pos::new(0, 10), false, 1), Pos::new(0, 8));
        assert_eq!(word_back(&b, Pos::new(0, 8), false, 1), Pos::new(0, 7));
        assert_eq!(word_back(&b, Pos::new(0, 7), false, 1), Pos::new(0, 4));
        assert_eq!(word_back(&b, Pos::new(0, 4), false, 1), Pos::new(0, 0));
        assert_eq!(word_back(&b, Pos::new(0, 2), false, 1), Pos::new(0, 0));
    }

    #[test]
    fn word_end_basic() {
        let b = buf("foo bar+baz");
        assert_eq!(word_end(&b, Pos::new(0, 0), false, 1), Pos::new(0, 2));
        assert_eq!(word_end(&b, Pos::new(0, 2), false, 1), Pos::new(0, 6));
        assert_eq!(word_end(&b, Pos::new(0, 6), false, 1), Pos::new(0, 7));
        assert_eq!(word_end(&b, Pos::new(0, 0), false, 3), Pos::new(0, 7));
    }

    #[test]
    fn find_char_kinds() {
        let b = buf("abcabc");
        let p = Pos::new(0, 0);
        assert_eq!(find_char(&b, p, FindKind::To, 'c', 1), Some(Pos::new(0, 2)));
        assert_eq!(find_char(&b, p, FindKind::To, 'c', 2), Some(Pos::new(0, 5)));
        assert_eq!(find_char(&b, p, FindKind::Till, 'c', 1), Some(Pos::new(0, 1)));
        assert_eq!(find_char(&b, Pos::new(0, 5), FindKind::ToBack, 'a', 1), Some(Pos::new(0, 3)));
        assert_eq!(find_char(&b, Pos::new(0, 5), FindKind::TillBack, 'a', 1), Some(Pos::new(0, 4)));
        assert_eq!(find_char(&b, p, FindKind::To, 'z', 1), None);
    }

    #[test]
    fn paragraph_motion() {
        let b = buf("a\nb\n\nc\nd");
        assert_eq!(paragraph(&b, Pos::new(0, 0), true, 1), Pos::new(2, 0));
        assert_eq!(paragraph(&b, Pos::new(2, 0), true, 1), Pos::new(4, 0));
        assert_eq!(paragraph(&b, Pos::new(4, 0), false, 1), Pos::new(2, 0));
    }

    #[test]
    fn percent_matching() {
        let b = buf("fn foo(a, (b))\n{\n  x\n}");
        assert_eq!(matching_pair(&b, Pos::new(0, 6)), Some(Pos::new(0, 13)));
        assert_eq!(matching_pair(&b, Pos::new(0, 13)), Some(Pos::new(0, 6)));
        assert_eq!(matching_pair(&b, Pos::new(1, 0)), Some(Pos::new(3, 0)));
        // Cursor before any bracket jumps from the first one after it.
        assert_eq!(matching_pair(&b, Pos::new(0, 0)), Some(Pos::new(0, 13)));
    }

    #[test]
    fn right_is_char_aware() {
        let b = buf("a😀b");
        assert_eq!(right(&b, Pos::new(0, 0), 1), Pos::new(0, 1));
        assert_eq!(right(&b, Pos::new(0, 1), 1), Pos::new(0, 3));
        assert_eq!(right(&b, Pos::new(0, 3), 1), Pos::new(0, 4));
        assert_eq!(right(&b, Pos::new(0, 3), 5), Pos::new(0, 4));
    }
}
