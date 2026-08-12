//! Buffer search behind `/ ? n N * #`, and the match plumbing `:substitute`
//! runs on.
//!
//! Patterns are Vim regular expressions (see regex.rs) compiled once and then
//! applied line by line: matching is case-sensitive by default (Vim's
//! `noignorecase`) and never spans a line break, so `^` and `$` anchor to the
//! ends of a line.

use std::cell::{Cell, RefCell};

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

/// Most match ranges a `preview` reports. Highlighting is a courtesy, not an
/// enumeration; past this the host would be painting off-screen anyway.
const PREVIEW_CAP: usize = 1000;

/// The last search, replayed by `n` / `N`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub pattern: String,
    /// Direction of the search that set it: `n` repeats it, `N` flips it.
    pub backward: bool,
}

/// One match inside a line. `start` and `end` are UTF-16 columns, the
/// coordinates an edit is expressed in; the capture groups are kept as byte
/// spans into the line, so reading one is a slice of text the caller already
/// has rather than a string allocated per match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    pub start: usize,
    pub end: usize,
    /// Byte span of the whole match (0) and of capture groups 1–9; `None`
    /// where the group took part in no branch of the match.
    groups: [Option<(u32, u32)>; 10],
}

impl LineMatch {
    /// Byte span of the whole match, for slicing the line it came from.
    pub fn bytes(&self) -> (usize, usize) {
        let (a, b) = self.groups[0].unwrap_or_default();
        (a as usize, b as usize)
    }

    /// Text of group `n` — 0 being the whole match — taken from `line`, which
    /// must be the line this match was found in.
    pub fn group<'a>(&self, line: &'a str, n: usize) -> Option<&'a str> {
        let (a, b) = (*self.groups.get(n)?)?;
        line.get(a as usize..b as usize)
    }
}

/// Working memory for a scan: the line decomposed for the VM, the column map
/// its answers are reported through, and the VM's own stack. Held by the
/// pattern and reused line to line, so `:%s` over a buffer allocates a
/// handful of times rather than several times per match.
#[derive(Clone, Debug, Default)]
struct Scratch {
    chars: Vec<char>,
    /// UTF-16 column and byte offset of each char, plus one past the last;
    /// empty for an ASCII line, where all three indices coincide.
    cols: Vec<(u32, u32)>,
    ascii: bool,
    vm: crate::regex::Scratch,
}

impl Scratch {
    fn load(&mut self, line: &str) {
        let Scratch { chars, cols, ascii, .. } = self;
        chars.clear();
        *ascii = line.is_ascii();
        if *ascii {
            // No decoding, and no column map: index, column and offset agree.
            chars.extend(line.bytes().map(char::from));
            return;
        }
        chars.extend(line.chars());
        cols.clear();
        cols.reserve(chars.len() + 1);
        let (mut col, mut byte) = (0u32, 0u32);
        for ch in chars.iter() {
            cols.push((col, byte));
            col += ch.len_utf16() as u32;
            byte += ch.len_utf8() as u32;
        }
        cols.push((col, byte));
    }

    /// UTF-16 column of char `i` (or of the line's end, at `i == len`).
    fn col(&self, i: usize) -> usize {
        let i = i.min(self.chars.len());
        if self.ascii { i } else { self.cols[i].0 as usize }
    }

    /// Byte offset of char `i` (or of the line's end, at `i == len`).
    fn byte(&self, i: usize) -> u32 {
        let i = i.min(self.chars.len());
        if self.ascii { i as u32 } else { self.cols[i].1 }
    }
}

/// A compiled search pattern, with the step budget for the operation it was
/// compiled for.
#[derive(Clone, Debug)]
pub struct Pattern {
    re: Regex,
    budget: Cell<u32>,
    scratch: RefCell<Scratch>,
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
        Ok(Pattern {
            re: Regex::new(src, ignore_case)?,
            budget: Cell::new(DEFAULT_BUDGET),
            scratch: RefCell::default(),
        })
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
        let s = &mut *self.scratch.borrow_mut();
        s.load(line);
        let mut budget = self.budget.get();
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(at) = self.re.next_start(&s.chars, i) {
            if let Some(caps) = self.re.match_at(&s.chars, at, &mut budget, &mut s.vm) {
                out.push(s.col(caps.start()));
            }
            i = at + 1;
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
        let mut out = Vec::new();
        self.find_all_into(line, global, &mut out);
        out
    }

