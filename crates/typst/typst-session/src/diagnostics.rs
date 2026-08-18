//! `SourceDiagnostic` → file, byte range, message.
//!
//! Everything transport-neutral happens here; the UTF-16 conversion LSP needs
//! belongs to `typst-lsp-core::convert`, which is the single place in the stack
//! that knows about LSP positions.

use std::ops::Range;

use ecow::EcoString;
use typst::WorldExt;
use typst::diag::{Severity, SourceDiagnostic};
use typst::syntax::FileId;

/// A diagnostic resolved against the world that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The file it belongs to. `None` for detached spans, which the caller
    /// attaches to the main file.
    pub file: Option<FileId>,
    /// The byte range within that file, if the span resolves to one.
    pub range: Option<Range<usize>>,
    /// Error or warning.
    pub severity: Severity,
    /// The primary message, with `hint:` lines appended.
    pub message: EcoString,
    /// Hints, also folded into `message` — kept separate so a client that
    /// renders them differently can.
    pub hints: Vec<EcoString>,
    /// The `#import` / `#include` / call chain that reached the problem, plus
    /// any hint that carries a span of its own.
    pub related: Vec<Related>,
}

/// A secondary location attached to a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Related {
    /// The file the note points into.
    pub file: FileId,
    /// The byte range within it.
    pub range: Range<usize>,
    /// What to say about it.
    pub message: EcoString,
}

impl Diagnostic {
    /// Resolve one compiler diagnostic against the world it came from.
    pub fn resolve(world: &dyn typst::World, diagnostic: &SourceDiagnostic) -> Self {
        let file = diagnostic.span.id();
        let range = world.range(diagnostic.span);

        let mut message = diagnostic.message.clone();
        let mut hints = Vec::new();
        let mut related = Vec::new();

        for hint in &diagnostic.hints {
            hints.push(hint.v.clone());
            // VSCode renders multi-line diagnostic messages in the hover, so
            // appending is how a hint actually reaches the reader.
            message.push('\n');
            message.push_str("hint: ");
            message.push_str(&hint.v);

            if let (Some(file), Some(range)) = (hint.span.id(), world.range(hint.span)) {
                related.push(Related { file, range, message: hint.v.clone() });
            }
        }

        for step in &diagnostic.trace {
            let Some(file) = step.span.id() else { continue };
            let Some(range) = world.range(step.span) else { continue };
            related.push(Related {
                file,
                range,
                message: EcoString::from(step.v.to_string()),
            });
        }

        Self {
            file,
            range,
            severity: diagnostic.severity,
            message,
            hints,
            related,
        }
    }

    /// Whether this is an error rather than a warning.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Resolve a batch of compiler diagnostics.
pub fn resolve_all<'a>(
    world: &dyn typst::World,
    diagnostics: impl IntoIterator<Item = &'a SourceDiagnostic>,
) -> Vec<Diagnostic> {
    diagnostics.into_iter().map(|d| Diagnostic::resolve(world, d)).collect()
}
