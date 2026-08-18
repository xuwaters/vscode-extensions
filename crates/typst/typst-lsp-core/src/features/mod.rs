//! Every LSP feature, one module each.
//!
//! Each module adds an `impl` block to [`Server`], so a feature is a file and
//! the dispatch table is the index.

pub mod code_actions;
pub mod code_lens;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod folding;
pub mod formatting;
pub mod hover;
pub mod inlay_hints;
pub mod lifecycle;
pub mod links;
pub mod postfix;
pub mod preview;
pub mod references;
pub mod rename;
pub mod selection;
pub mod semantic_tokens;
pub mod signature_help;
pub mod symbols;

use lsp_types::{Position, Uri};
use typst::World;
use typst::syntax::{FileId, Source};

use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// The source text for a document URI, from the overlay or from the host.
    pub(crate) fn source_of(&self, uri: &Uri) -> Option<(FileId, Source)> {
        let id = self.uris().to_file_id(uri)?;
        let source = self.session().world().source(id).ok()?;
        Some((id, source))
    }

    /// A document URI plus position, resolved to a byte offset.
    pub(crate) fn locate(
        &self,
        uri: &Uri,
        position: Position,
    ) -> Option<(FileId, Source, usize)> {
        let (id, source) = self.source_of(uri)?;
        let offset = crate::convert::position_to_offset(&source, position);
        Some((id, source, offset))
    }

    /// The last successfully compiled document, if there is one.
    ///
    /// IDE features pass this to `typst-ide` as an `Option`; a missing or stale
    /// document costs label completions and label hovers, and nothing else.
    pub(crate) fn last_good(&self) -> Option<&typst_layout::PagedDocument> {
        self.session().last_good()
    }
}
