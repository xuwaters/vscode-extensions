//! Folding: multi-line containers (closing bracket left visible), runs of
//! line comments, multi-line block comments and strings.

use crate::ast::{File, Value};
use crate::lexer::{lex, TokenKind};
use crate::spans::SpanTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldKind {
    Comment,
    Region,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldingRange {
    pub start_line: u32,
    pub end_line: u32,
    pub kind: FoldKind,
}

pub fn folding_ranges(file: &File, source: &str, spans: &SpanTable) -> Vec<FoldingRange> {
    let line = |offset: u32| spans.offset_to_line_col(source, offset).line;
    let mut out = Vec::new();
    if let Some(root) = &file.value {
        containers(root, &line, &mut out);
    }
    // Comments and strings are easiest found in the token stream.
    let tokens = lex(source).tokens;
    let mut run: Option<(u32, u32)> = None;
    for tok in &tokens {
        let (start, end) = (line(tok.span.start), line(tok.span.end));
        let continues_run = tok.kind == TokenKind::LineComment && tok.newlines_before == 1;
        if let Some((first, last)) = run {
            if continues_run && start == last + 1 {
                run = Some((first, start));
                continue;
            }
            if last > first {
                out.push(FoldingRange { start_line: first, end_line: last, kind: FoldKind::Comment });
            }
            run = None;
        }
        match tok.kind {
            TokenKind::LineComment => run = Some((start, start)),
            TokenKind::BlockComment if end > start => {
                out.push(FoldingRange { start_line: start, end_line: end, kind: FoldKind::Comment })
            }
            TokenKind::String | TokenKind::ByteString if end > start => {
                out.push(FoldingRange { start_line: start, end_line: end, kind: FoldKind::Region })
            }
            _ => {}
        }
    }
    if let Some((first, last)) = run.filter(|(f, l)| l > f) {
        out.push(FoldingRange { start_line: first, end_line: last, kind: FoldKind::Comment });
    }
    out.sort_by_key(|r| (r.start_line, r.end_line));
    out
}

fn containers(v: &Value, line: &impl Fn(u32) -> u32, out: &mut Vec<FoldingRange>) {
    let Some(c) = v.container() else { return };
    if let Some(close) = c.close {
        let (open, close) = (line(c.open.start), line(close.start));
        if close > open + 1 {
            out.push(FoldingRange { start_line: open, end_line: close - 1, kind: FoldKind::Region });
        }
    }
    for e in &c.entries {
        containers(&e.value, line, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use pretty_assertions::assert_eq;

    fn folds(src: &str) -> Vec<(u32, u32, FoldKind)> {
        let (file, _) = parse(src);
        folding_ranges(&file, src, &SpanTable::new(src))
            .into_iter()
            .map(|r| (r.start_line, r.end_line, r.kind))
            .collect()
    }

    #[test]
    fn containers_fold_up_to_the_closing_line() {
        let src = "Config(\n    a: [\n        1,\n    ],\n    b: (1, 2),\n)";
        assert_eq!(folds(src), vec![(0, 4, FoldKind::Region), (1, 2, FoldKind::Region)]);
    }

    #[test]
    fn comment_runs_and_blocks() {
        let src = "// a\n// b\n// c\n\n// lone\n/* x\n y */\n1";
        assert_eq!(folds(src), vec![(0, 2, FoldKind::Comment), (5, 6, FoldKind::Comment)]);
    }

    #[test]
    fn multiline_strings() {
        assert_eq!(folds("\"a\nb\nc\""), vec![(0, 2, FoldKind::Region)]);
    }
}
