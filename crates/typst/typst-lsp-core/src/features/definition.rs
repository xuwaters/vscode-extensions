//! Goto-definition — `typst_ide::definition`.
//!
//! Three outcomes, three answers:
//!
//! * `Span` → a `Location` in the owning file.
//! * `File` → the whole imported or included file, at `0:0`.
//! * `Std` → no location exists, so we answer `null` and let hover carry the
//!   standard-library documentation instead. Tinymist opens a generated docs
//!   page here; that needs a docs pipeline we are not building.

use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse, Location, Position, Range};
use typst::World;
use typst::syntax::Side;
use typst_ide::Definition;

use crate::convert::range_to_lsp;
use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// `textDocument/definition`.
    pub fn definition(
        &mut self,
        params: GotoDefinitionParams,
    ) -> Option<GotoDefinitionResponse> {
        let position = params.text_document_position_params;
        let (_, source, cursor) = self.locate(&position.text_document.uri, position.position)?;

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
        })?;

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
                range: Range {
                    start: Position { line: 0, character: 0 },
                    end: Position { line: 0, character: 0 },
                },
            })),
            // A standard-library item has no source location to go to.
            Definition::Std(_) => None,
        }
    }
}
