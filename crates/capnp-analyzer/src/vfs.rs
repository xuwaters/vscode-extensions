//! In-memory workspace: uri → (source, parsed file, diagnostics). Future
//! iterations will extend this with include-path handling and a symbol index
//! for cross-file lookups.

use crate::diagnostics::{analyze, Analysis};
use rustc_hash::FxHashMap;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct FileUri(pub String);

pub struct Workspace {
    files: FxHashMap<FileUri, FileState>,
}

pub struct FileState {
    pub source: String,
    pub analysis: Analysis,
}

impl Workspace {
    pub fn new() -> Self {
        Workspace { files: FxHashMap::default() }
    }

    pub fn update(&mut self, uri: &str, source: String) {
        let analysis = analyze(&source);
        self.files.insert(FileUri(uri.into()), FileState { source, analysis });
    }

    pub fn remove(&mut self, uri: &str) {
        self.files.remove(&FileUri(uri.into()));
    }

    pub fn get(&self, uri: &str) -> Option<&FileState> {
        self.files.get(&FileUri(uri.into()))
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}
