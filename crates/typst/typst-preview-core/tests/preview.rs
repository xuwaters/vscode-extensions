//! P3-13: page-hash diff correctness and a cursor↔click round trip.
//!
//! The diff test is the important one. "Applying the patches to the previous
//! page list reproduces the new page list" is the property the whole
//! incremental preview rests on — if it can be violated, the webview silently
//! shows stale pages, which is much worse than showing none.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;
use typst::foundations::Bytes;
use typst::syntax::{FileId, RootedPath, VirtualPath, VirtualRoot};
use typst::text::FontInfo;
use typst_preview_core::pages::PagePatch;
use typst_preview_core::{DocumentPoint, JumpTarget, PreviewSession, TiffColor, jump, patch};
use typst_session::fs::{FsFiles, SystemClock};
use typst_session::ports::{FaceDescriptor, FontProvider, NoPackages};
use typst_session::{Session, SessionWorld};

struct BundledFonts {
    faces: Vec<FaceDescriptor>,
    data: Vec<Bytes>,
}

impl BundledFonts {
    fn new() -> Self {
        let mut faces = Vec::new();
        let mut data = Vec::new();
        for file in typst_assets::fonts() {
            let bytes = Bytes::new(file);
            for (index, info) in FontInfo::iter(file).enumerate() {
                faces.push(FaceDescriptor { info, index: index as u32 });
                data.push(bytes.clone());
            }
        }
        Self { faces, data }
    }
}

impl FontProvider for BundledFonts {
    fn faces(&self) -> &[FaceDescriptor] {
        &self.faces
    }

    fn data(&self, face: usize) -> Option<Bytes> {
        self.data.get(face).cloned()
    }
}

type TestSession = Session<FsFiles, BundledFonts, NoPackages, SystemClock>;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn file_id(path: &str) -> FileId {
    FileId::new(RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).unwrap(),
    ))
}

fn session(main: &str) -> TestSession {
    let world = SessionWorld::new(
        FsFiles::new(fixtures()),
        BundledFonts::new(),
        NoPackages,
        SystemClock,
        file_id(main),
    );
    Session::new(world, 1)
}

/// A document long enough to span several pages.
fn multi_page_source(sections: usize) -> String {
    let mut text = String::new();
    for section in 0..sections {
        text.push_str(&format!("= Section {section}\n\n#lorem(160)\n\n#pagebreak()\n\n"));
    }
    text
}

#[test]
fn measure_reports_a_page_per_page_with_real_dimensions() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, multi_page_source(4));

    let outcome = session.compile(1);
    assert!(outcome.ok, "fixture must compile");
    let document = outcome.document.unwrap();

    let mut preview = PreviewSession::new();
    let metrics = preview.measure(&document).to_vec();

    assert_eq!(metrics.len(), document.pages().len());
    assert!(metrics.len() >= 4, "expected at least one page per section");
    for (index, page) in metrics.iter().enumerate() {
        assert_eq!(page.index, index);
        // A4 by default: 595 × 842 pt.
        assert!((page.width_pt - 595.0).abs() < 1.0, "width {}", page.width_pt);
        assert!((page.height_pt - 842.0).abs() < 1.0, "height {}", page.height_pt);
    }
}

#[test]
fn an_unchanged_page_costs_no_bytes() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, multi_page_source(3));

    let document = session.compile(1).document.unwrap();
    let want: Vec<usize> = (0..document.pages().len()).collect();

    let first = patch::diff(&document, &want, &FxHashMap::default());
    assert!(first.iter().all(|p| matches!(p, PagePatch::Replace { .. })));

    let mut known = FxHashMap::default();
    patch::apply(&mut known, &first);

    let second = patch::diff(&document, &want, &known);
    assert!(
        second.iter().all(|p| matches!(p, PagePatch::Unchanged { .. })),
        "re-requesting an unmodified document must ship nothing"
    );
}

/// The property the incremental preview rests on: applying the patch set to the
/// previous page list reproduces the new one, exactly.
#[test]
fn applying_patches_reproduces_the_new_page_list() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    let mut text = multi_page_source(5);
    session.open(id, text.clone());

    let before = session.compile(1).document.unwrap();
    let want: Vec<usize> = (0..before.pages().len()).collect();

    let mut known = FxHashMap::default();
    let initial = patch::diff(&before, &want, &known);
    patch::apply(&mut known, &initial);

    // Edit the second section — the kind of change that shifts nothing.
    let marker = "= Section 1";
    let at = text.find(marker).unwrap() + marker.len();
    text.insert_str(at, " rewritten");
    session.replace(id, &text);

    let after = session.compile(2).document.unwrap();
    let want: Vec<usize> = (0..after.pages().len()).collect();
    let patches = patch::diff(&after, &want, &known);
    patch::apply(&mut known, &patches);

    let expected: FxHashMap<usize, u64> = after
        .pages()
        .iter()
        .enumerate()
        .map(|(index, page)| (index, typst_preview_core::page_hash(page)))
        .collect();

    assert_eq!(known, expected, "the client's page map must match the document");
}

