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
            collect_decl_imports(d, &mut out);
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
    /// 3. Among all loaded files whose URI ends with the import path, the
    ///    one closest to the importer by shared directory prefix. Ties are
    ///    broken in favour of the shallower (shorter) URI, so a vendored
    ///    copy buried under `temp/…` loses to a sibling at the same level.
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
        let mut best: Option<(&FileUri, usize, usize)> = None;
        for uri in self.files.keys() {
            if uri == importer { continue; }
            if !(uri.0.ends_with(&suffix) || uri.0.ends_with(path)) { continue; }
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

fn collect_decl_imports(d: &crate::ast::Decl, out: &mut Vec<String>) {
    use crate::ast::*;
    match d {
        Decl::Using(u) => {
            if let Some(path) = &u.import_path {
                out.push(path.value.clone());
            }
        }
        Decl::Struct(s) => {
            for m in &s.members {
                collect_member_imports(m, out);
            }
        }
        Decl::Interface(i) => {
            for sup in &i.superclasses {
                collect_type_ref_imports(sup, out);
            }
            for m in &i.methods {
                if let Some(params) = &m.params {
                    for p in params {
                        collect_type_ref_imports(&p.ty, out);
                    }
                }
                if let Some(results) = &m.results {
                    for p in results {
                        collect_type_ref_imports(&p.ty, out);
                    }
                }
            }
            for n in &i.nested {
                collect_member_imports(n, out);
            }
        }
        Decl::Const(c) => collect_type_ref_imports(&c.ty, out),
        Decl::Annotation(a) => {
            if let Some(ty) = &a.ty {
                collect_type_ref_imports(ty, out);
            }
        }
        _ => {}
    }
}

fn collect_member_imports(m: &crate::ast::StructMember, out: &mut Vec<String>) {
    use crate::ast::*;
    match m {
        StructMember::Field(f) => match &f.body {
            FieldBody::Slot { ty, .. } => collect_type_ref_imports(ty, out),
            FieldBody::NamedUnion(ub) => {
                for inner in &ub.members {
                    collect_member_imports(&StructMember::Field(inner.clone()), out);
                }
            }
            FieldBody::NamedGroup(gb) => {
                for inner in &gb.members {
                    collect_member_imports(inner, out);
                }
            }
        },
        StructMember::AnonUnion(ub) => {
            for inner in &ub.members {
                collect_member_imports(&StructMember::Field(inner.clone()), out);
            }
        }
        StructMember::Struct(s) => {
            for m in &s.members {
                collect_member_imports(m, out);
            }
        }
        StructMember::Interface(i) => {
            for sup in &i.superclasses {
                collect_type_ref_imports(sup, out);
            }
            for m in &i.methods {
                if let Some(params) = &m.params {
                    for p in params {
                        collect_type_ref_imports(&p.ty, out);
                    }
                }
                if let Some(results) = &m.results {
                    for p in results {
                        collect_type_ref_imports(&p.ty, out);
                    }
                }
            }
            for n in &i.nested {
                collect_member_imports(n, out);
            }
        }
        StructMember::Const(c) => collect_type_ref_imports(&c.ty, out),
        StructMember::Annotation(a) => {
            if let Some(ty) = &a.ty {
                collect_type_ref_imports(ty, out);
            }
        }
        StructMember::Enum(_) | StructMember::Using(_) => {}
    }
}

fn collect_type_ref_imports(t: &crate::ast::TypeRef, out: &mut Vec<String>) {
    if let Some(p) = &t.import_path {
        out.push(p.value.clone());
    }
    for a in &t.args {
        collect_type_ref_imports(a, out);
    }
}

/// Number of path segments (split on `/`) that match between two URIs,
/// counted from the start. Both URIs are expected to share the same scheme
/// prefix (e.g. `file:///`), which contributes a constant offset to the
/// score that's harmless for ranking purposes.
fn shared_dir_segments(a: &str, b: &str) -> usize {
    a.split('/').zip(b.split('/')).take_while(|(x, y)| x == y).count()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_match_prefers_sibling_over_vendored_copy() {
        let mut ws = Workspace::new();
        ws.update("file:///work/myproj/main.capnp", String::new());
        ws.update("file:///work/myproj/rpc.capnp", String::new());
        ws.update(
            "file:///work/temp/capnproto-rust/some/path/rpc.capnp",
            String::new(),
        );
        let importer = FileUri("file:///work/myproj/main.capnp".into());
        let resolved = ws.resolve_import_path(&importer, "rpc.capnp");
        assert_eq!(
            resolved.as_ref().map(|u| u.0.as_str()),
            Some("file:///work/myproj/rpc.capnp"),
        );
    }

    #[test]
    fn suffix_match_breaks_ties_by_shorter_path() {
        let mut ws = Workspace::new();
        ws.update("file:///work/main.capnp", String::new());
        ws.update("file:///work/rpc.capnp", String::new());
        ws.update(
            "file:///work/temp/capnproto-rust/some/path/rpc.capnp",
            String::new(),
        );
        let importer = FileUri("file:///work/main.capnp".into());
        let resolved = ws.resolve_import_path(&importer, "rpc.capnp");
        assert_eq!(
            resolved.as_ref().map(|u| u.0.as_str()),
            Some("file:///work/rpc.capnp"),
        );
    }

    #[test]
    fn suffix_match_picks_vendored_copy_when_importer_lives_there() {
        let mut ws = Workspace::new();
        ws.update("file:///work/myproj/rpc.capnp", String::new());
        ws.update(
            "file:///work/temp/capnproto-rust/some/path/rpc.capnp",
            String::new(),
        );
        ws.update(
            "file:///work/temp/capnproto-rust/some/path/main.capnp",
            String::new(),
        );
        let importer = FileUri(
            "file:///work/temp/capnproto-rust/some/path/main.capnp".into(),
        );
        let resolved = ws.resolve_import_path(&importer, "rpc.capnp");
        assert_eq!(
            resolved.as_ref().map(|u| u.0.as_str()),
            Some("file:///work/temp/capnproto-rust/some/path/rpc.capnp"),
        );
    }
}
