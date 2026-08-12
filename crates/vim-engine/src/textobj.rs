//! Text objects: `iw`, `aw`, `i"`, `a(`, … Resolved to a charwise range
//! `[start, end)` in UTF-16 columns.

use crate::buffer::{Buffer, Pos, chars_with_cols, utf16_len};
use crate::motion::{CharClass, Walker, class, scan_match};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextObject {
    Word { big: bool },
    Quote(char),
    Bracket(char, char),
}

impl TextObject {
    pub fn parse(ch: char) -> Option<TextObject> {
        match ch {
            'w' => Some(TextObject::Word { big: false }),
            'W' => Some(TextObject::Word { big: true }),
            '"' | '\'' | '`' => Some(TextObject::Quote(ch)),
            '(' | ')' | 'b' => Some(TextObject::Bracket('(', ')')),
            '{' | '}' | 'B' => Some(TextObject::Bracket('{', '}')),
            '[' | ']' => Some(TextObject::Bracket('[', ']')),
            '<' | '>' => Some(TextObject::Bracket('<', '>')),
            _ => None,
        }
    }
}

/// Resolve a text object at `pos`. Returns `[start, end)` or None when the
/// object does not exist there (e.g. `i(` outside any parens).
pub fn range(buf: &Buffer, pos: Pos, obj: TextObject, around: bool) -> Option<(Pos, Pos)> {
    match obj {
        TextObject::Word { big } => word_range(buf, pos, big, around),
        TextObject::Quote(q) => quote_range(buf, pos, q, around),
        TextObject::Bracket(open, close) => bracket_range(buf, pos, open, close, around),
    }
}

/// `iw`: the run of same-class chars under the cursor (whitespace runs count).
/// `aw`: the word plus trailing whitespace, or leading whitespace if none.
fn word_range(buf: &Buffer, pos: Pos, big: bool, around: bool) -> Option<(Pos, Pos)> {
    let cols = chars_with_cols(buf.line(pos.line));
    if cols.is_empty() {
        return None;
    }
    let idx = cols.partition_point(|&(c, _)| c <= pos.col).saturating_sub(1);
    let cls = class(cols[idx].1, big);
    let mut start = idx;
    while start > 0 && class(cols[start - 1].1, big) == cls {
        start -= 1;
    }
    let mut end = idx;
    while end + 1 < cols.len() && class(cols[end + 1].1, big) == cls {
        end += 1;
    }
    let line_len = utf16_len(buf.line(pos.line));
    let mut start_col = cols[start].0;
    let mut end_col = cols
        .get(end + 1)
        .map_or(line_len, |&(c, _)| c);
    if around && cls != CharClass::Ws {
        let mut ate_trailing = false;
        let mut e = end;
        while e + 1 < cols.len() && class(cols[e + 1].1, big) == CharClass::Ws {
            e += 1;
            ate_trailing = true;
        }
        if ate_trailing {
            end_col = cols.get(e + 1).map_or(line_len, |&(c, _)| c);
        } else {
            let mut s = start;
            while s > 0 && class(cols[s - 1].1, big) == CharClass::Ws {
                s -= 1;
            }
            start_col = cols[s].0;
        }
    }
    Some((Pos::new(pos.line, start_col), Pos::new(pos.line, end_col)))
}

/// Quotes are matched on the current line only, like Vim. The cursor may be
/// inside a pair or before one (then the next pair on the line is used).
fn quote_range(buf: &Buffer, pos: Pos, quote: char, around: bool) -> Option<(Pos, Pos)> {
    let cols = chars_with_cols(buf.line(pos.line));
    let quotes: Vec<usize> = cols
        .iter()
        .filter(|&&(_, ch)| ch == quote)
        .map(|&(c, _)| c)
        .collect();
    // Pair quotes up left to right; pick the pair containing or after cursor.
    let mut pair = None;
    for chunk in quotes.chunks(2) {
        if let [open, close] = *chunk {
            if pos.col <= close {
                pair = Some((open, close));
                break;
            }
        }
    }
    let (open, close) = pair?;
    let quote_w = quote.len_utf16();
    if around {
        // `a"` includes the quotes plus trailing whitespace (leading if none).
        let mut end = close + quote_w;
        let mut start = open;
        let after: Vec<&(usize, char)> = cols.iter().filter(|&&(c, _)| c >= end).collect();
        let trailing = after.iter().take_while(|&&&(_, ch)| ch == ' ' || ch == '\t').count();
        if trailing > 0 {
            end = after
                .get(trailing)
                .map_or(utf16_len(buf.line(pos.line)), |&&(c, _)| c);
        } else {
            while let Some(&(c, ch)) = cols.iter().rev().find(|&&(c, _)| c < start) {
                if ch == ' ' || ch == '\t' {
                    start = c;
                } else {
                    break;
                }
            }
        }
        Some((Pos::new(pos.line, start), Pos::new(pos.line, end)))
    } else {
        Some((Pos::new(pos.line, open + quote_w), Pos::new(pos.line, close)))
    }
}

