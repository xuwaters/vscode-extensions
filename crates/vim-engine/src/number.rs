//! `ctrl-a` / `ctrl-x`: add to or subtract from the number at or after the
//! cursor. Follows Vim's `do_addsub` (vim/src/ops.c) with its default
//! `'nrformats'` of `bin,hex`: `0x1f` and `0b101` keep their base, prefix
//! case and width, everything else is decimal, and a `-` right in front of a
//! decimal makes it negative (`:help CTRL-A`).
//!
//! Columns in and out are UTF-16 units, like every other column the engine
//! deals in. Only ASCII characters are ever digits, so the scan itself is
//! over chars.

use crate::buffer::{chars_with_cols, utf16_len};

/// One number rewritten: the text in `[start, end)` becomes `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bumped {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Bumped {
    /// Column of the new number's last char, where `ctrl-a` leaves the
    /// cursor.
    pub fn last_col(&self) -> usize {
        let last = self.text.chars().last().map_or(0, char::len_utf16);
        self.start + utf16_len(&self.text) - last
    }
}

/// Normal-mode `ctrl-a`: the number under the cursor, or the first one after
/// it on the line. A cursor anywhere in `0x1f` — the `x` included — means
/// that hex number, not the `1f` behind it.
pub fn bump_at(line: &str, col: usize, amount: u64, subtract: bool) -> Option<Bumped> {
    let cols = chars_with_cols(line);
    let p: Vec<char> = cols.iter().map(|&(_, ch)| ch).collect();
    let at = cols.partition_point(|&(c, _)| c <= col).checked_sub(1)?;
    if at >= p.len() {
        return None;
    }
    let mut i = at;
    // Back over what could be the digits of a binary or hex number, to see
    // whether a `0x` / `0b` prefix sits in front of them.
    while i > 0 && is_bin(p[i]) {
        i -= 1;
    }
    while i > 0 && p[i].is_ascii_hexdigit() {
        i -= 1;
    }
    if !prefix_at(&p, i, 'x') {
        // Not inside a hex number after all: the binary digits it might be
        // are decimal ones too, so look for `0b` behind those instead.
        i = at;
        while i > 0 && p[i].is_ascii_digit() {
            i -= 1;
        }
    }
    if prefix_at(&p, i, 'x') || prefix_at(&p, i, 'b') {
        i -= 1; // onto the `0`
    } else {
        // Forward to the first digit, then back to where its run starts.
        i = at;
        while i < p.len() && !p[i].is_ascii_digit() {
            i += 1;
        }
        if i == p.len() {
            return None;
        }
        while i > 0 && p[i - 1].is_ascii_digit() {
            i -= 1;
        }
    }
    let negative = i > 0 && p[i - 1] == '-';
    rewrite(&cols, &p, i, p.len(), negative, amount, subtract)
}

/// Visual-mode `ctrl-a` on one line's share of the selection, chars
/// `[from, to)`: the first number that starts inside it, read no further
/// than its end. A `-` counts only when the selection covers it.
pub fn bump_in(line: &str, from: usize, to: usize, amount: u64, subtract: bool) -> Option<Bumped> {
    let cols = chars_with_cols(line);
    let p: Vec<char> = cols.iter().map(|&(_, ch)| ch).collect();
    let first = cols.partition_point(|&(c, _)| c < from);
    let limit = cols.partition_point(|&(c, _)| c < to).min(p.len());
    let mut i = first;
    while i < limit && !p[i].is_ascii_digit() {
        i += 1;
    }
    if i >= limit {
        return None;
    }
    let negative = i > first && p[i - 1] == '-';
    rewrite(&cols, &p, i, limit, negative, amount, subtract)
}

fn is_bin(ch: char) -> bool {
    ch == '0' || ch == '1'
}

/// Is `p[i]` the `x` (or `b`) of a `0x1f` (`0b10`) with a digit after it?
fn prefix_at(p: &[char], i: usize, kind: char) -> bool {
    let digit = |ch: char| if kind == 'x' { ch.is_ascii_hexdigit() } else { is_bin(ch) };
    i > 0
        && p[i].eq_ignore_ascii_case(&kind)
        && p[i - 1] == '0'
        && p.get(i + 1).is_some_and(|&ch| digit(ch))
}

