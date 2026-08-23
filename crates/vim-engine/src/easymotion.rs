//! EasyMotion-style jumps: label every place a motion could land on screen,
//! then let one or two keystrokes pick one.
//!
//! Behaviour follows the plugin (Lokaltog/vim-easymotion), independently
//! reimplemented: targets come from the same places — word starts, word ends,
//! line starts, occurrences of a literal — only the lines the host says are
//! on screen are considered, and labels are handed out nearest-first so the
//! closest jumps cost a single key.

use crate::buffer::{Buffer, Pos, byte_to_utf16, chars_with_cols};
use crate::motion::{CharClass, class};

/// Which side of the cursor a jump looks at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Forward,
    Backward,
    /// Both ways at once (`s`, `/`): the nearest targets get the short labels
    /// whichever side of the cursor they are on.
    Both,
}

/// What a jump labels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind<'a> {
    /// `w` / `b` (and their WORD forms): the first char of every word.
    WordStart { big: bool },
    /// `e` / `ge`: the last char of every word.
    WordEnd { big: bool },
    /// `j` / `k`: the first non-blank of every line.
    Line,
    /// `f` `t` `s` `/`: every occurrence of a literal. `before` is `t`/`T`'s
    /// rule — land one char short of the match, on the side the jump came
    /// from.
    Chars { needle: &'a str, before: bool },
}

/// Every position `kind` can land on inside `view`, nearest to `cursor`
/// first. An empty line is a target of its own for word jumps, as
/// easymotion's `^$` alternative makes it.
pub fn collect(buf: &Buffer, view: (usize, usize), cursor: Pos, kind: Kind, dir: Dir) -> Vec<Pos> {
    let last = buf.last_line();
    let (first, stop) = (view.0.min(last), view.1.min(last));
    let mut out: Vec<Pos> = Vec::new();
    for line in first..=stop {
        match kind {
            Kind::Line => out.push(Pos::new(line, buf.first_non_blank(line))),
            Kind::WordStart { big } => {
                out.extend(word_cols(buf.line(line), big, false).into_iter().map(|c| Pos::new(line, c)));
            }
            Kind::WordEnd { big } => {
                out.extend(word_cols(buf.line(line), big, true).into_iter().map(|c| Pos::new(line, c)));
            }
            Kind::Chars { needle, before } => {
                char_cols(buf.line(line), needle, before, dir, line, &mut out);
            }
        }
    }
    // A line jump moves off the cursor's line whatever the columns say; the
    // rest compare position to position.
    let linewise = matches!(kind, Kind::Line);
    out.retain(|p| match (dir, linewise) {
        (Dir::Forward, true) => p.line > cursor.line,
        (Dir::Forward, false) => *p > cursor,
        (Dir::Backward, true) => p.line < cursor.line,
        (Dir::Backward, false) => *p < cursor,
        (Dir::Both, true) => p.line != cursor.line,
        (Dir::Both, false) => *p != cursor,
    });
    match dir {
        Dir::Forward => {} // already in document order: the nearest is first
        Dir::Backward => out.reverse(),
        Dir::Both => {
            out.sort_by_key(|p| (p.line.abs_diff(cursor.line), p.col.abs_diff(cursor.col)));
        }
    }
    out
}

/// Labels for `count` targets, nearest first, spelled with `keys`.
///
/// As few keys as possible become group prefixes: with 27 marker keys the
/// first 26 targets cost one keystroke and everything after them two. Targets
/// past what two keys can name are dropped — they are off the far end of a
/// screenful, and a target with no label is worse than no target.
pub fn labels(count: usize, keys: &[char]) -> Vec<String> {
    let k = keys.len();
    if k < 2 {
        return Vec::new();
    }
    if count <= k {
        return keys.iter().take(count).map(|&c| c.to_string()).collect();
    }
    let mut groups = 1;
    while groups < k && (k - groups) + groups * k < count {
        groups += 1;
    }
    let singles = k - groups;
    let n = count.min(singles + groups * k);
    (0..n)
        .map(|i| match i.checked_sub(singles) {
            None => keys[i].to_string(),
            Some(j) => [keys[singles + j / k], keys[j % k]].iter().collect(),
        })
        .collect()
}

/// Columns of the word starts (or ends) of one line. An empty line is one
/// target at column 0; a blank one holds none.
fn word_cols(line: &str, big: bool, ends: bool) -> Vec<usize> {
    let cols = chars_with_cols(line);
    if cols.is_empty() {
        return vec![0];
    }
    let mut out: Vec<usize> = Vec::new();
    for (i, &(col, ch)) in cols.iter().enumerate() {
        let cls = class(ch, big);
        if cls == CharClass::Ws {
            continue;
        }
        let neighbour = if ends {
            cols.get(i + 1)
        } else {
            i.checked_sub(1).map(|j| &cols[j])
        };
        if neighbour.is_none_or(|&(_, n)| class(n, big) != cls) {
            out.push(col);
        }
    }
    out
}

