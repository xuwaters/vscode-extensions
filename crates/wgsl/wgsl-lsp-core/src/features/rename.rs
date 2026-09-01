//! `textDocument/prepareRename` and `textDocument/rename`.

use std::collections::HashMap;

use lsp_types::{
    PrepareRenameResponse, RenameParams, TextDocumentPositionParams, TextEdit, WorkspaceEdit,
};
use wgsl_syntax::builtins;

use crate::Server;
use crate::features::references::name_at;

impl Server {
    /// Whether the thing under the cursor can be renamed, and where it is.
    ///
    /// Refusing early matters: a client that gets a range here will show a
    /// rename box, and a rename box that produces an invalid program is worse
    /// than no rename box at all.
    pub fn prepare_rename(
        &mut self,
        params: TextDocumentPositionParams,
    ) -> Option<PrepareRenameResponse> {
        let (document, offset) = self.locate(&params.text_document.uri, params.position)?;
        let reference = document.parsed().reference_at(offset)?;
        let name = document.slice(reference.span);

        // The language's own names are not ours to rename.
        if builtins::is_reserved(document.language, name) {
            return None;
        }
        // A member access renames the *declaration* of a field, which may be
        // in another file entirely. Rather than rename it here and miss its
        // other uses, decline.
        if reference.is_member && document.parsed().resolve_name(name, offset).is_none() {
            return None;
        }

        Some(PrepareRenameResponse::RangeWithPlaceholder {
            range: document.range(reference.span),
            placeholder: name.to_string(),
        })
    }

    // `Uri`'s interior mutability is a parse cache inside fluent-uri that takes
    // no part in `Hash` or `Eq`, and `WorkspaceEdit::changes` is keyed by `Uri`
    // upstream — there is no other map to reach for.
    #[allow(clippy::mutable_key_type)]
    pub fn rename(&mut self, params: RenameParams) -> Option<WorkspaceEdit> {
        let position = params.text_document_position;
        let (document, offset) = self.locate(&position.text_document.uri, position.position)?;
        let name = name_at(document, offset)?;

        if builtins::is_reserved(document.language, name) {
            return None;
        }
        // Renaming to a name the language already defines produces a program
        // that either shadows a builtin or fails to compile.
        if builtins::is_reserved(document.language, &params.new_name) {
            return None;
        }

        let edits: Vec<TextEdit> = document
            .parsed()
            .occurrences(document.text(), name)
            .map(|reference| TextEdit {
                range: document.range(reference.span),
                new_text: params.new_name.clone(),
            })
            .collect();

        if edits.is_empty() {
            return None;
        }

        let mut changes = HashMap::new();
        changes.insert(document.uri.clone(), edits);
        Some(WorkspaceEdit { changes: Some(changes), ..WorkspaceEdit::default() })
    }
}
