//! SVG and PNG export.
//!
//! Export wants the opposite trade-off from the preview: completeness over
//! latency. `svg_merged` writes the whole document with shared glyph
//! definitions — right for a file on disk, wrong for a live preview, since it
//! cannot be diffed per page.
//!
//! PDF export lives in `typst-session`, which is the crate that already carries
//! the `typst-pdf` dependency.

use ecow::EcoString;
use typst::layout::Abs;
use typst_layout::PagedDocument;
use typst_render::RenderOptions;
use typst_svg::SvgOptions;

/// Whole-document SVG, pages stacked with a gap between them.
pub fn export_svg(document: &PagedDocument, gap_pt: f64) -> String {
    typst_svg::svg_merged(document, &SvgOptions::default(), Abs::pt(gap_pt))
}

/// One page as PNG bytes at the given resolution.
pub fn export_png(
    document: &PagedDocument,
    page: usize,
    ppi: f64,
) -> Result<Vec<u8>, EcoString> {
    let page = document
        .pages()
        .get(page)
        .ok_or_else(|| EcoString::from("page out of range"))?;

    let options = RenderOptions {
        pixel_per_pt: (ppi / 72.0).into(),
        ..RenderOptions::default()
    };

    typst_render::render(page, &options)
        .encode_png()
        .map_err(|err| EcoString::from(err.to_string()))
}

/// Every page as PNG bytes, in order.
pub fn export_png_all(
    document: &PagedDocument,
    ppi: f64,
) -> Result<Vec<Vec<u8>>, EcoString> {
    (0..document.pages().len()).map(|page| export_png(document, page, ppi)).collect()
}
