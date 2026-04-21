//! Completion: context-aware suggestions for keywords, scalar types, and
//! symbols visible from the current file.

use super::position::enclosing_scope_at;
use crate::resolve::{SymbolKind, WorkspaceIndex};
use crate::vfs::{FileUri, Workspace};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CompletionKind {
    Keyword,
    Scalar,
    Message,
    Enum,
    EnumValue,
    Field,
    Service,
    Rpc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    pub insert_text: String,
    pub kind: CompletionKind,
    pub detail: String,
}

const KEYWORDS: &[&str] = &[
    "syntax", "edition", "package", "import", "public", "weak",
    "option", "message", "enum", "service", "rpc", "returns",
    "stream", "oneof", "map", "reserved", "to", "max", "repeated",
    "optional", "required", "extensions", "extend", "true", "false",
];

const SCALARS: &[&str] = &[
    "double", "float", "int32", "int64", "uint32", "uint64",
    "sint32", "sint64", "fixed32", "fixed64", "sfixed32", "sfixed64",
    "bool", "string", "bytes",
];

pub fn completion(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
    offset: u32,
) -> Vec<CompletionItem> {
    let mut out: Vec<CompletionItem> = Vec::new();

    for kw in KEYWORDS {
        out.push(CompletionItem {
            label: (*kw).into(),
            insert_text: (*kw).into(),
            kind: CompletionKind::Keyword,
            detail: "keyword".into(),
        });
    }
    for sc in SCALARS {
        out.push(CompletionItem {
            label: (*sc).into(),
            insert_text: (*sc).into(),
            kind: CompletionKind::Scalar,
            detail: "scalar type".into(),
        });
    }

    let Some(pf) = ws.file(uri) else { return out };
    let scope = enclosing_scope_at(&pf.ast, offset);

    let Some(visible) = index.visible_files(uri) else { return out };
    for sym in index.all_symbols() {
        if !visible.contains(&sym.file) {
            continue;
        }
        let kind = match sym.kind {
            SymbolKind::Message => CompletionKind::Message,
            SymbolKind::Enum => CompletionKind::Enum,
            SymbolKind::EnumValue => CompletionKind::EnumValue,
            SymbolKind::Field => CompletionKind::Field,
            SymbolKind::Service => CompletionKind::Service,
            SymbolKind::Rpc => CompletionKind::Rpc,
            SymbolKind::Oneof => continue,
        };
        // Prefer the shortest in-scope form: if the symbol's FQN shares
        // the scope prefix, offer the tail; otherwise offer the FQN.
        let insert = shortest_reference(&scope, &sym.fqn);
        out.push(CompletionItem {
            label: sym.name.to_string(),
            insert_text: insert,
            kind,
            detail: sym.fqn.to_string(),
        });
    }
    out
}

fn shortest_reference(scope: &str, fqn: &str) -> String {
    if scope.is_empty() {
        return fqn.to_string();
    }
    let prefix = format!("{}.", scope);
    if let Some(rest) = fqn.strip_prefix(&prefix) {
        return rest.to_string();
    }
    // Look for any ancestor scope that would let us shorten.
    let mut cur = scope;
    while let Some(i) = cur.rfind('.') {
        cur = &cur[..i];
        let prefix = format!("{}.", cur);
        if let Some(rest) = fqn.strip_prefix(&prefix) {
            return rest.to_string();
        }
    }
    fqn.to_string()
}
