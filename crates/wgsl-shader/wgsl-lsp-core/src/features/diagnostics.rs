//! Publishing problems.
//!
//! One authority per language, and after RFC 012 phase 5 there is no third
//! case:
//!
//! - **WGSL** is naga's. It type-checks and this server does not, so its
//!   problems are authoritative; the syntax layer only adds what naga has no
//!   way to say, such as an unbalanced delimiter in a file it could not parse.
//! - **GLSL** is ours, in every dialect. The preprocessor, the parser and
//!   semantic analysis each carry a stable `GLSL####` code and a severity, and
//!   all three are published together —
//!   [design/diagnostics.md](../../../../../docs/rfc/012-glsl-analyzer/design/diagnostics.md).
//!
//! What used to live here and does not any more is the *skip*: a
//! `#version 300 es` or OpenGL-style shader is valid GLSL that naga did not
//! implement, so it was highlighted and never checked. It is checked now
//! ([decision 0008](../../../../../docs/rfc/012-glsl-analyzer/decisions/0008-naga-glsl-in-dropped.md)).
//!
//! The conservatism that keeps it honest lives one layer down, in
//! `glsl-analysis`: a file whose preprocessing or parse failed gets resolution
//! and hover and **no semantic errors at all**, because a false squiggle costs
//! more than a missed one.

use analyzer_core::diagnostics::{DiagnosticCode, Severity};
use analyzer_core::spans::ByteSpan;
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Uri};

use crate::Server;
use crate::state::Document;

/// The `source` field, which the Problems panel shows beside each entry.
const NAGA: &str = "naga";
const SYNTAX: &str = "wgsl-syntax";
const GLSL: &str = "glsl";

/// Everything wrong with a document.
pub fn diagnostics(document: &Document) -> Vec<Diagnostic> {
    match document.glsl() {
        Some(_) => glsl_diagnostics(document),
        None => wgsl_diagnostics(document),
    }
}

/// Everything our own GLSL pipeline found: preprocessor, parser, semantics.
fn glsl_diagnostics(document: &Document) -> Vec<Diagnostic> {
    let Some(glsl) = document.glsl() else {
        return Vec::new();
    };
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut push = |code: &'static str, severity: Severity, message: &str, span: ByteSpan| {
        diagnostics.push(Diagnostic {
            range: document.range(span),
            severity: Some(lsp_severity(severity)),
            code: Some(NumberOrString::String(code.to_string())),
            source: Some(GLSL.to_string()),
            message: message.to_string(),
            ..Diagnostic::default()
        });
    };
    for problem in &glsl.pp.diagnostics {
        push(problem.code.as_str(), problem.severity, &problem.message, problem.span);
    }
    for problem in &glsl.tree.diagnostics {
        push(problem.code.as_str(), problem.severity, &problem.message, problem.span);
    }
    for problem in &glsl.analysis.diagnostics {
        push(problem.code.as_str(), problem.severity, &problem.message, problem.span);
    }
    diagnostics.sort_by_key(|d| (d.range.start.line, d.range.start.character));
    diagnostics
}

fn lsp_severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Info => DiagnosticSeverity::INFORMATION,
        Severity::Hint => DiagnosticSeverity::HINT,
    }
}

/// Everything naga and the WGSL syntax layer found.
fn wgsl_diagnostics(document: &Document) -> Vec<Diagnostic> {
    let analysis = document.analysis();
    let mut diagnostics: Vec<Diagnostic> = analysis
        .problems
        .iter()
        .map(|problem| Diagnostic {
            range: document.range(problem.span),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some(NAGA.to_string()),
            message: problem.message.clone(),
            ..Diagnostic::default()
        })
        .collect();

    let naga_ran = analysis.module.is_some() || !analysis.problems.is_empty();
    for problem in &document.parsed().diagnostics {
        let unbalanced = problem.message.starts_with("unmatched")
            || problem.message.starts_with("unclosed");
        // An unbalanced delimiter is also what made naga fail, and reporting
        // it twice in two places is worse than reporting naga's version once.
        if unbalanced && naga_ran {
            continue;
        }
        diagnostics.push(Diagnostic {
            range: document.range(problem.span),
            severity: Some(if unbalanced {
                DiagnosticSeverity::ERROR
            } else {
                DiagnosticSeverity::WARNING
            }),
            source: Some(SYNTAX.to_string()),
            message: problem.message.clone(),
            ..Diagnostic::default()
        });
    }

    diagnostics
}

impl Server {
    /// Compute and publish diagnostics for one document.
    pub(crate) fn publish_diagnostics_for(&mut self, uri: &Uri) {
        let Some(document) = self.document(uri) else {
            return;
        };
        let version = document.version;
        let diagnostics = diagnostics(document);

        // A file that had problems and now has none must be published as an
        // empty array, or the squiggles stay on screen forever.
        let key = uri.as_str().to_string();
        if diagnostics.is_empty() && !self.published().contains(&key) {
            return;
        }
        if diagnostics.is_empty() {
            self.published().remove(&key);
        } else {
            self.published().insert(key);
        }

        let params = serde_json::json!({
            "uri": uri.as_str(),
            "version": version,
            "diagnostics": diagnostics,
        });
        self.notify("textDocument/publishDiagnostics", params);
    }

    /// Clear a document's diagnostics, on close.
    pub(crate) fn clear_diagnostics(&mut self, uri: &Uri) {
        let key = uri.as_str().to_string();
        if !self.published().remove(&key) {
            return;
        }
        self.notify(
            "textDocument/publishDiagnostics",
            serde_json::json!({ "uri": uri.as_str(), "diagnostics": [] }),
        );
    }
}
