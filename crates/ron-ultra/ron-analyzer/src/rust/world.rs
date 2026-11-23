//! The Rust side of the workspace, read lazily through a [`FileLoader`].
//!
//! Nothing is scanned up front. A schema annotation names a path; resolving
//! it reads the `Cargo.toml` above the RON file, the crate root, and then
//! one module file per `mod` on the way — and caches every read, including
//! the misses, until the host says a file changed.
//!
//! Resolution follows rustc's module rules (`foo.rs` or `foo/mod.rs`,
//! `#[path]`, inline modules) and its name lookup order: items, then
//! explicit `use` imports, then extern crates, then glob imports.
//! Visibility is ignored; a schema only needs to find the declaration.

use super::cargo::{parse_manifest, Manifest};
use super::parser::parse_items;
use super::syntax::{Item, ItemKind, UseImport};
use crate::spans::SpanTable;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The host's view of the filesystem. `read` gets a URI and answers the
/// file's text, or `None` when there is no such file.
pub trait FileLoader {
    fn read(&self, uri: &str) -> Option<String>;
}

/// A loader with nothing on disk, for documents outside any folder.
pub struct NoFiles;

impl FileLoader for NoFiles {
    fn read(&self, _uri: &str) -> Option<String> {
        None
    }
}

#[derive(Debug)]
pub struct RustFile {
    pub uri: String,
    pub source: String,
    pub spans: SpanTable,
    pub items: Vec<Item>,
}

#[derive(Debug)]
pub struct Crate {
    /// The name code uses for it (`my_game`).
    pub name: String,
    /// The directory holding its `Cargo.toml`, with a trailing `/`.
    pub manifest_dir: String,
    /// `lib.rs`, `main.rs`, or wherever `[lib] path` points.
    pub root: String,
    pub manifest: Rc<Manifest>,
    pub is_lib: bool,
}

#[derive(Debug, Clone)]
pub struct ModPath {
    pub krate: Rc<Crate>,
    pub segments: Vec<String>,
}

impl ModPath {
    pub fn root(krate: Rc<Crate>) -> ModPath {
        ModPath { krate, segments: Vec::new() }
    }

    pub fn child(&self, name: &str) -> ModPath {
        let mut segments = self.segments.clone();
        segments.push(name.to_string());
        ModPath { krate: self.krate.clone(), segments }
    }

    pub fn parent(&self) -> Option<ModPath> {
        let mut segments = self.segments.clone();
        segments.pop()?;
        Some(ModPath { krate: self.krate.clone(), segments })
    }

    fn key(&self) -> String {
        format!("{}#{}", self.krate.root, self.segments.join("::"))
    }

    /// `crate::config`, or `my_game::config` for another crate.
    pub fn display(&self, from: Option<&Crate>) -> String {
        let head = match from {
            Some(c) if c.root == self.krate.root => "crate".to_string(),
            _ => self.krate.name.clone(),
        };
        std::iter::once(head).chain(self.segments.iter().cloned()).collect::<Vec<_>>().join("::")
    }
}

/// A module's items, wherever they live: a whole file or an inline
/// `mod name { ... }` inside one.
#[derive(Debug, Clone)]
pub struct Scope {
    pub path: ModPath,
    pub file: Rc<RustFile>,
    /// Indices of the inline `mod` items to descend through in `file`.
    pub inline: Vec<usize>,
    /// Where `mod child;` looks for `child.rs` and `child/mod.rs`.
    pub children_dir: String,
}

impl Scope {
    pub fn items(&self) -> &[Item] {
        items_at(&self.file, &self.inline)
    }
}

fn items_at<'f>(file: &'f RustFile, inline: &[usize]) -> &'f [Item] {
    let mut items: &[Item] = &file.items;
    for &i in inline {
        match items.get(i).map(|it| &it.kind) {
            Some(ItemKind::Mod { body: Some(body) }) => items = body,
            _ => return &[],
        }
    }
    items
}

/// A struct, enum or type alias, found.
#[derive(Debug, Clone)]
pub struct ItemRef {
    pub file: Rc<RustFile>,
    /// Inline module indices, then the item's own index.
    pub index: Vec<usize>,
    /// The module it is declared in, which is where its field types are
    /// resolved.
    pub module: ModPath,
}