#[test]
fn a_shorter_document_removes_the_pages_that_went_away() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, multi_page_source(5));

    let long = session.compile(1).document.unwrap();
    let long_pages = long.pages().len();
    let want: Vec<usize> = (0..long_pages).collect();

    let mut known = FxHashMap::default();
    let initial = patch::diff(&long, &want, &known);
    patch::apply(&mut known, &initial);

    session.replace(id, &multi_page_source(2));
    let short = session.compile(2).document.unwrap();
    assert!(short.pages().len() < long_pages);

    // The webview only asks about the pages it can see; the pages past the end
    // must still be removed or they linger in the DOM.
    let patches = patch::diff(&short, &[0, 1], &known);
    patch::apply(&mut known, &patches);

    assert_eq!(
        known.keys().copied().max().unwrap(),
        short.pages().len() - 1,
        "orphan pages survived a shortening edit"
    );
}

#[test]
fn only_the_edited_page_is_re_rendered() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    let mut text = multi_page_source(6);
    session.open(id, text.clone());

    let before = session.compile(1).document.unwrap();
    let want: Vec<usize> = (0..before.pages().len()).collect();
    let mut known = FxHashMap::default();
    let initial = patch::diff(&before, &want, &known);
    patch::apply(&mut known, &initial);

    let marker = "= Section 4";
    let at = text.find(marker).unwrap() + marker.len();
    text.insert(at, 'x');
    session.replace(id, &text);

    let after = session.compile(2).document.unwrap();
    let want: Vec<usize> = (0..after.pages().len()).collect();
    let patches = patch::diff(&after, &want, &known);

    let replaced = patches
        .iter()
        .filter(|p| matches!(p, PagePatch::Replace { .. }))
        .count();
    assert_eq!(
        replaced, 1,
        "editing one page should re-render one page, not {}",
        after.pages().len()
    );
}

/// Round trip: put the cursor in body text, ask where it lands on the page,
/// then click that point and check we come back into the same paragraph.
///
/// The round trip is deliberately not asserted to be exact.
/// `jump_from_cursor` resolves to the *span* the cursor sits in, and a typst
/// paragraph is one `Text` node — so a cursor anywhere in it maps to the
/// paragraph's first glyph. Clicking is glyph-precise in the other direction,
/// which the second half of this test pins down.
#[test]
fn cursor_to_page_to_source_round_trips() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    let text = "= Heading\n\nThe quick brown fox jumps over the lazy dog.\n";
    session.open(id, text.into());

    let document = session.compile(1).document.unwrap();
    let source = session.world().vfs().opened(id).unwrap().clone();

    let paragraph = text.find("The quick").unwrap();
    let cursor = text.find("brown").unwrap() + 2;
    let positions = jump::from_cursor(&document, &source, cursor);
    assert!(!positions.is_empty(), "a word in body text must map to the page");

    let landing = positions[0];
    let target = jump::from_click(session.world(), &document, landing)
        .expect("clicking where the cursor points must resolve");

    let JumpTarget::Source { file, offset } = target else {
        panic!("expected a source jump, got {target:?}");
    };
    assert_eq!(file, id);
    assert!(
        (paragraph..paragraph + "The quick brown fox jumps over the lazy dog.".len())
            .contains(&offset),
        "landed at {offset}, outside the paragraph that was clicked"
    );

    // Clicking further along the same line must land further along the text —
    // this is what makes click-to-source useful rather than merely correct.
    let later = jump::from_click(
        session.world(),
        &document,
        DocumentPoint { x_pt: landing.x_pt + 60.0, ..landing },
    )
    .expect("a click further right must still hit the line");

    let JumpTarget::Source { offset: later_offset, .. } = later else {
        panic!("expected a source jump, got {later:?}");
    };
    assert!(
        later_offset > offset,
        "clicking right of {offset} landed at {later_offset}"
    );
}

