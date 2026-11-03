//! WESL's module system, on top of the WGSL support.
//!
//! A WESL file *is* a module, named by its path from the package root:
//! `shaders/lights/point.wesl` is `package::lights::point`, and `super::`
//! climbs from the file's own module. A path that does not start with either
//! names a dependency, which `wesl.toml` maps to a folder of its own. This is
//! where those paths become files and declarations.
//!
//! Everything the syntax layer and naga do for WGSL still applies inside one
//! file; this module answers only what crosses files:
//!
//! - **Resolution** ([`Server::wesl_resolve`]): a path to the module or the
//!   declaration it names, or to the segment where it stops naming anything.
//!   Open documents answer for themselves; every other file comes from the
//!   [`crate::WorkspaceIndex`], which is all the server can see — it runs in
//!   WASM with no filesystem.
//! - **The features**: definition, hover, completion and semantic tokens
//!   follow an import into the file it names, and diagnostics report an
//!   import that names nothing.
//! - **Validation** ([`link`]): naga reads one WGSL module and knows nothing
//!   of imports or `@if`, so a file that uses them is linked with what it
//!   imports, under the `wesl.features` setting ([`conditions`]), and naga
//!   checks that. Its problems map back to the file.
//!
//! **What is not checked.** Types *in the editor's answers*: hover and
//! completion read the file's own naga module, which a file that imports does
//! not have. And nothing is linked while a path names nothing or leads into a
//! dependency the server cannot see — a false squiggle costs more than a
//! missed one.

pub mod conditions;
pub mod link;
pub mod manifest;

use lsp_types::{
    CompletionItem, CompletionItemKind, Diagnostic, DiagnosticSeverity, Location, Position,
    Range, Uri,
};
use wgsl_syntax::{Language, Parsed, PathRoot, SymbolKind};

use crate::Server;
use crate::features::completion::{item, rank};
use crate::features::{doc_comment, lsp_completion_kind};
use crate::state::Document;

/// The `source` field of a WESL diagnostic.
const WESL: &str = "wesl";

/// File extensions a module may be written in, the preferred one first: a
/// `.wesl` file shadows a `.wgsl` one of the same name, as in wgsl-analyzer.
const EXTENSIONS: [&str; 2] = ["wesl", "wgsl"];

/// A declaration in some module.
#[derive(Debug, Clone)]
pub struct Item {
    pub uri: Uri,
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    pub detail: String,
    pub doc: Option<String>,
}

/// A module: the directory its children live in, and the file holding its
/// own declarations when it has one — a directory of modules with no file
/// beside it is a module too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub dir: String,
    pub file: Option<String>,
}

/// What a path names.
#[derive(Debug, Clone)]
pub enum Resolution {
    Module(Module),
    Item(Item),
    /// Segment `at` names nothing in `parent`, the module before it.
    Missing { at: usize, parent: String },
    /// More `super`s than the file is deep.
    TooManySupers,
    /// A dependency the server cannot see. Not an error: a package manager
    /// provides it, and nothing of it was pushed here.
    Unknown,
}

/// The package a file belongs to.
#[derive(Debug, Clone)]
struct Package {
    /// The directory `package::` names.
    root: String,
    /// Whether the file is outside any package — no `wesl.toml` above it, or
    /// one whose root does not contain it. The root is then the file's own
    /// directory, and `super` is not held to the package's depth.
    detached: bool,
    /// Path dependencies, by name, to the directory `name::` names.
    dependencies: Vec<(String, String)>,
}

/// A path written out in full, root and all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullPath {
    pub root: PathRoot,
    pub segments: Vec<String>,
}

impl FullPath {
    /// The path as WESL writes it.
    pub fn display(&self) -> String {
        display(self.root, &self.segments)
    }
}

