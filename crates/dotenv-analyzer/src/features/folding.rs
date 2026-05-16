//! Folding ranges — runs of comment lines and multi-line quoted values.

use crate::ast::{Entry, File, ValueKind};
use crate::spans::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldingRange {
    pub span: ByteSpan,
    pub kind: FoldKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldKind {
    Comment,
    Region,
}

pub fn folding_ranges(file: &File, source: &str) -> Vec<FoldingRange> {
    let mut out = Vec::new();
    // Comment runs (>= 2 consecutive comment lines).
    let mut run: Option<(u32, u32)> = None;
    for entry in &file.entries {
        if let Entry::Comment(c) = entry {
            let span = c.span;
            run = Some(match run {
                Some((start, _end)) => (start, span.end),
                None => (span.start, span.end),
            });
        } else if let Some((start, end)) = run.take() {
            if has_multiple_lines(start, end, source) {
                out.push(FoldingRange {
                    span: ByteSpan::new(start, end),
                    kind: FoldKind::Comment,
                });
            }
        }
    }
    if let Some((start, end)) = run {
        if has_multiple_lines(start, end, source) {
            out.push(FoldingRange {
                span: ByteSpan::new(start, end),
                kind: FoldKind::Comment,
            });
        }
    }
    // Multi-line quoted values.
    for entry in &file.entries {
        if let Entry::Assignment(a) = entry {
            if matches!(
                a.value_kind,
                ValueKind::DoubleQuoted | ValueKind::SingleQuoted | ValueKind::UnclosedDouble | ValueKind::UnclosedSingle
            ) {
                if has_multiple_lines(a.span.start, a.span.end, source) {
                    out.push(FoldingRange {
                        span: a.span,
                        kind: FoldKind::Region,
                    });
                }
            }
        }
    }
    out
}

fn has_multiple_lines(start: u32, end: u32, source: &str) -> bool {
    let bytes = source.as_bytes();
    let s = start as usize;
    let e = (end as usize).min(bytes.len());
    let mut newlines = 0;
    for &b in &bytes[s..e] {
        if b == b'\n' {
            newlines += 1;
            if newlines >= 2 {
                return true;
            }
        }
    }
    // If there's at least one newline but no second one, treat that as
    // a single line plus its terminator — not foldable.
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    #[test]
    fn folds_consecutive_comments() {
        let src = "# a\n# b\n# c\nFOO=1\n";
        let pf = parse(FileUri::new("t"), src.to_string());
        let ranges = folding_ranges(&pf.ast, &pf.source);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].kind, FoldKind::Comment);
    }

    #[test]
    fn folds_multiline_quoted() {
        let src = "MSG=\"line1\nline2\nline3\"\n";
        let pf = parse(FileUri::new("t"), src.to_string());
        let ranges = folding_ranges(&pf.ast, &pf.source);
        assert!(ranges.iter().any(|r| r.kind == FoldKind::Region));
    }
}
