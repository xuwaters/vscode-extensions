//! Fixtures, split by task so a failure names the thing that broke.
//!
//! Phase 2 — the token layer and the preprocessor: [`lexer`] is P2-01,
//! [`directives`] P2-02, [`macro_table`] P2-03, [`conditionals`] P2-04,
//! [`expansion`] P2-05, [`api`] P2-06 and [`corpus`] P2-07.
//!
//! Phase 3 — the parser and the CST: [`cst`] is P3-01, [`declarations`] P3-02,
//! [`functions`] P3-03, [`expressions`] P3-04, [`recovery`] P3-05,
//! [`dialects`] P3-06, [`corpus_parse`] P3-07 and [`outline`] P3-08.
//!
//! Expansion fixtures assert *spans*, not just text. Decision 0003 is about
//! where a token points, and a fixture that only checks spelling would pass
//! with the provenance rules inverted. Parser fixtures assert the *dump* — the
//! tree's shape — for the same reason: a snapshot of the text would pass with
//! the precedence table upside down.

mod api;
mod conditionals;
mod corpus;
mod corpus_parse;
mod cst;
mod declarations;
mod dialects;
mod directives;
mod expansion;
mod expressions;
mod functions;
mod lexer;
mod macro_table;
mod outline;
mod recovery;

use analyzer_core::diagnostics::{DiagnosticCode, Severity};

use crate::cst::SyntaxTree;
use crate::diagnostics::PpCode;
use crate::preprocessor::Preprocessed;

/// The spelling of every live token, in order.
fn texts(pp: &Preprocessed) -> Vec<&str> {
    pp.tokens.iter().map(|t| t.text.as_str()).collect()
}

/// The *source* text every live token is attributed to. This is the assertion
/// that actually tests provenance: it reads the original bytes back through
/// each token's span, so a wrong span shows up as wrong text.
fn attributed<'a>(pp: &'a Preprocessed, source: &'a str) -> Vec<&'a str> {
    pp.tokens.iter().map(|t| &source[t.span.start as usize..t.span.end as usize]).collect()
}

/// Every diagnostic code reported, in order.
fn codes(pp: &Preprocessed) -> Vec<PpCode> {
    pp.diagnostics.iter().map(|d| d.code).collect()
}

/// The messages of the error-severity diagnostics, for the "no errors" checks
/// that want to say *which* error when they fail.
fn errors(pp: &Preprocessed) -> Vec<&str> {
    pp.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect()
}

// -- phase 3 helpers -------------------------------------------------------

/// Preprocess and parse, which is what every parser fixture starts with.
fn parsed(source: &str) -> (Preprocessed, SyntaxTree) {
    crate::parse_source(source)
}

/// The tree's shape as indented text — the thing parser fixtures assert.
fn dump(source: &str) -> String {
    let (pp, tree) = parsed(source);
    tree.dump(&pp, source)
}

/// Every parser diagnostic's stable code, in order.
fn parse_codes(tree: &SyntaxTree) -> Vec<&'static str> {
    tree.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// The messages of the error-severity parser diagnostics.
fn parse_errors(tree: &SyntaxTree) -> Vec<&str> {
    tree.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect()
}
