//! Workspace-wide symbol index and name resolution.
//!
//! The index has three layers:
//!
//! 1. **Per-file symbol tree** (`index_file`) — scans the AST and collects a
//!    flat FQN → [`Symbol`] map plus a hierarchical `DefTree` for outlines.
//! 2. **Workspace symbol index** ([`WorkspaceIndex`]) — merges every file's
//!    symbol map and maintains an import graph with `public` transitivity.
//! 3. **Use-site resolver** ([`WorkspaceIndex::resolve_type_ref`]) — given a
//!    type reference appearing at a site, walk enclosing scopes outward until
//!    the FQN lands in the index and is visible through the importer's set
//!    of (transitively public) imports.

mod references;
mod use_sites;

pub use references::{RefSite, ReferenceIndex};
pub use use_sites::{collect_type_use_sites, TypeUseSite};

use crate::ast;
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxHashSet};
use smol_str::SmolStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Message,
    Enum,
    EnumValue,
    Field,
    Oneof,
    Service,
    Rpc,
}

impl SymbolKind {
    pub fn is_type(self) -> bool {
        matches!(self, SymbolKind::Message | SymbolKind::Enum)
    }
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub fqn: SmolStr,
    pub name: SmolStr,
    pub kind: SymbolKind,
    pub file: FileUri,
    pub name_span: ByteSpan,
    pub full_span: ByteSpan,
    /// For field symbols, the textual rendering of the field type (for hover).
    pub detail: Option<String>,
    /// Leading doc-comments captured at the definition (for hover).
    pub doc: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct FileSymbols {
    /// FQN -> Symbol. Preserves source order to drive document-symbol output.
    pub entries: IndexMap<SmolStr, Symbol>,
    /// Top-level FQNs declared in this file (messages, enums, services).
    pub top_level: Vec<SmolStr>,
    /// Package scope, e.g. `google.protobuf` (empty string when unset).
    pub package: SmolStr,
}

pub fn index_file(file_uri: &FileUri, file: &ast::File) -> FileSymbols {
    let mut out = FileSymbols::default();
    let pkg = match &file.package {
        Some(p) => SmolStr::new(p.name.to_display()),
        None => SmolStr::default(),
    };
    out.package = pkg.clone();
    let pkg_prefix = pkg.as_str();
    for item in &file.items {
        match item {
            ast::TopLevelItem::Message(m) => {
                let fqn = join(pkg_prefix, &m.name.name);
                out.top_level.push(SmolStr::new(&fqn));
                index_message(pkg_prefix, m, file_uri, &mut out);
            }
            ast::TopLevelItem::Enum(e) => {
                let fqn = join(pkg_prefix, &e.name.name);
                out.top_level.push(SmolStr::new(&fqn));
                index_enum(pkg_prefix, e, file_uri, &mut out);
            }
            ast::TopLevelItem::Service(s) => {
                let fqn = join(pkg_prefix, &s.name.name);
                out.top_level.push(SmolStr::new(&fqn));
                index_service(pkg_prefix, s, file_uri, &mut out);
            }
            ast::TopLevelItem::Extend(_) => {}
        }
    }
    out
}

fn index_message(scope: &str, m: &ast::Message, file_uri: &FileUri, out: &mut FileSymbols) {
    let fqn = join(scope, &m.name.name);
    let doc = ast::doc_comment_text(&m.leading_comments);
    out.entries.insert(
        SmolStr::new(&fqn),
        Symbol {
            fqn: SmolStr::new(&fqn),
            name: m.name.name.clone(),
            kind: SymbolKind::Message,
            file: file_uri.clone(),
            name_span: m.name.span,
            full_span: m.span,
            detail: None,
            doc,
        },
    );
    for f in &m.fields {
        insert_field(&fqn, f, file_uri, out);
    }
    for o in &m.oneofs {
        let oneof_fqn = join(&fqn, &o.name.name);
        out.entries.insert(
            SmolStr::new(&oneof_fqn),
            Symbol {
                fqn: SmolStr::new(&oneof_fqn),
                name: o.name.name.clone(),
                kind: SymbolKind::Oneof,
                file: file_uri.clone(),
                name_span: o.name.span,
                full_span: o.span,
                detail: None,
                doc: None,
            },
        );
        for f in &o.fields {
            insert_field(&fqn, f, file_uri, out);
        }
    }
    for nm in &m.nested_messages {
        index_message(&fqn, nm, file_uri, out);
    }
    for ne in &m.nested_enums {
        index_enum(&fqn, ne, file_uri, out);
    }
}

fn insert_field(parent_fqn: &str, f: &ast::FieldDecl, file_uri: &FileUri, out: &mut FileSymbols) {
    let ff = join(parent_fqn, &f.name.name);
    let doc = ast::doc_comment_text(&f.leading_comments);
    let detail = Some(render_field_detail(f));
    out.entries.insert(
        SmolStr::new(&ff),
        Symbol {
            fqn: SmolStr::new(&ff),
            name: f.name.name.clone(),
            kind: SymbolKind::Field,
            file: file_uri.clone(),
            name_span: f.name.span,
            full_span: f.span,
            detail,
            doc,
        },
    );
}

fn index_enum(scope: &str, e: &ast::EnumDecl, file_uri: &FileUri, out: &mut FileSymbols) {
    let fqn = join(scope, &e.name.name);
    let doc = ast::doc_comment_text(&e.leading_comments);
    out.entries.insert(
        SmolStr::new(&fqn),
        Symbol {
            fqn: SmolStr::new(&fqn),
            name: e.name.name.clone(),
            kind: SymbolKind::Enum,
            file: file_uri.clone(),
            name_span: e.name.span,
            full_span: e.span,
            detail: None,
            doc,
        },
    );
    for v in &e.values {
        let vf = join(&fqn, &v.name.name);
        let vdoc = ast::doc_comment_text(&v.leading_comments);
        out.entries.insert(
            SmolStr::new(&vf),
            Symbol {
                fqn: SmolStr::new(&vf),
                name: v.name.name.clone(),
                kind: SymbolKind::EnumValue,
                file: file_uri.clone(),
                name_span: v.name.span,
                full_span: v.span,
                detail: v.number.as_i64().map(|n| format!("= {}", n)),
                doc: vdoc,
            },
        );
    }
}

fn index_service(scope: &str, s: &ast::Service, file_uri: &FileUri, out: &mut FileSymbols) {
    let fqn = join(scope, &s.name.name);
    let doc = ast::doc_comment_text(&s.leading_comments);
    out.entries.insert(
        SmolStr::new(&fqn),
        Symbol {
            fqn: SmolStr::new(&fqn),
            name: s.name.name.clone(),
            kind: SymbolKind::Service,
            file: file_uri.clone(),
            name_span: s.name.span,
            full_span: s.span,
            detail: None,
            doc,
        },
    );
    for m in &s.methods {
        let mf = join(&fqn, &m.name.name);
        let mdoc = ast::doc_comment_text(&m.leading_comments);
        let detail = format!(
            "rpc ({}{}) returns ({}{})",
            if m.input.streaming { "stream " } else { "" },
            m.input.ty.to_display(),
            if m.output.streaming { "stream " } else { "" },
            m.output.ty.to_display(),
        );
        out.entries.insert(
            SmolStr::new(&mf),
            Symbol {
                fqn: SmolStr::new(&mf),
                name: m.name.name.clone(),
                kind: SymbolKind::Rpc,
                file: file_uri.clone(),
                name_span: m.name.span,
                full_span: m.span,
                detail: Some(detail),
                doc: mdoc,
            },
        );
    }
}

fn render_field_detail(f: &ast::FieldDecl) -> String {
    let ty = match &f.ty {
        ast::TypeRef::Scalar(s, _) => s.as_str().to_string(),
        ast::TypeRef::Named(q) => q.to_display(),
        ast::TypeRef::Map(m) => format!(
            "map<{}, {}>",
            render_type_label(&m.key),
            render_type_label(&m.value)
        ),
        ast::TypeRef::Missing(_) => "?".into(),
    };
    let label = match f.label {
        ast::FieldLabel::Repeated => "repeated ",
        ast::FieldLabel::Optional => "optional ",
        ast::FieldLabel::Required => "required ",
        ast::FieldLabel::None => "",
    };
    let num = f
        .number
        .as_i64()
        .map(|n| format!(" = {}", n))
        .unwrap_or_default();
    format!("{}{}{}", label, ty, num)
}

fn render_type_label(t: &ast::TypeRef) -> String {
    match t {
        ast::TypeRef::Scalar(s, _) => s.as_str().to_string(),
        ast::TypeRef::Named(q) => q.to_display(),
        ast::TypeRef::Map(m) => format!(
            "map<{}, {}>",
            render_type_label(&m.key),
            render_type_label(&m.value)
        ),
        ast::TypeRef::Missing(_) => "?".into(),
    }
}

fn join(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", scope, name)
    }
}

