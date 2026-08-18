//! Code lenses: "Preview" and "Export as…" above the first line.

use lsp_types::{CodeLens, CodeLensParams, Command, Position, Range};

use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// `textDocument/codeLens`.
    pub fn code_lenses(&mut self, params: CodeLensParams) -> Option<Vec<CodeLens>> {
        // Only offer them on a file the compiler can actually reach — and a
        // bibliography is data, not a document to preview or export.
        let (id, _) = self.source_of(&params.text_document.uri)?;
        if super::bibtex::is_bib(id) {
            return None;
        }

        let first_line = Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: 0, character: 0 },
        };
        let uri = serde_json::to_value(&params.text_document.uri).ok()?;

        Some(vec![
            CodeLens {
                range: first_line,
                command: Some(Command {
                    title: "$(open-preview) Preview".into(),
                    command: "typstUltra.showPreviewToSide".into(),
                    arguments: Some(vec![uri.clone()]),
                }),
                data: None,
            },
            CodeLens {
                range: first_line,
                command: Some(Command {
                    title: "Export as…".into(),
                    command: "typstUltra.export".into(),
                    arguments: Some(vec![uri]),
                }),
                data: None,
            },
        ])
    }
}
