//! SVG, PNG, and TIFF export.
//!
//! Export wants the opposite trade-off from the preview: completeness over
//! latency. `svg_merged` writes the whole document with shared glyph
//! definitions — right for a file on disk, wrong for a live preview, since it
//! cannot be diffed per page.
//!
//! PDF export lives in `typst-session`, which is the crate that already carries
//! the `typst-pdf` dependency.

use ecow::EcoString;
use typst::foundations::Smart;
use typst::layout::{Abs, Frame, FrameItem, Point, Sides, Size};
use typst::syntax::Span;
use typst::visualize::{Geometry, Paint};
use typst_layout::{Page, PagedDocument};
use typst_render::RenderOptions;
use typst_svg::SvgOptions;

/// Whole-document SVG, pages stacked with a gap between them.
pub fn export_svg(document: &PagedDocument, gap_pt: f64) -> String {
    typst_svg::svg_merged(document, &SvgOptions::default(), Abs::pt(gap_pt))
}

/// One page as PNG bytes at the given resolution.
///
/// The resolution is recorded in the file as well as used to render it, so a
/// 300 ppi page prints at its real size rather than at whatever the viewer
/// assumes — usually 72 or 96 ppi, which would make it three or four times too
/// large.
pub fn export_png(
    document: &PagedDocument,
    page: usize,
    ppi: f64,
) -> Result<Vec<u8>, EcoString> {
    let pixmap = render(page_at(document, page)?, ppi);
    let png = pixmap.encode_png().map_err(|err| EcoString::from(err.to_string()))?;
    Ok(with_physical_size(png, ppi))
}

/// Every page as PNG bytes, in order.
pub fn export_png_all(
    document: &PagedDocument,
    ppi: f64,
) -> Result<Vec<Vec<u8>>, EcoString> {
    (0..document.pages().len()).map(|page| export_png(document, page, ppi)).collect()
}

/// How a TIFF stores its pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TiffColor {
    /// 8-bit RGBA.
    Rgba,
    /// One bit per pixel, black or white, with grey tones dithered. A twentieth
    /// the size of RGBA before compression, and the format fax gateways and
    /// high-resolution print workflows expect.
    Bilevel,
}

/// How much of a page to rasterise at once, in RGBA bytes.
///
/// typst only renders whole pages, and a page at 1200 ppi is 557 MB of RGBA —
/// memory the WASM heap would keep for the rest of the session, since linear
/// memory never shrinks. Rendering the page as a stack of bands, each one TIFF
/// strip, caps that at this much whatever the resolution, for the price of
/// walking the page's frame once per band.
const BAND_BYTES: usize = 16 << 20;

/// Every page as one multi-page TIFF.
///
/// TIFF holds pages natively, so unlike PNG this is one file — which is what
/// anything that asks for a TIFF (a print shop, a fax gateway, an archive)
/// expects. Every page is Deflate-compressed and carries its resolution.
pub fn export_tiff(
    document: &PagedDocument,
    ppi: f64,
    color: TiffColor,
) -> Result<Vec<u8>, EcoString> {
    let pages = document.pages();
    if pages.is_empty() {
        return Err("the document has no pages".into());
    }

    let mut tiff = TiffWriter::new(ppi, color, pages.len());
    for page in pages {
        // The same arithmetic `typst_render::render` sizes its canvas with, f32
        // included, so a banded page has exactly the rows a whole one would.
        let pixel_per_pt = (ppi / 72.0) as f32;
        let size = page.frame.size();
        let width = (pixel_per_pt * size.x.to_pt() as f32).round().max(1.0) as u32;
        let height = (pixel_per_pt * size.y.to_pt() as f32).round().max(1.0) as u32;
        let rows_per_strip = (BAND_BYTES / (width as usize * 4)).clamp(1, height as usize) as u32;

        let mut strips = Vec::new();
        for top in (0..height).step_by(rows_per_strip as usize) {
            let rows = rows_per_strip.min(height - top);
            let band = render_band(page, ppi, top, rows);
            let samples = match color {
                TiffColor::Rgba => rgba_samples(&band),
                TiffColor::Bilevel => bilevel_samples(&band, top),
            };
            strips.push(miniz_oxide::deflate::compress_to_vec_zlib(&samples, 6));
        }

        tiff.push(width, height, rows_per_strip, &strips);
    }
    Ok(tiff.finish())
}