    /// `find_all` writing into the caller's vector, which is cleared first.
    /// A `:%s` walking a buffer keeps one across lines and so allocates for
    /// its matches once, not once per line.
    pub fn find_all_into(&self, line: &str, global: bool, out: &mut Vec<LineMatch>) {
        out.clear();
        if !self.re.line_may_match(line) {
            return;
        }
        let s = &mut *self.scratch.borrow_mut();
        s.load(line);
        let mut budget = self.budget.get();
        let mut i = 0;
        let mut previous = usize::MAX;
        while let Some(at) = self.re.next_start(&s.chars, i) {
            let Some(caps) = self.re.match_at(&s.chars, at, &mut budget, &mut s.vm) else {
                i = at + 1;
                continue;
            };
            let (start, end) = (caps.start(), caps.end());
            // An empty match where the last one ended is not a second thing
            // to replace: `:s/x*/-/g` over "axb" gives "-a-b-", not "-a--b-".
            if start == end && start == previous {
                i = at + 1;
                continue;
            }
            previous = end;
            let mut groups = [None; 10];
            for (n, span) in groups.iter_mut().take(self.re.group_count() + 1).enumerate() {
                // `\zs` past `\ze` can invert a span; report it empty rather
                // than slice backwards.
                *span = caps.group(n).map(|(a, b)| (s.byte(a), s.byte(b).max(s.byte(a))));
            }
            out.push(LineMatch { start: s.col(start), end: s.col(end), groups });
            if !global {
                break;
            }
            i = if end > at { end } else { at + 1 };
        }
        self.budget.set(budget);
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

/// Like `find` with the whole buffer enumerated: where the `count`-th match
/// from `from` lands, plus its rank among all matches and their total, both
/// in document order — the numbers behind "match N of M". Wrapping works the
/// same as `find`'s; matches n would visit (overlapping starts included) are
/// what is counted. When the pattern `gave_up` mid-scan the totals cover only
/// what was seen.
pub fn find_ranked(
    buf: &Buffer,
    from: Pos,
    pat: &Pattern,
    backward: bool,
    count: usize,
) -> Option<(Pos, usize, usize)> {
    let mut starts: Vec<Pos> = Vec::new();
    for line in 0..buf.line_count() {
        for col in pat.matches(buf.line(line)) {
            starts.push(Pos::new(line, col));
        }
    }
    let total = starts.len();
    if total == 0 {
        return None;
    }
    let count = count.max(1);
    let idx = if backward {
        // Predecessor of the first match at-or-past the cursor, wrapping.
        let ge = starts.partition_point(|&p| p < from);
        (ge + total - 1 - (count - 1) % total) % total
    } else {
        // First match strictly past the cursor, wrapping.
        let gt = starts.partition_point(|&p| p <= from);
        (gt + (count - 1) % total) % total
    };
    Some((starts[idx], idx, total))
}

/// Incremental-search UI for a `/`/`?` prompt still being typed: up to
/// `PREVIEW_CAP` non-overlapping match ranges as flat `[line, start, end]`
/// triples (UTF-16 columns), plus the range the view should peek at — the
/// nearest match in the search direction, wrapping, which is where `<cr>`
/// would land. The peeked match is left out of the pile so the host can give
/// it a stronger decoration.
pub fn preview(
    buf: &Buffer,
    from: Pos,
    pat: &Pattern,
    backward: bool,
) -> (Vec<u32>, Option<[u32; 3]>) {
    let mut stored: Vec<[u32; 3]> = Vec::new();
    let (mut first, mut last) = (None, None); // buffer-wide extremes, for the wrap
    let (mut before, mut after) = (None, None); // nearest on each side of the cursor
    let mut found = Vec::new();
    'scan: for line in 0..buf.line_count() {
        pat.find_all_into(buf.line(line), true, &mut found);
        for m in &found {
            let t = [line as u32, m.start as u32, m.end as u32];
            if stored.len() < PREVIEW_CAP {
                stored.push(t);
            }
            first.get_or_insert(t);
            last = Some(t);
            let p = Pos::new(line, m.start);
            if p > from {
                after.get_or_insert(t);
                // Forward needs nothing from the lines below once the pile
                // is full; backward still needs the buffer's last match.
                if !backward && stored.len() >= PREVIEW_CAP {
                    break 'scan;
                }
            } else if p < from {
                before = Some(t);
            }
        }
    }
    let current = if backward { before.or(last) } else { after.or(first) };
    if let Some(c) = current {
        if let Some(i) = stored.iter().position(|&t| t == c) {
            stored.remove(i);
        }
    }
    (stored.into_iter().flatten().collect(), current)
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
        // …but not where the previous match just ended: `:s/x*/-/g` over
        // "axb" is Vim's "-a-b-", not "-a--b-".
        assert_eq!(spans("x*", "axb", true), vec![(0, 0), (1, 2), (3, 3)]);
        assert_eq!(spans("a*", "a", true), vec![(0, 1)]);
        assert_eq!(spans("a*", "ba", true), vec![(0, 0), (1, 2)]);
    }

