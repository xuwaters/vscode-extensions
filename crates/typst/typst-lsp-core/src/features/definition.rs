//! Goto-definition — `typst_ide::definition`.
//!
//! Three outcomes, three answers:
//!
//! * `Span` → a `Location` in the owning file.
//! * `File` → the whole imported or included file, at `0:0`.
//! * `Std` → no location exists, so we answer `null` and let hover carry the
//!   standard-library documentation instead. Tinymist opens a generated docs
//!   page here; that needs a docs pipeline we are not building.
//!
//! Three things `typst-ide` has no notion of are handled here: a label used as
//! a value in code — `#context counter(heading).at(<intro>)` — a path string
//! that is not an import — `#bibliography("refs.bib")` — and a citation key,
//! which lives in a `.bib` file the compiled document only carries labels from.

use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse, Location, Position, Range};
use typst::World;
use typst::syntax::Side;
use typst_ide::Definition;

use crate::convert::range_to_lsp;
use crate::{Ports, Server};

/// Where a whole-file answer points: the top of the file.
const FILE_START: Range = Range {
    start: Position { line: 0, character: 0 },
    end: Position { line: 0, character: 0 },
};

impl<Q: Ports> Server<Q> {
    /// `textDocument/definition`.
    pub fn definition(
        &mut self,
        params: GotoDefinitionParams,
    ) -> Option<GotoDefinitionResponse> {
        let position = params.text_document_position_params;

        // In a bibliography: `crossref` and `@string` references.
        if let Some((id, source, bib)) = self.bib_of(&position.text_document.uri) {
            let cursor = crate::convert::position_to_offset(&source, position.position);
            return self
                .bib_definition(id, &source, &bib, cursor)
                .map(GotoDefinitionResponse::Scalar);
        }

        let (id, source, cursor) =
            self.locate(&position.text_document.uri, position.position)?;

        // A label names a declaration written elsewhere in the document, and
        // both its spellings — `@intro`, and `<intro>` used as a value in code
        // — should jump to it. `typst-ide` answers only the first, and only
        // from a compiled document; the syntax walk answers both, and keeps
        // answering while the document is failing to compile.
        if let Some(name) = self.label_at(&source, cursor)
            && let Some(location) = self.label_declaration(id, &name)
            // Standing on the declaration, the jump would go nowhere.
            && !on_declaration(&source, &position.text_document.uri, &location, cursor)
        {
            return Some(GotoDefinitionResponse::Scalar(location));
        }

        let definition = typst_ide::definition(
            self.session().world(),
            self.last_good(),
            &source,
            cursor,
            Side::Before,
        )
        .or_else(|| {
            typst_ide::definition(
                self.session().world(),
                self.last_good(),
                &source,
                cursor,
                Side::After,
            )
        });

        let Some(definition) = definition else {
            // A path that is not an import: `bibliography`, `image`, `read` and
            // the rest all name a file the reader should be able to jump into.
            if let Some(path) = crate::features::links::path_at(&source, cursor)
                && let Some(file) = self.resolve_path_id(id, &path)
                && let Some(uri) = self.uris().to_uri(file)
            {
                return Some(GotoDefinitionResponse::Scalar(Location {
                    uri,
                    range: FILE_START,
                }));
            }

            // A citation key is defined in a `.bib` file, which typst-ide has
            // no notion of: it can only place labels the compiled document
            // carries.
            let (file, target, entry) = self.cited_entry(&source, cursor)?;
            return Some(GotoDefinitionResponse::Scalar(Location {
                uri: self.uris().to_uri(file)?,
                range: range_to_lsp(&target, entry.key_range.clone()),
            }));
        };

        match definition {
            Definition::Span(span) => {
                let file = span.id()?;
                let target = self.session().world().source(file).ok()?;
                let range = typst::WorldExt::range(self.session().world(), span)?;
                Some(GotoDefinitionResponse::Scalar(Location {
                    uri: self.uris().to_uri(file)?,
                    range: range_to_lsp(&target, range),
                }))
            }
            Definition::File(file) => Some(GotoDefinitionResponse::Scalar(Location {
                uri: self.uris().to_uri(file)?,
                range: FILE_START,
            })),
            // A standard-library item has no source location to go to.
            Definition::Std(_) => None,
        }
    }
}

/// Whether the cursor is already inside the declaration we would jump to.
fn on_declaration(
    source: &typst::syntax::Source,
    uri: &lsp_types::Uri,
    location: &Location,
    cursor: usize,
) -> bool {
    if &location.uri != uri {
        return false;
    }
    let range = crate::convert::range_from_lsp(source, location.range);
    range.start <= cursor && cursor <= range.end
}
