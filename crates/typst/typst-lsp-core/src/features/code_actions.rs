//! Code actions: a small curated set, each tied to something the cursor or a
//! diagnostic is actually pointing at.
//!
//! Deliberately not a grab bag. Four actions that come up constantly while
//! writing typst, and nothing speculative.

use std::collections::HashMap;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse,
    TextEdit, Uri, WorkspaceEdit,
};
use typst::syntax::{LinkedNode, Side, Source, SyntaxKind};

use crate::convert::{range_from_lsp, range_to_lsp};
use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// `textDocument/codeAction`.
    pub fn code_actions(&mut self, params: CodeActionParams) -> Option<CodeActionResponse> {
        let uri = params.text_document.uri.clone();
        let (_, source) = self.source_of(&uri)?;
        let selection = range_from_lsp(&source, params.range);

        let mut actions = Vec::new();

        if let Some(action) = add_missing_import(&source, &params, &uri) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
        if let Some(action) = wrap_in_code_block(&source, &selection, &uri) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
        if let Some(action) = string_to_content_block(&source, &selection, &uri) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
        if let Some(action) = add_label_to_heading(&source, &selection, &uri) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }

        Some(actions)
    }
}

/// `WorkspaceEdit::changes` is keyed by `Uri`, which clippy flags as a mutable
/// key type because `fluent_uri` caches inside it. The map's shape is the
/// protocol's rather than ours, and a key is never mutated once inserted.
#[allow(clippy::mutable_key_type)]
fn edit(uri: &Uri, edits: Vec<TextEdit>) -> WorkspaceEdit {
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    WorkspaceEdit { changes: Some(changes), ..WorkspaceEdit::default() }
}

fn action(title: &str, uri: &Uri, edits: Vec<TextEdit>, preferred: bool) -> CodeAction {
    CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(edit(uri, edits)),
        is_preferred: preferred.then_some(true),
        ..CodeAction::default()
    }
}

/// "unknown variable: foo" on a call is usually a forgotten import; offer the
/// import line at the top of the file.
fn add_missing_import(
    source: &Source,
    params: &CodeActionParams,
    uri: &Uri,
) -> Option<CodeAction> {
    let diagnostic = params
        .context
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.starts_with("unknown variable:"))?;

    let name = diagnostic.message.strip_prefix("unknown variable:")?.trim();
    // A name with a hyphen is the "meant subtraction" case upstream hints at,
    // not a missing import.
    if name.is_empty() || name.contains(' ') {
        return None;
    }

    let insert_at = crate::convert::offset_to_position(source, 0);
    Some(action(
        &format!("Import `{name}` from a module"),
        uri,
        vec![TextEdit {
            range: lsp_types::Range { start: insert_at, end: insert_at },
            new_text: format!("#import \"\": {name}\n"),
        }],
        false,
    ))
}

/// Wrap the selection in `#{…}` so markup becomes code.
fn wrap_in_code_block(
    source: &Source,
    selection: &std::ops::Range<usize>,
    uri: &Uri,
) -> Option<CodeAction> {
    if selection.is_empty() {
        return None;
    }
    let text = source.text().get(selection.clone())?;

    Some(action(
        "Wrap in a code block `#{…}`",
        uri,
        vec![TextEdit {
            range: range_to_lsp(source, selection.clone()),
            new_text: format!("#{{{text}}}"),
        }],
        false,
    ))
}

/// Turn a string literal into a content block: `"text"` → `[text]`.
fn string_to_content_block(
    source: &Source,
    selection: &std::ops::Range<usize>,
    uri: &Uri,
) -> Option<CodeAction> {
    let root = LinkedNode::new(source.root());
    let leaf = root.leaf_at(selection.start, Side::After)?;

    let mut node = Some(leaf);
    let string = loop {
        let current = node?;
        if current.kind() == SyntaxKind::Str {
            break current;
        }
        node = current.parent().cloned();
    };

    let raw = source.text().get(string.range())?;
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;

    Some(action(
        "Convert to a content block `[…]`",
        uri,
        vec![TextEdit {
            range: range_to_lsp(source, string.range()),
            new_text: format!("[{inner}]"),
        }],
        false,
    ))
}

/// Add `<label>` to the heading the cursor is on, so it can be referenced.
fn add_label_to_heading(
    source: &Source,
    selection: &std::ops::Range<usize>,
    uri: &Uri,
) -> Option<CodeAction> {
    let root = LinkedNode::new(source.root());
    let leaf = root.leaf_at(selection.start, Side::Before)?;

    let mut node = Some(leaf);
    let heading = loop {
        let current = node?;
        if current.kind() == SyntaxKind::Heading {
            break current;
        }
        node = current.parent().cloned();
    };

    let text = source.text().get(heading.range())?;
    if text.contains('<') {
        return None;
    }

    let slug: String = text
        .trim_start_matches('=')
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').replace("--", "-");
    if slug.is_empty() {
        return None;
    }

    let end = heading.range().end;
    let position = crate::convert::offset_to_position(source, end);
    Some(action(
        &format!("Add label `<{slug}>`"),
        uri,
        vec![TextEdit {
            range: lsp_types::Range { start: position, end: position },
            new_text: format!(" <{slug}>"),
        }],
        false,
    ))
}