/// Rows `top..top + rows` of a page, rendered on their own.
///
/// The band is a page of its own, the band's height, holding the real page's
/// frame shifted up by `top`; anything outside it is simply not drawn. The
/// page fill moves into the frame too, since a gradient fill is laid out
/// against the page it fills — the band's would be the wrong size. A solid
/// fill stays a fill, so the canvas is flooded edge to edge as it would be.
fn render_band(page: &Page, ppi: f64, top: u32, rows: u32) -> tiny_skia::Pixmap {
    let pixel_per_pt = ppi / 72.0;
    let size = page.frame.size();
    let offset = Point::new(Abs::zero(), -Abs::pt(f64::from(top) / pixel_per_pt));

    let mut frame = Frame::hard(Size::new(size.x, Abs::pt(f64::from(rows) / pixel_per_pt)));
    let fill = match page.fill_or_white() {
        Some(Paint::Solid(color)) => Some(Paint::Solid(color)),
        Some(paint) => {
            let shape = Geometry::Rect(size).filled(paint);
            frame.push(offset, FrameItem::Shape(shape, Span::detached()));
            None
        }
        None => None,
    };
    frame.push_frame(offset, page.frame.clone());

    let band = Page {
        frame,
        bleed: Sides::default(),
        fill: Smart::Custom(fill),
        numbering: page.numbering.clone(),
        supplement: page.supplement.clone(),
        number: page.number,
    };
    render(&band, ppi)
}

/// Unassociated RGBA with horizontal differencing (TIFF predictor 2).
///
/// Unassociated rather than tiny-skia's premultiplied pixels: every reader
/// handles it, and associated alpha is one some still get wrong. Differencing
/// makes each sample its difference from the same channel one pixel to the
/// left; a page is mostly flat runs of paper, which this turns into zeros.
fn rgba_samples(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    let mut samples = Vec::with_capacity(pixmap.data().len());
    for pixel in pixmap.pixels() {
        let color = pixel.demultiply();
        samples.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }

    let row = pixmap.width() as usize * 4;
    for line in samples.chunks_exact_mut(row) {
        for index in (4..row).rev() {
            line[index] = line[index].wrapping_sub(line[index - 4]);
        }
    }
    samples
}