/// Where the cursor at `cursor` lands, and where the text `at` lands.
fn landings(text: &str, cursor: usize, at: &str) -> (Vec<DocumentPoint>, Vec<DocumentPoint>) {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, text.into());

    let document = session.compile(1).document.unwrap();
    let source = session.world().vfs().opened(id).unwrap().clone();

    let expected = jump::from_cursor(&document, &source, text.find(at).unwrap() + 1);
    assert!(!expected.is_empty(), "{at:?} must map to the page");
    (jump::from_cursor(&document, &source, cursor), expected)
}

/// A blank line maps nowhere upstream; it should take the preview to the text
/// that follows it, not leave the preview wherever it was.
#[test]
fn a_cursor_on_a_blank_line_maps_to_the_next_text() {
    let text = "First paragraph.\n\n\n\nSecond paragraph.\n";
    let blank = text.find("\n\n").unwrap() + 2;
    let (landed, expected) = landings(text, blank, "Second");
    assert_eq!(landed, expected);
}

#[test]
fn a_cursor_in_a_comment_maps_to_the_next_text() {
    let text = "// just a comment\n\n= Heading\n";
    let (landed, expected) = landings(text, 5, "Heading");
    assert_eq!(landed, expected);
}

/// Past the last text there is nothing after, so the text before wins.
#[test]
fn a_cursor_after_the_last_text_maps_to_the_text_before() {
    let text = "Only paragraph.\n\n\n";
    let (landed, expected) = landings(text, text.len() - 1, "Only");
    assert_eq!(landed, expected);
}

/// Text in a function body that is never called is not on the page; the
/// fallback must skip past it rather than give up.
#[test]
fn the_fallback_skips_text_that_is_not_on_the_page() {
    let text = "#let unused() = [hidden words]\n\nVisible paragraph.\n";
    let (landed, expected) = landings(text, 2, "Visible");
    assert_eq!(landed, expected);
}

#[test]
fn a_document_with_no_text_maps_nowhere() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    let text = "// just a comment\n\n#let x = 1\n";
    session.open(id, text.into());

    let document = session.compile(1).document.unwrap();
    let source = session.world().vfs().opened(id).unwrap().clone();

    assert!(jump::from_cursor(&document, &source, 5).is_empty());
}

#[test]
fn a_click_on_blank_margin_resolves_to_nothing() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, "= Heading\n\nBody.\n".into());

    let document = session.compile(1).document.unwrap();
    let target = jump::from_click(
        session.world(),
        &document,
        DocumentPoint { page: 0, x_pt: 5.0, y_pt: 800.0 },
    );

    assert!(target.is_none());
}

#[test]
fn export_produces_svg_and_png() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, multi_page_source(2));

    let document = session.compile(1).document.unwrap();

    let svg = typst_preview_core::export_svg(&document, 12.0);
    assert!(svg.starts_with("<svg"), "expected an SVG document");

    let png = typst_preview_core::export_png(&document, 0, 144.0).unwrap();
    assert_eq!(&png[1..4], b"PNG", "expected PNG magic bytes");

    assert!(typst_preview_core::export_png(&document, 999, 144.0).is_err());
}

#[test]
fn png_export_records_its_resolution() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, multi_page_source(1));

    let document = session.compile(1).document.unwrap();
    let png = typst_preview_core::export_png(&document, 0, 300.0).unwrap();

    // Straight after the signature and IHDR, as the spec requires.
    assert_eq!(&png[37..41], b"pHYs");
    let per_metre = u32::from_be_bytes(png[41..45].try_into().unwrap());
    assert_eq!(per_metre, 11811, "300 ppi is 11811 pixels per metre");
    assert_eq!(png[49], 1, "the unit is the metre");

    // And it is still a PNG the encoder's own decoder accepts.
    let pixmap = tiny_skia::Pixmap::decode_png(&png).unwrap();
    let a4_width = (595.28 * 300.0 / 72.0_f64).ceil() as u32;
    assert!(pixmap.width().abs_diff(a4_width) <= 1, "got {}", pixmap.width());
}

