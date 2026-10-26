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

// ---- Vim's cursor arithmetic -----------------------------------------------
//
// The sentence motion and the text objects are ports of Vim's search.c and
// textobject.c, whose stopping rules lean on exactly how `inc()`/`dec()`
// step: a position may rest on a line's end, which Vim sees as the line's
// NUL. Porting them over these keeps every off-by-one where Vim has it.

/// What `gchar` reports at a line's end: Vim's NUL.
pub(crate) const NUL: char = '\0';

/// The char at `p`, or `NUL` at (or past) its line's end.
pub(crate) fn gchar(buf: &Buffer, p: Pos) -> char {
    buf.char_at(p).unwrap_or(NUL)
}

/// Vim's `VIM_ISWHITE`: only space and tab count.
pub(crate) fn is_white(ch: char) -> bool {
    ch == ' ' || ch == '\t'
}

/// Vim's `inc()`: one char forward, resting on the line's end before
/// crossing to the next line. Returns 0 inside a line, 2 on landing on its
/// end, 1 on crossing a line break, and -1 (unmoved) at the buffer's end.
pub(crate) fn inc(buf: &Buffer, p: &mut Pos) -> i32 {
    let len = buf.line_len(p.line);
    if p.col < len {
        p.col += buf.char_at(*p).map_or(1, char::len_utf16);
        return if p.col < len { 0 } else { 2 };
    }
    if p.line < buf.last_line() {
        *p = Pos::new(p.line + 1, 0);
        1
    } else {
        -1
    }
}

/// Vim's `dec()`: one char back; from column 0 onto the previous line's
/// end. Returns 1 on crossing a line, -1 (unmoved) at the buffer's start.
pub(crate) fn dec(buf: &Buffer, p: &mut Pos) -> i32 {
    if p.col > 0 {
        let line = buf.line(p.line);
        let byte = crate::buffer::utf16_to_byte(line, p.col);
        let prev = line[..byte].char_indices().next_back().map_or(0, |(b, _)| b);
        p.col = crate::buffer::byte_to_utf16(line, prev);
        0
    } else if p.line > 0 {
        p.line -= 1;
        p.col = buf.line_len(p.line);
        1
    } else {
        -1
    }
}

/// Vim's `incl()`: `inc()` that skips a non-empty line's end.
pub(crate) fn incl(buf: &Buffer, p: &mut Pos) -> i32 {
    let r = inc(buf, p);
    if r >= 1 && p.col != 0 { inc(buf, p) } else { r }
}

/// Vim's `decl()`: `dec()` that skips a non-empty line's end.
pub(crate) fn decl(buf: &Buffer, p: &mut Pos) -> i32 {
    let r = dec(buf, p);
    if r == 1 && p.col != 0 { dec(buf, p) } else { r }
}

/// Vim's `linewhite()`: a line holding nothing but blanks. `ip`/`ap` take
/// these as paragraph boundaries, though `{`/`}` don't (`:help ap`).
pub(crate) fn line_white(buf: &Buffer, line: usize) -> bool {
    buf.line(line).chars().all(is_white)
}

/// Vim's default `'paragraphs'` and `'sections'`: nroff macros, two chars
/// each, that start a paragraph or section when they follow a `.` in
/// column 0 (`:help paragraph`).
const PARAGRAPH_MACROS: &str = "IPLPPPQPP TPHPLIPpLpItpplpipbp";
const SECTION_MACROS: &str = "SHNHH HUnhsh";

/// Vim's `startPS(lnum, NUL, FALSE)`: whether `line` starts a paragraph or
/// section — an empty line, a form feed in column 0, or one of the nroff
/// macros above. These are sentence boundaries too (`:help sentence`).
pub(crate) fn starts_paragraph(buf: &Buffer, line: usize) -> bool {
    let s = buf.line(line).as_bytes();
    match s.first() {
        None | Some(b'\x0c') => true,
        Some(b'.') => in_macro(SECTION_MACROS, &s[1..]) || in_macro(PARAGRAPH_MACROS, &s[1..]),
        _ => false,
    }
}

