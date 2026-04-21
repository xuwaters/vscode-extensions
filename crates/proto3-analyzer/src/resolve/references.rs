//! Reverse index: for every resolved type-use site, record a pointer back
//! from the definition's FQN to the (file, span) where the reference
//! appears. Drives find-references and rename.

use super::{collect_type_use_sites, Resolution, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use rustc_hash::FxHashMap;
use smol_str::SmolStr;

#[derive(Debug, Clone)]
pub struct RefSite {
    pub file: FileUri,
    pub span: ByteSpan,
}

#[derive(Debug, Default, Clone)]
pub struct ReferenceIndex {
    by_fqn: FxHashMap<SmolStr, Vec<RefSite>>,
}

impl ReferenceIndex {
    pub fn build(ws: &Workspace, index: &WorkspaceIndex) -> Self {
        let mut by_fqn: FxHashMap<SmolStr, Vec<RefSite>> = FxHashMap::default();
        for (uri, pf) in ws.files() {
            let sites = collect_type_use_sites(&pf.ast);
            for site in sites {
                if let Resolution::Found { symbol, .. } =
                    index.resolve_type(uri, site.enclosing_scope.as_str(), &site.name)
                {
                    by_fqn.entry(symbol.fqn.clone()).or_default().push(RefSite {
                        file: uri.clone(),
                        span: site.span,
                    });
                }
            }
        }
        ReferenceIndex { by_fqn }
    }

    pub fn references(&self, fqn: &str) -> &[RefSite] {
        self.by_fqn.get(fqn).map(|v| v.as_slice()).unwrap_or(&[])
    }
}