#[test]
fn tiff_export_writes_every_page_into_one_file() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, multi_page_source(3));

    let document = session.compile(1).document.unwrap();
    let tiff = typst_preview_core::export_tiff(&document, 300.0, TiffColor::Rgba).unwrap();
    assert_eq!(&tiff[..4], b"II*\0", "expected little-endian TIFF magic");

    let pages = read_tiff(&tiff);
    assert!(pages.len() >= 3, "got {} pages", pages.len());
    assert_eq!(pages.len(), document.pages().len());

    for (index, page) in pages.iter().enumerate() {
        assert_eq!(page.page_number, (index, pages.len()));
        assert_eq!(page.ppi, 300);
        assert_eq!((page.bits.as_slice(), page.photometric), (&[8, 8, 8, 8][..], 2));
        assert_eq!(page.samples.len(), (page.width * page.height * 4) as usize);
    }

    // A 300 ppi A4 page is more than one band, so it is more than one strip —
    // and the strips put back together are the page rendered whole.
    let first = &pages[0];
    assert!(first.strips > 1, "expected a banded page, got {} strip", first.strips);

    let mut samples = first.samples.clone();
    let row = first.width as usize * 4;
    for line in samples.chunks_exact_mut(row) {
        for index in 4..row {
            line[index] = line[index].wrapping_add(line[index - 4]);
        }
    }

    let png = typst_preview_core::export_png(&document, 0, 300.0).unwrap();
    let whole = tiny_skia::Pixmap::decode_png(&png).unwrap();
    assert_eq!((first.width, first.height), (whole.width(), whole.height()));

    let differing = whole
        .pixels()
        .iter()
        .zip(samples.as_chunks::<4>().0)
        .filter(|(pixel, banded)| {
            let pixel = pixel.demultiply();
            let whole = [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()];
            // Anti-aliasing may round a shade differently at a band's edge.
            whole.iter().zip(banded.iter()).any(|(a, b)| a.abs_diff(*b) > 2)
        })
        .count();
    assert_eq!(differing, 0, "banded pixels differ from the whole page");
}

#[test]
fn bilevel_tiff_export_is_one_bit_black_on_white() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, "#set page(fill: none)\n= Title\n\n#lorem(40)\n".into());

    let document = session.compile(1).document.unwrap();
    let tiff = typst_preview_core::export_tiff(&document, 1200.0, TiffColor::Bilevel).unwrap();

    let pages = read_tiff(&tiff);
    assert_eq!(pages.len(), 1);
    let page = &pages[0];

    assert_eq!(page.ppi, 1200);
    assert_eq!((page.bits.as_slice(), page.photometric), (&[1][..], 0), "1 bit, WhiteIsZero");
    let a4_width = (595.28 * 1200.0 / 72.0_f64).round() as u32;
    assert_eq!(page.width, a4_width);
    assert!(page.strips > 1, "a 1200 ppi page is rendered in bands");

    let stride = page.width.div_ceil(8) as usize;
    assert_eq!(page.samples.len(), stride * page.height as usize);

    // A transparent page still comes out as white paper, with black text on
    // it — and the text is solid, not dithered into grey.
    assert!(page.samples[..stride * 100].iter().all(|&byte| byte == 0), "top margin is white");
    let black = page.samples.iter().map(|byte| byte.count_ones() as usize).sum::<usize>();
    let total = (page.width * page.height) as usize;
    assert!(black > 0 && black < total / 10, "{black} of {total} pixels black");

    // Over a hundred times smaller than the same page as RGBA, uncompressed.
    assert!(tiff.len() < total * 4 / 100, "{} bytes", tiff.len());
}

/// One TIFF page, read back.
struct TiffPage {
    width: u32,
    height: u32,
    bits: Vec<u32>,
    photometric: u32,
    ppi: u32,
    page_number: (usize, usize),
    strips: usize,
    /// Every strip inflated, in order.
    samples: Vec<u8>,
}

/// Just enough of a little-endian TIFF reader to check what export wrote.
fn read_tiff(tiff: &[u8]) -> Vec<TiffPage> {
    let u16_at = |at: usize| u16::from_le_bytes(tiff[at..at + 2].try_into().unwrap());
    let u32_at = |at: usize| u32::from_le_bytes(tiff[at..at + 4].try_into().unwrap());

    // Every value of a SHORT or LONG field, wherever it is stored.
    let values = |directory: usize, tag: u16| -> Vec<u32> {
        let entry = (0..u16_at(directory) as usize)
            .map(|index| directory + 2 + index * 12)
            .find(|&entry| u16_at(entry) == tag)
            .unwrap_or_else(|| panic!("tag {tag} missing"));
        let (kind, count) = (u16_at(entry + 2), u32_at(entry + 4) as usize);
        let size = if kind == 3 { 2 } else { 4 };
        let start = if size * count <= 4 { entry + 8 } else { u32_at(entry + 8) as usize };
        (0..count)
            .map(|index| match size {
                2 => u32::from(u16_at(start + index * 2)),
                _ => u32_at(start + index * 4),
            })
            .collect()
    };

    let mut pages = Vec::new();
    let mut next = u32_at(4) as usize;
    while next != 0 {
        assert_eq!(next % 2, 0, "directories start on a word boundary");
        let directory = next;

        let resolution = values(directory, 282)[0] as usize;
        let page_number = values(directory, 297);
        let offsets = values(directory, 273);
        let counts = values(directory, 279);
        assert_eq!(offsets.len(), counts.len());

        let mut samples = Vec::new();
        for (&offset, &count) in offsets.iter().zip(&counts) {
            let strip = &tiff[offset as usize..(offset + count) as usize];
            samples.extend(miniz_oxide::inflate::decompress_to_vec_zlib(strip).unwrap());
        }

        pages.push(TiffPage {
            width: values(directory, 256)[0],
            height: values(directory, 257)[0],
            bits: values(directory, 258),
            photometric: values(directory, 262)[0],
            ppi: u32_at(resolution) / u32_at(resolution + 4),
            page_number: (page_number[0] as usize, page_number[1] as usize),
            strips: offsets.len(),
            samples,
        });
        next = u32_at(directory + 2 + u16_at(directory) as usize * 12) as usize;
    }
    pages
}