/// Vim's `inmacro()`: does `s` (the line after its `.`) name one of the
/// two-char macros in `opt`? A space in the option matches a space in the
/// line or the line ending there.
fn in_macro(opt: &str, s: &[u8]) -> bool {
    let m = opt.as_bytes();
    let s0 = s.first().copied().unwrap_or(0);
    let s1 = if s0 == 0 { 0 } else { s.get(1).copied().unwrap_or(0) };
    let mut i = 0;
    while i < m.len() {
        let (m0, m1) = (m[i], m.get(i + 1).copied().unwrap_or(0));
        if (m0 == s0 || (m0 == b' ' && (s0 == 0 || s0 == b' ')))
            && (m1 == s1 || ((m1 == 0 || m1 == b' ') && (s0 == 0 || s1 == 0 || s1 == b' ')))
        {
            return true;
        }
        i += 2;
    }
    false
}

/// `(` / `)`: the start of the `count`-th sentence backward or forward —
/// Vim's `findsent()`. A sentence ends at a `.`, `!` or `?` followed by
/// any number of `)`, `]`, `"`, `'` and then a line end, space or tab; empty
/// lines and the other paragraph starts are sentences of their own
/// (`:help sentence`).
///
/// The result may sit on a line's end (the buffer's last sentence runs to
/// it): normal mode clamps that, an operator takes it as the exclusive end.
/// None when the count runs past the buffer's end, as Vim fails then; running
/// out going backward stops at the buffer's start instead.
pub fn sentence(buf: &Buffer, pos: Pos, forward: bool, count: usize) -> Option<Pos> {
    let step = |p: &mut Pos| if forward { incl(buf, p) } else { decl(buf, p) };
    let mut pos = pos;
    let mut count = count.max(1);
    let mut noskip = false; // don't skip blanks: we stopped at the buffer's end
    while count > 0 {
        count -= 1;
        let prev_pos = pos;
        'found: {
            if gchar(buf, pos) == NUL {
                // On an empty line: skip to a non-empty one.
                while step(&mut pos) != -1 && gchar(buf, pos) == NUL {}
                if forward {
                    break 'found;
                }
            } else if forward && pos.col == 0 && starts_paragraph(buf, pos.line) {
                // On a paragraph start going forward: the next line is the
                // next sentence.
                if pos.line == buf.last_line() {
                    return None;
                }
                pos.line += 1;
                break 'found;
            } else if !forward {
                decl(buf, &mut pos);
            }

            // Go back to the previous non-white, non-punctuation char.
            let mut found_dot = false;
            loop {
                let c = gchar(buf, pos);
                if !(is_white(c) || ".!?)]\"'".contains(c)) {
                    break;
                }
                let mut tpos = pos;
                if decl(buf, &mut tpos) == -1 || (buf.line_len(tpos.line) == 0 && forward) {
                    break;
                }
                if found_dot {
                    break;
                }
                if ".!?".contains(c) {
                    found_dot = true;
                }
                if ")]\"'".contains(c) && !".!?)]\"'".contains(gchar(buf, tpos)) {
                    break;
                }
                decl(buf, &mut pos);
            }

            // Find the end of the sentence.
            let start_line = pos.line;
            loop {
                let c = gchar(buf, pos);
                if c == NUL || (pos.col == 0 && starts_paragraph(buf, pos.line)) {
                    if !forward && pos.line != start_line {
                        pos.line += 1;
                    }
                    break;
                }
                if c == '.' || c == '!' || c == '?' {
                    let mut tpos = pos;
                    let mut at_end = false;
                    let after = loop {
                        if inc(buf, &mut tpos) == -1 {
                            at_end = true;
                            break NUL;
                        }
                        let c = gchar(buf, tpos);
                        if !")]\"'".contains(c) {
                            break c;
                        }
                    };
                    if at_end || is_white(after) || after == NUL {
                        pos = tpos;
                        if gchar(buf, pos) == NUL {
                            inc(buf, &mut pos);
                        }
                        break;
                    }
                }
                if step(&mut pos) == -1 {
                    if count > 0 {
                        return None;
                    }
                    noskip = true;
                    break;
                }
            }
        }
        // Skip the blanks before the sentence.
        while !noskip && is_white(gchar(buf, pos)) {
            if incl(buf, &mut pos) == -1 {
                break;
            }
        }
        if pos == prev_pos {
            // Didn't actually move: advance one char and try again.
            if step(&mut pos) == -1 {
                if count > 0 {
                    return None;
                }
                break;
            }
            count += 1;
        }
    }
    Some(pos)
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

