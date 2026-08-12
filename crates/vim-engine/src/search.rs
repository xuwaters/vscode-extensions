//! Buffer search behind `/ ? n N * #`.
//!
//! Patterns are literal text plus Vim's word-boundary atoms `\<` and `\>`;
//! any other backslash escape stands for the character it precedes. Matching
//! is case-sensitive (Vim's default `noignorecase`) and never spans a line
//! break, which keeps interactive searching useful without linking a regex
//! engine into the WASM bundle.

use crate::buffer::{Buffer, Pos, byte_to_utf16, chars_with_cols};
use crate::motion::{CharClass, class};

/// The last search, replayed by `n` / `N`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub pattern: String,
    /// Direction of the search that set it: `n` repeats it, `N` flips it.
    pub backward: bool,
}

/// A parsed pattern: literal text with optional word-boundary anchors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    text: String,
    at_word_start: bool,
    at_word_end: bool,
}

impl Pattern {
    /// Parse a pattern; `None` when nothing is left to match on.
    pub fn parse(src: &str) -> Option<Pattern> {
        let mut text = String::new();
        let mut at_word_start = false;
        let mut at_word_end = false;
        let mut chars = src.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                text.push(ch);
                continue;
            }
            match chars.next() {
                Some('<') if text.is_empty() => at_word_start = true,
                Some('>') if chars.peek().is_none() => at_word_end = true,
                Some(c) => text.push(c),
                None => text.push('\\'),
            }
        }
        (!text.is_empty()).then_some(Pattern {
            text,
            at_word_start,
            at_word_end,
        })
    }

    /// Start columns (UTF-16) of every match in `line`, ascending. Matches
    /// may overlap, as Vim's do.
    fn matches(&self, line: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let mut from = 0;
        while let Some(rel) = line[from..].find(&self.text) {
            let byte = from + rel;
            if self.boundaries_ok(line, byte) {
                out.push(byte_to_utf16(line, byte));
            }
            from = byte + line[byte..].chars().next().map_or(1, char::len_utf8);
        }
        out
    }

    /// Do the `\<` / `\>` anchors hold for a match starting at `byte`?
    fn boundaries_ok(&self, line: &str, byte: usize) -> bool {
        if self.at_word_start && line[..byte].chars().next_back().is_some_and(is_word) {
            return false;
        }
        let end = byte + self.text.len();
        if self.at_word_end && line[end..].chars().next().is_some_and(is_word) {
            return false;
        }
        true
    }
}

fn is_word(ch: char) -> bool {
    class(ch, false) == CharClass::Word
}

/// The `count`-th match from `from`, wrapping at the buffer ends (Vim's
/// `wrapscan`). Forward searches start strictly after `from`, backward ones
/// strictly before it; a full wrap can land back on `from` when it is the
/// only match.
pub fn find(buf: &Buffer, from: Pos, pat: &Pattern, backward: bool, count: usize) -> Option<Pos> {
    let mut pos = from;
    for _ in 0..count.max(1) {
        pos = step(buf, pos, pat, backward)?;
    }
    Some(pos)
}

fn step(buf: &Buffer, from: Pos, pat: &Pattern, backward: bool) -> Option<Pos> {
    let n = buf.line_count();
    // Visit every line once in search order, then the starting line again so
    // a lone match on it is still found after the wrap.
    for k in 0..=n {
        let line = if backward {
            (from.line + n - k % n) % n
        } else {
            (from.line + k) % n
        };
        let cols = pat.matches(buf.line(line));
        let keep = |c: usize| {
            if k == 0 {
                if backward { c < from.col } else { c > from.col }
            } else if k == n {
                if backward { c >= from.col } else { c <= from.col }
            } else {
                true
            }
        };
        let hit = if backward {
            cols.iter().rev().find(|&&c| keep(c))
        } else {
            cols.iter().find(|&&c| keep(c))
        };
        if let Some(&col) = hit {
            return Some(Pos::new(line, col));
        }
    }
    None
}