impl ItemRef {
    pub fn item(&self) -> &Item {
        let (last, inline) = self.index.split_last().expect("an item index");
        &items_at(&self.file, inline)[*last]
    }

    pub fn same(&self, other: &ItemRef) -> bool {
        self.file.uri == other.file.uri && self.index == other.index
    }
}

#[derive(Debug, Clone)]
pub enum Res {
    Item(ItemRef),
    Module(ModPath),
    /// A path into a crate we do not read, such as `std` or `serde_json`.
    External(Vec<String>),
}

pub struct World {
    loader: Box<dyn FileLoader>,
    files: RefCell<HashMap<String, Option<Rc<RustFile>>>>,
    manifests: RefCell<HashMap<String, Option<Rc<Manifest>>>>,
}

/// Cycle guard for name lookup: the (module, name) pairs being resolved.
type Guard = Vec<String>;

impl World {
    pub fn new(loader: Box<dyn FileLoader>) -> World {
        World { loader, files: RefCell::default(), manifests: RefCell::default() }
    }

    /// Forget a cached file or manifest, hit or miss.
    pub fn invalidate(&self, uri: &str) {
        self.files.borrow_mut().remove(uri);
        self.manifests.borrow_mut().remove(uri);
    }

    pub fn clear(&self) {
        self.files.borrow_mut().clear();
        self.manifests.borrow_mut().clear();
    }

    /// Whether `uri` was read (or looked for), so a change to it matters.
    pub fn is_tracked(&self, uri: &str) -> bool {
        self.files.borrow().contains_key(uri) || self.manifests.borrow().contains_key(uri)
    }

    pub fn file(&self, uri: &str) -> Option<Rc<RustFile>> {
        if let Some(hit) = self.files.borrow().get(uri) {
            return hit.clone();
        }
        let loaded = self.loader.read(uri).map(|source| {
            Rc::new(RustFile {
                uri: uri.to_string(),
                spans: SpanTable::new(&source),
                items: parse_items(&source),
                source,
            })
        });
        self.files.borrow_mut().insert(uri.to_string(), loaded.clone());
        loaded
    }

    fn manifest(&self, dir: &str) -> Option<Rc<Manifest>> {
        let uri = format!("{dir}Cargo.toml");
        if let Some(hit) = self.manifests.borrow().get(&uri) {
            return hit.clone();
        }
        let loaded = self.loader.read(&uri).map(|text| Rc::new(parse_manifest(&text)));
        self.manifests.borrow_mut().insert(uri, loaded.clone());
        loaded
    }

    fn exists(&self, uri: &str) -> bool {
        self.file(uri).is_some()
    }

    // ---- crates ----------------------------------------------------------

    /// The crates of the nearest package above `uri`: its library first,
    /// then its `src/main.rs` binary.
    pub fn crates_for(&self, uri: &str) -> Vec<Rc<Crate>> {
        let mut dir = dir_of(uri).to_string();
        loop {
            if let Some(m) = self.manifest(&dir) {
                if m.package.is_some() {
                    return self.package_crates(&dir, m);
                }
            }
            match parent_dir(&dir) {
                Some(p) => dir = p,
                None => return Vec::new(),
            }
        }
    }

    fn package_crates(&self, dir: &str, m: Rc<Manifest>) -> Vec<Rc<Crate>> {
        let mut out = Vec::new();
        if let Some(lib) = self.lib_crate(dir, m.clone()) {
            out.push(lib);
        }
        let main = format!("{dir}src/main.rs");
        if self.exists(&main) {
            let name = m.package.clone().unwrap_or_default();
            out.push(Rc::new(Crate { name, manifest_dir: dir.to_string(), root: main, manifest: m, is_lib: false }));
        }
        out
    }

    fn lib_crate(&self, dir: &str, m: Rc<Manifest>) -> Option<Rc<Crate>> {
        let root = match &m.lib_path {
            Some(p) => join(dir, p),
            None => format!("{dir}src/lib.rs"),
        };
        if !self.exists(&root) {
            return None;
        }
        let name = m.lib_name.clone().or_else(|| m.package.clone()).unwrap_or_default();
        Some(Rc::new(Crate { name, manifest_dir: dir.to_string(), root, manifest: m, is_lib: true }))
    }

