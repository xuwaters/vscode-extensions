//! Selection ranges — expand-selection that respects typst's actual grammar,
//! including math.
//!
//! The ancestor chain of `LinkedNode::leaf_at`, innermost first, which is
//! exactly the shape LSP wants.

use lsp_types::{SelectionRange, SelectionRangeParams};
use typst::syntax::{LinkedNode, Side, Source};

use crate::convert::range_to_lsp;
use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// `textDocument/selectionRange`.
    pub fn selection_ranges(
        &mut self,
        params: SelectionRangeParams,
    ) -> Option<Vec<SelectionRange>> {
        let (_, source) = self.source_of(&params.text_document.uri)?;

        Some(
            params
                .positions
                .into_iter()
                .map(|position| {
                    let offset = crate::convert::position_to_offset(&source, position);
                    selection_range_at(&source, offset)
                })
                .collect(),
        )
    }
}

/// The ancestor chain at an offset, as a nested `SelectionRange`.
pub fn selection_range_at(source: &Source, offset: usize) -> SelectionRange {
    let root = LinkedNode::new(source.root());
    let leaf = root
        .leaf_at(offset, Side::Before)
        .or_else(|| root.leaf_at(offset, Side::After));

    // Collect outermost-first so the nesting can be built by folding inwards.
    let mut chain = Vec::new();
    let mut node = leaf;
    while let Some(current) = node {
        let range = current.range();
        if chain.last() != Some(&range) {
            chain.push(range);
        }
        node = current.parent().cloned();
    }

    if chain.is_empty() {
        chain.push(0..source.text().len());
    }

    let mut result: Option<Box<SelectionRange>> = None;
    for range in chain.into_iter().rev() {
        result = Some(Box::new(SelectionRange {
            range: range_to_lsp(source, range),
            parent: result,
        }));
    }

    *result.expect("the chain is never empty")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flatten the chain into byte-ish ranges for readable assertions.
    fn chain(text: &str, offset: usize) -> Vec<(u32, u32, u32, u32)> {
        let source = Source::detached(text);
        let mut out = Vec::new();
        let mut current = Some(selection_range_at(&source, offset));
        while let Some(range) = current {
            out.push((
                range.range.start.line,
                range.range.start.character,
                range.range.end.line,
                range.range.end.character,
            ));
            current = range.parent.map(|parent| *parent);
        }
        out
    }

    #[test]
    fn the_chain_grows_outwards() {
        let text = "#let f(x) = x + 1\n";
        let at = text.find("x + 1").unwrap() + 1;
        let ranges = chain(text, at);

        assert!(ranges.len() > 1, "expected an ancestor chain");
        for pair in ranges.windows(2) {
            let (inner, outer) = (pair[0], pair[1]);
            assert!(
                (outer.0, outer.1) <= (inner.0, inner.1)
                    && (outer.2, outer.3) >= (inner.2, inner.3),
                "range {inner:?} is not contained by its parent {outer:?}"
            );
        }
    }

    #[test]
    fn math_has_its_own_chain() {
        let text = "$ sum_(i=1)^n i $\n";
        let at = text.find("i=1").unwrap() + 1;
        let ranges = chain(text, at);
        assert!(ranges.len() > 2, "math should nest several levels: {ranges:?}");
    }

    #[test]
    fn an_empty_document_still_answers() {
        let ranges = chain("", 0);
        assert_eq!(ranges.len(), 1);
    }
}
