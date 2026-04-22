//! Minimal virtual filesystem — a map from file URI to its parsed state.
//!
//! Makefile analysis is single-file: we do not follow `include` directives
//! during parsing (resolving those is left to Phase 2). The workspace
//! therefore exists only to hold the `ParsedFile`s the editor has pushed
//! at us via `update_file`.

use crate::diagnostics::MakeDiagnostic;
use crate::parse::{self, ParsedFile};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileUri(String);

impl FileUri {
    pub fn new(uri: impl Into<String>) -> Self {
        FileUri(uri.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct Workspace {
    files: HashMap<FileUri, ParsedFile>,
}

impl Workspace {
    pub fn new() -> Self {
        Workspace { files: HashMap::new() }
    }

    pub fn update_file(&mut self, uri: FileUri, source: String) {
        let parsed = parse::parse(uri.clone(), source);
        self.files.insert(uri, parsed);
    }

    pub fn remove_file(&mut self, uri: &FileUri) {
        self.files.remove(uri);
    }

    pub fn file(&self, uri: &FileUri) -> Option<&ParsedFile> {
        self.files.get(uri)
    }

    pub fn diagnostics_for(&self, uri: &FileUri) -> Vec<MakeDiagnostic> {
        self.files
            .get(uri)
            .map(|f| f.diagnostics.clone())
            .unwrap_or_default()
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}
