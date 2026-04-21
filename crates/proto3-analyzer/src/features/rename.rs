//! Rename: produce a [`WorkspaceEdit`] that renames every occurrence of
//! the symbol at the cursor (definition + type-reference sites).
//!
//! Only the trailing identifier is renamed. For a reference like
//! `pkg.Outer.Inner` we replace just the last component when the FQN
//! tail matches the definition's short name. Otherwise the whole span
//! is replaced — this matches what users expect from `protoc` rename.

use super::position::{field_at_name, type_use_at};
use crate::resolve::{collect_type_use_sites, Resolution, Symbol, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: ByteSpan,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceEdit {
    /// file URI -> ordered edits (non-overlapping, sorted descending by start
    /// so appliers can edit in-place without shifting later ranges).
    pub changes: FxHashMap<String, Vec<TextEdit>>,
}

/// Validate that the cursor is on a renameable identifier and return the
/// span VS Code should use to prime its rename widget.
pub fn prepare_rename(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
    offset: u32,
) -> Option<ByteSpan> {
    let pf = ws.file(uri)?;
    if let Some((field, _)) = field_at_name(&pf.ast, offset) {
        return Some(field.name.span);
    }
    let site = type_use_at(&pf.ast, offset)?;
    match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.name) {
        Resolution::Found { symbol, .. } => Some(tail_span(&site.name, &symbol)),
        Resolution::Unknown { .. } => None,
    }
}

pub fn rename(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
    offset: u32,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    if !is_valid_ident(new_name) {
        return None;
    }
    let pf = ws.file(uri)?;

    // Field rename path: edit the definition's name span only. (Field
    // references by name don't exist in proto3 outside text-format option
    // literals, which we leave to Phase 4.)
    if let Some((field, _parent_fqn)) = field_at_name(&pf.ast, offset) {
        let mut edit = WorkspaceEdit::default();
        edit.changes.insert(
            uri.as_str().to_string(),
            vec![TextEdit { range: field.name.span, new_text: new_name.to_string() }],
        );
        return Some(edit);
    }

    // Type rename path: find the symbol, then rewrite the tail of every
    // resolved type-reference to match the new name.
    let site = type_use_at(&pf.ast, offset)?;
    let symbol = match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.name) {
        Resolution::Found { symbol, .. } => symbol,
        Resolution::Unknown { .. } => return None,
    };

    let mut edit = WorkspaceEdit::default();
    // Definition site — replace the name span directly.
    edit.changes
        .entry(symbol.file.as_str().to_string())
        .or_default()
        .push(TextEdit { range: symbol.name_span, new_text: new_name.to_string() });

    // Walk every file's type-use sites and collect matching tails.
    for (other_uri, other_pf) in ws.files() {
        let sites = collect_type_use_sites(&other_pf.ast);
        for s in sites {
            let Resolution::Found { symbol: resolved, .. } =
                index.resolve_type(other_uri, s.enclosing_scope.as_str(), &s.name)
            else {
                continue;
            };
            if resolved.fqn != symbol.fqn {
                continue;
            }
            edit.changes
                .entry(other_uri.as_str().to_string())
                .or_default()
                .push(TextEdit { range: tail_span(&s.name, &symbol), new_text: new_name.to_string() });
        }
    }

    for edits in edit.changes.values_mut() {
        edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
        edits.dedup_by(|a, b| a.range == b.range);
    }
    Some(edit)
}

/// The span covering just the short-name tail of `name` — the part that
/// corresponds to the symbol's own identifier.
fn tail_span(name: &crate::ast::QualifiedName, symbol: &Symbol) -> ByteSpan {
    match name.parts.last() {
        Some(last) if last.name == symbol.name => last.span,
        _ => name.span,
    }
}

fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
