//! The compile pipeline, exercised natively through the `std::fs` ports.
//!
//! Covers P1-05 (compile-then-evict, with the ratio assertion that catches a
//! reordering), P1-06 (diagnostic resolution), and P1-16 (a snapshot corpus
//! including a multi-file project, so diagnostic fan-out to non-open files is
//! covered).

mod support;

use std::time::Instant;

use support::{fixtures, render_diagnostics, session_at, session_with};
use typst_session::ports::{PackageProvider, PackageResolution};

#[test]
fn a_clean_document_produces_no_diagnostics() {
    let mut session = session_at(&fixtures(), "ok.typ");
    let outcome = session.compile(1);

    assert!(outcome.ok, "expected a successful compile");
    assert!(outcome.document.is_some());
    insta::assert_snapshot!(render_diagnostics(&outcome.diagnostics));
}

#[test]
fn a_syntax_error_is_reported_with_a_range() {
    let mut session = session_at(&fixtures(), "syntax-error.typ");
    let outcome = session.compile(1);

    assert!(!outcome.ok);
    assert!(outcome.diagnostics.iter().any(|d| d.is_error()));
    insta::assert_snapshot!(render_diagnostics(&outcome.diagnostics));
}

#[test]
fn an_unknown_function_is_reported_with_a_hint() {
    let mut session = session_at(&fixtures(), "unknown-function.typ");
    let outcome = session.compile(1);

    assert!(!outcome.ok);
    insta::assert_snapshot!(render_diagnostics(&outcome.diagnostics));
}

#[test]
fn an_unknown_font_is_a_warning_not_an_error() {
    let mut session = session_at(&fixtures(), "unknown-font.typ");
    let outcome = session.compile(1);

    assert!(outcome.ok, "an unknown font must not fail the compile");
    assert!(outcome.diagnostics.iter().all(|d| !d.is_error()));
    insta::assert_snapshot!(render_diagnostics(&outcome.diagnostics));
}

/// P1-16: an error inside an imported file must be reported against *that*
/// file's id, with the import chain as related information, so the Problems
/// panel can file it under the right URI even though the editor never opened it.
#[test]
fn an_error_in_an_imported_file_is_attributed_to_that_file() {
    let mut session = session_at(&fixtures().join("multi"), "main.typ");
    let outcome = session.compile(1);

    assert!(!outcome.ok);
    let files: Vec<String> = outcome
        .diagnostics
        .iter()
        .filter_map(|d| d.file)
        .map(|id| id.get().vpath().get_with_slash().to_string())
        .collect();
    assert!(
        files.iter().any(|f| f.contains("broken.typ")),
        "expected a diagnostic on the imported file, got {files:?}"
    );

    insta::assert_snapshot!(render_diagnostics(&outcome.diagnostics));
}

#[test]
fn a_pending_package_is_reported_and_recorded_for_the_host() {
    let mut session = session_at(&fixtures(), "packaged.typ");
    let outcome = session.compile(1);

    assert!(!outcome.ok);
    assert_eq!(
        outcome.pending_packages.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        vec!["@preview/example:0.1.0"],
        "the host needs the spec so it can download it and trigger a recompile"
    );
    assert!(
        outcome.diagnostics.iter().any(|d| d.message.contains("downloading")),
        "the user needs to see why the import failed"
    );
}

/// A provider that refuses every package, as `packages.enabled: false` does.
struct PackagesDisabled;

impl PackageProvider for PackagesDisabled {
    fn resolve(&self, _spec: &typst::syntax::package::PackageSpec) -> PackageResolution {
        PackageResolution::Failed("package downloads are disabled".into())
    }
}

#[test]
fn a_refused_package_produces_a_clear_diagnostic_rather_than_a_hang() {
    let mut session = session_with(&fixtures(), "packaged.typ", PackagesDisabled);
    let outcome = session.compile(1);

    assert!(!outcome.ok);
    assert!(outcome.pending_packages.is_empty());
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|d| d.message.contains("package downloads are disabled")),
        "the reason must reach the user: {:#?}",
        outcome.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn the_last_good_document_survives_a_failing_compile() {
    let mut session = session_at(&fixtures(), "ok.typ");
    let id = support::file_id("ok.typ");
    session.open(id, std::fs::read_to_string(fixtures().join("ok.typ")).unwrap());

    let good = session.compile(1);
    assert!(good.ok);
    let page_count = good.document.as_ref().unwrap().pages().len();

    // Break it the way a keystroke does, mid-word.
    session.replace(id, "= Broken\n\n#let x = (1, 2");
    let bad = session.compile(2);

    assert!(!bad.ok);
    assert_eq!(
        bad.document.as_ref().map(|d| d.pages().len()),
        Some(page_count),
        "a transient syntax error must not blank the preview"
    );
}

#[test]
fn an_open_document_beats_the_copy_on_disk() {
    let mut session = session_at(&fixtures(), "ok.typ");
    let id = support::file_id("ok.typ");
    session.open(id, "= Overlay wins\n".into());

    let outcome = session.compile(1);
    assert!(outcome.ok);

    let text = session.world().vfs().opened(id).unwrap().text().to_string();
    assert_eq!(text, "= Overlay wins\n");
}

#[test]
fn an_incremental_edit_updates_the_open_source() {
    let mut session = session_at(&fixtures(), "ok.typ");
    let id = support::file_id("ok.typ");
    session.open(id, "= Title\n".into());

    assert!(session.edit(id, 2..7, "Heading"));
    assert_eq!(session.world().vfs().opened(id).unwrap().text(), "= Heading\n");

    // An out-of-bounds range is refused rather than panicking, so the server
    // can fall back to a full resync.
    assert!(!session.edit(id, 0..999, "x"));
}

/// P1-05: the invariant that pays for the whole design.
///
/// Compiling *then* evicting keeps the memoized layout the next compile reuses.
/// Evicting first throws it away, which the feasibility spike measured at 411 ms
/// against 7 ms — a 50× regression that no functional test would have caught.
/// If someone reorders the two calls in `Session::compile`, this fails.
#[test]
fn a_warm_recompile_is_much_faster_than_a_cold_one() {
    let root = fixtures();
    let mut session = session_at(&root, "generated.typ");
    let id = support::file_id("generated.typ");

    let mut text = String::new();
    for section in 0..20 {
        text.push_str(&format!(
            "= Section {section}\n\n#lorem(120)\n\n$ sum_(i=1)^n i = (n(n+1))/2 $\n\n#lorem(80)\n\n"
        ));
    }
    session.open(id, text.clone());

    let start = Instant::now();
    let cold = session.compile(1);
    let cold_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!(cold.ok, "the generated document must compile");

    // Type one character at the end, the way a person does, and take the best
    // of several so a scheduler hiccup does not fail the build.
    let mut warm_ms = f64::MAX;
    for step in 0..5 {
        let at = text.len();
        text.push('x');
        session.edit(id, at..at, "x");

        let start = Instant::now();
        let warm = session.compile(2 + step);
        warm_ms = warm_ms.min(start.elapsed().as_secs_f64() * 1000.0);
        assert!(warm.ok);
    }

    assert!(
        warm_ms * 3.0 < cold_ms,
        "warm recompile {warm_ms:.1}ms vs cold {cold_ms:.1}ms — memoization is not \
         being reused. Check that Session::compile still evicts *after* compiling."
    );
}