impl Server {
    /// Resolve a path written in the file at `from`.
    pub fn wesl_resolve(&self, from: &Uri, path: &FullPath) -> Resolution {
        let package = self.package_of(from.as_str());
        let files = self.module_files();

        let (mut module, segments) = match path.root {
            PathRoot::Package => {
                (Module { dir: package.root.clone(), file: None }, &path.segments[..])
            }
            PathRoot::Super(count) => {
                let Some(dir) = super_dir(&package, from.as_str(), count) else {
                    return Resolution::TooManySupers;
                };
                (Module { file: module_file(&files, &dir), dir }, &path.segments[..])
            }
            PathRoot::Bare => {
                let Some((name, rest)) = path.segments.split_first() else {
                    return Resolution::Unknown;
                };
                let Some((_, dir)) = package.dependencies.iter().find(|(dep, _)| dep == name)
                else {
                    return Resolution::Unknown;
                };
                (Module { dir: dir.clone(), file: None }, rest)
            }
        };
        // Where a `Missing` is reported: segment indices are into the whole
        // path, which for a dependency includes its name.
        let offset = path.segments.len() - segments.len();

        for (i, name) in segments.iter().enumerate() {
            let last = i + 1 == segments.len();
            let child = child_module(&files, &module.dir, name);
            let item = || module.file.as_deref().and_then(|file| self.item(file, name));

            // An item and a module of the same name: the last segment of a
            // path prefers the item, which is what an import of it means;
            // any other segment has to be the module, to have children.
            if last {
                if let Some(item) = item() {
                    return Resolution::Item(item);
                }
            }
            match child {
                Some(child) => module = child,
                None => {
                    return Resolution::Missing {
                        at: offset + i,
                        parent: display(path.root, &path.segments[..offset + i]),
                    };
                }
            }
        }
        Resolution::Module(module)
    }

    /// The full path a name in `document` stands for, if it is a WESL path or
    /// a name an import brought in.
    ///
    /// `utils::helper` where `import package::shared::utils;` is in scope
    /// stands for `package::shared::utils::helper`; inside an import
    /// statement the path is taken as written.
    pub fn wesl_path_at(&self, document: &Document, offset: u32) -> Option<FullPath> {
        path_at(document.parsed(), document.text(), offset)
    }

    /// Where the WESL path or imported name at `offset` leads, when it leads
    /// out of this file.
    pub fn wesl_definition(&self, document: &Document, offset: u32) -> Option<Location> {
        let path = self.wesl_path_at(document, offset)?;
        match self.wesl_resolve(&document.uri, &path) {
            Resolution::Item(item) => {
                Some(Location { uri: item.uri, range: item.selection_range })
            }
            Resolution::Module(Module { file: Some(file), .. }) => Some(Location {
                uri: file.parse().ok()?,
                range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            }),
            _ => None,
        }
    }

    /// The hover for a WESL path or an imported name.
    pub fn wesl_hover(&self, document: &Document, offset: u32) -> Option<String> {
        let path = self.wesl_path_at(document, offset)?;
        let fence = |code: &str| format!("```{}\n{code}\n```", Language::Wesl.id());

        match self.wesl_resolve(&document.uri, &path) {
            Resolution::Item(item) => {
                let declaration = if item.detail.is_empty() { &item.name } else { &item.detail };
                let mut parent = path.clone();
                parent.segments.pop();
                let mut markdown =
                    format!("{}\n\nFrom `{}`.", fence(declaration), parent.display());
                if let Some(doc) = item.doc {
                    markdown.push_str("\n\n");
                    markdown.push_str(&doc);
                }
                Some(markdown)
            }
            Resolution::Module(module) => {
                let place = match &module.file {
                    Some(file) => format!("`{}`", file_name(file)),
                    None => format!("the `{}` directory", file_name(&module.dir)),
                };
                let declaration = fence(&format!("module {}", path.display()));
                Some(format!("{declaration}\n\nA module, in {place}."))
            }
            _ => None,
        }
    }

