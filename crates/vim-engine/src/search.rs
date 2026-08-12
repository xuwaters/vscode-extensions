//! Buffer search behind `/ ? n N * #`, and the match plumbing `:substitute`
//! runs on.
//!
//! Patterns are Vim regular expressions (see regex.rs) compiled once and then
//! applied line by line: matching is case-sensitive by default (Vim's
//! `noignorecase`) and never spans a line break, so `^` and `$` anchor to the
//! ends of a line.

use std::cell::Cell;

use crate::buffer::{Buffer, Pos, chars_with_cols};
use crate::motion::{CharClass, class};
use crate::regex::Regex;

/// Budget for a pattern compiled without a buffer in hand (tests); real
/// operations call `budget_for` instead.
const DEFAULT_BUDGET: u32 = 30_000_000;

/// Steps allowed per byte of buffer, plus a floor for small ones. The
/// cheapest pattern that cannot reject lines by literal — `\d\{9}` and its
/// kind — costs about two steps per character, so this leaves several times
/// the headroom an honest scan needs while capping a catastrophic one at a
/// fraction of a second. Patterns *with* a literal barely touch the budget:
/// whole lines are rejected before the VM starts.
const STEPS_PER_BYTE: u64 = 8;
const BUDGET_FLOOR: u64 = 4_000_000;

/// The last search, replayed by `n` / `N`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub pattern: String,
    /// Direction of the search that set it: `n` repeats it, `N` flips it.
    pub backward: bool,
}

/// One match inside a line. Columns are UTF-16; `groups[0]` is the whole
/// matched text and `groups[n]` capture group `n` (`None` if it took part in
/// no branch of the match).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    pub start: usize,
    pub end: usize,
    pub groups: Vec<Option<String>>,
}

/// A compiled search pattern, with the step budget for the operation it was
/// compiled for.
#[derive(Clone, Debug)]
pub struct Pattern {
    re: Regex,
    budget: Cell<u32>,
}

impl Pattern {
    /// Compile a case-sensitive pattern (`\c` in the pattern still wins).
    pub fn parse(src: &str) -> Result<Pattern, String> {
        Pattern::parse_case(src, false)
    }

    /// Compile with an explicit case default, as `:s///i` and `:s///I` need.
    pub fn parse_case(src: &str, ignore_case: bool) -> Result<Pattern, String> {
        if src.is_empty() {
            return Err("empty pattern".into());
        }
        Ok(Pattern { re: Regex::new(src, ignore_case)?, budget: Cell::new(DEFAULT_BUDGET) })
    }

    /// Size the step budget for one operation over `buf` — a `/` search
    /// across it, a `:%s` over its lines. Call this once, before the scan:
    /// what the operation cannot finish within it, it abandons.
    pub fn budget_for(&self, buf: &Buffer) {
        let bytes: u64 = (0..buf.line_count()).map(|i| buf.line(i).len() as u64).sum();
        let steps = BUDGET_FLOOR + STEPS_PER_BYTE * bytes;
        self.budget.set(steps.min(u32::MAX as u64) as u32);
    }

    /// Did this pattern run out of steps? Then the results so far are partial
    /// and the caller should say so rather than report "not found".
    pub fn gave_up(&self) -> bool {
        self.budget.get() == 0
    }

    #[cfg(test)]
    fn with_budget(src: &str, budget: u32) -> Pattern {
        let pat = Pattern::parse(src).expect("valid pattern");
        pat.budget.set(budget);
        pat
    }

