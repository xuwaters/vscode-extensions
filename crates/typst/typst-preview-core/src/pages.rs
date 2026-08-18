//! Page measurement and rendering.
//!
//! The numbers that force this shape, from the feasibility spike: a text-heavy
//! A4 page is **386 KB of SVG** and a 30-page document is **11.4 MB**, while
//! rendering *one* page costs ~5 ms. So the preview never materializes a whole
//! document — it measures every page cheaply (hashing frames, rendering
//! nothing) and renders only the pages the viewport can see whose hash the
//! client does not already hold.

use serde::{Deserialize, Serialize};
use typst::utils::hash128;
use typst_layout::PagedDocument;
use typst_svg::SvgOptions;

use crate::round::round_coordinates;

/// Decimal places kept in rendered SVG coordinates.
///
/// 1/100 of a typographic point — about 1/7200 inch — which is orders of
/// magnitude below anything a screen or an imagesetter can resolve, and cuts
/// the bytes that dominate a page. See [`crate::round`].
pub const COORDINATE_DECIMALS: usize = 2;

/// Above this many bytes of SVG, `RenderMode::Auto` switches a page to PNG.
pub const AUTO_PNG_THRESHOLD_BYTES: usize = 1_000_000;

/// How pages are rendered for the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderMode {
    /// Vector pages. Zoomable, searchable, and the default.
    #[default]
    Svg,
    /// Raster pages. Much smaller for graphics-heavy documents, at the cost of
    /// zoom fidelity and find-in-preview.
    Png,
    /// SVG per page until a page exceeds
    /// [`AUTO_PNG_THRESHOLD_BYTES`], then PNG for that page only.
    Auto,
}

/// What a rendered page is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageFormat {
    /// `content` is SVG markup.
    Svg,
    /// `content` is base64-encoded PNG.
    Png,
}

/// A page's identity and dimensions, without any rendering having happened.
///
/// Drives the webview's placeholder layout, so the scrollbar is correct for the
/// whole document from the first frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageMetrics {
    /// Zero-based page index.
    pub index: usize,
    /// Width in typographic points.
    pub width_pt: f64,
    /// Height in typographic points.
    pub height_pt: f64,
    /// Content hash. Page identity is this, not the index — insert a paragraph
    /// on page 2 of a 60-page document and pages 3..60 shift by one but keep
    /// their hashes, so the webview re-anchors instead of re-fetching 58 pages.
    pub hash: u64,
}

/// What to do with one page, given what the client already holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum PagePatch {
    /// The client's copy is current; nothing crosses the wire.
    Unchanged { index: usize },
    /// New or changed content for this page.
    #[serde(rename_all = "camelCase")]
    Replace {
        /// Zero-based page index.
        index: usize,
        /// The new content hash, as lowercase hex.
        hash: String,
        /// Whether `content` is SVG markup or a base64 PNG.
        format: PageFormat,
        /// The page.
        content: String,
    },
    /// The page no longer exists — the document got shorter.
    Removed { index: usize },
}

/// Hash a page's content.
///
/// Hashes the laid-out `Page`, not the rendered SVG string: hashing is far
/// cheaper than rendering, which is what lets `measure` run on every compile
/// while `render` runs only for visible pages.
pub fn page_hash(page: &typst_layout::Page) -> u64 {
    hash128(page) as u64
}

/// Format a hash the way the wire protocol carries it.
pub fn hash_hex(hash: u64) -> String {
    format!("{hash:016x}")
}

/// Measure every page: dimensions and content hash, no rendering.
pub fn measure(document: &PagedDocument) -> Vec<PageMetrics> {
    document
        .pages()
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let size = page.frame.size();
            PageMetrics {
                index,
                width_pt: size.x.to_pt(),
                height_pt: size.y.to_pt(),
                hash: page_hash(page),
            }
        })
        .collect()
}

/// Render one page to SVG, with coordinates rounded.
pub fn render_page(document: &PagedDocument, index: usize) -> Option<String> {
    let page = document.pages().get(index)?;
    let svg = typst_svg::svg(page, &SvgOptions::default());
    Some(round_coordinates(&svg, COORDINATE_DECIMALS))
}

/// Render one page in whichever format the mode calls for.
///
/// `Auto` renders SVG first and only falls back to PNG if that page turns out
/// to be huge — which is the right way round, because the threshold is about
/// *this* page's content, not the document's average.
pub fn render_page_in(
    document: &PagedDocument,
    index: usize,
    mode: RenderMode,
    ppi: f64,
) -> Option<(PageFormat, String)> {
    match mode {
        RenderMode::Svg => Some((PageFormat::Svg, render_page(document, index)?)),
        RenderMode::Png => Some((PageFormat::Png, render_png(document, index, ppi)?)),
        RenderMode::Auto => {
            let svg = render_page(document, index)?;
            if svg.len() <= AUTO_PNG_THRESHOLD_BYTES {
                return Some((PageFormat::Svg, svg));
            }
            match render_png(document, index, ppi) {
                Some(png) => Some((PageFormat::Png, png)),
                // A raster failure is not a reason to show nothing.
                None => Some((PageFormat::Svg, svg)),
            }
        }
    }
}

fn render_png(document: &PagedDocument, index: usize, ppi: f64) -> Option<String> {
    crate::export::export_png(document, index, ppi)
        .ok()
        .map(|bytes| crate::base64::encode(&bytes))
}