    /// Every problem with the paths a WESL document writes: an import or a
    /// qualified name that names nothing, or climbs out of its package.
    ///
    /// A path into a dependency the server cannot see is not reported — it
    /// comes from a package manager, and silence is the honest answer.
    pub fn wesl_diagnostics(&self, document: &Document) -> Vec<Diagnostic> {
        let parsed = document.parsed();
        let mut diagnostics = Vec::new();
        let mut report = |span: analyzer_core::spans::ByteSpan, message: String| {
            diagnostics.push(Diagnostic {
                range: document.range(span),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some(WESL.to_string()),
                message,
                ..Diagnostic::default()
            });
        };

        for import in &parsed.imports {
            let path = FullPath {
                root: import.root,
                segments: import.segments.iter().map(|(name, _)| name.clone()).collect(),
            };
            match self.wesl_resolve(&document.uri, &path) {
                Resolution::Missing { at, parent } => {
                    let (name, span) = &import.segments[at];
                    report(*span, missing(name, &parent));
                }
                Resolution::TooManySupers => {
                    let span = import.root_span.unwrap_or(import.segments[0].1);
                    report(span, TOO_MANY_SUPERS.to_string());
                }
                _ => {}
            }
        }

        // Qualified names in code, checked once each at their last segment —
        // the resolver reports the first segment that fails, wherever it is.
        for reference in &parsed.references {
            if !reference.is_member
                || in_import_statement(parsed, reference.span.start)
                || !follows_path_separator(parsed, document.text(), reference.span.start)
                || precedes_path_separator(document.text(), reference.span.end)
            {
                continue;
            }
            let Some(prefix) = parsed.path_prefix(document.text(), reference.span.start) else {
                continue;
            };
            let Some(path) = self.wesl_path_at(document, reference.span.start) else {
                continue;
            };
            // The spans of the segments as written. An alias an import brought
            // in stands for the import's whole path, so the written segments
            // line up with the tail of the resolved one; a failure at or
            // before the alias is the import's, and reported on it above.
            let mut written: Vec<(String, analyzer_core::spans::ByteSpan)> = prefix.segments;
            written.push((document.slice(reference.span).to_string(), reference.span));
            let hidden = path.segments.len() - written.len();
            let expanded = prefix.root == PathRoot::Bare
                && expand_import(parsed, &[written[0].0.clone()], written[0].1.start).is_some();
            let first_reportable = if expanded { hidden + 1 } else { hidden };

            match self.wesl_resolve(&document.uri, &path) {
                Resolution::Missing { at, parent } if at >= first_reportable => {
                    let (name, span) = &written[at - hidden];
                    report(*span, missing(name, &parent));
                }
                Resolution::TooManySupers => {
                    let span = prefix.root_span.unwrap_or(written[0].1);
                    report(span, TOO_MANY_SUPERS.to_string());
                }
                _ => {}
            }
        }
        diagnostics
    }

    /// Completion where only the module view knows the answer: on a path,
    /// the modules and declarations under it so far; straight after
    /// `import`, where a path can start. `None` anywhere else, so the
    /// caller's own contexts apply.
    pub fn wesl_completion(&self, document: &Document, offset: u32) -> Option<Vec<CompletionItem>> {
        let parsed = document.parsed();
        // Mid-word, the word under the cursor is what is being completed and
        // the path is what precedes *it*.
        let anchor = parsed
            .token_before(offset)
            .filter(|token| {
                matches!(
                    token.kind,
                    wgsl_syntax::lexer::TokenKind::Ident
                        | wgsl_syntax::lexer::TokenKind::Type
                        | wgsl_syntax::lexer::TokenKind::Keyword
                ) && token.span.contains(offset)
            })
            .map_or(offset, |token| token.span.start);

        let after_import = parsed
            .token_before(anchor)
            .is_some_and(|token| document.slice(token.span) == "import");
        if after_import {
            return Some(self.import_roots(document));
        }
        self.path_completion(document, anchor)
    }

    /// Show each imported name in a completion list as what it names — a
    /// function as a function, with its signature and doc comment — rather
    /// than as the import it technically is.
    pub fn wesl_describe_imports(&self, document: &Document, items: &mut [CompletionItem]) {
        let parsed = document.parsed();
        for import in &parsed.imports {
            let name = &parsed.symbols[import.symbol].name;
            let path = FullPath {
                root: import.root,
                segments: import.segments.iter().map(|(segment, _)| segment.clone()).collect(),
            };
            let Resolution::Item(target) = self.wesl_resolve(&document.uri, &path) else {
                continue;
            };
            for completion in items.iter_mut().filter(|completion| &completion.label == name) {
                if completion.kind != Some(CompletionItemKind::MODULE) {
                    continue;
                }
                let described = crate::features::completion::documented(
                    item(name, lsp_completion_kind(target.kind), target.detail.clone(), rank::FILE),
                    target.doc.as_deref().unwrap_or(""),
                );
                *completion = described;
            }
        }
    }