/// Landing columns for every occurrence of `needle` in one line.
fn char_cols(line: &str, needle: &str, before: bool, dir: Dir, at: usize, out: &mut Vec<Pos>) {
    if needle.is_empty() {
        return;
    }
    for (byte, _) in line.match_indices(needle) {
        let col = if !before {
            byte_to_utf16(line, byte)
        } else if dir == Dir::Backward {
            // `T` lands just past the match, and only if there is a char there.
            let end = byte + needle.len();
            if end >= line.len() {
                continue;
            }
            byte_to_utf16(line, end)
        } else {
            // `t` lands just short of it.
            match line[..byte].chars().next_back() {
                Some(ch) => byte_to_utf16(line, byte) - ch.len_utf16(),
                None => continue,
            }
        };
        out.push(Pos::new(at, col));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn buf(s: &str) -> Buffer {
        Buffer::from_text(s)
    }

    fn cols(ps: &[Pos]) -> Vec<(usize, usize)> {
        ps.iter().map(|p| (p.line, p.col)).collect()
    }

    #[test]
    fn word_starts_forward_and_back() {
        let b = buf("foo bar+baz\nqux");
        let view = (0, 1);
        let fwd = collect(&b, view, Pos::new(0, 0), Kind::WordStart { big: false }, Dir::Forward);
        assert_eq!(cols(&fwd), [(0, 4), (0, 7), (0, 8), (1, 0)]);
        // WORD folds the punctuation into its neighbours.
        let big = collect(&b, view, Pos::new(0, 0), Kind::WordStart { big: true }, Dir::Forward);
        assert_eq!(cols(&big), [(0, 4), (1, 0)]);
        // Backward is nearest-first too, which is the reverse of the document.
        let back = collect(&b, view, Pos::new(1, 2), Kind::WordStart { big: false }, Dir::Backward);
        assert_eq!(cols(&back), [(1, 0), (0, 8), (0, 7), (0, 4), (0, 0)]);
    }

    #[test]
    fn word_ends_and_empty_lines() {
        let b = buf("foo bar\n\nbaz");
        let ends = collect(&b, (0, 2), Pos::new(0, 0), Kind::WordEnd { big: false }, Dir::Forward);
        assert_eq!(cols(&ends), [(0, 2), (0, 6), (1, 0), (2, 2)]);
        // A blank line has no word on it; an empty one is a target itself.
        let blank = buf("a\n   \nb");
        let starts = collect(&blank, (0, 2), Pos::new(0, 0), Kind::WordStart { big: false }, Dir::Forward);
        assert_eq!(cols(&starts), [(2, 0)]);
    }

    #[test]
    fn line_targets_skip_the_cursor_line() {
        let b = buf("  one\ntwo\n\n    four");
        let down = collect(&b, (0, 3), Pos::new(1, 0), Kind::Line, Dir::Forward);
        assert_eq!(cols(&down), [(2, 0), (3, 4)]);
        let up = collect(&b, (0, 3), Pos::new(1, 0), Kind::Line, Dir::Backward);
        assert_eq!(cols(&up), [(0, 2)]);
    }

    #[test]
    fn char_targets_both_ways_are_nearest_first() {
        //          0123456789
        let b = buf("axbxcxdxex");
        let both = collect(
            &b,
            (0, 0),
            Pos::new(0, 5),
            Kind::Chars { needle: "x", before: false },
            Dir::Both,
        );
        assert_eq!(cols(&both), [(0, 3), (0, 7), (0, 1), (0, 9)]);
        // `t` stops one short of the match, `T` one past it.
        let till = collect(
            &b,
            (0, 0),
            Pos::new(0, 0),
            Kind::Chars { needle: "x", before: true },
            Dir::Forward,
        );
        assert_eq!(cols(&till), [(0, 2), (0, 4), (0, 6), (0, 8)]);
        let till_back = collect(
            &b,
            (0, 0),
            Pos::new(0, 9),
            Kind::Chars { needle: "x", before: true },
            Dir::Backward,
        );
        assert_eq!(cols(&till_back), [(0, 8), (0, 6), (0, 4), (0, 2)]);
    }

    #[test]
    fn multi_char_needles_and_view_bounds() {
        let b = buf("foo bar\nfoo baz\nfoo qux");
        // Only the lines the host says are visible get labelled.
        let hits = collect(
            &b,
            (1, 2),
            Pos::new(1, 0),
            Kind::Chars { needle: "foo", before: false },
            Dir::Both,
        );
        assert_eq!(cols(&hits), [(2, 0)]);
    }

    #[test]
    fn labels_grow_only_where_they_must() {
        let keys: Vec<char> = "abc".chars().collect();
        assert_eq!(labels(2, &keys), ["a", "b"]);
        assert_eq!(labels(3, &keys), ["a", "b", "c"]);
        // One key turns into a prefix; the nearest two still cost one press.
        assert_eq!(labels(5, &keys), ["a", "b", "ca", "cb", "cc"]);
        assert_eq!(labels(6, &keys), ["a", "ba", "bb", "bc", "ca", "cb"]);
        // Past what two keys can spell, the far targets are dropped: with
        // three keys that is nine labels, every one of them two presses.
        assert_eq!(labels(99, &keys).len(), 9);
        assert_eq!(labels(99, &keys)[0], "aa");
        assert_eq!(labels(99, &keys).last().unwrap(), "cc");
        assert!(labels(3, &keys[..1]).is_empty());
    }

    #[test]
    fn utf16_columns_survive_the_scan() {
        //          0 1  3 4 5 6 7 8  10
        let b = buf("a😀 bé x😀y");
        // The emoji is punctuation to vim's classes, so it starts a word of
        // its own; every column is a UTF-16 one.
        let starts = collect(&b, (0, 0), Pos::new(0, 0), Kind::WordStart { big: false }, Dir::Forward);
        assert_eq!(cols(&starts), [(0, 1), (0, 4), (0, 7), (0, 8), (0, 10)]);
        let big = collect(&b, (0, 0), Pos::new(0, 0), Kind::WordStart { big: true }, Dir::Forward);
        assert_eq!(cols(&big), [(0, 4), (0, 7)]);
        let hits = collect(
            &b,
            (0, 0),
            Pos::new(0, 0),
            Kind::Chars { needle: "😀", before: false },
            Dir::Forward,
        );
        assert_eq!(cols(&hits), [(0, 1), (0, 8)]);
    }
}
