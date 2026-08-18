//! Rename, and the three refusals.
//!
//! Rename applies the same occurrence set `references` finds. What matters as
//! much as the edit is the refusal: three situations cannot be renamed
//! correctly, and each says so explicitly rather than silently doing nothing or
//! — worse — renaming half of them.

use std::collections::HashMap;

use lsp_types::{
    PrepareRenameResponse, RenameParams, TextDocumentPositionParams, TextEdit, Uri,
    WorkspaceEdit,
};
use typst::syntax::{LinkedNode, Side, Source, SyntaxKind, VirtualRoot};
use typst_ide::Definition;

use crate::convert::range_to_lsp;
use crate::dispatch::ResponseError;
use crate::features::references::Target;
use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// `textDocument/prepareRename`.
    ///
    /// Returning `None` makes the client show its own "cannot rename here";
    /// returning an error makes it show ours, which is what the three cases
    /// below want.
    pub fn prepare_rename(
        &mut self,
        params: TextDocumentPositionParams,
    ) -> Option<PrepareRenameResponse> {
        let (id, source, cursor) = self.locate(&params.text_document.uri, params.position)?;

        // In a bibliography the renameable thing is the citation key, and the
        // three refusals below are all about typst definitions.
        if super::bibtex::is_bib(id) {
            let range = self.bib_key_range(&source, cursor)?;
            return Some(PrepareRenameResponse::Range(range_to_lsp(&source, range)));
        }

        let target = self.resolve_target(&source, cursor)?;

        if self.refusal(&source, cursor, &target).is_some() {
            return None;
        }

        let root = LinkedNode::new(source.root());
        let leaf = root
            .leaf_at(cursor, Side::Before)
            .or_else(|| root.leaf_at(cursor, Side::After))?;

        let range = match target {
            // Rename the name, not the delimiters: `<intro>` renames `intro`.
            Target::Label(_) => {
                let range = enclosing_label_range(&leaf)?;
                let text = source.text().get(range.clone())?;
                let start = range.start + text.len() - text.trim_start_matches(['<', '@']).len();
                let end = range.end - (text.len() - text.trim_end_matches('>').len());
                start..end
            }
            _ => leaf.range(),
        };

        Some(PrepareRenameResponse::Range(range_to_lsp(&source, range)))
    }

    /// `textDocument/rename`.
    ///
    /// The `mutable_key_type` allow covers `WorkspaceEdit::changes`, which is
    /// keyed by `Uri`: `fluent_uri` caches inside it, so clippy flags the map,
    /// but its shape is the protocol's and a key is never mutated.
    #[allow(clippy::mutable_key_type)]
    pub fn rename(&mut self, params: RenameParams) -> Result<Option<WorkspaceEdit>, ResponseError> {
        let position = params.text_document_position;
        let Some((id, source, cursor)) =
            self.locate(&position.text_document.uri, position.position)
        else {
            return Ok(None);
        };

        let Some(target) = self.target_at(id, &source, cursor) else {
            return Ok(None);
        };

        // The refusals are about typst definitions — a package's, the standard
        // library's, a file's. None of them can apply to a citation key.
        if !super::bibtex::is_bib(id)
            && let Some(message) = self.refusal(&source, cursor, &target)
        {
            return Err(ResponseError::internal(message));
        }

        let locations = self.find_references(id, &target, true);
        if locations.is_empty() {
            return Ok(None);
        }

        let new_text = params.new_name;
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
        for location in locations {
            let range = self.rename_range(&target, &location);
            changes
                .entry(location.uri)
                .or_default()
                .push(TextEdit { range, new_text: new_text.clone() });
        }

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        }))
    }

    /// The range a citation key occupies at a cursor in a bibliography.
    fn bib_key_range(
        &self,
        source: &Source,
        cursor: usize,
    ) -> Option<std::ops::Range<usize>> {
        let name = self.bib_key_at(source, cursor)?;
        let bib = crate::bib::Bib::parse(source.text());
        let entry = bib.entry_at(cursor)?;

        if entry.key_range.contains(&cursor) || entry.key_range.end == cursor {
            return Some(entry.key_range.clone());
        }

        // A `crossref` value: the key inside the braces, not the braces.
        let field = entry
            .fields
            .iter()
            .find(|field| field.value.as_ref().is_some_and(|v| v.range.contains(&cursor)))?;
        let value = field.value.as_ref()?;
        let text = source.text().get(value.range.clone())?;
        let start = value.range.start + text.find(name.as_str())?;
        Some(start..start + name.len())
    }

    /// A label occurrence covers its delimiters; the edit must not.
    fn rename_range(&self, target: &Target, location: &lsp_types::Location) -> lsp_types::Range {
        let Target::Label(_) = target else { return location.range };

        let Some(id) = self.uris().to_file_id(&location.uri) else {
            return location.range;
        };
        // A bibliography's occurrences already point at the bare key.
        if super::bibtex::is_bib(id) {
            return location.range;
        }
        let Ok(source) = typst::World::source(self.session().world(), id) else {
            return location.range;
        };

        let range = crate::convert::range_from_lsp(&source, location.range);
        let Some(text) = source.text().get(range.clone()) else { return location.range };

        let start = range.start + text.len() - text.trim_start_matches(['<', '@']).len();
        let end = range.end - (text.len() - text.trim_end_matches('>').len());
        range_to_lsp(&source, start..end)
    }

    /// The message explaining why this symbol cannot be renamed, or `None` if
    /// it can be.
    fn refusal(&self, source: &Source, cursor: usize, target: &Target) -> Option<String> {
        if let Target::Std(name) = target {
            return Some(format!(
                "Cannot rename `{name}`: it is a standard library item"
            ));
        }

        match super::references::definition_origin(self, source, cursor) {
            Some(Definition::Std(_)) => {
                Some("Cannot rename a standard library item".to_string())
            }
            Some(Definition::Span(span)) => {
                let file = span.id()?;
                match file.get().root() {
                    VirtualRoot::Package(spec) => Some(format!(
                        "Cannot rename an item defined in package `{spec}` — package \
                         sources are read-only"
                    )),
                    VirtualRoot::Project => None,
                }
            }
            Some(Definition::File(file)) => match file.get().root() {
                VirtualRoot::Package(spec) => {
                    Some(format!("Cannot rename a file in package `{spec}`"))
                }
                VirtualRoot::Project => Some(
                    "Cannot rename a file from here — rename it in the explorer instead"
                        .to_string(),
                ),
            },
            None => match target {
                // A binding we resolved but whose definition typst-ide cannot
                // place is, by construction, outside the compile graph.
                Target::Binding { name, .. } => Some(format!(
                    "Cannot rename: `{name}` is not reachable from the current main file"
                )),
                _ => None,
            },
        }
    }
}

fn enclosing_label_range(leaf: &LinkedNode) -> Option<std::ops::Range<usize>> {
    let mut node = Some(leaf.clone());
    while let Some(current) = node {
        if matches!(current.kind(), SyntaxKind::Label | SyntaxKind::Ref) {
            return Some(current.range());
        }
        node = current.parent().cloned();
    }
    None
}
