//! Name resolution.
//!
//! Mojom's scoping model: every file optionally declares a `module a.b.c;`
//! that prefixes the fully-qualified names of the types it defines. A type
//! reference is resolved innermost-out: the enclosing struct/interface scope
//! first, then the file's module, then the global (module-less) namespace.
//! Cross-file visibility is governed by `import "…";` statements.
//!
//! The module exposes:
//! 1. [`index_file`] — FQN → [`Symbol`] map for one parsed file, plus the
//!    list of top-level FQNs.
//! 2. [`collect_type_use_sites`] — every user-type reference with its
//!    enclosing scope, so hover / definition / unknown-type diagnostics can
//!    answer "what is named here".
//! 3. [`WorkspaceIndex`] — merges every file's map and resolves references
//!    through the workspace's import graph.

use crate::ast::*;
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxHashSet};
use smol_str::SmolStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Module,
    Struct,
    Union,
    Interface,
    Enum,
    EnumValue,
    Const,
    Field,
    Method,
}

impl SymbolKind {
    /// Symbols that a type reference may legitimately resolve to.
    pub fn is_type(self) -> bool {
        matches!(
            self,
            SymbolKind::Struct | SymbolKind::Union | SymbolKind::Interface | SymbolKind::Enum
        )
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
    pub detail: Option<String>,
    pub doc: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct FileSymbols {
    /// FQN -> Symbol, in source order.
    pub entries: IndexMap<SmolStr, Symbol>,
    /// Top-level FQNs declared in this file.
    pub top_level: Vec<SmolStr>,
    /// The file's module name (empty for module-less files).
    pub module: SmolStr,
}

/// Build the symbol map for a single file. `source` is only used to extract
/// doc comments from the raw text.
pub fn index_file(file_uri: &FileUri, file: &File, source: &str) -> FileSymbols {
    let mut out = FileSymbols::default();
    let module = file.module.as_ref().map(|m| m.name.clone()).unwrap_or_default();
    out.module = module.clone();

    for d in &file.decls {
        match d {
            Decl::Struct(s) => {
                let fqn = join(&module, &s.name.text);
                out.top_level.push(SmolStr::new(&fqn));
                index_struct(&module, s, file_uri, source, &mut out);
            }
            Decl::Union(u) => {
                let fqn = join(&module, &u.name.text);
                out.top_level.push(SmolStr::new(&fqn));
                index_union(&module, u, file_uri, source, &mut out);
            }
            Decl::Interface(i) => {
                let fqn = join(&module, &i.name.text);
                out.top_level.push(SmolStr::new(&fqn));
                index_interface(&module, i, file_uri, source, &mut out);
            }
            Decl::Enum(e) => {
                let fqn = join(&module, &e.name.text);
                out.top_level.push(SmolStr::new(&fqn));
                index_enum(&module, e, file_uri, source, &mut out);
            }
            Decl::Const(c) => {
                let fqn = join(&module, &c.name.text);
                out.top_level.push(SmolStr::new(&fqn));
                insert_const(&module, c, file_uri, source, &mut out);
            }
        }
    }

    out
}

fn index_struct(scope: &str, s: &Struct, file_uri: &FileUri, source: &str, out: &mut FileSymbols) {
    let fqn = join(scope, &s.name.text);
    insert(
        out,
        Symbol {
            fqn: SmolStr::new(&fqn),
            name: s.name.text.clone(),
            kind: SymbolKind::Struct,
            file: file_uri.clone(),
            name_span: s.name.span,
            full_span: s.span,
            detail: None,
            doc: extract_doc_comment(source, s.span.start),
        },
    );
    for m in &s.members {
        match m {
            StructMember::Field(f) => insert_field(&fqn, f, file_uri, source, out),
            StructMember::Const(c) => insert_const(&fqn, c, file_uri, source, out),
            StructMember::Enum(e) => index_enum(&fqn, e, file_uri, source, out),
        }
    }
}

fn index_union(scope: &str, u: &Union, file_uri: &FileUri, source: &str, out: &mut FileSymbols) {
    let fqn = join(scope, &u.name.text);
    insert(
        out,
        Symbol {
            fqn: SmolStr::new(&fqn),
            name: u.name.text.clone(),
            kind: SymbolKind::Union,
            file: file_uri.clone(),
            name_span: u.name.span,
            full_span: u.span,
            detail: None,
            doc: extract_doc_comment(source, u.span.start),
        },
    );
    for f in &u.fields {
        insert_field(&fqn, f, file_uri, source, out);
    }
}

fn index_interface(scope: &str, i: &Interface, file_uri: &FileUri, source: &str, out: &mut FileSymbols) {
    let fqn = join(scope, &i.name.text);
    insert(
        out,
        Symbol {
            fqn: SmolStr::new(&fqn),
            name: i.name.text.clone(),
            kind: SymbolKind::Interface,
            file: file_uri.clone(),
            name_span: i.name.span,
            full_span: i.span,
            detail: None,
            doc: extract_doc_comment(source, i.span.start),
        },
    );
    for m in &i.members {
        match m {
            InterfaceMember::Method(meth) => {
                let mf = join(&fqn, &meth.name.text);
                insert(
                    out,
                    Symbol {
                        fqn: SmolStr::new(&mf),
                        name: meth.name.text.clone(),
                        kind: SymbolKind::Method,
                        file: file_uri.clone(),
                        name_span: meth.name.span,
                        full_span: meth.span,
                        detail: meth.ordinal.as_ref().map(|o| format!("@{}", o.value)),
                        doc: extract_doc_comment(source, meth.span.start),
                    },
                );
            }
            InterfaceMember::Const(c) => insert_const(&fqn, c, file_uri, source, out),
            InterfaceMember::Enum(e) => index_enum(&fqn, e, file_uri, source, out),
        }
    }
}

fn index_enum(scope: &str, e: &EnumDecl, file_uri: &FileUri, source: &str, out: &mut FileSymbols) {
    let fqn = join(scope, &e.name.text);
    insert(
        out,
        Symbol {
            fqn: SmolStr::new(&fqn),
            name: e.name.text.clone(),
            kind: SymbolKind::Enum,
            file: file_uri.clone(),
            name_span: e.name.span,
            full_span: e.span,
            detail: None,
            doc: extract_doc_comment(source, e.span.start),
        },
    );
    for v in &e.values {
        let vf = join(&fqn, &v.name.text);
        insert(
            out,
            Symbol {
                fqn: SmolStr::new(&vf),
                name: v.name.text.clone(),
                kind: SymbolKind::EnumValue,
                file: file_uri.clone(),
                name_span: v.name.span,
                full_span: v.span,
                detail: None,
                doc: extract_doc_comment(source, v.span.start),
            },
        );
    }
}

fn insert_field(scope: &str, f: &Field, file_uri: &FileUri, source: &str, out: &mut FileSymbols) {
    let ff = join(scope, &f.name.text);
    insert(
        out,
        Symbol {
            fqn: SmolStr::new(&ff),
            name: f.name.text.clone(),
            kind: SymbolKind::Field,
            file: file_uri.clone(),
            name_span: f.name.span,
            full_span: f.span,
            detail: Some(f.ty.label.clone()),
            doc: extract_doc_comment(source, f.span.start),
        },
    );
}

fn insert_const(scope: &str, c: &ConstDecl, file_uri: &FileUri, source: &str, out: &mut FileSymbols) {
    let cf = join(scope, &c.name.text);
    insert(
        out,
        Symbol {
            fqn: SmolStr::new(&cf),
            name: c.name.text.clone(),
            kind: SymbolKind::Const,
            file: file_uri.clone(),
            name_span: c.name.span,
            full_span: c.span,
            detail: Some(c.ty.label.clone()),
            doc: extract_doc_comment(source, c.span.start),
        },
    );
}

fn insert(out: &mut FileSymbols, sym: Symbol) {
    out.entries.entry(sym.fqn.clone()).or_insert(sym);
}

fn join(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", scope, name)
    }
}

/// Walk the raw source backwards from `decl_start` and collect the
/// contiguous `//`-comment lines immediately preceding it. Stops at the first
/// non-comment / blank line.
pub fn extract_doc_comment(source: &str, decl_start: u32) -> Option<String> {
    let start = decl_start as usize;
    if start > source.len() {
        return None;
    }
    let line_of_decl = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let above = &source[..line_of_decl];
    let mut doc_lines: Vec<String> = Vec::new();
    for line in above.rsplit_terminator('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("//") {
            doc_lines.push(rest.trim_start().to_string());
        } else {
            break;
        }
    }
    if doc_lines.is_empty() {
        return None;
    }
    doc_lines.reverse();
    Some(doc_lines.join("\n"))
}