    /// The modules and declarations under the path written before `anchor`.
    fn path_completion(&self, document: &Document, anchor: u32) -> Option<Vec<CompletionItem>> {
        let parsed = document.parsed();
        let offset = anchor;
        let prefix = parsed.path_prefix(document.text(), anchor)?;

        let segments: Vec<String> = prefix.segments.iter().map(|(name, _)| name.clone()).collect();
        let in_import = in_import_statement(parsed, offset);
        let path = match prefix.root {
            PathRoot::Bare if !in_import => expand_import(parsed, &segments, anchor)
                .unwrap_or(FullPath { root: PathRoot::Bare, segments }),
            root => FullPath { root, segments },
        };

        let Resolution::Module(module) = self.wesl_resolve(&document.uri, &path) else {
            return Some(Vec::new());
        };
        let files = self.module_files();
        let mut items: Vec<CompletionItem> = children(&files, &module.dir)
            .into_iter()
            .map(|name| item(&name, CompletionItemKind::MODULE, "module".to_string(), rank::FILE))
            .collect();
        if let Some(file) = &module.file {
            for declaration in self.items(file) {
                items.push(crate::features::completion::documented(
                    item(
                        &declaration.name,
                        lsp_completion_kind(declaration.kind),
                        declaration.detail.clone(),
                        rank::LOCAL,
                    ),
                    declaration.doc.as_deref().unwrap_or(""),
                ));
            }
        }
        Some(items)
    }

    /// Completion straight after `import`: where a path can start.
    fn import_roots(&self, document: &Document) -> Vec<CompletionItem> {
        let package = self.package_of(document.uri.as_str());
        let keyword = |name: &str, detail: &str| {
            item(name, CompletionItemKind::KEYWORD, detail.to_string(), rank::LOCAL)
        };
        let mut items = vec![
            keyword("package", "this package's root"),
            keyword("super", "the module above this one"),
        ];
        for (name, _) in &package.dependencies {
            let detail = "dependency".to_string();
            items.push(item(name, CompletionItemKind::MODULE, detail, rank::FILE));
        }
        items
    }

    /// What a path written in the file at `from` turns out to name, for an
    /// icon and a colour. `None` when it names nothing this server can see.
    pub fn wesl_kind_of(&self, from: &Uri, path: &FullPath) -> Option<ImportKind> {
        match self.wesl_resolve(from, path) {
            Resolution::Item(item) => Some(ImportKind::Item(item.kind)),
            Resolution::Module(_) => Some(ImportKind::Module),
            _ => None,
        }
    }

    /// The package the file at `uri` belongs to: the nearest `wesl.toml` above
    /// it, or, with none, the file's own directory.
    fn package_of(&self, uri: &str) -> Package {
        let dir = parent(uri);
        let manifest = self
            .index()
            .manifests()
            .filter(|manifest| is_within(dir, &manifest.dir))
            .max_by_key(|manifest| manifest.dir.len());

        let Some(manifest) = manifest else {
            return Package { root: dir.to_string(), detached: true, dependencies: Vec::new() };
        };
        let root = join(&manifest.dir, &manifest.root);
        let dependencies = manifest
            .dependencies
            .iter()
            .map(|(name, path)| {
                let dir = join(&manifest.dir, path);
                // The dependency's own manifest says where its root is; one
                // the host has not pushed gets the default.
                let root = self
                    .index()
                    .manifests()
                    .find(|other| other.dir == dir)
                    .map_or_else(
                        || join(&dir, manifest::DEFAULT_ROOT),
                        |other| join(&other.dir, &other.root),
                    );
                (name.clone(), root)
            })
            .collect();
        Package { detached: !is_within(dir, &root), root, dependencies }
    }

    /// Every WGSL and WESL file the server knows of, open or not.
    fn module_files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .documents()
            .filter(|document| document.language.is_wgsl_family())
            .map(|document| document.uri.as_str().to_string())
            .collect();
        files.extend(
            self.index()
                .files()
                .filter(|(_, language)| language.is_wgsl_family())
                .map(|(uri, _)| uri.as_str().to_string()),
        );
        files
    }

    /// The module-scope declaration `name` in the file at `uri`.
    fn item(&self, uri: &str, name: &str) -> Option<Item> {
        self.items(uri).into_iter().find(|item| item.name == name)
    }

    /// Every module-scope declaration in the file at `uri` — the open
    /// document when there is one, the index otherwise. What a file imports
    /// is not among them: WESL does not re-export.
    fn items(&self, uri: &str) -> Vec<Item> {
        if let Some(document) = self.documents().find(|document| document.uri.as_str() == uri) {
            let parsed = document.parsed();
            return parsed
                .roots
                .iter()
                .map(|&index| &parsed.symbols[index])
                .filter(|symbol| symbol.kind.is_declaration())
                .map(|symbol| Item {
                    uri: document.uri.clone(),
                    name: symbol.name.clone(),
                    kind: symbol.kind,
                    range: document.range(symbol.full_span),
                    selection_range: document.range(symbol.name_span),
                    detail: symbol.detail.clone(),
                    doc: doc_comment(parsed, document.text(), symbol.full_span),
                })
                .collect();
        }
        self.index()
            .entries()
            .iter()
            .filter(|entry| {
                entry.uri.as_str() == uri
                    && entry.container.is_none()
                    && entry.kind.is_declaration()
            })
            .map(|entry| Item {
                uri: entry.uri.clone(),
                name: entry.name.clone(),
                kind: entry.kind,
                range: entry.range,
                selection_range: entry.selection_range,
                detail: entry.detail.clone(),
                doc: entry.doc.clone(),
            })
            .collect()
    }
}