    /// A crate `krate` can name as `name::...`: a path dependency, a
    /// workspace dependency with a path, or — from a binary — its own
    /// package's library.
    pub fn dependency(&self, krate: &Crate, name: &str) -> Option<Rc<Crate>> {
        if !krate.is_lib && krate.manifest.package.as_deref() == Some(name) {
            return self.lib_crate(&krate.manifest_dir, krate.manifest.clone());
        }
        let dep = krate.manifest.dependencies.iter().find(|d| d.name == name)?;
        let dir = if let Some(path) = &dep.path {
            dir_join(&krate.manifest_dir, path)
        } else if dep.workspace {
            let (root_dir, root) = self.workspace_root(&krate.manifest_dir)?;
            let wdep = root.workspace_dependencies.iter().find(|d| d.name == name)?;
            dir_join(&root_dir, wdep.path.as_deref()?)
        } else {
            return None;
        };
        let m = self.manifest(&dir)?;
        self.lib_crate(&dir, m)
    }

    fn workspace_root(&self, from: &str) -> Option<(String, Rc<Manifest>)> {
        let mut dir = from.to_string();
        loop {
            if let Some(m) = self.manifest(&dir) {
                if m.is_workspace {
                    return Some((dir, m));
                }
            }
            dir = parent_dir(&dir)?;
        }
    }

    /// The names a path may start with besides `crate`, `self` and `super`:
    /// the crate's local dependencies.
    pub fn dependency_names(&self, krate: &Crate) -> Vec<String> {
        let mut names: Vec<String> = krate
            .manifest
            .dependencies
            .iter()
            .filter(|d| d.path.is_some() || d.workspace)
            .map(|d| d.name.clone())
            .collect();
        if !krate.is_lib {
            names.extend(krate.manifest.package.clone());
        }
        names
    }

    // ---- modules ---------------------------------------------------------

    pub fn root_scope(&self, krate: Rc<Crate>) -> Option<Scope> {
        let file = self.file(&krate.root)?;
        let children_dir = dir_of(&krate.root).to_string();
        Some(Scope { path: ModPath::root(krate), file, inline: Vec::new(), children_dir })
    }

    pub fn scope(&self, path: &ModPath) -> Option<Scope> {
        let mut scope = self.root_scope(path.krate.clone())?;
        for seg in &path.segments {
            scope = self.child_scope(&scope, seg)?;
        }
        Some(scope)
    }

    fn child_scope(&self, scope: &Scope, name: &str) -> Option<Scope> {
        let (index, item) = scope
            .items()
            .iter()
            .enumerate()
            .find(|(_, it)| matches!(it.kind, ItemKind::Mod { .. }) && it.name() == name)?;
        let path = scope.path.child(name);
        if let ItemKind::Mod { body: Some(_) } = &item.kind {
            let mut inline = scope.inline.clone();
            inline.push(index);
            let children_dir = format!("{}{name}/", scope.children_dir);
            return Some(Scope { path, file: scope.file.clone(), inline, children_dir });
        }
        if let Some(p) = &item.path_attr {
            // A file named by `#[path]` resolves its own children like a
            // `mod.rs` does: next to itself.
            let base = if scope.inline.is_empty() { dir_of(&scope.file.uri).to_string() } else { scope.children_dir.clone() };
            let uri = join(&base, p);
            let file = self.file(&uri)?;
            let children_dir = dir_of(&uri).to_string();
            return Some(Scope { path, file, inline: Vec::new(), children_dir });
        }
        let children_dir = format!("{}{name}/", scope.children_dir);
        let file = self
            .file(&format!("{}{name}.rs", scope.children_dir))
            .or_else(|| self.file(&format!("{children_dir}mod.rs")))?;
        Some(Scope { path, file, inline: Vec::new(), children_dir })
    }

    // ---- names -----------------------------------------------------------

