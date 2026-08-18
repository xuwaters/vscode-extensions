//! Turns a compiled `PagedDocument` into something a webview can display,
//! incrementally.
//!
//! Three jobs, each shaped by a measured number from the feasibility spike:
//!
//! * **Measure** every page without rendering it (~2 ms for 30 pages), so
//!   placeholders and the scrollbar are correct from the first frame.
//! * **Render** only the pages in view whose content hash the client does not
//!   hold (~5 ms per page), so a keystroke ships one page rather than thirty.
//! * **Map positions** both ways between the source and the page, using
//!   upstream's own `jump_from_cursor` / `jump_from_click`.

pub mod base64;
pub mod export;
pub mod jump;
pub mod pages;
pub mod patch;
pub mod round;

use rustc_hash::FxHashMap;
use typst_layout::PagedDocument;

pub use export::{export_png, export_png_all, export_svg};
pub use jump::{DocumentPoint, JumpTarget};
pub use pages::{
    PageFormat, PageMetrics, PagePatch, RenderMode, hash_hex, measure, page_hash,
    render_page, render_page_in,
};
pub use patch::{RenderOptions, apply, diff, diff_with};
pub use round::round_coordinates;

/// Per-document preview state: the last measurement, so the server can answer
/// "how many pages, how big" without re-walking the document.
#[derive(Debug, Default)]
pub struct PreviewSession {
    metrics: Vec<PageMetrics>,
}

impl PreviewSession {
    /// A session with nothing measured yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-measure after a compile. Cheap: hashes frames, renders nothing.
    pub fn measure(&mut self, document: &PagedDocument) -> &[PageMetrics] {
        self.metrics = pages::measure(document);
        &self.metrics
    }

    /// The last measurement.
    pub fn metrics(&self) -> &[PageMetrics] {
        &self.metrics
    }

    /// How many pages the last measurement saw.
    pub fn page_count(&self) -> usize {
        self.metrics.len()
    }

    /// Render the requested pages, skipping those the client already holds.
    pub fn render(
        &self,
        document: &PagedDocument,
        want: &[usize],
        known: &FxHashMap<usize, u64>,
        options: RenderOptions,
    ) -> Vec<PagePatch> {
        patch::diff_with(document, want, known, options)
    }
}