/// `ge` / `gE`: end of the previous word (Vim's `bckend_word`). Off the word
/// the cursor is in, then back over blanks to the last char of the one
/// before; an empty line counts as a word and stops it.
///
/// The flag is Vim's FAIL: a count that runs out of buffer *between* words
/// fails the motion (an operator is dropped), one that runs out halfway
/// through a word does not — both still land on the first char.
pub fn word_end_back(buf: &Buffer, pos: Pos, big: bool, count: usize) -> (Pos, bool) {
    let mut w = Walker::new(buf, pos);
    for _ in 0..count {
        let start = class(w.char(), big);
        if !w.prev() {
            return (w.pos, false);
        }
        if start != CharClass::Ws {
            while class(w.char(), big) == start {
                if !w.prev() {
                    return (w.pos, true);
                }
            }
        }
        while class(w.char(), big) == CharClass::Ws {
            if w.on_empty_line() {
                break;
            }
            if !w.prev() {
                return (w.pos, true);
            }
        }
    }
    (w.pos, true)
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
    let at = idx.min(cols.len())
        + cols[idx.min(cols.len())..]
            .iter()
            .position(|&(_, ch)| is_bracket(ch))?;
    let (col, ch) = cols[at];
    let start = Pos::new(pos.line, col);
    // Vim takes the bracket it landed on as the pattern for the other end:
    // a `\(` matches a `\)` and skips a bare one (search.c `match_escaped`).
    let escaped = backslashes(&cols, at) % 2 == 1;
    let forward = PAIRS.iter().any(|&(o, _)| o == ch);
    let &(open, close) = PAIRS.iter().find(|&&(o, c)| ch == o || ch == c)?;
    let (nested, wanted) = if forward { (open, close) } else { (close, open) };
    find_unmatched(buf, start, nested, wanted, forward, true, escaped)
}

fn is_bracket(ch: char) -> bool {
    PAIRS.iter().any(|&(o, c)| ch == o || ch == c)
}

/// `[(` `[{` `])` `]}`: the `count`-th unmatched bracket in one direction —
/// the edge of the block the cursor sits in, from anywhere inside it. Nested
/// pairs passed on the way are skipped whole, and scanning starts *beside*
/// the cursor, so a cursor already on a bracket looks past it and repeating
/// the key walks out one level at a time.
///
/// A count that runs out of blocks stops at the outermost one it reached
/// rather than failing, which is what Vim's `nv_bracket_block` does with the
/// last position it found.
pub fn unmatched_bracket(
    buf: &Buffer,
    pos: Pos,
    open: char,
    close: char,
    forward: bool,
    count: usize,
) -> Option<Pos> {
    // Travelling forward, an `(` opens a nested pair and a `)` may be the
    // unmatched one; backward the roles swap.
    let (nested, wanted) = if forward { (open, close) } else { (close, open) };
    let mut found: Option<Pos> = None;
    for _ in 0..count.max(1) {
        let from = found.unwrap_or(pos);
        match find_unmatched(buf, from, nested, wanted, forward, true, false) {
            Some(next) => found = Some(next),
            None => break,
        }
    }
    found
}

