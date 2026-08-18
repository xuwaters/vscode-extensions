//! Folding ranges.
//!
//! Headings fold to the start of the next heading of equal or lower level;
//! blocks fold to their braces; runs of line comments fold together. All three
//! are direct consequences of having a real syntax tree, and all three are
//! cheap.

use lsp_types::{FoldingRange, FoldingRangeKind, FoldingRangeParams};
use typst::syntax::{LinkedNode, Source, SyntaxKind, ast};

use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// `textDocument/foldingRange`.
    pub fn folding_ranges(&mut self, params: FoldingRangeParams) -> Option<Vec<FoldingRange>> {
        let (_, source) = self.source_of(&params.text_document.uri)?;
        Some(folding_ranges(&source))
    }
}

/// Every foldable region in a document.
pub fn folding_ranges(source: &Source) -> Vec<FoldingRange> {
    let mut byte_ranges: Vec<(std::ops::Range<usize>, Option<FoldingRangeKind>)> = Vec::new();

    collect_blocks(&LinkedNode::new(source.root()), &mut byte_ranges);
    collect_headings(source, &mut byte_ranges);
    collect_comment_runs(source, &mut byte_ranges);

    let lines = source.lines();
    let mut out = Vec::new();

    for (range, kind) in byte_ranges {
        let start_line = lines.byte_to_line(range.start).unwrap_or(0);
        let end_line = lines.byte_to_line(range.end.saturating_sub(1)).unwrap_or(start_line);

        // A region that does not cross a line boundary has nothing to fold.
        if end_line <= start_line {
            continue;
        }

        out.push(FoldingRange {
            start_line: start_line as u32,
            end_line: end_line as u32,
            kind,
            ..FoldingRange::default()
        });
    }

    out.sort_by_key(|range| (range.start_line, range.end_line));
    out.dedup_by_key(|range| (range.start_line, range.end_line));
    out
}

fn collect_blocks(
    node: &LinkedNode,
    out: &mut Vec<(std::ops::Range<usize>, Option<FoldingRangeKind>)>,
) {
    let foldable = matches!(
        node.kind(),
        SyntaxKind::CodeBlock
            | SyntaxKind::ContentBlock
            | SyntaxKind::Array
            | SyntaxKind::Dict
            | SyntaxKind::Args
            | SyntaxKind::Params
            | SyntaxKind::Raw
            | SyntaxKind::BlockComment
            | SyntaxKind::Equation
    );

    if foldable {
        let kind = match node.kind() {
            SyntaxKind::BlockComment => Some(FoldingRangeKind::Comment),
            _ => None,
        };
        out.push((node.range(), kind));
    }

    for child in node.children() {
        collect_blocks(&child, out);
    }
}

/// A heading folds down to the next heading of equal or lower level.
fn collect_headings(
    source: &Source,
    out: &mut Vec<(std::ops::Range<usize>, Option<FoldingRangeKind>)>,
) {
    let mut headings: Vec<(usize, usize)> = Vec::new(); // (start offset, depth)
    collect_heading_starts(&LinkedNode::new(source.root()), &mut headings);
    headings.sort_by_key(|(start, _)| *start);

    for (index, &(start, depth)) in headings.iter().enumerate() {
        let end = headings[index + 1..]
            .iter()
            .find(|(_, next_depth)| *next_depth <= depth)
            .map(|(next_start, _)| *next_start)
            .unwrap_or(source.text().len());
        out.push((start..end, None));
    }
}

fn collect_heading_starts(node: &LinkedNode, out: &mut Vec<(usize, usize)>) {
    if node.kind() == SyntaxKind::Heading
        && let Some(heading) = node.cast::<ast::Heading>()
    {
        out.push((node.range().start, heading.depth().get()));
    }
    for child in node.children() {
        collect_heading_starts(&child, out);
    }
}

/// Consecutive `//` lines fold as one region, the way they do in every other
/// language server.
fn collect_comment_runs(
    source: &Source,
    out: &mut Vec<(std::ops::Range<usize>, Option<FoldingRangeKind>)>,
) {
    let mut comments: Vec<std::ops::Range<usize>> = Vec::new();
    collect_line_comments(&LinkedNode::new(source.root()), &mut comments);
    comments.sort_by_key(|range| range.start);

    let lines = source.lines();
    let mut run: Option<(std::ops::Range<usize>, usize)> = None;

    for comment in comments {
        let line = lines.byte_to_line(comment.start).unwrap_or(0);
        match run.take() {
            Some((range, previous_line)) if line == previous_line + 1 => {
                run = Some((range.start..comment.end, line));
            }
            Some((range, _)) => {
                out.push((range, Some(FoldingRangeKind::Comment)));
                run = Some((comment, line));
            }
            None => run = Some((comment, line)),
        }
    }

    if let Some((range, _)) = run {
        out.push((range, Some(FoldingRangeKind::Comment)));
    }
}

fn collect_line_comments(node: &LinkedNode, out: &mut Vec<std::ops::Range<usize>>) {
    if node.kind() == SyntaxKind::LineComment {
        out.push(node.range());
    }
    for child in node.children() {
        collect_line_comments(&child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(text: &str) -> Vec<(u32, u32)> {
        folding_ranges(&Source::detached(text))
            .into_iter()
            .map(|range| (range.start_line, range.end_line))
            .collect()
    }

    #[test]
    fn a_heading_folds_to_the_next_one_of_the_same_level() {
        // 0: = One  1: (blank)  2: body  3: (blank)  4: = Two
        let folds = ranges("= One\n\nbody\n\n= Two\n\nmore\n");
        assert!(folds.contains(&(0, 3)), "got {folds:?}");
    }

    #[test]
    fn a_subheading_folds_inside_its_parent() {
        let folds = ranges("= One\n\n== Sub\n\nbody\n\n= Two\n");
        assert!(folds.iter().any(|&(start, _)| start == 2), "got {folds:?}");
    }

    #[test]
    fn a_code_block_folds() {
        let folds = ranges("#{\n  let x = 1\n  x\n}\n");
        assert!(folds.contains(&(0, 3)), "got {folds:?}");
    }

    #[test]
    fn consecutive_line_comments_fold_as_one_run() {
        let folds = ranges("// one\n// two\n// three\n\n= Heading\n");
        assert!(folds.contains(&(0, 2)), "got {folds:?}");
    }

    #[test]
    fn a_single_line_region_is_not_offered() {
        let folds = ranges("#{ let x = 1 }\n");
        assert!(folds.is_empty(), "got {folds:?}");
    }
}
