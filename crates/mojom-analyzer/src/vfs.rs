//! In-memory workspace: uri → (source, parse analysis) plus include-path
//! resolution for the `import "…";` paths that connect Mojom files.
//!
//! Each [`FileState`] owns its raw source (needed for doc-comment
//! extraction) and its parse analysis. The workspace also tracks a
//! reverse-import graph so a file update can return the set of files that
//! need to be re-diagnosed.

use crate::diagnostics::{analyze, Analysis};
use rustc_hash::{FxHashMap, FxHashSet};

pub use analyzer_core::vfs::FileUri;

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

    pub fn include_paths(&self) -> &[IncludePath] {
        &self.include_paths
    }

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

    /// Preload a file without overwriting an existing open-buffer copy.
    /// Intended for workspace bootstrapping: if the extension already has the
    /// user's edited source, we keep that.
    pub fn preload(&mut self, uri: &str, source: String) {
        if self.files.contains_key(&FileUri(uri.to_string())) {
            return;
        }
        self.update(uri, source);
    }

    fn imports_of(&self, uri: &FileUri) -> Vec<String> {
        let Some(state) = self.files.get(uri) else { return Vec::new() };
        state.analysis.file.imports.iter().map(|i| i.path.value.clone()).collect()
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

    /// Resolve an `import "path"` string to a known file URI. Matches in
    /// order:
    ///
    /// 1. Each include path prefix appended to the import path (Mojom imports
    ///    are conventionally written relative to a source root).
    /// 2. The importer's directory followed by the import path (relative
    ///    imports starting with `./` or `../`).
    /// 3. Among all loaded files whose URI ends with the import path, the one
    ///    closest to the importer by shared directory prefix. Ties are broken
    ///    in favour of the shallower (shorter) URI.
    pub fn resolve_import_path(&self, importer: &FileUri, path: &str) -> Option<FileUri> {
        for inc in &self.include_paths {
            let probe = FileUri(format!(
                "{}/{}",
                inc.0.trim_end_matches('/'),
                path.trim_start_matches('/'),
            ));
            if self.files.contains_key(&probe) {
                return Some(probe);
            }
        }
        if path.starts_with("./") || path.starts_with("../") {
            if let Some(dir) = importer_dir(importer) {
                let joined = join_path(&dir, path);
                let probe = FileUri(joined);
                if self.files.contains_key(&probe) {
                    return Some(probe);
                }
            }
        }
        let suffix = if path.starts_with('/') { path.to_string() } else { format!("/{}", path) };
        let mut best: Option<(&FileUri, usize, usize)> = None;
        for uri in self.files.keys() {
            if uri == importer {
                continue;
            }
            if !(uri.0.ends_with(&suffix) || uri.0.ends_with(path)) {
                continue;
            }
            let shared = shared_dir_segments(&importer.0, &uri.0);
            let len = uri.0.len();
            let better = match best {
                None => true,
                Some((_, s, l)) => shared > s || (shared == s && len < l),
            };
            if better {
                best = Some((uri, shared, len));
            }
        }
        best.map(|(u, _, _)| u.clone())
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

/// Given a file URI like `file:///a/b/foo.mojom`, return `file:///a/b`.
fn importer_dir(uri: &FileUri) -> Option<String> {
    let s = &uri.0;
    let slash = s.rfind('/')?;
    Some(s[..slash].to_string())
}

/// Number of path segments (split on `/`) that match between two URIs,
/// counted from the start.
fn shared_dir_segments(a: &str, b: &str) -> usize {
    a.split('/').zip(b.split('/')).take_while(|(x, y)| x == y).count()
}

fn join_path(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = dir.trim_end_matches('/').split('/').collect();
    for seg in rel.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_match_prefers_sibling_over_vendored_copy() {
        let mut ws = Workspace::new();
        ws.update("file:///work/myproj/main.mojom", String::new());
        ws.update("file:///work/myproj/dep.mojom", String::new());
        ws.update("file:///work/temp/vendor/some/path/dep.mojom", String::new());
        let importer = FileUri("file:///work/myproj/main.mojom".into());
        let resolved = ws.resolve_import_path(&importer, "dep.mojom");
        assert_eq!(
            resolved.as_ref().map(|u| u.0.as_str()),
            Some("file:///work/myproj/dep.mojom"),
        );
    }

    #[test]
    fn resolves_subdir_import() {
        let mut ws = Workspace::new();
        ws.update("file:///work/main.mojom", String::new());
        ws.update("file:///work/foo_module/foo.mojom", String::new());
        let importer = FileUri("file:///work/main.mojom".into());
        let resolved = ws.resolve_import_path(&importer, "foo_module/foo.mojom");
        assert_eq!(
            resolved.as_ref().map(|u| u.0.as_str()),
            Some("file:///work/foo_module/foo.mojom"),
        );
    }

    #[test]
    fn transitive_dependents_includes_importers() {
        let mut ws = Workspace::new();
        ws.update("file:///a.mojom", "struct A { int32 x; };".into());
        ws.update("file:///b.mojom", "import \"a.mojom\";\nstruct B { A a; };".into());
        let deps = ws.transitive_dependents(&FileUri("file:///a.mojom".into()));
        assert!(deps.iter().any(|u| u.0 == "file:///b.mojom"));
    }
}