// ── Workspace-wide index ──────────────────────────────────────────────

/// A workspace-wide view over every file's [`FileSymbols`] plus a resolved
/// import graph (with public-import transitivity).
#[derive(Debug, Default, Clone)]
pub struct WorkspaceIndex {
    /// Absolute FQN (no leading dot) -> Symbol. Every symbol appears here
    /// regardless of file.
    by_fqn: FxHashMap<SmolStr, Symbol>,
    /// URI -> per-file symbol summary.
    by_file: FxHashMap<FileUri, FileSymbols>,
    /// URI -> set of URIs it can see via direct or public-import chains.
    visible_from: FxHashMap<FileUri, FxHashSet<FileUri>>,
}

impl WorkspaceIndex {
    /// Build an index from every file currently in the workspace.
    pub fn build(ws: &Workspace) -> Self {
        let mut by_fqn: FxHashMap<SmolStr, Symbol> = FxHashMap::default();
        let mut by_file: FxHashMap<FileUri, FileSymbols> = FxHashMap::default();
        for (uri, pf) in ws.files() {
            let fs = index_file(uri, &pf.ast);
            for (fqn, sym) in &fs.entries {
                // Last-wins: later duplicate definitions don't change the
                // index shape — they're caught by duplicate-name diagnostics.
                by_fqn.entry(fqn.clone()).or_insert_with(|| sym.clone());
            }
            by_file.insert(uri.clone(), fs);
        }

        // Direct imports resolved through the workspace's import router.
        let mut direct: FxHashMap<FileUri, Vec<(FileUri, ast::ImportModifier)>> = FxHashMap::default();
        for (uri, pf) in ws.files() {
            let mut row = Vec::new();
            for imp in &pf.ast.imports {
                if let Some(target) = ws.resolve_import_path(uri, &imp.path) {
                    row.push((target, imp.modifier));
                }
            }
            direct.insert(uri.clone(), row);
        }

        // Transitive closure of public-import visibility.
        let mut visible_from: FxHashMap<FileUri, FxHashSet<FileUri>> = FxHashMap::default();
        for uri in by_file.keys() {
            let mut seen: FxHashSet<FileUri> = FxHashSet::default();
            // Every file sees itself.
            seen.insert(uri.clone());
            // BFS: walk direct imports, then only `public` re-exports.
            let mut stack: Vec<(FileUri, bool)> = direct
                .get(uri)
                .into_iter()
                .flatten()
                .map(|(t, m)| (t.clone(), matches!(m, ast::ImportModifier::Public)))
                .collect();
            while let Some((next, _via_public)) = stack.pop() {
                if !seen.insert(next.clone()) {
                    continue;
                }
                if let Some(ns) = direct.get(&next) {
                    for (t, m) in ns {
                        if matches!(m, ast::ImportModifier::Public) {
                            stack.push((t.clone(), true));
                        }
                    }
                }
            }
            // Well-known types are always visible — they're the standard
            // library. This mirrors what users expect from protoc.
            for wkt in by_file.keys() {
                if wkt.as_str().starts_with("proto3-wkt:") {
                    seen.insert(wkt.clone());
                }
            }
            visible_from.insert(uri.clone(), seen);
        }

        WorkspaceIndex { by_fqn, by_file, visible_from }
    }