// ── Type-use sites ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TypeUseSite {
    /// Dotted path tokens as written in source (e.g. ["foo", "Bar"]).
    pub path: Vec<Ident>,
    /// Enclosing scope FQN — drives the resolver's scope walk.
    pub enclosing_scope: SmolStr,
    /// Span of the reference.
    pub span: ByteSpan,
}

pub fn collect_type_use_sites(file: &File) -> Vec<TypeUseSite> {
    let module = file.module.as_ref().map(|m| m.name.as_str()).unwrap_or("");
    let mut out = Vec::new();
    for d in &file.decls {
        match d {
            Decl::Struct(s) => {
                let scope = join(module, &s.name.text);
                for m in &s.members {
                    match m {
                        StructMember::Field(f) => push_type_refs(&scope, &f.ty, &mut out),
                        StructMember::Const(c) => push_type_refs(&scope, &c.ty, &mut out),
                        StructMember::Enum(_) => {}
                    }
                }
            }
            Decl::Union(u) => {
                let scope = join(module, &u.name.text);
                for f in &u.fields {
                    push_type_refs(&scope, &f.ty, &mut out);
                }
            }
            Decl::Interface(i) => {
                let scope = join(module, &i.name.text);
                for m in &i.members {
                    match m {
                        InterfaceMember::Method(meth) => {
                            for p in &meth.params {
                                push_type_refs(&scope, &p.ty, &mut out);
                            }
                            if let Some(resp) = &meth.response {
                                for p in resp {
                                    push_type_refs(&scope, &p.ty, &mut out);
                                }
                            }
                        }
                        InterfaceMember::Const(c) => push_type_refs(&scope, &c.ty, &mut out),
                        InterfaceMember::Enum(_) => {}
                    }
                }
            }
            Decl::Const(c) => push_type_refs(module, &c.ty, &mut out),
            Decl::Enum(_) => {}
        }
    }
    out
}