/// The keyword under the cursor — or the next one on the line, per Vim — as
/// the target of `*` / `#`. Returns where it starts and its text.
pub fn word_under_cursor(buf: &Buffer, pos: Pos) -> Option<(Pos, String)> {
    let cols = chars_with_cols(buf.line(pos.line));
    let idx = cols.partition_point(|&(c, _)| c <= pos.col).saturating_sub(1);
    let mut start = if cols.get(idx).is_some_and(|&(_, ch)| is_word(ch)) {
        idx
    } else {
        (idx..cols.len()).find(|&i| is_word(cols[i].1))?
    };
    while start > 0 && is_word(cols[start - 1].1) {
        start -= 1;
    }
    let mut end = start;
    while end < cols.len() && is_word(cols[end].1) {
        end += 1;
    }
    let text = cols[start..end].iter().map(|&(_, ch)| ch).collect();
    Some((Pos::new(pos.line, cols[start].0), text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn pat(src: &str) -> Pattern {
        Pattern::parse(src).expect("non-empty pattern")
    }

    fn buf(s: &str) -> Buffer {
        Buffer::from_text(s)
    }

    #[test]
    fn parse_escapes_and_anchors() {
        assert_eq!(Pattern::parse(""), None);
        assert_eq!(Pattern::parse("\\<\\>"), None); // anchors only: nothing to match
        assert_eq!(
            pat("\\<foo\\>"),
            Pattern { text: "foo".into(), at_word_start: true, at_word_end: true }
        );
        // A backslash elsewhere just escapes the next char.
        assert_eq!(pat("a\\.b").text, "a.b");
        assert_eq!(pat("a\\\\b").text, "a\\b");
        // `\<` past the start is literal.
        assert_eq!(pat("a\\<b").text, "a<b");
    }

    #[test]
    fn matches_respect_word_boundaries() {
        assert_eq!(pat("foo").matches("foo foobar xfoo"), vec![0, 4, 12]);
        assert_eq!(pat("\\<foo\\>").matches("foo foobar xfoo"), vec![0]);
        assert_eq!(pat("\\<foo").matches("foo foobar xfoo"), vec![0, 4]);
        assert_eq!(pat("aa").matches("aaaa"), vec![0, 1, 2]); // overlapping
    }

    #[test]
    fn forward_wraps_around() {
        let b = buf("foo\nbar\nfoo baz");
        let p = pat("foo");
        assert_eq!(find(&b, Pos::new(0, 0), &p, false, 1), Some(Pos::new(2, 0)));
        assert_eq!(find(&b, Pos::new(2, 0), &p, false, 1), Some(Pos::new(0, 0)));
        assert_eq!(find(&b, Pos::new(0, 0), &p, false, 2), Some(Pos::new(0, 0)));
        assert_eq!(find(&b, Pos::new(0, 0), &pat("baz"), false, 1), Some(Pos::new(2, 4)));
        assert_eq!(find(&b, Pos::new(0, 0), &pat("nope"), false, 1), None);
    }

    #[test]
    fn backward_wraps_around() {
        let b = buf("foo\nbar\nfoo baz");
        let p = pat("foo");
        assert_eq!(find(&b, Pos::new(2, 0), &p, true, 1), Some(Pos::new(0, 0)));
        assert_eq!(find(&b, Pos::new(0, 0), &p, true, 1), Some(Pos::new(2, 0)));
        assert_eq!(find(&b, Pos::new(1, 0), &p, true, 2), Some(Pos::new(2, 0)));
    }

    #[test]
    fn single_match_is_found_again_after_a_full_wrap() {
        let b = buf("a foo b");
        let p = pat("foo");
        assert_eq!(find(&b, Pos::new(0, 2), &p, false, 1), Some(Pos::new(0, 2)));
        assert_eq!(find(&b, Pos::new(0, 2), &p, true, 1), Some(Pos::new(0, 2)));
    }

    #[test]
    fn word_under_cursor_finds_and_skips() {
        let b = buf("foo bar_1 + baz");
        assert_eq!(
            word_under_cursor(&b, Pos::new(0, 1)),
            Some((Pos::new(0, 0), "foo".into()))
        );
        // Mid-word: walks back to the start.
        assert_eq!(
            word_under_cursor(&b, Pos::new(0, 6)),
            Some((Pos::new(0, 4), "bar_1".into()))
        );
        // On punctuation: the next keyword on the line.
        assert_eq!(
            word_under_cursor(&b, Pos::new(0, 10)),
            Some((Pos::new(0, 12), "baz".into()))
        );
        assert_eq!(word_under_cursor(&buf("  + -"), Pos::new(0, 0)), None);
    }

    #[test]
    fn utf16_columns_are_reported() {
        let b = buf("a😀foo");
        assert_eq!(find(&b, Pos::new(0, 0), &pat("foo"), false, 1), Some(Pos::new(0, 3)));
    }
}