/// What an imported name resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Module,
    Item(SymbolKind),
}

const TOO_MANY_SUPERS: &str = "too many `super`s: this climbs above the package root";

fn missing(name: &str, parent: &str) -> String {
    format!("cannot find `{name}` in `{parent}`")
}

/// A path as WESL writes it.
fn display(root: PathRoot, segments: &[String]) -> String {
    let mut parts: Vec<String> = match root {
        PathRoot::Package => vec!["package".to_string()],
        PathRoot::Super(count) => vec!["super".to_string(); count as usize],
        PathRoot::Bare => Vec::new(),
    };
    parts.extend(segments.iter().cloned());
    parts.join("::")
}

/// Whether a WESL document uses anything WGSL lacks — an import, a `::` path,
/// or conditional translation — and so cannot be handed to naga as it stands.
pub fn uses_wesl_syntax(parsed: &Parsed, source: &str) -> bool {
    !parsed.imports.is_empty()
        || parsed.tokens.iter().any(|token| match token.kind {
            wgsl_syntax::lexer::TokenKind::Attribute => {
                matches!(parsed.text(source, token.span), "@if" | "@elif" | "@else")
            }
            wgsl_syntax::lexer::TokenKind::Keyword => {
                matches!(parsed.text(source, token.span), "import" | "package" | "super")
            }
            _ => false,
        })
        || parsed.references.iter().any(|reference| {
            reference.is_member && follows_path_separator(parsed, source, reference.span.start)
        })
}

/// The full path the name at `offset` stands for — see
/// [`Server::wesl_path_at`], which this answers for any parsed source.
fn path_at(parsed: &Parsed, source: &str, offset: u32) -> Option<FullPath> {
    let reference = parsed.reference_at(offset)?;
    let name = parsed.text(source, reference.span).to_string();
    let in_import = in_import_statement(parsed, offset);

    match parsed.path_prefix(source, reference.span.start) {
        Some(prefix) => {
            let mut segments: Vec<String> =
                prefix.segments.iter().map(|(segment, _)| segment.clone()).collect();
            segments.push(name);
            if prefix.root == PathRoot::Bare && !in_import {
                if let Some(expanded) = expand_import(parsed, &segments, offset) {
                    return Some(expanded);
                }
            }
            Some(FullPath { root: prefix.root, segments })
        }
        // The first segment of an import names a package.
        None if in_import && !reference.is_declaration => {
            Some(FullPath { root: PathRoot::Bare, segments: vec![name] })
        }
        None => expand_import(parsed, &[name], offset),
    }
}

/// The path an import brought in, with whatever `segments` adds after its
/// name — or `None` when the first segment is not an imported name.
fn expand_import(parsed: &Parsed, segments: &[String], offset: u32) -> Option<FullPath> {
    let (first, rest) = segments.split_first()?;
    let index = parsed.resolve_name(first, offset)?;
    let import = parsed.import_for(index)?;
    let mut expanded: Vec<String> = import.segments.iter().map(|(name, _)| name.clone()).collect();
    expanded.extend(rest.iter().cloned());
    Some(FullPath { root: import.root, segments: expanded })
}

/// Whether `offset` falls inside an import statement.
fn in_import_statement(parsed: &Parsed, offset: u32) -> bool {
    parsed
        .imports
        .iter()
        .any(|import| parsed.symbols[import.symbol].full_span.contains(offset))
}