/// One bit per pixel, set for black (TIFF's WhiteIsZero), rows byte-aligned.
///
/// Each pixel is laid on white paper, reduced to its luminance, and compared
/// against an 8×8 ordered-dither threshold rather than a flat 50%: text comes
/// out solid either way, but a grey table rule or a light shaded box would
/// otherwise turn black or vanish. Ordered dithering also needs no state
/// carried between bands — only the absolute row, `top`, so the pattern does
/// not restart at each band's edge.
fn bilevel_samples(pixmap: &tiny_skia::Pixmap, top: u32) -> Vec<u8> {
    /// The 8×8 Bayer matrix.
    const BAYER: [[u8; 8]; 8] = [
        [0, 32, 8, 40, 2, 34, 10, 42],
        [48, 16, 56, 24, 50, 18, 58, 26],
        [12, 44, 4, 36, 14, 46, 6, 38],
        [60, 28, 52, 20, 62, 30, 54, 22],
        [3, 35, 11, 43, 1, 33, 9, 41],
        [51, 19, 59, 27, 49, 17, 57, 25],
        [15, 47, 7, 39, 13, 45, 5, 37],
        [63, 31, 55, 23, 61, 29, 53, 21],
    ];

    let width = pixmap.width() as usize;
    let stride = width.div_ceil(8);
    let mut samples = vec![0u8; stride * pixmap.height() as usize];

    for (y, (pixels, out)) in
        pixmap.pixels().chunks_exact(width).zip(samples.chunks_exact_mut(stride)).enumerate()
    {
        let thresholds = &BAYER[(top as usize + y) % 8];
        for (x, pixel) in pixels.iter().enumerate() {
            // Premultiplied, so laying it on white adds what alpha leaves.
            let paper = 255 - u32::from(pixel.alpha());
            let (r, g, b) = (
                u32::from(pixel.red()) + paper,
                u32::from(pixel.green()) + paper,
                u32::from(pixel.blue()) + paper,
            );
            let luminance = (77 * r + 150 * g + 29 * b) >> 8;
            let threshold = u32::from(thresholds[x % 8]) * 4 + 2;
            if luminance < threshold {
                out[x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    samples
}

fn page_at(document: &PagedDocument, page: usize) -> Result<&Page, EcoString> {
    document.pages().get(page).ok_or_else(|| EcoString::from("page out of range"))
}

fn render(page: &Page, ppi: f64) -> tiny_skia::Pixmap {
    let options = RenderOptions {
        pixel_per_pt: (ppi / 72.0).into(),
        ..RenderOptions::default()
    };
    typst_render::render(page, &options)
}

/// Insert a `pHYs` chunk recording `ppi` into an encoded PNG.
///
/// tiny-skia writes none. The chunk goes straight after `IHDR`, which is where
/// the spec requires it to be — before the image data — and `IHDR` is always
/// the first chunk and always the same length.
fn with_physical_size(png: Vec<u8>, ppi: f64) -> Vec<u8> {
    const AFTER_IHDR: usize = 8 + (4 + 4 + 13 + 4);

    let per_metre = (ppi / 0.0254).round() as u32;
    let mut body = Vec::with_capacity(4 + 9);
    body.extend_from_slice(b"pHYs");
    body.extend_from_slice(&per_metre.to_be_bytes());
    body.extend_from_slice(&per_metre.to_be_bytes());
    body.push(1); // The unit is the metre.

    let mut out = Vec::with_capacity(png.len() + 4 + body.len() + 4);
    out.extend_from_slice(&png[..AFTER_IHDR]);
    out.extend_from_slice(&9u32.to_be_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32fast::hash(&body).to_be_bytes());
    out.extend_from_slice(&png[AFTER_IHDR..]);
    out
}

/// A little-endian baseline TIFF, written one page at a time.
///
/// Each page is its strips followed by its directory, and each directory links
/// to the next; the header's link to the first is filled in once the first
/// directory's offset is known, after its strips are written.
struct TiffWriter {
    out: Vec<u8>,
    /// Where the link to the next directory goes: the header's, then each
    /// directory's last field in turn.
    link: usize,
    color: TiffColor,
    /// The resolution as a TIFF RATIONAL.
    resolution: (u32, u32),
    page: u16,
    pages: u16,
}

/// TIFF field types.
const SHORT: u16 = 3;
const LONG: u16 = 4;
const RATIONAL: u16 = 5;

impl TiffWriter {
    fn new(ppi: f64, color: TiffColor, pages: usize) -> Self {
        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        out.extend_from_slice(&42u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());

        Self {
            out,
            link: 4,
            color,
            resolution: ((ppi * 1000.0).round() as u32, 1000),
            page: 0,
            pages: pages.min(u16::MAX as usize) as u16,
        }
    }

    /// One page, as its Deflate-compressed strips in order.
    fn push(&mut self, width: u32, height: u32, rows_per_strip: u32, strips: &[Vec<u8>]) {
        let mut offsets = Vec::with_capacity(strips.len());
        for strip in strips {
            offsets.push(self.out.len() as u32);
            self.out.extend_from_slice(strip);
            self.align();
        }
        let counts: Vec<u32> = strips.iter().map(|strip| strip.len() as u32).collect();

        // The values too large to sit inside their entry. A single LONG fits,
        // so a one-strip page names its strip directly.
        let strip_offsets = self.long_array(&offsets);
        let strip_counts = self.long_array(&counts);
        let resolution = self.out.len() as u32;
        self.out.extend_from_slice(&self.resolution.0.to_le_bytes());
        self.out.extend_from_slice(&self.resolution.1.to_le_bytes());

        let mut entries: Vec<(u16, u16, u32, u32)> = vec![
            (254, LONG, 1, 2), // NewSubfileType: one page of several.
            (256, LONG, 1, width),
            (257, LONG, 1, height),
        ];
        match self.color {
            TiffColor::Rgba => {
                let bits = self.out.len() as u32;
                for _ in 0..4 {
                    self.out.extend_from_slice(&8u16.to_le_bytes());
                }
                entries.extend([
                    (258, SHORT, 4, bits), // BitsPerSample: 8, 8, 8, 8.
                    (259, SHORT, 1, 8),    // Compression: Deflate.
                    (262, SHORT, 1, 2),    // PhotometricInterpretation: RGB.
                ]);
            }
            TiffColor::Bilevel => entries.extend([
                (258, SHORT, 1, 1), // BitsPerSample.
                (259, SHORT, 1, 8), // Compression: Deflate.
                (262, SHORT, 1, 0), // PhotometricInterpretation: WhiteIsZero.
            ]),
        }
        let samples = match self.color {
            TiffColor::Rgba => 4,
            TiffColor::Bilevel => 1,
        };
        entries.extend([
            (273, LONG, strips.len() as u32, strip_offsets),
            (277, SHORT, 1, samples), // SamplesPerPixel.
            (278, LONG, 1, rows_per_strip),
            (279, LONG, strips.len() as u32, strip_counts),
            (282, RATIONAL, 1, resolution),
            (283, RATIONAL, 1, resolution),
            (284, SHORT, 1, 1), // PlanarConfiguration: interleaved.
            (296, SHORT, 1, 2), // ResolutionUnit: inch.
            // PageNumber: this page, of how many — two SHORTs in one entry.
            (297, SHORT, 2, u32::from(self.page) | u32::from(self.pages) << 16),
        ]);
        if self.color == TiffColor::Rgba {
            entries.extend([
                (317, SHORT, 1, 2), // Predictor: horizontal differencing.
                (338, SHORT, 1, 2), // ExtraSamples: unassociated alpha.
            ]);
        }

        let directory = self.out.len() as u32;
        self.out[self.link..self.link + 4].copy_from_slice(&directory.to_le_bytes());

        // Entries are in ascending tag order, as the spec requires.
        self.out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, kind, count, value) in entries {
            self.out.extend_from_slice(&tag.to_le_bytes());
            self.out.extend_from_slice(&kind.to_le_bytes());
            self.out.extend_from_slice(&count.to_le_bytes());
            // A single SHORT sits in the low half of the value field.
            self.out.extend_from_slice(&value.to_le_bytes());
        }

        self.link = self.out.len();
        self.out.extend_from_slice(&0u32.to_le_bytes());
        self.page = self.page.saturating_add(1);
    }

    /// The value field for `values`: the one LONG itself, or where they all
    /// went.
    fn long_array(&mut self, values: &[u32]) -> u32 {
        if let [value] = values {
            return *value;
        }
        let offset = self.out.len() as u32;
        for value in values {
            self.out.extend_from_slice(&value.to_le_bytes());
        }
        offset
    }

    fn finish(self) -> Vec<u8> {
        self.out
    }

    /// Directories and the values they point at start on a word boundary.
    fn align(&mut self) {
        if self.out.len() % 2 == 1 {
            self.out.push(0);
        }
    }
}