    /// Resolve a path as written in `scope`.
    pub fn resolve_path(&self, scope: &Scope, global: bool, segments: &[String]) -> Option<Res> {
        self.resolve_path_in(scope, global, segments, &mut Vec::new())
    }

    fn resolve_path_in(&self, scope: &Scope, global: bool, segments: &[String], guard: &mut Guard) -> Option<Res> {
        let (first, rest) = segments.split_first()?;
        let mut cur = if global {
            self.extern_crate(&scope.path.krate, first).unwrap_or_else(|| Res::External(vec![first.clone()]))
        } else {
            match first.as_str() {
                "crate" => Res::Module(ModPath::root(scope.path.krate.clone())),
                "self" => Res::Module(scope.path.clone()),
                "super" => Res::Module(scope.path.parent()?),
                _ => match self.lookup(scope, first, guard) {
                    Some(r) => r,
                    // Most likely a crate we do not read, like `serde_json`.
                    None if !rest.is_empty() => Res::External(vec![first.clone()]),
                    None => return None,
                },
            }
        };
        for seg in rest {
            cur = match cur {
                Res::Module(m) => match seg.as_str() {
                    "super" => Res::Module(m.parent()?),
                    "self" => Res::Module(m),
                    _ => {
                        let s = self.scope(&m)?;
                        self.lookup(&s, seg, guard)?
                    }
                },
                Res::External(mut p) => {
                    p.push(seg.clone());
                    Res::External(p)
                }
                // `Enum::Variant` is not a type.
                Res::Item(_) => return None,
            };
        }
        Some(cur)
    }

    fn extern_crate(&self, krate: &Rc<Crate>, name: &str) -> Option<Res> {
        if matches!(name, "std" | "core" | "alloc") {
            return Some(Res::External(vec![name.to_string()]));
        }
        self.dependency(krate, name).map(|c| Res::Module(ModPath::root(c)))
    }

    /// Find `name` in a module's type namespace.
    pub fn lookup_name(&self, scope: &Scope, name: &str) -> Option<Res> {
        self.lookup(scope, name, &mut Vec::new())
    }

    fn lookup(&self, scope: &Scope, name: &str, guard: &mut Guard) -> Option<Res> {
        let key = format!("{}#{name}", scope.path.key());
        if guard.contains(&key) || guard.len() > 32 {
            return None;
        }
        guard.push(key);
        let found = self.lookup_unguarded(scope, name, guard);
        guard.pop();
        found
    }

    fn lookup_unguarded(&self, scope: &Scope, name: &str, guard: &mut Guard) -> Option<Res> {
        let items = scope.items();
        for (i, item) in items.iter().enumerate() {
            if item.name() != name {
                continue;
            }
            match item.kind {
                ItemKind::Mod { .. } => return Some(Res::Module(scope.path.child(name))),
                ItemKind::Struct { .. } | ItemKind::Enum { .. } | ItemKind::TypeAlias { .. } => {
                    let mut index = scope.inline.clone();
                    index.push(i);
                    return Some(Res::Item(ItemRef { file: scope.file.clone(), index, module: scope.path.clone() }));
                }
                ItemKind::Use(_) => {}
            }
        }
        let imports: Vec<&UseImport> = items
            .iter()
            .filter_map(|it| match &it.kind {
                ItemKind::Use(list) => Some(list),
                _ => None,
            })
            .flatten()
            .collect();
        for import in imports.iter().filter(|i| i.binding.as_deref() == Some(name)) {
            if let Some(r) = self.resolve_path_in(scope, import.global, &import.path, guard) {
                return Some(r);
            }
        }
        if let Some(r) = self.extern_crate(&scope.path.krate, name) {
            return Some(r);
        }
        for import in imports.iter().filter(|i| i.glob) {
            if let Some(Res::Module(m)) = self.resolve_path_in(scope, import.global, &import.path, guard) {
                if let Some(s) = self.scope(&m) {
                    if let Some(r) = self.lookup(&s, name, guard) {
                        return Some(r);
                    }
                }
            }
        }
        None
    }