    pub fn lookup(&self, fqn: &str) -> Option<&Symbol> {
        self.by_fqn.get(fqn)
    }

    pub fn file_symbols(&self, uri: &FileUri) -> Option<&FileSymbols> {
        self.by_file.get(uri)
    }

    pub fn all_symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.by_fqn.values()
    }

    /// Files visible from `uri` (direct imports + public-import chains + self
    /// + bundled well-known types).
    pub fn visible_files(&self, uri: &FileUri) -> Option<&FxHashSet<FileUri>> {
        self.visible_from.get(uri)
    }

    /// Resolve a type-reference `name` appearing inside `enclosing_scope`
    /// (an FQN like `pkg.Outer.Inner` — the message that contains the use
    /// site — or the file's package when at top level). Returns the matching
    /// symbol and a `visibility_ok` flag so callers can distinguish
    /// "unknown type" from "exists but not imported here".
    pub fn resolve_type(
        &self,
        importer: &FileUri,
        enclosing_scope: &str,
        name: &ast::QualifiedName,
    ) -> Resolution {
        let name_str = name.to_display();
        let name_str = name_str.trim_start_matches('.');

        // Candidate FQNs in proto3 scope-walk order: innermost-out.
        let candidates: Vec<String> = if name.absolute {
            vec![name_str.to_string()]
        } else {
            scope_candidates(enclosing_scope, name_str)
        };

        for cand in &candidates {
            if let Some(sym) = self.by_fqn.get(cand.as_str()) {
                let visibility_ok = self
                    .visible_from
                    .get(importer)
                    .map_or(false, |set| set.contains(&sym.file));
                return Resolution::Found { symbol: sym.clone(), visibility_ok };
            }
        }
        Resolution::Unknown { candidates }
    }

    /// Produce suggestions (edit-distance 1-2) for an unresolved name.
    pub fn suggest_similar(&self, name: &str) -> Vec<String> {
        let target = name.trim_start_matches('.');
        let target_short = target.rsplit('.').next().unwrap_or(target);
        let mut scored: Vec<(u32, &SmolStr)> = Vec::new();
        for fqn in self.by_fqn.keys() {
            let short = fqn.rsplit('.').next().unwrap_or(fqn.as_str());
            let d = levenshtein(target_short, short);
            if d <= 2 {
                scored.push((d, fqn));
            }
        }
        scored.sort_by_key(|(d, _)| *d);
        scored.into_iter().take(5).map(|(_, f)| f.to_string()).collect()
    }
}