/// Replace the number whose first digit is `p[i]` (its sign, when
/// `negative`, at `p[i - 1]`), reading no further than `limit`.
fn rewrite(
    cols: &[(usize, char)],
    p: &[char],
    i: usize,
    limit: usize,
    negative: bool,
    amount: u64,
    subtract: bool,
) -> Option<Bumped> {
    let first_digit = *p.get(i)?;
    if !first_digit.is_ascii_digit() {
        return None;
    }
    let (base, pre) = if i + 2 < limit && prefix_at(p, i + 1, 'x') {
        (16, Some(p[i + 1]))
    } else if i + 2 < limit && prefix_at(p, i + 1, 'b') {
        (2, Some(p[i + 1]))
    } else {
        (10, None)
    };
    let digits_from = if pre.is_some() { i + 2 } else { i };
    // Only a decimal has a sign; `-0x10` is a dash and a hex number. A
    // sign that counts is rewritten along with the digits.
    let signed = negative && pre.is_none();
    let start = if signed { i - 1 } else { i };
    let mut negative = signed;
    let mut end = digits_from;
    let (mut n, mut overflow) = (0u64, false);
    while end < limit {
        let Some(d) = p[end].to_digit(base) else { break };
        match n.checked_mul(base as u64).and_then(|v| v.checked_add(d as u64)) {
            Some(v) => n = v,
            None => {
                // Too big to hold: Vim pins it at the maximum and leaves it
                // there rather than adding to a value it could not read.
                n = u64::MAX;
                overflow = true;
            }
        }
        end += 1;
    }

    let old = n;
    let subtract = subtract != negative;
    if !overflow {
        n = if subtract { n.wrapping_sub(amount) } else { n.wrapping_add(amount) };
    }
    if pre.is_none() {
        // A decimal that crossed zero changes sign instead of wrapping.
        if subtract && n > old {
            n = n.wrapping_neg();
            negative = !negative;
        } else if !subtract && n < old {
            n = !n;
            negative = !negative;
        }
        if n == 0 {
            negative = false;
        }
    }

    let mut text = String::new();
    if negative {
        text.push('-');
    }
    let body = match base {
        16 => {
            // Hex digits take the case of the last letter in the old number,
            // the `x` of the prefix included.
            let upper = p[i..end]
                .iter()
                .rev()
                .find(|ch| ch.is_ascii_alphabetic())
                .is_some_and(|ch| ch.is_ascii_uppercase());
            if upper { format!("{n:X}") } else { format!("{n:x}") }
        }
        2 => format!("{n:b}"),
        _ => n.to_string(),
    };
    if let Some(pre) = pre {
        text.push('0');
        text.push(pre);
    }
    // A number written with leading zeros keeps its width: `007` becomes
    // `008`, `0x00ff` becomes `0x0100`.
    let width = end - digits_from;
    if first_digit == '0' {
        text.extend(std::iter::repeat_n('0', width.saturating_sub(body.len())));
    }
    text.push_str(&body);
    let col = |k: usize| match cols.get(k) {
        Some(&(c, _)) => c,
        None => cols.last().map_or(0, |&(c, ch)| c + ch.len_utf16()),
    };
    Some(Bumped { start: col(start), end: col(end), text })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ctrl-a` (`ctrl-x` for a negative amount) at `col`: the line it
    /// leaves and the column the cursor lands on.
    fn bump(line: &str, col: usize, amount: i64) -> Option<(String, usize)> {
        let b = bump_at(line, col, amount.unsigned_abs(), amount < 0)?;
        Some((splice(line, &b), b.last_col()))
    }

    /// `line` with `b` applied; columns are UTF-16, so go through chars.
    fn splice(line: &str, b: &Bumped) -> String {
        let cols = chars_with_cols(line);
        let byte = |col: usize| {
            line.char_indices()
                .zip(&cols)
                .find(|&(_, &(c, _))| c >= col)
                .map_or(line.len(), |((at, _), _)| at)
        };
        let mut out = line.to_string();
        out.replace_range(byte(b.start)..byte(b.end), &b.text);
        out
    }

    #[test]
    fn decimal_under_and_after_the_cursor() {
        assert_eq!(bump("x 41 y", 0, 1), Some(("x 42 y".into(), 3)));
        assert_eq!(bump("x 41 y", 3, 1), Some(("x 42 y".into(), 3)));
        assert_eq!(bump("a1 b2", 4, 1), Some(("a1 b3".into(), 4)));
        assert_eq!(bump("x 9 y", 0, 1), Some(("x 10 y".into(), 3)));
        assert_eq!(bump("abc", 0, 1), None);
        assert_eq!(bump("x 1 y", 4, 1), None); // nothing at or after
    }

    #[test]
    fn negative_numbers_cross_zero() {
        assert_eq!(bump("x -5 y", 0, 10), Some(("x 5 y".into(), 2)));
        assert_eq!(bump("x 3 y", 0, -5), Some(("x -2 y".into(), 3)));
        assert_eq!(bump("x -1", 0, 1), Some(("x 0".into(), 2)));
        // The dash in front of the number found is its sign.
        assert_eq!(bump("x 5-3", 3, 1), Some(("x 5-2".into(), 4)));
    }

    #[test]
    fn leading_zeros_keep_the_width() {
        assert_eq!(bump("x 007 y", 0, 1), Some(("x 008 y".into(), 4)));
        assert_eq!(bump("099", 0, 1), Some(("100".into(), 2)));
        assert_eq!(bump("000", 0, -1), Some(("-001".into(), 3)));
    }

    #[test]
    fn hex_and_binary_keep_their_base() {
        assert_eq!(bump("x 0xff y", 0, 1), Some(("x 0x100 y".into(), 6)));
        assert_eq!(bump("x 0xFe y", 0, 1), Some(("x 0xff y".into(), 5)));
        assert_eq!(bump("0xFF", 3, 1), Some(("0x100".into(), 4)));
        assert_eq!(bump("0x0F", 0, 1), Some(("0x10".into(), 3)));
        assert_eq!(bump("0x00ff", 0, 1), Some(("0x0100".into(), 5)));
        assert_eq!(bump("x 0b101 y", 0, -3), Some(("x 0b010 y".into(), 6)));
        // On the `x`, or deep in the digits, it is still that number.
        assert_eq!(bump("0x1f", 1, 1), Some(("0x20".into(), 3)));
        assert_eq!(bump("0b11", 3, 1), Some(("0b100".into(), 4)));
        // A dash is not a hex number's sign, and hex wraps unsigned.
        assert_eq!(bump("x -0x10", 0, 1), Some(("x -0x11".into(), 6)));
        assert_eq!(
            bump("x 0x0010", 0, -20),
            Some(("x 0xfffffffffffffffc".into(), 19))
        );
        // `0x` with no hex digit after it is just a zero.
        assert_eq!(bump("0xg", 0, 1), Some(("1xg".into(), 0)));
    }

    #[test]
    fn huge_numbers_follow_vim() {
        // Past u64: pinned at the maximum and left there.
        assert_eq!(
            bump("x 99999999999999999999999 y", 0, 1),
            Some(("x 18446744073709551615 y".into(), 21))
        );
        // At the maximum: one more wraps to the far side of zero.
        assert_eq!(
            bump("x 18446744073709551615 y", 0, 1),
            Some(("x -18446744073709551615 y".into(), 22))
        );
    }

    #[test]
    fn visual_reads_only_inside_the_selection() {
        let b = bump_in("a 5 b 6", 2, 7, 1, false).unwrap();
        assert_eq!((b.start, b.end, b.text.as_str()), (2, 3, "6"));
        // The dash sits outside the selection, so the number is positive.
        let b = bump_in("c -7 d", 3, 6, 1, false).unwrap();
        assert_eq!((b.start, b.end, b.text.as_str()), (3, 4, "8"));
        let b = bump_in("c -7 d", 0, 6, 1, false).unwrap();
        assert_eq!((b.start, b.end, b.text.as_str()), (2, 4, "-6"));
        // Digits past the selection's end are not read.
        let b = bump_in("123", 0, 2, 1, false).unwrap();
        assert_eq!((b.start, b.end, b.text.as_str()), (0, 2, "13"));
        assert_eq!(bump_in("c -", 0, 3, 1, false), None);
    }

    #[test]
    fn columns_are_utf16() {
        // `😀` is two UTF-16 units wide.
        assert_eq!(bump("😀 1", 0, 1), Some(("😀 2".into(), 3)));
        assert_eq!(bump("😀 1", 2, 1), Some(("😀 2".into(), 3)));
    }
}