    /// Everything nameable in a module, for completing a path: modules,
    /// structs, enums and aliases, plus what its `use`s bring in.
    pub fn names_in(&self, scope: &Scope) -> Vec<(String, Res)> {
        let mut out: Vec<(String, Res)> = Vec::new();
        for (i, item) in scope.items().iter().enumerate() {
            match &item.kind {
                ItemKind::Mod { .. } => out.push((item.name().to_string(), Res::Module(scope.path.child(item.name())))),
                ItemKind::Struct { .. } | ItemKind::Enum { .. } | ItemKind::TypeAlias { .. } => {
                    let mut index = scope.inline.clone();
                    index.push(i);
                    out.push((
                        item.name().to_string(),
                        Res::Item(ItemRef { file: scope.file.clone(), index, module: scope.path.clone() }),
                    ));
                }
                ItemKind::Use(list) => {
                    for import in list {
                        let Some(binding) = &import.binding else { continue };
                        if let Some(r @ (Res::Item(_) | Res::Module(_))) =
                            self.resolve_path(scope, import.global, &import.path)
                        {
                            out.push((binding.clone(), r));
                        }
                    }
                }
            }
        }
        out
    }
}

// ---- URIs --------------------------------------------------------------------

/// Everything up to and including the last `/`.
pub fn dir_of(uri: &str) -> &str {
    match uri.rfind('/') {
        Some(i) => &uri[..=i],
        None => "",
    }
}

/// Length of `scheme://authority/`, the part a walk upward never leaves.
fn root_len(uri: &str) -> usize {
    match uri.find("://") {
        Some(i) => uri[i + 3..].find('/').map_or(uri.len(), |j| i + 3 + j + 1),
        None => usize::from(uri.starts_with('/')),
    }
}

/// The directory above `dir` (which ends in `/`), or `None` at the root.
pub fn parent_dir(dir: &str) -> Option<String> {
    if dir.len() <= root_len(dir) {
        return None;
    }
    let trimmed = &dir[..dir.len() - 1];
    let i = trimmed.rfind('/')?;
    let parent = &trimmed[..=i];
    (parent.len() >= root_len(dir)).then(|| parent.to_string())
}

/// Resolve a relative file path against a directory URI.
pub fn join(dir: &str, rel: &str) -> String {
    let root = root_len(dir).min(dir.len());
    let mut parts: Vec<String> = dir[root..].split('/').filter(|s| !s.is_empty()).map(str::to_string).collect();
    for seg in rel.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s.replace('%', "%25").replace(' ', "%20")),
        }
    }
    format!("{}{}", &dir[..root], parts.join("/"))
}