/// Vim's bracket scan (search.c `findmatchlimit`): step one position at a
/// time from `start` until an unmatched `wanted` bracket turns up — `nested`
/// opens a level, `wanted` closes one. The position `start` itself is stepped
/// off before anything is examined, so a scan that begins on a bracket never
/// reports it.
///
/// `smart_quotes` is Vim's default `'cpoptions'` (no `%`): brackets inside a
/// `"..."` string are ignored, but only on lines whose quotes pair up, and only
/// from where the scan *enters* the line — a scan starting inside a string
/// counts as outside it, which is Vim's own "complicated, isn't it?" rule.
/// `'x'` character literals are stepped over the same way. Text objects turn
/// this off while they look for the opening bracket, as Vim does by forcing
/// `cpo` to `%` around that search (and only that one).
///
/// `escaped` says the scan started from a backslash-escaped bracket; only
/// brackets escaped the same way then count.
pub(crate) fn find_unmatched(
    buf: &Buffer,
    start: Pos,
    nested: char,
    wanted: char,
    forward: bool,
    smart_quotes: bool,
    escaped: bool,
) -> Option<Pos> {
    let mut line = start.line;
    let mut cols = chars_with_cols(buf.line(line));
    // The char index of `start` in its line; `cols.len()` stands for the line
    // break, a position Vim visits too (it is the line's NUL).
    let mut idx = cols.partition_point(|&(c, _)| c < start.col);
    let mut quotes_even = smart_quotes && quotes_pair_up(&cols);
    let mut inquote = false;
    let mut depth = 0usize;
    loop {
        if forward {
            if idx >= cols.len() {
                if line + 1 >= buf.line_count() {
                    return None;
                }
                line += 1;
                cols = chars_with_cols(buf.line(line));
                quotes_even = smart_quotes && quotes_pair_up(&cols);
                idx = 0;
            } else {
                idx += 1;
            }
        } else if idx == 0 {
            if line == 0 {
                return None;
            }
            line -= 1;
            cols = chars_with_cols(buf.line(line));
            quotes_even = smart_quotes && quotes_pair_up(&cols);
            idx = cols.len();
        } else {
            idx -= 1;
        }
        let Some(&(col, ch)) = cols.get(idx) else {
            inquote = false; // the line break ends any string
            continue;
        };
        if ch == '"' {
            if quotes_even && backslashes(&cols, idx).is_multiple_of(2) {
                inquote = !inquote;
            }
            continue;
        }
        if ch == '\'' && smart_quotes {
            // Step over `'x'` and `'\x'`, which never hold a real bracket.
            if forward {
                if idx + 3 < cols.len() && cols[idx + 1].1 == '\\' && cols[idx + 3].1 == '\'' {
                    idx += 3;
                } else if idx + 2 < cols.len() && cols[idx + 2].1 == '\'' {
                    idx += 2;
                }
            } else if idx >= 2 && cols[idx - 2].1 == '\'' {
                idx -= 2;
            } else if idx >= 3 && cols[idx - 2].1 == '\\' && cols[idx - 3].1 == '\'' {
                idx -= 3;
            }
            continue;
        }
        if inquote || (ch != nested && ch != wanted) {
            continue;
        }
        if (backslashes(&cols, idx) % 2 == 1) != escaped {
            continue;
        }
        if ch == nested {
            depth += 1;
        } else {
            match depth.checked_sub(1) {
                Some(d) => depth = d,
                None => return Some(Pos::new(line, col)),
            }
        }
    }
}

/// Backslashes immediately before `idx`, which decide whether the char there
/// is escaped.
fn backslashes(cols: &[(usize, char)], idx: usize) -> usize {
    cols[..idx].iter().rev().take_while(|&&(_, ch)| ch == '\\').count()
}