fn push_type_refs(scope: &str, ty: &TypeRef, out: &mut Vec<TypeUseSite>) {
    for r in &ty.refs {
        out.push(TypeUseSite {
            path: r.path.clone(),
            enclosing_scope: SmolStr::new(scope),
            span: r.span,
        });
    }
}

// ── Workspace index ───────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct WorkspaceIndex {
    by_fqn: FxHashMap<SmolStr, Symbol>,
    by_file: FxHashMap<FileUri, FileSymbols>,
    /// URI -> set of URIs visible to it (itself + resolved imports).
    visible_from: FxHashMap<FileUri, FxHashSet<FileUri>>,
}

impl WorkspaceIndex {
    pub fn build(ws: &Workspace) -> Self {
        let mut by_fqn: FxHashMap<SmolStr, Symbol> = FxHashMap::default();
        let mut by_file: FxHashMap<FileUri, FileSymbols> = FxHashMap::default();
        let mut direct_imports: FxHashMap<FileUri, FxHashSet<FileUri>> = FxHashMap::default();

        for (uri, state) in ws.files() {
            let fs = index_file(uri, &state.analysis.file, &state.source);
            for (fqn, sym) in &fs.entries {
                by_fqn.entry(fqn.clone()).or_insert_with(|| sym.clone());
            }
            let mut deps: FxHashSet<FileUri> = FxHashSet::default();
            for imp in &state.analysis.file.imports {
                if let Some(target) = ws.resolve_import_path(uri, &imp.path.value) {
                    deps.insert(target);
                }
            }
            direct_imports.insert(uri.clone(), deps);
            by_file.insert(uri.clone(), fs);
        }

        let mut visible_from: FxHashMap<FileUri, FxHashSet<FileUri>> = FxHashMap::default();
        for uri in by_file.keys() {
            let mut seen: FxHashSet<FileUri> = FxHashSet::default();
            seen.insert(uri.clone());
            if let Some(deps) = direct_imports.get(uri) {
                for d in deps {
                    seen.insert(d.clone());
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

    pub fn visible_files(&self, uri: &FileUri) -> Option<&FxHashSet<FileUri>> {
        self.visible_from.get(uri)
    }

    /// Resolve a dotted type path (`Foo`, `foo.Bar`, `Outer.Inner`) appearing
    /// inside `enclosing_scope` in file `importer`.
    pub fn resolve_type(
        &self,
        importer: &FileUri,
        enclosing_scope: &str,
        path: &[Ident],
    ) -> Resolution {
        if path.is_empty() {
            return Resolution::Unknown { candidates: Vec::new() };
        }
        let candidates = scope_candidates(enclosing_scope, path);
        let local = self.by_file.get(importer);
        for cand in &candidates {
            // Prefer a symbol declared in the importing file itself.
            if let Some(sym) = local.and_then(|fs| fs.entries.get(cand.as_str())) {
                if sym.kind.is_type() {
                    return Resolution::Found { symbol: sym.clone(), visibility_ok: true };
                }
            }
            if let Some(sym) = self.by_fqn.get(cand.as_str()) {
                if !sym.kind.is_type() {
                    continue;
                }
                let visibility_ok = self
                    .visible_from
                    .get(importer)
                    .is_some_and(|set| set.contains(&sym.file));
                return Resolution::Found { symbol: sym.clone(), visibility_ok };
            }
        }
        Resolution::Unknown { candidates }
    }
}

#[derive(Debug, Clone)]
pub enum Resolution {
    Found {
        symbol: Symbol,
        /// When false, the symbol exists but isn't visible to the importer —
        /// typically because the file that defines it is not imported here.
        visibility_ok: bool,
    },
    Unknown {
        candidates: Vec<String>,
    },
}

/// Given `scope = "M.Outer"` and `path = ["Foo", "Bar"]`, produce the
/// scope-walk candidates in innermost-out order:
/// ["M.Outer.Foo.Bar", "M.Foo.Bar", "Foo.Bar"].
fn scope_candidates(scope: &str, path: &[Ident]) -> Vec<String> {
    let tail = path.iter().map(|i| i.text.as_str()).collect::<Vec<_>>().join(".");
    let mut out = Vec::new();
    let mut cur = scope.to_string();
    loop {
        if cur.is_empty() {
            out.push(tail.clone());
            break;
        }
        out.push(format!("{}.{}", cur, tail));
        match cur.rfind('.') {
            Some(i) => cur.truncate(i),
            None => cur.clear(),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::vfs::Workspace;

    fn fu() -> FileUri {
        FileUri("file:///t.mojom".into())
    }

    #[test]
    fn extracts_doc_comments() {
        let src = "// doc line 1\n// doc line 2\nstruct Foo {};\n";
        let parsed = parse(src);
        let fs = index_file(&fu(), &parsed.file, src);
        let sym = fs.entries.get("Foo").unwrap();
        assert_eq!(sym.doc.as_deref(), Some("doc line 1\ndoc line 2"));
    }

    #[test]
    fn module_qualifies_top_level_names() {
        let src = "module foo.bar;\nstruct S { int32 id; };";
        let parsed = parse(src);
        let fs = index_file(&fu(), &parsed.file, src);
        assert!(fs.entries.contains_key("foo.bar.S"));
        assert!(fs.entries.contains_key("foo.bar.S.id"));
    }

    #[test]
    fn nested_enum_is_qualified() {
        let src = "interface I { enum E { kA, kB }; };";
        let parsed = parse(src);
        let fs = index_file(&fu(), &parsed.file, src);
        assert!(fs.entries.contains_key("I"));
        assert!(fs.entries.contains_key("I.E"));
        assert!(fs.entries.contains_key("I.E.kA"));
    }

    #[test]
    fn resolves_same_module_type() {
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.mojom",
            "module m;\nstruct Foo { int32 id; };\nstruct Bar { Foo f; };".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.mojom".into());
        let path = vec![Ident { text: "Foo".into(), span: ByteSpan::EMPTY }];
        match idx.resolve_type(&uri, "m.Bar", &path) {
            Resolution::Found { symbol, visibility_ok: true } => {
                assert_eq!(symbol.fqn.as_str(), "m.Foo");
            }
            other => panic!("expected Found, got {:?}", other),
        }
    }

    #[test]
    fn resolves_across_import() {
        let mut ws = Workspace::new();
        ws.update("file:///dep.mojom", "module dep;\nstruct Foo { int32 id; };".into());
        ws.update(
            "file:///main.mojom",
            "import \"dep.mojom\";\nstruct Bar { dep.Foo f; };".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///main.mojom".into());
        let path = vec![
            Ident { text: "dep".into(), span: ByteSpan::EMPTY },
            Ident { text: "Foo".into(), span: ByteSpan::EMPTY },
        ];
        match idx.resolve_type(&uri, "Bar", &path) {
            Resolution::Found { symbol, visibility_ok: true } => {
                assert_eq!(symbol.fqn.as_str(), "dep.Foo");
            }
            other => panic!("expected cross-file Found, got {:?}", other),
        }
    }

    #[test]
    fn unimported_type_is_not_visible() {
        let mut ws = Workspace::new();
        ws.update("file:///dep.mojom", "module dep;\nstruct Foo { int32 id; };".into());
        ws.update("file:///main.mojom", "struct Bar { dep.Foo f; };".into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///main.mojom".into());
        let path = vec![
            Ident { text: "dep".into(), span: ByteSpan::EMPTY },
            Ident { text: "Foo".into(), span: ByteSpan::EMPTY },
        ];
        match idx.resolve_type(&uri, "Bar", &path) {
            Resolution::Found { visibility_ok: false, .. } => {}
            other => panic!("expected visibility_ok=false, got {:?}", other),
        }
    }

    #[test]
    fn scope_walk_order() {
        let cand = scope_candidates("A.B", &[Ident { text: "C".into(), span: ByteSpan::EMPTY }]);
        assert_eq!(cand, vec!["A.B.C".to_string(), "A.C".into(), "C".into()]);
    }
}
