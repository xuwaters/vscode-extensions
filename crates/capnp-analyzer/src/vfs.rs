//! In-memory workspace: uri → (source, parsed file, diagnostics) plus
//! include-path resolution for `import "…"` strings that appear inside
//! `using X = import "…";` aliases.
//!
//! Each [`FileState`] owns its raw source (needed for doc-comment
//! extraction) and its parse analysis. The workspace also tracks a
//! reverse-import graph so a file update can return the set of files that
//! need to be re-diagnosed.

use crate::diagnostics::{analyze, Analysis};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileUri(pub String);

impl FileUri {
    pub fn as_str(&self) -> &str { &self.0 }
}

#[derive(Debug, Clone)]
pub struct IncludePath(pub String);

pub struct FileState {
    pub source: String,
    pub analysis: Analysis,
}

pub struct Workspace {
    files: FxHashMap<FileUri, FileState>,
    include_paths: Vec<IncludePath>,
    reverse_imports: FxHashMap<FileUri, FxHashSet<FileUri>>,
}

impl Workspace {
    pub fn new() -> Self {
        Workspace {
            files: FxHashMap::default(),
            include_paths: Vec::new(),
            reverse_imports: FxHashMap::default(),
        }
    }

    pub fn set_include_paths(&mut self, paths: Vec<String>) {
        self.include_paths = paths.into_iter().map(IncludePath).collect();
    }

    pub fn include_paths(&self) -> &[IncludePath] { &self.include_paths }

    pub fn files(&self) -> impl Iterator<Item = (&FileUri, &FileState)> {
        self.files.iter()
    }

    pub fn file(&self, uri: &FileUri) -> Option<&FileState> {
        self.files.get(uri)
    }

    pub fn get(&self, uri: &str) -> Option<&FileState> {
        self.files.get(&FileUri(uri.into()))
    }

    pub fn update(&mut self, uri: &str, source: String) {
        let key = FileUri(uri.to_string());
        let prev_imports = self.imports_of(&key);
        let analysis = analyze(&source);
        self.files.insert(key.clone(), FileState { source, analysis });
        self.rebuild_reverse_imports(&key, prev_imports);
    }

    pub fn remove(&mut self, uri: &str) {
        let key = FileUri(uri.to_string());
        let prev = self.imports_of(&key);
        for p in prev {
            if let Some(target) = self.resolve_import_path(&key, &p) {
                if let Some(set) = self.reverse_imports.get_mut(&target) {
                    set.remove(&key);
                }
            }
        }
        self.files.remove(&key);
    }

    fn imports_of(&self, uri: &FileUri) -> Vec<String> {
        let Some(state) = self.files.get(uri) else { return Vec::new() };
        let mut out = Vec::new();
        for d in &state.analysis.file.decls {
            if let crate::ast::Decl::Using(u) = d {
                if let Some(path) = &u.import_path {
                    out.push(path.value.clone());
                }
            }
        }
        out
    }

    fn rebuild_reverse_imports(&mut self, uri: &FileUri, previous: Vec<String>) {
        for prev in &previous {
            if let Some(resolved) = self.resolve_import_path(uri, prev) {
                if let Some(set) = self.reverse_imports.get_mut(&resolved) {
                    set.remove(uri);
                }
            }
        }
        let current = self.imports_of(uri);
        for imp in &current {
            if let Some(resolved) = self.resolve_import_path(uri, imp) {
                self.reverse_imports.entry(resolved).or_default().insert(uri.clone());
            }
        }
    }

    /// URIs of every file that transitively imports `uri`, including `uri`
    /// itself. Callers use this to refresh dependents after an edit.
    pub fn transitive_dependents(&self, uri: &FileUri) -> Vec<FileUri> {
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

    /// Resolve `import "path"` to a known file URI. Matches in order:
    ///
    /// 1. Each include path prefix appended to the import path.
    /// 2. The importer's directory followed by the import path (relative
    ///    imports starting with `./` or `../`).
    /// 3. Any loaded file whose URI ends with the import path.
    pub fn resolve_import_path(&self, importer: &FileUri, path: &str) -> Option<FileUri> {
        for inc in &self.include_paths {
            let probe = FileUri(format!(
                "{}/{}",
                inc.0.trim_end_matches('/'),
                path.trim_start_matches('/'),
            ));
            if self.files.contains_key(&probe) { return Some(probe); }
        }
        if path.starts_with("./") || path.starts_with("../") {
            if let Some(dir) = importer_dir(importer) {
                let joined = join_path(&dir, path);
                let probe = FileUri(joined);
                if self.files.contains_key(&probe) { return Some(probe); }
            }
        }
        let suffix = if path.starts_with('/') { path.to_string() } else { format!("/{}", path) };
        for uri in self.files.keys() {
            if uri != importer && (uri.0.ends_with(&suffix) || uri.0.ends_with(path)) {
                return Some(uri.clone());
            }
        }
        None
    }

    /// Preload a file without overwriting an existing open-buffer copy.
    /// Intended for workspace bootstrapping: if the extension already has
    /// the user's edited source, we keep that.
    pub fn preload(&mut self, uri: &str, source: String) {
        if self.files.contains_key(&FileUri(uri.to_string())) {
            return;
        }
        self.update(uri, source);
    }
}

impl Default for Workspace {
    fn default() -> Self { Self::new() }
}

/// Given a file URI like `file:///a/b/foo.capnp`, return `file:///a/b`.
fn importer_dir(uri: &FileUri) -> Option<String> {
    let s = &uri.0;
    let slash = s.rfind('/')?;
    Some(s[..slash].to_string())
}

fn join_path(dir: &str, rel: &str) -> String {
    // Normalise `./` and walk `../` segments against the directory.
    let mut parts: Vec<&str> = dir.trim_end_matches('/').split('/').collect();
    for seg in rel.split('/') {
        match seg {
            "." | "" => {}
            ".." => { parts.pop(); }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
