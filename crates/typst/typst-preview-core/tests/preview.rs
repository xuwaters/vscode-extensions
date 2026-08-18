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
use typst_preview_core::{DocumentPoint, JumpTarget, PreviewSession, jump, patch};
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

#[test]
fn a_cursor_in_a_comment_maps_nowhere() {
    let mut session = session("doc.typ");
    let id = file_id("doc.typ");
    let text = "// just a comment\n\n= Heading\n";
    session.open(id, text.into());

    let document = session.compile(1).document.unwrap();
    let source = session.world().vfs().opened(id).unwrap().clone();

    assert!(
        jump::from_cursor(&document, &source, 5).is_empty(),
        "the preview should stay put rather than jump somewhere arbitrary"
    );
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

// ── P4-05 and P4-11: render modes and coordinate rounding ────────────────────

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
