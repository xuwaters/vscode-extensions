//! `textDocument/foldingRange`.

use lsp_types::{FoldingRange, FoldingRangeKind, FoldingRangeParams};
use wgsl_syntax::BlockKind;

use crate::Server;

impl Server {
    pub fn folding_ranges(&mut self, params: FoldingRangeParams) -> Option<Vec<FoldingRange>> {
        let document = self.document(&params.text_document.uri)?;
        let text = document.text();

        let ranges = document
            .parsed()
            .blocks
            .iter()
            .filter(|block| {
                // Parentheses and brackets fold badly: a wrapped argument list
                // is not a region a reader wants collapsed, and the marker in
                // the gutter is pure noise.
                matches!(block.kind, BlockKind::Brace | BlockKind::Comment)
            })
            .filter_map(|block| {
                let start = document.position(block.span.start);
                let end = document.position(block.span.end);
                if end.line <= start.line {
                    return None;
                }

                // Keep a closing brace that sits alone on its line visible:
                // folding it away leaves the block looking unterminated.
                let end_line = match block.kind {
                    BlockKind::Brace if alone_on_its_line(text, block.span.end) => end.line - 1,
                    _ => end.line,
                };
                if end_line <= start.line {
                    return None;
                }

                Some(FoldingRange {
                    start_line: start.line,
                    end_line,
                    kind: match block.kind {
                        BlockKind::Comment => Some(FoldingRangeKind::Comment),
                        _ => Some(FoldingRangeKind::Region),
                    },
                    ..FoldingRange::default()
                })
            })
            .collect();

        Some(ranges)
    }
}

/// Whether the character ending at `end` is the first non-whitespace on its
/// line — i.e. the `}` of a block written in the usual style.
fn alone_on_its_line(text: &str, end: u32) -> bool {
    let end = end as usize;
    if end == 0 || end > text.len() {
        return false;
    }
    text[..end - 1]
        .rsplit('\n')
        .next()
        .is_some_and(|before| before.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closing_brace_on_its_own_line_is_detected() {
        let text = "fn f() {\n    let x = 1;\n}\n";
        let close = text.rfind('}').unwrap() as u32 + 1;
        assert!(alone_on_its_line(text, close));

        let text = "fn f() { let x = 1; }\n";
        let close = text.rfind('}').unwrap() as u32 + 1;
        assert!(!alone_on_its_line(text, close));
    }
}