    /// Start columns (UTF-16) of every match in `line`, ascending. Matches
    /// may overlap, as Vim's do: `aa` hits three times in `aaaa`.
    pub fn matches(&self, line: &str) -> Vec<usize> {
        if !self.re.line_may_match(line) {
            return Vec::new();
        }
        let (chars, cols) = scan(line);
        let mut budget = self.budget.get();
        let mut out = Vec::new();
        for i in 0..=chars.len() {
            if let Some(caps) = self.re.match_at(&chars, i, &mut budget) {
                out.push(cols[caps.start()]);
            }
        }
        self.budget.set(budget);
        // `\zs` can report the same start from several attempts, and can
        // report them out of order; `step` needs them ascending and unique.
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Non-overlapping matches in `line`, as `:s` replaces them: the first
    /// one only, unless `global`. An empty match advances by one char so the
    /// scan always terminates.
    pub fn find_all(&self, line: &str, global: bool) -> Vec<LineMatch> {
        if !self.re.line_may_match(line) {
            return Vec::new();
        }
        let (chars, cols) = scan(line);
        let mut budget = self.budget.get();
        let mut out = Vec::new();
        let mut i = 0;
        while i <= chars.len() {
            let Some(caps) = self.re.match_at(&chars, i, &mut budget) else {
                i += 1;
                continue;
            };
            let (start, end) = (caps.start(), caps.end());
            let groups = (0..=9)
                .map(|n| {
                    caps.group(n)
                        .map(|(s, e)| chars[s.min(chars.len())..e.min(chars.len())].iter().collect())
                })
                .collect();
            out.push(LineMatch { start: cols[start], end: cols[end], groups });
            if !global {
                break;
            }
            i = if end > i { end } else { i + 1 };
        }
        self.budget.set(budget);
        out
    }
}

/// A line's chars alongside the UTF-16 column each one starts at (plus the
/// column just past the end, so match ends map too).
fn scan(line: &str) -> (Vec<char>, Vec<usize>) {
    let chars: Vec<char> = line.chars().collect();
    let mut cols = Vec::with_capacity(chars.len() + 1);
    let mut col = 0;
    for ch in &chars {
        cols.push(col);
        col += ch.len_utf16();
    }
    cols.push(col);
    (chars, cols)
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

/// Escape `text` so a pattern matches it literally (used by `*` and `#`).
pub fn escape_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if !ch.is_alphanumeric() && ch != '_' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn pat(src: &str) -> Pattern {
        Pattern::parse(src).expect("valid pattern")
    }

    fn buf(s: &str) -> Buffer {
        Buffer::from_text(s)
    }

    fn spans(src: &str, line: &str, global: bool) -> Vec<(usize, usize)> {
        pat(src)
            .find_all(line, global)
            .iter()
            .map(|m| (m.start, m.end))
            .collect()
    }

    #[test]
    fn empty_and_invalid_patterns_are_rejected() {
        assert_eq!(Pattern::parse("").err(), Some("empty pattern".into()));
        assert!(Pattern::parse(r"\(foo").is_err());
    }

    #[test]
    fn matches_respect_word_boundaries() {
        assert_eq!(pat("foo").matches("foo foobar xfoo"), vec![0, 4, 12]);
        assert_eq!(pat(r"\<foo\>").matches("foo foobar xfoo"), vec![0]);
        assert_eq!(pat(r"\<foo").matches("foo foobar xfoo"), vec![0, 4]);
        assert_eq!(pat("aa").matches("aaaa"), vec![0, 1, 2]); // overlapping
    }

    #[test]
    fn matches_are_regular_expressions() {
        assert_eq!(pat(r"a.c").matches("abc a c axc"), vec![0, 4, 8]);
        assert_eq!(pat(r"\d\+").matches("x 42 y 7"), vec![2, 3, 7]);
        assert_eq!(pat(r"^x").matches("x x"), vec![0]);
        assert_eq!(pat(r"x$").matches("x x"), vec![2]);
        assert_eq!(pat(r"a\.b").matches("a.b axb"), vec![0]);
    }

    #[test]
    fn find_all_walks_non_overlapping_matches() {
        assert_eq!(spans("aa", "aaaaa", true), vec![(0, 2), (2, 4)]);
        assert_eq!(spans("aa", "aaaaa", false), vec![(0, 2)]);
        // Empty matches step forward one char instead of spinning.
        assert_eq!(spans("x*", "ab", true), vec![(0, 0), (1, 1), (2, 2)]);
        assert_eq!(spans(r"\d\+", "a1b22c", true), vec![(1, 2), (3, 5)]);
    }

    #[test]
    fn find_all_reports_group_text() {
        let m = &pat(r"\(\w\+\)=\(\d\+\)").find_all("x: size=42;", true)[0];
        assert_eq!(m.groups[0], Some("size=42".into()));
        assert_eq!(m.groups[1], Some("size".into()));
        assert_eq!(m.groups[2], Some("42".into()));
        assert_eq!(m.groups[3], None);
    }

    #[test]
    fn the_step_budget_spans_the_whole_operation() {
        // Exponential backtracking: one line is affordable, a buffer of them
        // is not — which is the point of budgeting the operation, not the
        // line. (A per-line budget times a big file is a frozen editor.)
        // No literal in the pattern, so the line prefilter cannot help.
        let line = "aaaaaaaa";
        let p = Pattern::with_budget(r"\(a*\)*\d", 200_000);
        assert!(p.matches(line).is_empty());
        assert!(!p.gave_up());
        let b = buf(&format!("{}\n", line).repeat(50));
        assert_eq!(find(&b, Pos::new(0, 0), &p, false, 1), None);
        assert!(p.gave_up());
    }

    #[test]
    fn a_line_without_the_required_literal_is_skipped() {
        // Same answers as an unfiltered scan, at a fraction of the work.
        let p = pat("needle");
        assert!(p.matches("no match here").is_empty());
        assert_eq!(p.matches("a needle here"), vec![2]);
        assert!(pat(r".*=.*;zz").matches("let a = b;").is_empty());
        assert_eq!(pat(r"\cNEEDLE").matches("a needle"), vec![2]); // no filter
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
        assert_eq!(spans("foo", "a😀foo", true), vec![(3, 6)]);
        assert_eq!(pat(".").matches("a😀b"), vec![0, 1, 3]);
    }

    #[test]
    fn escape_literal_neutralizes_pattern_syntax() {
        assert_eq!(escape_literal("a.b*"), r"a\.b\*");
        assert_eq!(pat(&escape_literal("a.b")).matches("axb a.b"), vec![4]);
    }
}
