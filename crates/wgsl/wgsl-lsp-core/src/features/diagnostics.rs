//! Publishing problems.
//!
//! Two sources, and which one speaks depends on whether naga ran:
//!
//! - **naga ran.** Its problems are authoritative — it type-checks, and this
//!   server does not. The syntax layer only adds what naga has no way to say,
//!   such as a name declared twice in one scope.
//! - **naga did not run** — a stage it does not implement, a `#version 300 es`
//!   source, a file too broken to parse. Then the syntax layer is all there
//!   is, and its unbalanced-delimiter findings are worth reporting on their
//!   own.
//!
//! What is deliberately *not* published: the reason naga was skipped. A GLSL ES
//! shader is valid GLSL that this validator does not implement, and saying so
//! once in the status bar is right where saying it on every line would be
//! wrong. `wgsl/shaderInfo` carries it instead.

use lsp_types::{Diagnostic, DiagnosticSeverity, Uri};

use crate::Server;
use crate::state::Document;

/// The `source` field, which the Problems panel shows beside each entry.
const NAGA: &str = "naga";
const SYNTAX: &str = "wgsl-syntax";

/// Everything wrong with a document.
pub fn diagnostics(document: &Document) -> Vec<Diagnostic> {
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
