//! Go-to-definition. Resolves the type reference at `offset` to a symbol
//! and returns its declaration location. Falls back to the enclosing file
//! for bare file aliases (`Cxx` in `Cxx.foo`).

use super::position::type_use_at;
use crate::resolve::{Resolution, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Location {
    pub file: String,
    pub range: ByteSpan,
}

pub fn definition(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
    offset: u32,
) -> Option<Location> {
    let file = &ws.file(uri)?.analysis.file;
    let site = type_use_at(file, offset)?;
    match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.path) {
        Resolution::Found { symbol, .. } => Some(Location {
            file: symbol.file.as_str().into(),
            range: symbol.name_span,
        }),
        Resolution::FileAlias { file, .. } => Some(Location {
            file: file.as_str().into(),
            range: ByteSpan::EMPTY,
        }),
        Resolution::Unknown { .. } => None,
    }
}