fn bracket_range(
    buf: &Buffer,
    pos: Pos,
    open: char,
    close: char,
    around: bool,
) -> Option<(Pos, Pos)> {
    let (open_pos, close_pos) = if buf.char_at(pos) == Some(open) {
        (pos, scan_match(buf, pos, open, close, true)?)
    } else if buf.char_at(pos) == Some(close) {
        (scan_match(buf, pos, open, close, false)?, pos)
    } else {
        let open_pos = enclosing_open(buf, pos, open, close)?;
        (open_pos, scan_match(buf, open_pos, open, close, true)?)
    };
    if around {
        let end = Pos::new(close_pos.line, close_pos.col + close.len_utf16());
        Some((open_pos, end))
    } else {
        let start = Pos::new(open_pos.line, open_pos.col + open.len_utf16());
        Some((start, close_pos))
    }
}

/// The unmatched opening bracket enclosing `pos` (whose char is not itself a
/// bracket of this pair): walk backward, closes push depth, opens pop it.
fn enclosing_open(buf: &Buffer, pos: Pos, open: char, close: char) -> Option<Pos> {
    let mut w = Walker::new(buf, pos);
    let mut depth = 0i32;
    loop {
        if !w.prev() {
            return None;
        }
        let ch = w.char();
        if ch == close {
            depth += 1;
        } else if ch == open {
            if depth == 0 {
                return Some(w.pos);
            }
            depth -= 1;
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
    fn inner_and_around_word() {
        let b = buf("foo bar baz");
        let (s, e) = range(&b, Pos::new(0, 5), TextObject::Word { big: false }, false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 4), Pos::new(0, 7)));
        let (s, e) = range(&b, Pos::new(0, 5), TextObject::Word { big: false }, true).unwrap();
        assert_eq!((s, e), (Pos::new(0, 4), Pos::new(0, 8)));
    }

    #[test]
    fn around_word_leading_ws_fallback() {
        let b = buf("foo bar");
        let (s, e) = range(&b, Pos::new(0, 5), TextObject::Word { big: false }, true).unwrap();
        assert_eq!((s, e), (Pos::new(0, 3), Pos::new(0, 7)));
    }

    #[test]
    fn inner_quote() {
        let b = buf(r#"say "hi there" now"#);
        let (s, e) = range(&b, Pos::new(0, 7), TextObject::Quote('"'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 5), Pos::new(0, 13)));
        // Cursor before the pair uses the next pair on the line.
        let (s, e) = range(&b, Pos::new(0, 0), TextObject::Quote('"'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 5), Pos::new(0, 13)));
    }

    #[test]
    fn around_quote_takes_trailing_space() {
        let b = buf(r#"a "b" c"#);
        let (s, e) = range(&b, Pos::new(0, 3), TextObject::Quote('"'), true).unwrap();
        assert_eq!((s, e), (Pos::new(0, 2), Pos::new(0, 6)));
    }

    #[test]
    fn inner_brackets_nested_multiline() {
        let b = buf("f(a, g(b),\n  c)");
        let (s, e) = range(&b, Pos::new(0, 3), TextObject::Bracket('(', ')'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 2), Pos::new(1, 3)));
        let (s, e) = range(&b, Pos::new(0, 7), TextObject::Bracket('(', ')'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 7), Pos::new(0, 8)));
        let (s, e) = range(&b, Pos::new(0, 3), TextObject::Bracket('(', ')'), true).unwrap();
        assert_eq!((s, e), (Pos::new(0, 1), Pos::new(1, 4)));
    }

    #[test]
    fn bracket_on_the_brackets_themselves() {
        let b = buf("(abc)");
        let (s, e) = range(&b, Pos::new(0, 0), TextObject::Bracket('(', ')'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 1), Pos::new(0, 4)));
        let (s, e) = range(&b, Pos::new(0, 4), TextObject::Bracket('(', ')'), false).unwrap();
        assert_eq!((s, e), (Pos::new(0, 1), Pos::new(0, 4)));
    }

    #[test]
    fn no_object_outside() {
        let b = buf("plain text");
        assert_eq!(range(&b, Pos::new(0, 0), TextObject::Bracket('(', ')'), false), None);
        assert_eq!(range(&b, Pos::new(0, 0), TextObject::Quote('"'), false), None);
    }
}
