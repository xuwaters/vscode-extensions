//! Completion. Two contexts:
//!
//! - **Type position** (cursor follows `(`, `,`, or `<`): propose builtin
//!   types plus every user type visible from the current file.
//! - **Declaration position** (start of file / after `;`): propose the
//!   declaration keywords.
//!
//! Context detection is byte-level and cheap: we look at the source bytes
//! just before the cursor rather than threading a context through the parser.

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

const BUILTINS: &[&str] = &[
    "bool", "int8", "uint8", "int16", "uint16", "int32", "uint32", "int64", "uint64", "float",
    "double", "string", "handle", "array", "map", "pending_remote", "pending_receiver",
    "pending_associated_remote", "pending_associated_receiver", "associated",
];

const KEYWORDS: &[&str] = &["module", "import", "struct", "union", "interface", "enum", "const"];

pub fn completion(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
    offset: u32,
) -> Vec<CompletionItem> {
    let Some(state) = ws.file(uri) else { return Vec::new() };
    let ctx = classify(&state.source, offset);

    match ctx {
        Context::Type => type_completions(index, uri),
        Context::Decl => decl_completions(),
        Context::Unknown => {
            let mut out = decl_completions();
            out.extend(type_completions(index, uri));
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
    while i > 0 && is_ident_continue(bytes[i - 1]) {
        i -= 1;
    }
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
        i -= 1;
    }
    if i == 0 {
        return Context::Decl;
    }
    match bytes[i - 1] {
        b'(' | b',' | b'<' => Context::Type,
        _ => Context::Unknown,
    }
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn type_completions(index: &WorkspaceIndex, uri: &FileUri) -> Vec<CompletionItem> {
    let mut out: Vec<CompletionItem> = BUILTINS
        .iter()
        .map(|n| CompletionItem {
            label: (*n).into(),
            insert_text: (*n).into(),
            kind: "type",
            detail: "builtin type".into(),
        })
        .collect();

    let module = index
        .file_symbols(uri)
        .map(|fs| fs.module.to_string())
        .unwrap_or_default();
    let visible = index.visible_files(uri);

    for sym in index.all_symbols() {
        if !sym.kind.is_type() {
            continue;
        }
        let Some(vis) = visible else { continue };
        if !vis.contains(&sym.file) {
            continue;
        }
        out.push(CompletionItem {
            label: sym.name.to_string(),
            insert_text: short_insert_text(&sym.fqn, &module),
            kind: kind_word(sym.kind),
            detail: sym.fqn.to_string(),
        });
    }
    out
}

/// Prefer the bare name when the symbol lives in the file's own module,
/// otherwise insert the module-qualified name.
fn short_insert_text(fqn: &str, module: &str) -> String {
    if module.is_empty() {
        return fqn.to_string();
    }
    let prefix = format!("{}.", module);
    if let Some(rest) = fqn.strip_prefix(&prefix) {
        return rest.to_string();
    }
    fqn.to_string()
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
        SymbolKind::Union => "struct",
        SymbolKind::Interface => "interface",
        SymbolKind::Enum => "enum",
        _ => "type",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::WorkspaceIndex;
    use crate::vfs::Workspace;

    #[test]
    fn proposes_user_types_in_generic_position() {
        let mut ws = Workspace::new();
        ws.update("file:///a.mojom", "struct Foo { int32 id; };\nstruct Bar { array< };".into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.mojom".into());
        let src = ws.get("file:///a.mojom").unwrap().source.clone();
        let offset = (src.find("array<").unwrap() + "array<".len()) as u32;
        let items = completion(&ws, &idx, &uri, offset);
        assert!(items.iter().any(|i| i.label == "Foo"));
        assert!(items.iter().any(|i| i.label == "int32"));
    }

    #[test]
    fn proposes_keywords_at_top_level() {
        let mut ws = Workspace::new();
        ws.update("file:///a.mojom", String::new());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.mojom".into());
        let items = completion(&ws, &idx, &uri, 0);
        assert!(items.iter().any(|i| i.label == "interface"));
    }
}
