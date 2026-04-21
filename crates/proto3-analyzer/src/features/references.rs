//! Find-references: given a cursor position, return every place the
//! symbol at that position is referenced — including the definition site
//! if requested.

use super::position::type_use_at;
use crate::resolve::{ReferenceIndex, Resolution, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub file: String,
    pub range: ByteSpan,
}

pub fn references(
    ws: &Workspace,
    index: &WorkspaceIndex,
    ref_index: &ReferenceIndex,
    uri: &FileUri,
    offset: u32,
    include_declaration: bool,
) -> Vec<Reference> {
    let Some(pf) = ws.file(uri) else { return Vec::new() };
    let site = match type_use_at(&pf.ast, offset) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let symbol = match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.name) {
        Resolution::Found { symbol, .. } => symbol,
        Resolution::Unknown { .. } => return Vec::new(),
    };

    let mut out: Vec<Reference> = ref_index
        .references(&symbol.fqn)
        .iter()
        .map(|r| Reference { file: r.file.as_str().to_string(), range: r.span })
        .collect();

    if include_declaration {
        out.push(Reference {
            file: symbol.file.as_str().to_string(),
            range: symbol.name_span,
        });
    }
    out
}