/// Whether a line's double quotes pair up. Vim only trusts its in-string
/// bookkeeping on such lines; on the rest it matches brackets everywhere.
/// A `"` inside a `'"'` literal is not a string delimiter and doesn't count.
fn quotes_pair_up(cols: &[(usize, char)]) -> bool {
    let mut quotes = 0usize;
    let mut i = 0;
    while i < cols.len() {
        let ch = cols[i].1;
        let char_literal = i > 0
            && cols[i - 1].1 == '\''
            && cols.get(i + 1).is_some_and(|&(_, c)| c == '\'');
        if ch == '"' && !char_literal {
            quotes += 1;
        }
        if ch == '\\' && i + 1 < cols.len() {
            i += 1;
        }
        i += 1;
    }
    quotes.is_multiple_of(2)
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
    fn word_end_back_basic() {
        let b = buf("foo bar+baz\n\n  qux");
        let ge = |line, col, big, count| word_end_back(&b, Pos::new(line, col), big, count);
        assert_eq!(ge(0, 9, false, 1), (Pos::new(0, 7), true)); // baz -> +
        assert_eq!(ge(0, 9, true, 1), (Pos::new(0, 2), true)); // WORD: back to foo
        assert_eq!(ge(0, 9, false, 2), (Pos::new(0, 6), true));
        // An empty line is a word end of its own.
        assert_eq!(ge(2, 3, false, 1), (Pos::new(1, 0), true));
        assert_eq!(ge(2, 3, false, 2), (Pos::new(0, 10), true));
        // Out of buffer partway through a word lands on the first char.
        assert_eq!(ge(0, 1, false, 1), (Pos::new(0, 0), true));
        // Already at the start: the motion fails.
        assert_eq!(ge(0, 0, false, 1), (Pos::new(0, 0), false));
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
    fn unmatched_brackets() {
        //          0123456789
        let b = buf("fn f() {\n  if (a) {\n    x\n  }\n}\n");
        let inside = Pos::new(2, 4); // on `x`
        // `]}` climbs out one block per press; `[{` climbs the other way.
        assert_eq!(
            unmatched_bracket(&b, inside, '{', '}', true, 1),
            Some(Pos::new(3, 2))
        );
        assert_eq!(
            unmatched_bracket(&b, inside, '{', '}', true, 2),
            Some(Pos::new(4, 0))
        );
        assert_eq!(
            unmatched_bracket(&b, inside, '{', '}', false, 1),
            Some(Pos::new(1, 9))
        );
        assert_eq!(
            unmatched_bracket(&b, inside, '{', '}', false, 2),
            Some(Pos::new(0, 7))
        );
        // A cursor already on a brace looks past it, so the key repeats.
        assert_eq!(
            unmatched_bracket(&b, Pos::new(3, 2), '{', '}', true, 1),
            Some(Pos::new(4, 0))
        );
        // Nested pairs on the way are stepped over, not counted.
        let flat = buf("a (b (c) d) e)");
        assert_eq!(
            unmatched_bracket(&flat, Pos::new(0, 6), '(', ')', true, 1),
            Some(Pos::new(0, 7))
        );
        assert_eq!(
            unmatched_bracket(&flat, Pos::new(0, 3), '(', ')', true, 1),
            Some(Pos::new(0, 10))
        );
        assert_eq!(
            unmatched_bracket(&flat, Pos::new(0, 12), '(', ')', true, 1),
            Some(Pos::new(0, 13))
        );
        // Nothing unmatched that way: the motion fails.
        assert_eq!(unmatched_bracket(&flat, Pos::new(0, 0), '(', ')', false, 1), None);
        assert_eq!(unmatched_bracket(&flat, Pos::new(0, 13), '(', ')', true, 1), None);
        // A count with nowhere left to climb stops at the outermost block
        // reached, as vim's nv_bracket_block does with its last position.
        assert_eq!(
            unmatched_bracket(&b, inside, '{', '}', true, 9),
            Some(Pos::new(4, 0))
        );
    }

    #[test]
    fn brackets_in_strings_are_skipped() {
        //          0123456789012345678901
        let b = buf("{ printf(\"}\"); x }");
        // The `}` inside the quotes is not the block's end (vim's smart
        // matching), so `%` and `]}` both look past it.
        assert_eq!(matching_pair(&b, Pos::new(0, 0)), Some(Pos::new(0, 17)));
        assert_eq!(
            unmatched_bracket(&b, Pos::new(0, 15), '{', '}', true, 1),
            Some(Pos::new(0, 17))
        );
        // Odd quotes on the line: vim can't tell which half is a string and
        // matches everywhere.
        let odd = buf("{ \" } }");
        assert_eq!(matching_pair(&odd, Pos::new(0, 0)), Some(Pos::new(0, 4)));
        // A char literal is stepped over whole.
        let lit = buf("{ c == '}' ; }");
        assert_eq!(matching_pair(&lit, Pos::new(0, 0)), Some(Pos::new(0, 13)));
        // Escaped brackets only match brackets escaped the same way.
        let esc = buf(r"a \( b ( c ) d \) e");
        assert_eq!(matching_pair(&esc, Pos::new(0, 3)), Some(Pos::new(0, 16)));
        assert_eq!(matching_pair(&esc, Pos::new(0, 7)), Some(Pos::new(0, 11)));
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
