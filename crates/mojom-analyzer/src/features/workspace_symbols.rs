//! Workspace symbol search. Returns every declared symbol across every loaded
//! file so the user's `Ctrl+T` query can fuzzy-match over FQNs.

use crate::resolve::{SymbolKind, WorkspaceIndex};
use crate::spans::ByteSpan;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceSymbolItem {
    pub name: String,
    pub fqn: String,
    pub kind: &'static str,
    pub file: String,
    pub range: ByteSpan,
    pub detail: Option<String>,
}

pub fn workspace_symbols(index: &WorkspaceIndex, query: &str) -> Vec<WorkspaceSymbolItem> {
    let q = query.to_lowercase();
    index
        .all_symbols()
        .filter(|s| q.is_empty() || s.fqn.to_lowercase().contains(&q))
        .map(|s| WorkspaceSymbolItem {
            name: s.name.to_string(),
            fqn: s.fqn.to_string(),
            kind: kind_word(s.kind),
            file: s.file.as_str().into(),
            range: s.name_span,
            detail: s.detail.clone(),
        })
        .collect()
}

fn kind_word(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Module => "namespace",
        SymbolKind::Struct => "struct",
        SymbolKind::Union => "struct",
        SymbolKind::Interface => "interface",
        SymbolKind::Enum => "enum",
        SymbolKind::EnumValue => "enumMember",
        SymbolKind::Const => "constant",
        SymbolKind::Field => "field",
        SymbolKind::Method => "method",
    }
}
