//! Fixtures, split by task so a failure names the thing that broke.
//!
//! [`resolution`] is P4-01, [`model`] P4-02, [`conversions`] P4-03,
//! [`members`] P4-04, [`inference`] P4-05, [`overloads`] P4-06,
//! [`availability`] P4-07, [`statements`] P4-08, [`catalogue`] P4-09 and
//! [`corpus`] P4-10.
//!
//! Two habits run through all of them. Fixtures assert *codes*, not messages,
//! except where the message is the point (a hint that names the replacement
//! spelling). And every fixture that expects an error also has a sibling that
//! expects **silence** on the valid version of the same code — the whole risk
//! in this phase is the false positive, so every rule is tested from both
//! sides.

mod availability;
mod catalogue;
mod conversions;
mod corpus;
mod inference;
mod members;
mod model;
mod overloads;
mod resolution;
mod statements;

use analyzer_core::diagnostics::{DiagnosticCode, Severity};
use glsl_spec::Stage;

use crate::{Analysis, Options, analyze_source};

/// Analyse a source with no stage known — the state a `.glsl` file is in.
pub(crate) fn analyze(source: &str) -> Analysis {
    analyze_source(source, &Options::default())
}

/// Analyse a source the host has told us the stage of.
pub(crate) fn analyze_in(source: &str, stage: Stage) -> Analysis {
    analyze_source(source, &Options { stage: Some(stage), ..Options::default() })
}

/// Every diagnostic code reported, in span order.
pub(crate) fn codes(analysis: &Analysis) -> Vec<&'static str> {
    analysis.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// Just the error-severity codes — what an editor paints red.
pub(crate) fn error_codes(analysis: &Analysis) -> Vec<&'static str> {
    analysis
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code.as_str())
        .collect()
}

/// The error messages, for the assertions where the wording is the point.
pub(crate) fn messages(analysis: &Analysis) -> Vec<&str> {
    analysis.diagnostics.iter().map(|d| d.message.as_str()).collect()
}

/// Assert that a source produces exactly these codes, errors and warnings
/// alike.
#[track_caller]
pub(crate) fn expect(source: &str, expected: &[&str]) {
    let analysis = analyze(source);
    assert_eq!(codes(&analysis), expected, "in:\n{source}\n{:#?}", messages(&analysis));
}

/// Assert that a source produces no error at all. The other half of every
/// fixture in this phase.
#[track_caller]
pub(crate) fn expect_clean(source: &str) {
    let analysis = analyze(source);
    let errors: Vec<String> = analysis
        .errors()
        .map(|d| format!("{} {}", d.code.as_str(), d.message))
        .collect();
    assert!(errors.is_empty(), "expected no error in:\n{source}\ngot: {errors:#?}");
}

/// The type of the expression the `«…»` markers bracket, printed.
///
/// The markers are stripped before analysis, so what is analysed is the source
/// as written and the offsets line up with it. The answer is the *smallest*
/// node that covers the marked range and has a type, which is what makes
/// `«camera.view»` ask about the member access rather than about `camera`.
#[track_caller]
pub(crate) fn type_of(marked: &str) -> String {
    let (source, start, end) = strip_markers(marked);
    let (pp, tree) = glsl_syntax::parse_source(&source);
    let analysis = crate::analyze(&tree, &pp, &Options::default());
    let mut best: Option<(u32, String)> = None;
    for (id, node) in tree.nodes() {
        if node.span.start > start || node.span.end < end {
            continue;
        }
        let Some(ty) = analysis.type_at(id) else {
            continue;
        };
        let width = node.span.len();
        if best.as_ref().is_none_or(|(previous, _)| width < *previous) {
            best = Some((width, analysis.type_name(ty)));
        }
    }
    best.map(|(_, name)| name).unwrap_or_else(|| "?".to_string())
}

/// Split `«` … `»` out of a source, answering the source and the byte range
/// they bracketed.
pub(crate) fn strip_markers(marked: &str) -> (String, u32, u32) {
    let Some(start) = marked.find('«') else {
        return (marked.to_string(), 0, 0);
    };
    let source = marked.replacen('«', "", 1).replacen('»', "", 1);
    let end = marked
        .find('»')
        .map(|end| end - '«'.len_utf8())
        .unwrap_or(start);
    (source, start as u32, end as u32)
}
