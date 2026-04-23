//! Completion. Two contexts:
//!
//! - **Type position** (cursor follows `:` or sits inside `List(`): propose
//!   built-in scalar types plus every type symbol visible from the current
//!   file.
//! - **Top-level / declaration position**: propose schema keywords.
//!
//! Context detection is byte-level and cheap: we look at the source bytes
//! just before the cursor rather than threading a context through the
//! parser.

use crate::ast::File;
use crate::features::position::enclosing_scope_at;
use crate::resolve::{SymbolKind, WorkspaceIndex};
use crate::vfs::{FileUri, Workspace};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CompletionItem {
    pub label: String,
    pub insert_text: String,
    pub kind: &'static str,
    pub detail: String,
}

const SCALARS: &[&str] = &[
    "Void", "Bool", "Int8", "Int16", "Int32", "Int64", "UInt8", "UInt16", "UInt32", "UInt64",
    "Float32", "Float64", "Text", "Data", "List", "AnyPointer", "Capability",
];

const KEYWORDS: &[&str] = &[
    "struct", "enum", "interface", "const", "annotation", "using", "import", "union", "group",
    "extends",
];

pub fn completion(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
    offset: u32,
) -> Vec<CompletionItem> {
    let Some(state) = ws.file(uri) else { return Vec::new(); };
    let ctx = classify(&state.source, offset);

    match ctx {
        Context::Type => type_completions(index, uri, &state.analysis.file, offset),
        Context::Decl => decl_completions(),
        Context::Unknown => {
            let mut out = decl_completions();
            out.extend(type_completions(index, uri, &state.analysis.file, offset));
            out
        }
    }
}

enum Context {
    Type,
    Decl,
    Unknown,
}

fn classify(source: &str, offset: u32) -> Context {
    let bytes = source.as_bytes();
    let mut i = (offset as usize).min(bytes.len());
    // Skip the identifier the user is typing.
    while i > 0 && is_ident_continue(bytes[i - 1]) {
        i -= 1;
    }
    // Skip whitespace.
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
        i -= 1;
    }
    if i == 0 {
        return Context::Decl;
    }
    match bytes[i - 1] {
        b':' | b'(' | b',' => Context::Type,
        b'{' | b';' | b'}' | b'\n' => Context::Decl,
        _ => Context::Unknown,
    }
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn type_completions(
    index: &WorkspaceIndex,
    uri: &FileUri,
    file: &File,
    offset: u32,
) -> Vec<CompletionItem> {
    let mut out: Vec<CompletionItem> = SCALARS
        .iter()
        .map(|n| CompletionItem {
            label: (*n).into(),
            insert_text: (*n).into(),
            kind: "type",
            detail: "scalar type".into(),
        })
        .collect();

    let scope = enclosing_scope_at(file, offset);
    let visible = index.visible_files(uri);

    for sym in index.all_symbols() {
        if !sym.kind.is_type() {
            continue;
        }
        let Some(vis) = visible else { continue };
        if !vis.contains(&sym.file) {
            continue;
        }
        let label = sym.name.to_string();
        let insert = short_insert_text(&sym.fqn, &scope);
        out.push(CompletionItem {
            label,
            insert_text: insert,
            kind: kind_word(sym.kind),
            detail: sym.fqn.to_string(),
        });
    }
    out
}

/// Prefer the shortest suffix of `fqn` that resolves unambiguously from
/// `scope`. Simple heuristic: if `fqn` starts with `scope.`, strip it.
fn short_insert_text(fqn: &str, scope: &str) -> String {
    if scope.is_empty() {
        return fqn.to_string();
    }
    let prefix = format!("{}.", scope);
    if let Some(rest) = fqn.strip_prefix(&prefix) {
        return rest.to_string();
    }
    fqn.rsplit('.').next().unwrap_or(fqn).to_string()
}

fn decl_completions() -> Vec<CompletionItem> {
    KEYWORDS
        .iter()
        .map(|k| CompletionItem {
            label: (*k).into(),
            insert_text: (*k).into(),
            kind: "keyword",
            detail: "keyword".into(),
        })
        .collect()
}

fn kind_word(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Interface => "interface",
        _ => "type",
    }
}
