//! Folding ranges: collapse each `[table]` / `[[array-of-tables]]` block
//! from its header line down to the line before the next header.
//!
//! This works straight off the source text rather than the AST so it folds
//! even sections we don't otherwise model (`[plugins.impl.x]`, …).

use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldingRange {
    pub span: ByteSpan,
    pub kind: FoldKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldKind {
    Region,
}

/// A line that, after leading whitespace, begins a TOML table header.
fn is_header(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

pub fn folding_ranges(source: &str) -> Vec<FoldingRange> {
    // Record (header_start_offset, content_end_offset) per section.
    let mut sections: Vec<(usize, usize)> = Vec::new();
    let mut open: Option<usize> = None;
    // Tracks the end offset of the last non-blank line seen since the
    // current header, so trailing blank lines fold away cleanly.
    let mut last_content_end = 0usize;

    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let trimmed_len = line.trim_end_matches(['\n', '\r']).len();
        let line_end = offset + trimmed_len;
        let body = &line[..trimmed_len.min(line.len())];

        if is_header(body) {
            if let Some(start) = open.take() {
                sections.push((start, last_content_end));
            }
            open = Some(offset);
            last_content_end = line_end;
        } else if !body.trim().is_empty() {
            last_content_end = line_end;
        }

        offset += line.len();
    }
    if let Some(start) = open.take() {
        sections.push((start, last_content_end));
    }

    sections
        .into_iter()
        .filter_map(|(start, end)| {
            let end = end.max(start);
            let span = ByteSpan::from_usize(start, end);
            // Only fold sections that actually span more than one line.
            if source[start..end].contains('\n') {
                Some(FoldingRange { span, kind: FoldKind::Region })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_each_table() {
        let src = "[config]\nskip_core_tasks = true\n\n[tasks.build]\ncommand = \"cargo\"\nargs = [\"build\"]\n";
        let ranges = folding_ranges(src);
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn single_line_table_is_not_folded() {
        let src = "[empty]\n[tasks.x]\ncommand = \"y\"\n";
        let ranges = folding_ranges(src);
        // `[empty]` has no body, so only `[tasks.x]` folds.
        assert_eq!(ranges.len(), 1);
    }
}
