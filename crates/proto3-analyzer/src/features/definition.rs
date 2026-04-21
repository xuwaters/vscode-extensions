//! Go-to-definition: map a cursor position to the name span of the defining
//! symbol (possibly in a different file).

use super::position::type_use_at;
use crate::resolve::{Resolution, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    let pf = ws.file(uri)?;
    let site = type_use_at(&pf.ast, offset)?;
    match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.name) {
        Resolution::Found { symbol, .. } => Some(Location {
            file: symbol.file.as_str().to_string(),
            range: symbol.name_span,
        }),
        Resolution::Unknown { .. } => None,
    }
}
