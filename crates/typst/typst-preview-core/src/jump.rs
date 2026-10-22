//! Two-way position mapping between the source and the rendered page.
//!
//! Both directions come from upstream's own jump machinery, unmodified. This is
//! the reason the project needs no compiler patch for preview sync: tinymist's
//! `no-content-hint` fork exists to solve a problem `jump_from_cursor` /
//! `jump_from_click` do not have.

use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};
use typst::introspection::PagedPosition;
use typst::layout::{Abs, Point};
use typst::syntax::{FileId, LinkedNode, Side, Source, SyntaxKind};
use typst_ide::{IdeWorld, Jump};
use typst_layout::PagedDocument;

/// A point on a rendered page, in typographic points from its top-left corner.
///
/// The webview converts client coordinates into this space by dividing by the
/// current zoom, so the server never needs to know about zoom or device pixel
/// ratio.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPoint {
    /// Zero-based page index.
    pub page: usize,
    /// Horizontal offset in points.
    pub x_pt: f64,
    /// Vertical offset in points.
    pub y_pt: f64,
}

/// Where a click in the preview should take the editor.
#[derive(Debug, Clone, PartialEq)]
pub enum JumpTarget {
    /// A byte offset in a source file.
    Source { file: FileId, offset: usize },
    /// An external URL. The host applies a scheme allowlist before opening it.
    Url(String),
    /// Another place in the same document — an internal link.
    Page(DocumentPoint),
}

/// Resolve a click on a rendered page.
pub fn from_click(
    world: &dyn IdeWorld,
    document: &PagedDocument,
    at: DocumentPoint,
) -> Option<JumpTarget> {
    let page = NonZeroUsize::new(at.page + 1)?;
    let position = PagedPosition {
        page,
        point: Point::new(Abs::pt(at.x_pt), Abs::pt(at.y_pt)),
    };

    match typst_ide::jump_from_click(world, document, &position)? {
        Jump::File(file, offset) => Some(JumpTarget::Source { file, offset }),
        Jump::Url(url) => Some(JumpTarget::Url(url.as_str().to_string())),
        Jump::Position(position) => Some(JumpTarget::Page(to_document_point(position))),
    }
}

/// How many text leaves on each side of an unmapped cursor to try. Each try is
/// a walk over the laid-out document, so this stays small.
const NEAREST_TEXT_TRIES: usize = 8;

/// Resolve a cursor position to the places it appears on the page.
///
/// Upstream only maps a cursor touching a `Text` or `MathText` leaf, so a
/// cursor on a blank line, in a comment or on a keyword would leave the preview
/// wherever it was. Those fall back to the nearest text that does appear: the
/// next text after the cursor, else the last before it. Nothing comes back only
/// when no text nearby is on the page at all.
pub fn from_cursor(
    document: &PagedDocument,
    source: &Source,
    cursor: usize,
) -> Vec<DocumentPoint> {
    let mut positions = typst_ide::jump_from_cursor(document, source, cursor);
    if positions.is_empty() {
        positions = nearest_text(document, source, cursor);
    }
    positions.into_iter().map(to_document_point).collect()
}

fn nearest_text(
    document: &PagedDocument,
    source: &Source,
    cursor: usize,
) -> Vec<PagedPosition> {
    fn is_text(node: &LinkedNode) -> bool {
        matches!(node.kind(), SyntaxKind::Text | SyntaxKind::MathText)
    }

    let root = LinkedNode::new(source.root());
    let Some(here) = root
        .leaf_at(cursor, Side::After)
        .or_else(|| root.leaf_at(cursor, Side::Before))
    else {
        return vec![];
    };

    let after = std::iter::successors(Some(here.clone()), |node| node.next_leaf())
        .filter(is_text)
        .take(NEAREST_TEXT_TRIES);
    let before = std::iter::successors(here.prev_leaf(), |node| node.prev_leaf())
        .filter(is_text)
        .take(NEAREST_TEXT_TRIES);

    // A text leaf can still be missing from the page — the body of a `#let`
    // that is never called — so keep going until one lands. Its end offset with
    // `Side::Before` is a cursor upstream resolves to that very leaf.
    after
        .chain(before)
        .map(|leaf| typst_ide::jump_from_cursor(document, source, leaf.range().end))
        .find(|positions| !positions.is_empty())
        .unwrap_or_default()
}

fn to_document_point(position: PagedPosition) -> DocumentPoint {
    DocumentPoint {
        page: position.page.get() - 1,
        x_pt: position.point.x.to_pt(),
        y_pt: position.point.y.to_pt(),
    }
}