fn dir_join(dir: &str, rel: &str) -> String {
    let mut joined = join(dir, rel);
    if !joined.ends_with('/') {
        joined.push('/');
    }
    joined
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An in-memory filesystem keyed by URI.
    pub struct MemFiles(pub HashMap<String, String>);

    impl FileLoader for MemFiles {
        fn read(&self, uri: &str) -> Option<String> {
            self.0.get(uri).cloned()
        }
    }

    pub fn world(files: &[(&str, &str)]) -> World {
        World::new(Box::new(MemFiles(files.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())))
    }

    fn segs(path: &str) -> Vec<String> {
        path.split("::").map(str::to_string).collect()
    }

    fn resolve(w: &World, from: &str, path: &str) -> Option<String> {
        let krate = w.crates_for(from).into_iter().next()?;
        let scope = w.root_scope(krate)?;
        match w.resolve_path(&scope, false, &segs(path))? {
            Res::Item(r) => Some(format!("{}::{}", r.module.display(None), r.item().name())),
            Res::Module(m) => Some(format!("mod {}", m.display(None))),
            Res::External(p) => Some(format!("extern {}", p.join("::"))),
        }
    }

    #[test]
    fn uris() {
        assert_eq!(dir_of("file:///a/b/c.ron"), "file:///a/b/");
        assert_eq!(parent_dir("file:///a/b/").as_deref(), Some("file:///a/"));
        assert_eq!(parent_dir("file:///a/").as_deref(), Some("file:///"));
        assert_eq!(parent_dir("file:///"), None);
        assert_eq!(join("file:///a/b/", "../c/d.rs"), "file:///a/c/d.rs");
        assert_eq!(join("file:///a/", "x y.rs"), "file:///a/x%20y.rs");
    }

    #[test]
    fn modules_files_and_imports() {
        let w = world(&[
            ("file:///ws/game/Cargo.toml", "[package]\nname = \"my-game\"\n[dependencies]\nshared = { path = \"../shared\" }\n"),
            ("file:///ws/game/src/lib.rs", "pub mod config;\nmod inline { pub mod deep { pub struct Deep; } }\npub use config::GameConfig as Root;\nuse shared::*;\n"),
            ("file:///ws/game/src/config.rs", "mod window;\npub use window::Window;\npub struct GameConfig;\n"),
            ("file:///ws/game/src/config/window/mod.rs", "pub struct Window;\n"),
            ("file:///ws/shared/Cargo.toml", "[package]\nname = \"shared\"\n"),
            ("file:///ws/shared/src/lib.rs", "pub struct Color;\n"),
        ]);
        let from = "file:///ws/game/assets/config.ron";
        assert_eq!(resolve(&w, from, "crate::config::GameConfig").as_deref(), Some("my_game::config::GameConfig"));
        assert_eq!(resolve(&w, from, "config::Window").as_deref(), Some("my_game::config::window::Window"));
        assert_eq!(resolve(&w, from, "Root").as_deref(), Some("my_game::config::GameConfig"));
        assert_eq!(resolve(&w, from, "inline::deep::Deep").as_deref(), Some("my_game::inline::deep::Deep"));
        assert_eq!(resolve(&w, from, "shared::Color").as_deref(), Some("shared::Color"));
        assert_eq!(resolve(&w, from, "Color").as_deref(), Some("shared::Color"));
        assert_eq!(resolve(&w, from, "std::path::PathBuf").as_deref(), Some("extern std::path::PathBuf"));
        assert_eq!(resolve(&w, from, "crate::config::Nope"), None);
        assert!(w.is_tracked("file:///ws/game/assets/Cargo.toml"));
    }

    #[test]
    fn workspace_dependencies_and_binaries() {
        let w = world(&[
            ("file:///ws/Cargo.toml", "[workspace]\n[workspace.dependencies]\ncommon = { path = \"crates/common\" }\n"),
            ("file:///ws/app/Cargo.toml", "[package]\nname = \"app\"\n[dependencies]\ncommon.workspace = true\n"),
            ("file:///ws/app/src/main.rs", "use common::Thing;\nuse app::Lib;\n"),
            ("file:///ws/app/src/lib.rs", "pub struct Lib;\n"),
            ("file:///ws/crates/common/Cargo.toml", "[package]\nname = \"common\"\n"),
            ("file:///ws/crates/common/src/lib.rs", "pub struct Thing;\n"),
        ]);
        let crates = w.crates_for("file:///ws/app/x.ron");
        assert_eq!(crates.len(), 2);
        assert!(crates[0].is_lib);
        let main = w.root_scope(crates[1].clone()).unwrap();
        let thing = w.resolve_path(&main, false, &segs("Thing"));
        assert!(matches!(thing, Some(Res::Item(r)) if r.item().name() == "Thing"));
        let lib = w.resolve_path(&main, false, &segs("Lib"));
        assert!(matches!(lib, Some(Res::Item(r)) if r.item().name() == "Lib"));
    }

    #[test]
    fn cyclic_globs_terminate() {
        let w = world(&[
            ("file:///p/Cargo.toml", "[package]\nname = \"p\"\n"),
            ("file:///p/src/lib.rs", "mod a { pub use super::*; pub use super::b::*; }\nmod b { pub use super::*; pub use super::a::*; }\nuse a::*;\nuse b::*;\n"),
        ]);
        assert_eq!(resolve(&w, "file:///p/x.ron", "Missing"), None);
    }

    #[test]
    fn invalidation_rereads() {
        let w = world(&[]);
        assert!(w.crates_for("file:///p/x.ron").is_empty());
        assert!(w.is_tracked("file:///p/Cargo.toml"));
        w.invalidate("file:///p/Cargo.toml");
        assert!(!w.is_tracked("file:///p/Cargo.toml"));
    }
}
