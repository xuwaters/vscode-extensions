//! The engine's regex against a reference implementation.
//!
//! Our patterns are Vim's and our VM backtracks; the `regex` crate spells
//! patterns differently and matches without backtracking. Where the two
//! dialects mean the same thing they must *answer* the same thing, and the
//! crate is a far better oracle than hand-written expectations — especially
//! for repeats, which the VM counts rather than walks (see `Inst::Rep`) and
//! which therefore have to give back the right number of characters when what
//! follows them fails.
//!
//! Subjects stay ASCII and single-line, so byte offsets, char indices and
//! UTF-16 columns coincide and the two engines' spans compare directly.

use vim_engine::search::Pattern;

/// The same pattern in both dialects: `(ours, the regex crate's)`.
const PATTERNS: &[(&str, &str)] = &[
    // Repeats, the reason this file exists.
    ("a*", "a*"),
    (r"a\+", "a+"),
    (r"a\?b", "a?b"),
    (r"a\{2}", "a{2}"),
    (r"a\{2,3}", "a{2,3}"),
    (r"a\{2,}", "a{2,}"),
    (r"a\{,3}", "a{0,3}"),
    (r"a\{2,4}b", "a{2,4}b"),
    (r"a\+ab", "a+ab"),
    (r"a\+b\+", "a+b+"),
    (r"\d\+", r"\d+"),
    (r"\d\{2,4}", r"\d{2,4}"),
    (r"\w\+", r"\w+"),
    (r"\s*x", r"[ \t]*x"),
    (r"[a-c]\+", "[a-c]+"),
    (r"[^ab]\+", "[^ab]+"),
    (r"\%(ab\)\+", "(?:ab)+"),
    (r"\(ab\)\+", "(ab)+"),
    (r"x\{-}y", "x*?y"),
    (r".\{-}b", ".*?b"),
    (r"a.\{-}b", "a.*?b"),
    (r"\d\{-1,}9", "[0-9]+?9"),
    (".*", ".*"),
    (".*=.*;", ".*=.*;"),
    (r"\d\+\.\d\+", r"\d+\.\d+"),
    // Everything else, for company.
    ("abc", "abc"),
    (r"a\|bc", "a|bc"),
    (r"\(a\|b\)\+c", "(a|b)+c"),
    ("^ab", "^ab"),
    ("ab$", "ab$"),
    (r"\<val\>", r"\bval\b"),
    (r"\(\w\+\)=\(\d\+\)", r"(\w+)=(\d+)"),
];

const SUBJECTS: &[&str] = &[
    "",
    "a",
    "b",
    "aaa",
    "ab",
    "aab",
    "aaab",
    "aaaaab",
    "abab",
    "ababc",
    "x 42 y 7",
    "let val = 3.14;",
    "size=42; other=7;",
    "a1b22c333",
    "  spaced   out\tthere ",
    "xxxxy",
    "9999",
    "12a34b",
    "zzz",
    "the quick brown fox",
];

/// Non-overlapping match spans, as `:s` walks them.
fn ours(pattern: &str, subject: &str) -> Vec<(usize, usize)> {
    let pat = Pattern::parse(pattern).expect("compiles");
    pat.find_all(subject, true).iter().map(|m| m.bytes()).collect()
}

fn theirs(pattern: &str, subject: &str) -> Vec<(usize, usize)> {
    let re = regex::Regex::new(pattern).expect("compiles");
    re.find_iter(subject).map(|m| (m.start(), m.end())).collect()
}

#[test]
fn matches_agree_with_the_regex_crate() {
    for (mine, theirs_src) in PATTERNS {
        for subject in SUBJECTS {
            assert_eq!(
                ours(mine, subject),
                theirs(theirs_src, subject),
                "pattern {mine:?} (ref {theirs_src:?}) over {subject:?}"
            );
        }
    }
}

#[test]
fn capture_groups_agree_with_the_regex_crate() {
    let cases: &[(&str, &str, &str)] = &[
        (r"\(\w\+\)=\(\d\+\)", r"(\w+)=(\d+)", "size=42; other=7;"),
        (r"\(a\+\)\(b\+\)", "(a+)(b+)", "xaabbb"),
        (r"\(a\|b\)\+", "(a|b)+", "abab c"),
        (r"\(x\?\)\(y\+\)", "(x?)(y+)", "zyyy xy"),
        (r"\(\d\{1,2}\)-\(\d\+\)", r"(\d{1,2})-(\d+)", "1-2 33-444 555-6"),
    ];
    for (mine, theirs_src, subject) in cases {
        let pat = Pattern::parse(mine).expect("compiles");
        let re = regex::Regex::new(theirs_src).expect("compiles");
        let found = pat.find_all(subject, true);
        let expected: Vec<_> = re.captures_iter(subject).collect();
        assert_eq!(found.len(), expected.len(), "match count for {mine:?}");
        for (m, caps) in found.iter().zip(expected) {
            for n in 0..=re.captures_len() - 1 {
                assert_eq!(
                    m.group(subject, n),
                    caps.get(n).map(|g| g.as_str()),
                    "group {n} of {mine:?} over {subject:?}"
                );
            }
        }
    }
}