    #[test]
    fn find_all_reports_group_text() {
        let line = "x: size=42;";
        let m = &pat(r"\(\w\+\)=\(\d\+\)").find_all(line, true)[0];
        assert_eq!(m.group(line, 0), Some("size=42"));
        assert_eq!(m.group(line, 1), Some("size"));
        assert_eq!(m.group(line, 2), Some("42"));
        assert_eq!(m.group(line, 3), None);
        assert_eq!(m.group(line, 10), None);
        assert_eq!(m.bytes(), (3, 10));
        // Group spans are bytes, columns are UTF-16: they part ways off ASCII.
        let line = "😀 size=42;";
        let m = &pat(r"\(\w\+\)=\(\d\+\)").find_all(line, true)[0];
        assert_eq!(m.group(line, 0), Some("size=42"));
        assert_eq!(m.group(line, 2), Some("42"));
        assert_eq!((m.start, m.end), (3, 10));
        assert_eq!(m.bytes(), (5, 12));
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
    fn find_ranked_reports_document_order_ranks() {
        let b = buf("foo bar\nbaz bar\nqux");
        let p = pat("bar");
        p.budget_for(&b);
        // Forward from the top: first match is rank 1 of 2.
        assert_eq!(
            find_ranked(&b, Pos::new(0, 0), &p, false, 1),
            Some((Pos::new(0, 4), 0, 2))
        );
        // From the first match: the second, then wrap back to the first.
        assert_eq!(
            find_ranked(&b, Pos::new(0, 4), &p, false, 1),
            Some((Pos::new(1, 4), 1, 2))
        );
        assert_eq!(
            find_ranked(&b, Pos::new(1, 4), &p, false, 1),
            Some((Pos::new(0, 4), 0, 2))
        );
        // Backward from the top wraps to the last match.
        assert_eq!(
            find_ranked(&b, Pos::new(0, 0), &p, true, 1),
            Some((Pos::new(1, 4), 1, 2))
        );
        // Counts step through the ring like repeated `n`.
        assert_eq!(
            find_ranked(&b, Pos::new(0, 0), &p, false, 2),
            Some((Pos::new(1, 4), 1, 2))
        );
        assert_eq!(find_ranked(&b, Pos::new(0, 0), &pat("zzz"), false, 1), None);
        // A lone match is its own successor after a full wrap.
        let b = buf("a foo b");
        assert_eq!(
            find_ranked(&b, Pos::new(0, 2), &pat("foo"), false, 1),
            Some((Pos::new(0, 2), 0, 1))
        );
    }

    #[test]
    fn find_ranked_agrees_with_find() {
        // Same traversal, so `n` (find) and its report (find_ranked) can't
        // drift apart — including overlapping starts.
        let b = buf("alpha 42\nbeta 7");
        let p = pat(r"\d\+");
        for from in [Pos::new(0, 0), Pos::new(0, 6), Pos::new(0, 7), Pos::new(1, 5)] {
            for backward in [false, true] {
                assert_eq!(
                    find(&b, from, &p, backward, 1),
                    find_ranked(&b, from, &p, backward, 1).map(|(pos, ..)| pos),
                    "from {from:?} backward {backward}"
                );
            }
        }
    }

    #[test]
    fn preview_reports_ranges_and_peeks_ahead() {
        let b = buf("foo bar\nbaz bar\nqux bar");
        let p = pat("bar");
        p.budget_for(&b);
        // Cursor at the top: peek at line 0's match, highlight the others.
        let (matches, current) = preview(&b, Pos::new(0, 0), &p, false);
        assert_eq!(current, Some([0, 4, 7]));
        assert_eq!(matches, vec![1, 4, 7, 2, 4, 7]);
        // Past the first match: the peek moves on.
        let (matches, current) = preview(&b, Pos::new(0, 4), &p, false);
        assert_eq!(current, Some([1, 4, 7]));
        assert_eq!(matches, vec![0, 4, 7, 2, 4, 7]);
        // Backward from the top wraps to the last match.
        let (_, current) = preview(&b, Pos::new(0, 0), &p, true);
        assert_eq!(current, Some([2, 4, 7]));
        // Nothing to find: nothing to show.
        assert_eq!(preview(&b, Pos::new(0, 0), &pat("zzz"), false), (vec![], None));
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
