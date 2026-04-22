//! Virtual file system and workspace — tracks parsed files, import graph,
//! and include-path resolution.

use crate::diagnostics::{
    run_all_checks, run_resolve_checks, run_style_checks, DiagnosticCode, ProtoDiagnostic,
    Severity, StyleConfig,
};
use crate::parse::{parse, ParsedFile};
use crate::resolve::WorkspaceIndex;
use crate::spans::ByteSpan;
use crate::well_known;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

/// A URI-like identifier for a file. We don't require a real `file://` —
/// the extension host can pass any canonical string. Well-known types use
/// a dedicated `proto3-wkt:` scheme.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileUri(pub String);

impl FileUri {
    pub fn new(s: impl Into<String>) -> Self {
        FileUri(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for FileUri {
    fn from(s: &str) -> Self {
        FileUri(s.to_string())
    }
}

impl From<String> for FileUri {
    fn from(s: String) -> Self {
        FileUri(s)
    }
}

/// An include-path entry. Matches `protoc -I ...` semantics.
#[derive(Debug, Clone)]
pub struct IncludePath(pub String);

#[derive(Debug, Clone, Serialize)]
pub struct ChangedFiles {
    pub affected: Vec<FileUri>,
}

pub struct Workspace {
    files: FxHashMap<FileUri, ParsedFile>,
    include_paths: Vec<IncludePath>,
    reverse_imports: FxHashMap<FileUri, FxHashSet<FileUri>>,
    style: StyleConfig,
}

impl Default for Workspace {
    fn default() -> Self {
        Self::with_bundled_well_known_types()
    }
}

impl Workspace {
    pub fn new() -> Self {
        Workspace {
            files: FxHashMap::default(),
            include_paths: Vec::new(),
            reverse_imports: FxHashMap::default(),
            style: StyleConfig::default(),
        }
    }

    pub fn set_style_config(&mut self, cfg: StyleConfig) {
        self.style = cfg;
    }

    pub fn with_bundled_well_known_types() -> Self {
        let mut ws = Self::new();
        for (rel, src) in well_known::all() {
            let uri = FileUri::new(format!("proto3-wkt:/{}", rel));
            let parsed = parse(uri.clone(), src.to_string());
            ws.files.insert(uri, parsed);
        }
        ws
    }

    pub fn set_include_paths(&mut self, paths: Vec<String>) {
        self.include_paths = paths.into_iter().map(IncludePath).collect();
    }

    pub fn include_paths(&self) -> &[IncludePath] {
        &self.include_paths
    }

    pub fn files(&self) -> impl Iterator<Item = (&FileUri, &ParsedFile)> {
        self.files.iter()
    }

    pub fn file(&self, uri: &FileUri) -> Option<&ParsedFile> {
        self.files.get(uri)
    }

    pub fn update_file(&mut self, uri: FileUri, source: String) -> ChangedFiles {
        let previous_imports: Vec<String> = self
            .files
            .get(&uri)
            .map(|f| f.imports().map(str::to_string).collect())
            .unwrap_or_default();

        let parsed = parse(uri.clone(), source);
        self.files.insert(uri.clone(), parsed);
        self.rebuild_reverse_imports(&uri, previous_imports);
        ChangedFiles {
            affected: self.transitive_dependents(&uri),
        }
    }

    pub fn remove_file(&mut self, uri: &FileUri) {
        let previous_imports: Vec<String> = self
            .files
            .get(uri)
            .map(|f| f.imports().map(str::to_string).collect())
            .unwrap_or_default();
        for prev in previous_imports {
            if let Some(resolved) = self.resolve_import_path(uri, &prev) {
                if let Some(set) = self.reverse_imports.get_mut(&resolved) {
                    set.remove(uri);
                }
            }
        }
        self.files.remove(uri);
    }

    fn rebuild_reverse_imports(&mut self, uri: &FileUri, previous: Vec<String>) {
        for prev in &previous {
            if let Some(resolved) = self.resolve_import_path(uri, prev) {
                if let Some(set) = self.reverse_imports.get_mut(&resolved) {
                    set.remove(uri);
                }
            }
        }
        let current: Vec<String> = self
            .files
            .get(uri)
            .map(|f| f.imports().map(str::to_string).collect())
            .unwrap_or_default();
        for imp in current {
            if let Some(resolved) = self.resolve_import_path(uri, &imp) {
                self.reverse_imports.entry(resolved).or_default().insert(uri.clone());
            }
        }
    }

    fn transitive_dependents(&self, uri: &FileUri) -> Vec<FileUri> {
        let mut out = vec![uri.clone()];
        let mut stack = vec![uri.clone()];
        let mut seen: FxHashSet<FileUri> = FxHashSet::default();
        seen.insert(uri.clone());
        while let Some(u) = stack.pop() {
            if let Some(set) = self.reverse_imports.get(&u) {
                for dep in set {
                    if seen.insert(dep.clone()) {
                        out.push(dep.clone());
                        stack.push(dep.clone());
                    }
                }
            }
        }
        out
    }

    /// Resolve `import "path"` to a known file URI, using include paths,
    /// the importer's own directory, and bundled well-known types.
    pub fn resolve_import_path(&self, importer: &FileUri, path: &str) -> Option<FileUri> {
        // Well-known first-match shortcut for `google/protobuf/*.proto`.
        let wkt_uri = FileUri::new(format!("proto3-wkt:/{}", path));
        if self.files.contains_key(&wkt_uri) {
            return Some(wkt_uri);
        }
        for inc in &self.include_paths {
            let probe = FileUri::new(format!("{}/{}", inc.0.trim_end_matches('/'), path));
            if self.files.contains_key(&probe) {
                return Some(probe);
            }
        }
        // Heuristic: match by suffix against known files.
        let suffix = format!("/{}", path);
        for uri in self.files.keys() {
            if uri.0.ends_with(&suffix) || uri.0.ends_with(path) {
                if uri != importer {
                    return Some(uri.clone());
                }
            }
        }
        None
    }

    /// Collect diagnostics for a single file: parse-time + semantic
    /// (duplicate field numbers, reserved clashes, oneof, map-key) +
    /// import resolution + cross-file name resolution.
    pub fn diagnostics_for(&self, uri: &FileUri) -> Vec<ProtoDiagnostic> {
        let Some(file) = self.files.get(uri) else { return Vec::new() };
        let mut out = file.diagnostics.clone();
        out.extend(run_all_checks(&file.ast));
        out.extend(self.import_diagnostics(uri, file));
        let index = WorkspaceIndex::build(self);
        out.extend(run_resolve_checks(self, &index, uri));
        out.extend(run_style_checks(&file.ast, self.style));
        out
    }

    /// Build a fresh workspace symbol index. Callers that need both
    /// diagnostics and feature queries should cache this and reuse it to
    /// avoid rebuilding per query.
    pub fn build_index(&self) -> WorkspaceIndex {
        WorkspaceIndex::build(self)
    }

    fn import_diagnostics(&self, uri: &FileUri, file: &ParsedFile) -> Vec<ProtoDiagnostic> {
        let mut out = Vec::new();
        for imp in &file.ast.imports {
            if self.resolve_import_path(uri, &imp.path).is_none() {
                out.push(ProtoDiagnostic::new(
                    DiagnosticCode::ImportUnresolved,
                    Severity::Error,
                    format!("Cannot resolve import `\"{}\"`", imp.path),
                    imp.path_span,
                ));
            }
        }
        out
    }
}

#[allow(dead_code)]
pub(crate) fn combined_span(a: ByteSpan, b: ByteSpan) -> ByteSpan {
    a.join(b)
}
