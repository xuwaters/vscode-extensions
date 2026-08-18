//! Publishing diagnostics.
//!
//! Three behaviours here are the difference between usable and irritating:
//!
//! 1. Diagnostics are published for **every file in the compile graph**, not
//!    just the open one, so an error inside an imported `theme.typ` lands in the
//!    Problems panel under that file's URI with the import chain attached.
//! 2. A file that had diagnostics and now has none is published as an **empty
//!    array**, or the squiggles persist forever.
//! 3. Results for **superseded document versions are dropped** — handled one
//!    level up, in `compile_now`.

use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location,
    PublishDiagnosticsParams, Uri,
};
use rustc_hash::FxHashMap;
use typst::World;
use typst::diag::Severity;
use typst::syntax::FileId;
use typst_session::CompileOutcome;

use crate::convert::range_to_lsp;
use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// Turn a compile outcome into `publishDiagnostics` notifications.
    pub(crate) fn publish_diagnostics(&mut self, outcome: &CompileOutcome) {
        if !self.settings().diagnostics.enabled {
            self.clear_all_diagnostics();
            return;
        }

        let main = self.session().world().main_id();
        let mut grouped: FxHashMap<FileId, Vec<Diagnostic>> = FxHashMap::default();

        for diagnostic in &outcome.diagnostics {
            // A detached span belongs to no file; attach it to the compile root
            // at offset 0 with the message intact rather than dropping it.
            let file = diagnostic.file.unwrap_or(main);
            let Some(converted) = self.convert_diagnostic(file, diagnostic) else {
                continue;
            };
            grouped.entry(file).or_default().push(converted);
        }

        let mut now_published = Vec::new();
        for (file, diagnostics) in grouped {
            let Some(uri) = self.uris().to_uri(file) else { continue };
            now_published.push(uri.as_str().to_string());
            self.send_diagnostics(uri, diagnostics);
        }

        // Clear the files that had diagnostics last time and do not now.
        let stale: Vec<String> = self
            .published
            .iter()
            .filter(|uri| !now_published.contains(uri))
            .cloned()
            .collect();
        for uri in stale {
            if let Ok(uri) = uri.parse::<Uri>() {
                self.send_diagnostics(uri, Vec::new());
            }
        }

        self.published = now_published.into_iter().collect();
    }

    /// Clear every diagnostic we have published.
    pub(crate) fn clear_all_diagnostics(&mut self) {
        let published: Vec<String> = self.published.drain().collect();
        for uri in published {
            if let Ok(uri) = uri.parse::<Uri>() {
                self.send_diagnostics(uri, Vec::new());
            }
        }
    }

    fn send_diagnostics(&mut self, uri: Uri, diagnostics: Vec<Diagnostic>) {
        let params = PublishDiagnosticsParams { uri, diagnostics, version: None };
        if let Ok(params) = serde_json::to_value(params) {
            self.notify("textDocument/publishDiagnostics", params);
        }
    }

    fn convert_diagnostic(
        &self,
        file: FileId,
        diagnostic: &typst_session::Diagnostic,
    ) -> Option<Diagnostic> {
        let source = self.session().world().source(file).ok()?;
        let byte_range = diagnostic.range.clone().unwrap_or(0..0);
        let range = range_to_lsp(&source, byte_range);

        let related: Vec<DiagnosticRelatedInformation> = diagnostic
            .related
            .iter()
            .filter_map(|related| {
                let source = self.session().world().source(related.file).ok()?;
                Some(DiagnosticRelatedInformation {
                    location: Location {
                        uri: self.uris().to_uri(related.file)?,
                        range: range_to_lsp(&source, related.range.clone()),
                    },
                    message: related.message.to_string(),
                })
            })
            .collect();

        Some(Diagnostic {
            range,
            severity: Some(match diagnostic.severity {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Warning => DiagnosticSeverity::WARNING,
            }),
            source: Some("typst".into()),
            message: diagnostic.message.to_string(),
            related_information: (!related.is_empty()).then_some(related),
            ..Diagnostic::default()
        })
    }
}