/// Whether a `::` ends right at `start`.
fn follows_path_separator(parsed: &Parsed, source: &str, start: u32) -> bool {
    let Some(second) = parsed.token_before(start) else { return false };
    let Some(first) = parsed.token_before(second.span.start) else { return false };
    second.span.end == start
        && parsed.text(source, second.span) == ":"
        && parsed.text(source, first.span) == ":"
        && first.span.end == second.span.start
}

/// Whether a `::` follows the name ending at `end`.
fn precedes_path_separator(source: &str, end: u32) -> bool {
    source.get(end as usize..).is_some_and(|rest| rest.trim_start().starts_with("::"))
}

/// The module file for the module whose directory is `dir`.
fn module_file(files: &[String], dir: &str) -> Option<String> {
    EXTENSIONS
        .iter()
        .map(|extension| format!("{dir}.{extension}"))
        .find(|candidate| files.iter().any(|file| file == candidate))
}

/// The child module `name` of the module whose directory is `dir`.
fn child_module(files: &[String], dir: &str, name: &str) -> Option<Module> {
    let child = format!("{dir}/{name}");
    let file = module_file(files, &child);
    let has_children = files.iter().any(|file| is_within(parent(file), &child));
    (file.is_some() || has_children).then_some(Module { dir: child, file })
}

/// The names of the modules directly under `dir`.
fn children(files: &[String], dir: &str) -> Vec<String> {
    let mut names: Vec<String> = files
        .iter()
        .filter_map(|file| file.strip_prefix(dir)?.strip_prefix('/'))
        .filter_map(|rest| match rest.split_once('/') {
            Some((directory, _)) => Some(directory.to_string()),
            None => {
                let (stem, extension) = rest.rsplit_once('.')?;
                EXTENSIONS.contains(&extension).then(|| stem.to_string())
            }
        })
        // A module is named by an identifier; a directory called `my shaders`
        // is not one.
        .filter(|name| {
            name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The directory the module `super` repeated `count` times names, from the
/// file at `uri`.
fn super_dir(package: &Package, uri: &str, count: u32) -> Option<String> {
    // The file's module is its directory plus its stem, so one `super`
    // names the module whose directory the file sits in.
    let mut dir = parent(uri).to_string();
    for _ in 1..count {
        dir = parent(&dir).to_string();
    }
    if !package.detached && !is_within(&dir, &package.root) {
        return None;
    }
    Some(dir)
}

/// Everything before the last `/`.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(dir, _)| dir)
}

/// The last component of a path.
fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Whether `path` is `dir` or anything below it.
fn is_within(path: &str, dir: &str) -> bool {
    path == dir || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

/// `relative` resolved against the directory `dir`, both URIs or paths with
/// `/` separators. `.` and `..` are folded away; nothing climbs above the
/// URI's authority.
fn join(dir: &str, relative: &str) -> String {
    // `file:///a/b` keeps its `file://` and empty authority intact.
    let (prefix, path) = match dir.find("://") {
        Some(at) => {
            let after = at + 3;
            let authority_end = dir[after..].find('/').map_or(dir.len(), |slash| after + slash);
            dir.split_at(authority_end)
        }
        None => ("", dir),
    };
    let mut parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    format!("{prefix}/{}", parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_folds_dots_and_keeps_the_scheme() {
        assert_eq!(join("file:///p/q", "./shaders/"), "file:///p/q/shaders");
        assert_eq!(join("file:///p/q", "../lib"), "file:///p/lib");
        assert_eq!(join("file:///p", "../../.."), "file:///");
    }

    #[test]
    fn children_are_files_and_directories_named_like_modules() {
        let files = [
            "file:///s/a.wesl",
            "file:///s/b.wgsl",
            "file:///s/b.wesl",
            "file:///s/lights/point.wesl",
            "file:///s/my dir/x.wesl",
            "file:///s/notes.txt",
            "file:///other/c.wesl",
        ]
        .map(String::from);
        assert_eq!(children(&files, "file:///s"), ["a", "b", "lights"]);
    }

    #[test]
    fn a_wesl_file_shadows_a_wgsl_one_of_the_same_name() {
        let files = ["file:///s/b.wgsl", "file:///s/b.wesl"].map(String::from);
        assert_eq!(module_file(&files, "file:///s/b").as_deref(), Some("file:///s/b.wesl"));
    }
}
