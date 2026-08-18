//! The real-world benchmark corpus — P4-08 and P4-10.
//!
//! The feasibility spike measured only synthetic `#lorem` documents, and said
//! so: every latency and SVG-size number in the RFC could have been optimistic
//! for anything with figures, tables, bibliographies, or vector graphics. This
//! closes that debt with four documents of genuinely different shape:
//!
//! | Fixture | What it stresses |
//! | --- | --- |
//! | `paper.typ` | Two-column layout, floats, a table, a bibliography, cross-references, display math |
//! | `book.typ` | Length. 25 chapters, an outline, running headers, ~100 A5 pages |
//! | `slides.typ` | Many small pages with heavy per-page styling |
//! | `graphics.typ` | Thousands of vector elements per page — the shape a CeTZ document has |
//!
//! Run it with:
//!
//! ```sh
//! cargo run --release -p typst-session --example corpus
//! ```
//!
//! It is an example rather than a `#[bench]` because the interesting output is
//! a table a person reads, not a single number a harness compares. Results are
//! written into `docs/rfc/010-typst-ultra/research/corpus.md`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use typst::foundations::Bytes;
use typst::syntax::{FileId, RootedPath, VirtualPath, VirtualRoot};
use typst::text::FontInfo;
use typst_session::fs::{FsFiles, FsPackages, SystemClock};
use typst_session::ports::{FaceDescriptor, FontProvider};
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

type Bench = Session<FsFiles, BundledFonts, FsPackages, SystemClock>;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/corpus")
}

fn file_id(path: &str) -> FileId {
    FileId::new(RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).expect("valid path"),
    ))
}

fn session(main: &str, evict_age: usize) -> Bench {
    let world = SessionWorld::new(
        FsFiles::new(corpus()),
        BundledFonts::new(),
        FsPackages::new(None),
        SystemClock,
        file_id(main),
    );
    Session::new(world, evict_age)
}

struct Row {
    name: &'static str,
    pages: usize,
    cold_ms: f64,
    warm_ms: f64,
    p95_ms: f64,
    largest_page_kb: f64,
    total_svg_mb: f64,
    render_ms: f64,
}

fn main() {
    let fixtures = [
        ("paper", "paper.typ"),
        ("book", "book.typ"),
        ("slides", "slides.typ"),
        ("graphics", "graphics.typ"),
    ];

    println!("\n## Compile latency and page size (evictAge = 1)\n");
    println!(
        "| Document | Pages | Cold | Keystroke | p95 | Largest page | All pages | 1 page render |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");

    for (name, file) in fixtures {
        let row = measure(name, file);
        println!(
            "| {} | {} | {:.0} ms | {:.1} ms | {:.1} ms | {:.0} KB | {:.1} MB | {:.1} ms |",
            row.name,
            row.pages,
            row.cold_ms,
            row.warm_ms,
            row.p95_ms,
            row.largest_page_kb,
            row.total_svg_mb,
            row.render_ms,
        );
    }

    // P4-10: does `evictAge: 1` still win on documents that are not synthetic?
    println!("\n## Eviction sweep on the real corpus\n");
    println!("| Document | Age | Warm-up | Steady | p95 |");
    println!("| --- | --- | --- | --- | --- |");

    for (name, file) in [("paper", "paper.typ"), ("book", "book.typ")] {
        for age in [1usize, 3, 10] {
            let (warmup, steady, p95) = sweep(file, age);
            println!(
                "| {name} | {age} | {warmup:.1} ms | {steady:.1} ms | {p95:.1} ms |"
            );
        }
    }
    println!();
}

/// Cold compile, keystroke recompiles, and SVG size for one fixture.
fn measure(name: &'static str, file: &'static str) -> Row {
    let mut session = session(file, 1);
    let id = file_id(file);
    let mut text = std::fs::read_to_string(corpus().join(file)).expect("fixture");
    session.open(id, text.clone());

    let started = Instant::now();
    let cold = session.compile(1);
    let cold_ms = started.elapsed().as_secs_f64() * 1000.0;

    let document = cold.document.clone().unwrap_or_else(|| {
        panic!("{name} failed to compile: {:#?}", first_messages(&cold));
    });
    let pages = document.pages().len();

    // Type one character at the end, forty times, the way a person does.
    let mut samples = Vec::new();
    for step in 0..40 {
        let at = text.len();
        text.push('x');
        session.edit(id, at..at, "x");

        let started = Instant::now();
        session.compile(2 + step);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }

    let warm_ms = median(&samples[30..]);
    let p95_ms = percentile(&samples, 95.0);

    // Page sizes, from the last good document.
    let document = session.last_good().expect("a good document").clone();
    let mut sizes = Vec::new();
    let render_started = Instant::now();
    for index in 0..document.pages().len() {
        let svg = typst_preview_core::render_page(&document, index).unwrap_or_default();
        sizes.push(svg.len());
    }
    let render_ms = render_started.elapsed().as_secs_f64() * 1000.0 / pages.max(1) as f64;

    Row {
        name,
        pages,
        cold_ms,
        warm_ms,
        p95_ms,
        largest_page_kb: sizes.iter().copied().max().unwrap_or(0) as f64 / 1024.0,
        total_svg_mb: sizes.iter().sum::<usize>() as f64 / 1_048_576.0,
        render_ms,
    }
}

/// Warm-up, steady-state, and p95 latency at one eviction age.
fn sweep(file: &'static str, age: usize) -> (f64, f64, f64) {
    let mut session = session(file, age);
    let id = file_id(file);
    let mut text = std::fs::read_to_string(corpus().join(file)).expect("fixture");
    session.open(id, text.clone());
    session.compile(1);

    let mut samples = Vec::new();
    for step in 0..40 {
        let at = text.len();
        text.push('x');
        session.edit(id, at..at, "x");

        let started = Instant::now();
        session.compile(2 + step);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }

    (median(&samples[..10]), median(&samples[30..]), percentile(&samples, 95.0))
}

fn median(values: &[f64]) -> f64 {
    percentile(values, 50.0)
}

fn percentile(values: &[f64], p: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN timings"));
    let index = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[index]
}

fn first_messages(outcome: &typst_session::CompileOutcome) -> Vec<String> {
    outcome
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .take(3)
        .map(|d| d.message.to_string())
        .collect()
}