#[derive(Debug, Clone)]
pub enum Resolution {
    Found { symbol: Symbol, visibility_ok: bool },
    Unknown { candidates: Vec<String> },
}

/// Generate lookup candidates for `name` resolving inside `scope`. Mirrors
/// proto3's "innermost-out" scope walk: given scope `pkg.A.B`, a reference
/// to `C` tries `pkg.A.B.C`, `pkg.A.C`, `pkg.C`, `C` in that order.
fn scope_candidates(scope: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = scope.to_string();
    loop {
        if cur.is_empty() {
            out.push(name.to_string());
            break;
        }
        out.push(format!("{}.{}", cur, name));
        match cur.rfind('.') {
            Some(i) => cur.truncate(i),
            None => cur.clear(),
        }
    }
    out
}

fn levenshtein(a: &str, b: &str) -> u32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m as u32;
    }
    if m == 0 {
        return n as u32;
    }
    let mut prev: Vec<u32> = (0..=m as u32).collect();
    let mut cur = vec![0u32; m + 1];
    for i in 1..=n {
        cur[0] = i as u32;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (cur[j - 1] + 1)
                .min(prev[j] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

pub fn workspace_symbols(ws: &Workspace) -> Vec<Symbol> {
    let idx = WorkspaceIndex::build(ws);
    idx.all_symbols().cloned().collect()
}
