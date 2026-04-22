//! Inlay hints: show the fully-qualified resolved name after a short
//! user-type reference, so readers can see which message a bare
//! identifier refers to without jumping to the definition.

use crate::resolve::{collect_type_use_sites, Resolution, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlayHint {
    /// Position (end of the type reference) where the hint should render.
    pub at: ByteSpan,
    pub label: String,
}

pub fn inlay_hints(ws: &Workspace, index: &WorkspaceIndex, uri: &FileUri) -> Vec<InlayHint> {
    let Some(pf) = ws.file(uri) else { return Vec::new() };
    let mut out = Vec::new();
    for site in collect_type_use_sites(&pf.ast) {
        // Only hint for unqualified, single-segment references — the ones
        // where the FQN isn't obvious in source.
        if site.name.absolute || site.name.parts.len() != 1 {
            continue;
        }
        let Resolution::Found { symbol, visibility_ok } =
            index.resolve_type(uri, site.enclosing_scope.as_str(), &site.name)
        else { continue };
        if !visibility_ok {
            continue;
        }
        // Don't hint when the symbol is defined in this same file — users
        // already know where local definitions live.
        if &symbol.file == uri {
            continue;
        }
        out.push(InlayHint {
            at: ByteSpan::new(site.span.end, site.span.end),
            label: format!(": .{}", symbol.fqn),
        });
    }
    out
}
