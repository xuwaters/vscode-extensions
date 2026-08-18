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
use typst::syntax::{FileId, Source};
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

/// Resolve a cursor position to the places it appears on the page.
///
/// Returns nothing for a cursor in a comment or on a keyword — upstream only
/// maps `Text` and `MathText` leaves — which is the correct behaviour: the
/// preview stays where it is rather than jumping somewhere arbitrary.
pub fn from_cursor(
    document: &PagedDocument,
    source: &Source,
    cursor: usize,
) -> Vec<DocumentPoint> {
    typst_ide::jump_from_cursor(document, source, cursor)
        .into_iter()
        .map(to_document_point)
        .collect()
}

fn to_document_point(position: PagedPosition) -> DocumentPoint {
    DocumentPoint {
        page: position.page.get() - 1,
        x_pt: position.point.x.to_pt(),
        y_pt: position.point.y.to_pt(),
    }
}