#[test]
fn coordinate_rounding_shrinks_a_real_page_without_changing_its_shape() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, "= Section\n\n#lorem(200)\n\n$ sum_(i=1)^n i $\n".into());

    let document = session.compile(1).document.unwrap();
    let page = &document.pages()[0];

    let full = typst_svg::svg(page, &typst_svg::SvgOptions::default());
    let rounded = typst_preview_core::round_coordinates(&full, 2);

    assert!(
        rounded.len() < full.len(),
        "rounding should shrink a real page: {} → {}",
        full.len(),
        rounded.len()
    );

    // The element counts must be identical — rounding shortens numbers, it does
    // not drop anything.
    for tag in ["<use", "<path", "<symbol", "<g "] {
        assert_eq!(
            full.matches(tag).count(),
            rounded.matches(tag).count(),
            "rounding changed the number of {tag} elements"
        );
    }

    let saved = 100.0 * (1.0 - rounded.len() as f64 / full.len() as f64);
    println!("coordinate rounding saved {saved:.1}% ({} → {} bytes)", full.len(), rounded.len());
}

#[test]
fn png_mode_produces_raster_pages() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, multi_page_source(2));

    let document = session.compile(1).document.unwrap();
    let patches = patch::diff_with(
        &document,
        &[0],
        &FxHashMap::default(),
        typst_preview_core::RenderOptions {
            mode: typst_preview_core::RenderMode::Png,
            ppi: 96.0,
        },
    );

    let PagePatch::Replace { format, content, .. } = &patches[0] else {
        panic!("expected a replacement, got {:?}", patches[0]);
    };
    assert_eq!(*format, typst_preview_core::PageFormat::Png);
    // Base64 of the PNG signature.
    assert!(content.starts_with("iVBORw0KGgo"), "not a PNG: {}", &content[..12]);
}

#[test]
fn auto_mode_keeps_an_ordinary_page_as_svg() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, multi_page_source(2));

    let document = session.compile(1).document.unwrap();
    let patches = patch::diff_with(
        &document,
        &[0],
        &FxHashMap::default(),
        typst_preview_core::RenderOptions {
            mode: typst_preview_core::RenderMode::Auto,
            ppi: 144.0,
        },
    );

    let PagePatch::Replace { format, content, .. } = &patches[0] else {
        panic!("expected a replacement");
    };
    assert_eq!(
        *format,
        typst_preview_core::PageFormat::Svg,
        "a text page is nowhere near the 1 MB threshold"
    );
    assert!(content.starts_with("<svg"));
}

#[test]
fn the_render_mode_does_not_change_a_page_hash() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    session.open(id, multi_page_source(2));

    let document = session.compile(1).document.unwrap();
    let known = FxHashMap::default();

    let svg = patch::diff_with(&document, &[0], &known, typst_preview_core::RenderOptions {
        mode: typst_preview_core::RenderMode::Svg,
        ppi: 144.0,
    });
    let png = patch::diff_with(&document, &[0], &known, typst_preview_core::RenderOptions {
        mode: typst_preview_core::RenderMode::Png,
        ppi: 144.0,
    });

    let hash_of = |patch: &PagePatch| match patch {
        PagePatch::Replace { hash, .. } => hash.clone(),
        other => panic!("expected a replacement, got {other:?}"),
    };

    // The hash identifies the *page*, not the rendering — otherwise switching
    // modes would invalidate every page the client holds.
    assert_eq!(hash_of(&svg[0]), hash_of(&png[0]));
}
