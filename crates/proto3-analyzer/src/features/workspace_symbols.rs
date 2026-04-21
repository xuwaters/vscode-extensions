//! Flat list of workspace-wide symbols for fuzzy search.

use crate::resolve::{workspace_symbols, Symbol};
use crate::vfs::Workspace;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSymbolItem {
    pub name: String,
    pub fqn: String,
    pub kind: String,
    pub file: String,
    pub range: crate::spans::ByteSpan,
    pub detail: Option<String>,
}

pub fn collect(ws: &Workspace) -> Vec<WorkspaceSymbolItem> {
    workspace_symbols(ws).into_iter().map(into_item).collect()
}

fn into_item(s: Symbol) -> WorkspaceSymbolItem {
    WorkspaceSymbolItem {
        name: s.name.to_string(),
        fqn: s.fqn.to_string(),
        kind: format!("{:?}", s.kind),
        file: s.file.0,
        range: s.name_span,
        detail: s.detail,
    }
}
