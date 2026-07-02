//! Name resolution.
//!
//! Cap'n Proto has a simpler scoping model than proto3: no packages, types
//! are resolved innermost-out through the chain of enclosing struct and
//! interface bodies. `using X = Y;` introduces a local alias that the
//! resolver substitutes before lookup.
//!
//! The module exposes three things:
//! 1. [`index_file`] — flat FQN → [`Symbol`] map for one parsed file,
//!    plus a list of type-use sites and file-level using-aliases.
//! 2. [`WorkspaceIndex`] — merges every file's map and resolves
//!    `import "…"` references through the workspace's VFS.
//! 3. [`WorkspaceIndex::resolve_type`] — given a type reference at a
//!    particular enclosing scope, walk outward until a candidate FQN
//!    matches and is visible to the importer.

use crate::ast::*;
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxHashSet};
use smol_str::SmolStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Struct,
    Enum,
    EnumMember,
    Interface,
    Method,
    Field,
    Union,
    Group,
    Constant,
    Annotation,
    Alias,
}

impl SymbolKind {
    pub fn is_type(self) -> bool {
        matches!(self, SymbolKind::Struct | SymbolKind::Enum | SymbolKind::Interface)
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

/// One declaration of a file-level `using Alias = …;`. We track three shapes:
/// - `Alias = import "path.capnp"` (a whole file alias)
/// - `Alias = import "path.capnp".Foo.Bar` (a specific type in another file)
/// - `Alias = Type.Path` (a type alias within the current file)
#[derive(Debug, Clone)]
pub enum UsingAlias {
    File {
        name: SmolStr,
        import_path: String,
        span: ByteSpan,
    },
    ImportType {
        name: SmolStr,
        import_path: String,
        /// Dotted path inside the imported file.
        target: Vec<SmolStr>,
        span: ByteSpan,
    },
    Type {
        name: SmolStr,
        /// Dotted path tokens as they appear in source.
        target: Vec<SmolStr>,
        span: ByteSpan,
    },
}

impl UsingAlias {
    pub fn name(&self) -> &SmolStr {
        match self {
            UsingAlias::File { name, .. }
            | UsingAlias::ImportType { name, .. }
            | UsingAlias::Type { name, .. } => name,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct FileSymbols {
    /// FQN -> Symbol, in source order.
    pub entries: IndexMap<SmolStr, Symbol>,
    /// Top-level FQNs declared in this file.
    pub top_level: Vec<SmolStr>,
    /// File-level using aliases, in source order.
    pub aliases: Vec<UsingAlias>,
}

/// Build the symbol map for a single file. `source` is only used to extract
/// doc comments from the raw text.
pub fn index_file(file_uri: &FileUri, file: &File, source: &str) -> FileSymbols {
    let mut out = FileSymbols::default();

    for d in &file.decls {
        match d {
            Decl::Using(u) => {
                if let Some(alias) = using_to_alias(u, source) {
                    out.aliases.push(alias);
                }
            }
            Decl::Struct(s) => {
                let fqn = SmolStr::new(&s.name.text);
                out.top_level.push(fqn.clone());
                index_struct("", s, file_uri, source, &mut out);
            }
            Decl::Enum(e) => {
                let fqn = SmolStr::new(&e.name.text);
                out.top_level.push(fqn.clone());
                index_enum("", e, file_uri, source, &mut out);
            }
            Decl::Interface(i) => {
                let fqn = SmolStr::new(&i.name.text);
                out.top_level.push(fqn.clone());
                index_interface("", i, file_uri, source, &mut out);
            }
            Decl::Const(c) => {
                let fqn = SmolStr::new(&c.name.text);
                out.top_level.push(fqn.clone());
                out.entries.insert(
                    fqn.clone(),
                    Symbol {
                        fqn: fqn.clone(),
                        name: c.name.text.clone(),
                        kind: SymbolKind::Constant,
                        file: file_uri.clone(),
                        name_span: c.name.span,
                        full_span: c.span,
                        detail: Some(type_label(&c.ty)),
                        doc: extract_doc_comment(source, c.span.start),
                    },
                );
            }
            Decl::Annotation(a) => {
                let fqn = SmolStr::new(&a.name.text);
                out.top_level.push(fqn.clone());
                out.entries.insert(
                    fqn.clone(),
                    Symbol {
                        fqn: fqn.clone(),
                        name: a.name.text.clone(),
                        kind: SymbolKind::Annotation,
                        file: file_uri.clone(),
                        name_span: a.name.span,
                        full_span: a.span,
                        detail: a.ty.as_ref().map(type_label),
                        doc: extract_doc_comment(source, a.span.start),
                    },
                );
            }
            Decl::TopAnnotation(_) => {}
        }
    }

    out
}

fn using_to_alias(u: &Using, _source: &str) -> Option<UsingAlias> {
    let name = u.name.as_ref()?;
    if let Some(path) = &u.import_path {
        if u.import_target.is_empty() {
            return Some(UsingAlias::File {
                name: name.text.clone(),
                import_path: path.value.clone(),
                span: u.span,
            });
        }
        return Some(UsingAlias::ImportType {
            name: name.text.clone(),
            import_path: path.value.clone(),
            target: u.import_target.iter().map(|i| i.text.clone()).collect(),
            span: u.span,
        });
    }
    // Local type alias: `using SlotId = Int16;` / `using T = Outer.Inner;`.
    Some(UsingAlias::Type {
        name: name.text.clone(),
        target: u.target.iter().map(|i| i.text.clone()).collect(),
        span: u.span,
    })
}

fn index_struct(
    scope: &str,
    s: &Struct,
    file_uri: &FileUri,
    source: &str,
    out: &mut FileSymbols,
) {
    let fqn = join(scope, &s.name.text);
    let sfqn = SmolStr::new(&fqn);
    out.entries.insert(
        sfqn.clone(),
        Symbol {
            fqn: sfqn,
            name: s.name.text.clone(),
            kind: SymbolKind::Struct,
            file: file_uri.clone(),
            name_span: s.name.span,
            full_span: s.span,
            detail: if s.type_params.is_empty() {
                None
            } else {
                Some(format!(
                    "({})",
                    s.type_params.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join(", ")
                ))
            },
            doc: extract_doc_comment(source, s.span.start),
        },
    );
    for m in &s.members {
        match m {
            StructMember::Field(f) => index_field(&fqn, f, file_uri, source, out),
            StructMember::AnonUnion(ub) => {
                for f in &ub.members {
                    index_field(&fqn, f, file_uri, source, out);
                }
            }
            StructMember::Struct(inner) => index_struct(&fqn, inner, file_uri, source, out),
            StructMember::Enum(inner) => index_enum(&fqn, inner, file_uri, source, out),
            StructMember::Interface(inner) => {
                index_interface(&fqn, inner, file_uri, source, out)
            }
            StructMember::Const(c) => {
                let cfqn = join(&fqn, &c.name.text);
                let sc = SmolStr::new(&cfqn);
                out.entries.insert(
                    sc.clone(),
                    Symbol {
                        fqn: sc,
                        name: c.name.text.clone(),
                        kind: SymbolKind::Constant,
                        file: file_uri.clone(),
                        name_span: c.name.span,
                        full_span: c.span,
                        detail: Some(type_label(&c.ty)),
                        doc: extract_doc_comment(source, c.span.start),
                    },
                );
            }
            StructMember::Annotation(_) | StructMember::Using(_) => {}
        }
    }
}

fn index_field(
    parent_fqn: &str,
    f: &Field,
    file_uri: &FileUri,
    source: &str,
    out: &mut FileSymbols,
) {
    let ff = join(parent_fqn, &f.name.text);
    let sf = SmolStr::new(&ff);
    let (kind, detail) = match &f.body {
        FieldBody::Slot { ty, .. } => (SymbolKind::Field, Some(type_label(ty))),
        FieldBody::NamedUnion(_) => (SymbolKind::Union, Some("union".into())),
        FieldBody::NamedGroup(_) => (SymbolKind::Group, Some("group".into())),
    };
    out.entries.insert(
        sf.clone(),
        Symbol {
            fqn: sf,
            name: f.name.text.clone(),
            kind,
            file: file_uri.clone(),
            name_span: f.name.span,
            full_span: f.span,
            detail,
            doc: extract_doc_comment(source, f.span.start),
        },
    );
    // Named union / group members live in the parent's scope too.
    match &f.body {
        FieldBody::NamedUnion(ub) => {
            for inner in &ub.members {
                index_field(parent_fqn, inner, file_uri, source, out);
            }
        }
        FieldBody::NamedGroup(gb) => {
            for inner in &gb.members {
                if let StructMember::Field(inf) = inner {
                    index_field(parent_fqn, inf, file_uri, source, out);
                }
            }
        }
        _ => {}
    }
}

fn index_enum(
    scope: &str,
    e: &EnumDecl,
    file_uri: &FileUri,
    source: &str,
    out: &mut FileSymbols,
) {
    let fqn = join(scope, &e.name.text);
    let sfqn = SmolStr::new(&fqn);
    out.entries.insert(
        sfqn.clone(),
        Symbol {
            fqn: sfqn,
            name: e.name.text.clone(),
            kind: SymbolKind::Enum,
            file: file_uri.clone(),
            name_span: e.name.span,
            full_span: e.span,
            detail: None,
            doc: extract_doc_comment(source, e.span.start),
        },
    );
    for en in &e.enumerants {
        let vf = join(&fqn, &en.name.text);
        let sv = SmolStr::new(&vf);
        out.entries.insert(
            sv.clone(),
            Symbol {
                fqn: sv,
                name: en.name.text.clone(),
                kind: SymbolKind::EnumMember,
                file: file_uri.clone(),
                name_span: en.name.span,
                full_span: en.span,
                detail: en.ordinal.as_ref().map(|o| format!("@{}", o.value)),
                doc: extract_doc_comment(source, en.span.start),
            },
        );
    }
}

fn index_interface(
    scope: &str,
    i: &Interface,
    file_uri: &FileUri,
    source: &str,
    out: &mut FileSymbols,
) {
    let fqn = join(scope, &i.name.text);
    let sfqn = SmolStr::new(&fqn);
    out.entries.insert(
        sfqn.clone(),
        Symbol {
            fqn: sfqn,
            name: i.name.text.clone(),
            kind: SymbolKind::Interface,
            file: file_uri.clone(),
            name_span: i.name.span,
            full_span: i.span,
            detail: if i.superclasses.is_empty() {
                None
            } else {
                Some(format!(
                    "extends ({})",
                    i.superclasses.iter().map(type_label).collect::<Vec<_>>().join(", ")
                ))
            },
            doc: extract_doc_comment(source, i.span.start),
        },
    );
    for m in &i.methods {
        let mf = join(&fqn, &m.name.text);
        let sm = SmolStr::new(&mf);
        out.entries.insert(
            sm.clone(),
            Symbol {
                fqn: sm,
                name: m.name.text.clone(),
                kind: SymbolKind::Method,
                file: file_uri.clone(),
                name_span: m.name.span,
                full_span: m.span,
                detail: m.ordinal.as_ref().map(|o| format!("@{}", o.value)),
                doc: extract_doc_comment(source, m.span.start),
            },
        );
    }
    for n in &i.nested {
        match n {
            StructMember::Struct(s) => index_struct(&fqn, s, file_uri, source, out),
            StructMember::Enum(e) => index_enum(&fqn, e, file_uri, source, out),
            StructMember::Interface(nested_i) => {
                index_interface(&fqn, nested_i, file_uri, source, out)
            }
            _ => {}
        }
    }
}

pub fn type_label(t: &TypeRef) -> String {
    let mut s = t.path.iter().map(|i| i.text.as_str()).collect::<Vec<_>>().join(".");
    if !t.args.is_empty() {
        let args = t.args.iter().map(type_label).collect::<Vec<_>>().join(", ");
        s.push('(');
        s.push_str(&args);
        s.push(')');
    }
    s
}

fn join(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", scope, name)
    }
}

/// Walk the raw source backwards from `decl_start` and collect the
/// contiguous `#`-comment lines immediately preceding it. Stops at the
/// first non-comment / blank line.
pub fn extract_doc_comment(source: &str, decl_start: u32) -> Option<String> {
    let start = decl_start as usize;
    let line_of_decl = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let above = &source[..line_of_decl];
    let mut doc_lines: Vec<String> = Vec::new();
    for line in above.rsplit_terminator('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() { break; }
        if let Some(rest) = trimmed.strip_prefix('#') {
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

// ── Type-use sites: collected so hover / definition can answer
// "what type is named at this offset" without re-walking the AST. ─────────

#[derive(Debug, Clone)]
pub struct TypeUseSite {
    /// When the reference is written as `import "foo.capnp".Foo`, this is the
    /// raw import string. [`path`](Self::path) holds the dotted tail.
    pub import_path: Option<String>,
    /// Dotted path tokens as written in source (e.g. ["Foo", "Bar"]).
    pub path: Vec<Ident>,
    /// Enclosing scope FQN — drives the resolver's scope walk.
    pub enclosing_scope: SmolStr,
    /// Span of the full reference (first ident through last ident).
    pub span: ByteSpan,
}

pub fn collect_type_use_sites(file: &File) -> Vec<TypeUseSite> {
    let mut out = Vec::new();
    for d in &file.decls {
        match d {
            Decl::Using(u) => visit_using("", u, &mut out),
            Decl::Struct(s) => visit_struct("", s, &mut out),
            Decl::Enum(_) => {}
            Decl::Interface(i) => visit_interface("", i, &mut out),
            Decl::Const(c) => visit_type_ref("", &c.ty, &mut out),
            Decl::Annotation(a) => {
                if let Some(ty) = &a.ty {
                    visit_type_ref("", ty, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}

fn visit_using(scope: &str, u: &Using, out: &mut Vec<TypeUseSite>) {
    let Some(path) = u.import_path.as_ref() else {
        // Plain local alias — the RHS is an ordinary type reference, so it
        // participates in hover / definition / unknown-type checking.
        if let (Some(first), Some(last)) = (u.target.first(), u.target.last()) {
            out.push(TypeUseSite {
                import_path: None,
                path: u.target.clone(),
                enclosing_scope: SmolStr::new(scope),
                span: first.span.join(last.span),
            });
        }
        return;
    };
    // Site covering just the `"…"` string — lets F12/hover on the filename
    // resolve to the imported file itself.
    out.push(TypeUseSite {
        import_path: Some(path.value.clone()),
        path: Vec::new(),
        enclosing_scope: SmolStr::new(scope),
        span: path.span,
    });
    // Site covering the `.Foo.Bar` dotted tail, if present.
    if let (Some(first), Some(last)) = (u.import_target.first(), u.import_target.last()) {
        out.push(TypeUseSite {
            import_path: Some(path.value.clone()),
            path: u.import_target.clone(),
            enclosing_scope: SmolStr::new(scope),
            span: first.span.join(last.span),
        });
    }
}

fn visit_struct(scope: &str, s: &Struct, out: &mut Vec<TypeUseSite>) {
    let fqn = join(scope, &s.name.text);
    for sup in &s.annotations {
        visit_annotation_app(scope, sup, out);
    }
    for m in &s.members {
        visit_member(&fqn, m, out);
    }
}

fn visit_interface(scope: &str, i: &Interface, out: &mut Vec<TypeUseSite>) {
    let fqn = join(scope, &i.name.text);
    for sup in &i.superclasses {
        visit_type_ref(&fqn, sup, out);
    }
    for m in &i.methods {
        if let Some(params) = &m.params {
            for p in params {
                visit_type_ref(&fqn, &p.ty, out);
            }
        }
        if let Some(results) = &m.results {
            for p in results {
                visit_type_ref(&fqn, &p.ty, out);
            }
        }
    }
    for n in &i.nested {
        visit_member(&fqn, n, out);
    }
}

fn visit_member(scope: &str, m: &StructMember, out: &mut Vec<TypeUseSite>) {
    match m {
        StructMember::Field(f) => {
            if let FieldBody::Slot { ty, .. } = &f.body {
                visit_type_ref(scope, ty, out);
            }
            if let FieldBody::NamedUnion(ub) = &f.body {
                for inner in &ub.members {
                    visit_member(scope, &StructMember::Field(inner.clone()), out);
                }
            }
            if let FieldBody::NamedGroup(gb) = &f.body {
                for inner in &gb.members {
                    visit_member(scope, inner, out);
                }
            }
        }
        StructMember::AnonUnion(ub) => {
            for inner in &ub.members {
                visit_member(scope, &StructMember::Field(inner.clone()), out);
            }
        }
        StructMember::Struct(s) => visit_struct(scope, s, out),
        StructMember::Enum(_) => {}
        StructMember::Interface(i) => visit_interface(scope, i, out),
        StructMember::Const(c) => visit_type_ref(scope, &c.ty, out),
        StructMember::Annotation(a) => {
            if let Some(ty) = &a.ty {
                visit_type_ref(scope, ty, out);
            }
        }
        StructMember::Using(u) => visit_using(scope, u, out),
    }
}

fn visit_type_ref(scope: &str, t: &TypeRef, out: &mut Vec<TypeUseSite>) {
    if let Some(import) = &t.import_path {
        // Site over the `"…"` string alone so F12 on the filename jumps to
        // the imported file. The dotted tail (if any) gets its own narrower
        // site below so cursor-on-ident resolves to the referenced type.
        out.push(TypeUseSite {
            import_path: Some(import.value.clone()),
            path: Vec::new(),
            enclosing_scope: SmolStr::new(scope),
            span: import.span,
        });
        if let (Some(first), Some(last)) = (t.path.first(), t.path.last()) {
            out.push(TypeUseSite {
                import_path: Some(import.value.clone()),
                path: t.path.clone(),
                enclosing_scope: SmolStr::new(scope),
                span: first.span.join(last.span),
            });
        }
    } else if !t.path.is_empty() {
        out.push(TypeUseSite {
            import_path: None,
            path: t.path.clone(),
            enclosing_scope: SmolStr::new(scope),
            span: t.span,
        });
    }
    for a in &t.args {
        visit_type_ref(scope, a, out);
    }
}

fn visit_annotation_app(_scope: &str, _a: &AnnotationApp, _out: &mut Vec<TypeUseSite>) {
    // Annotation applications aren't type references in the usual sense —
    // they resolve into the annotation-declaration namespace, which we don't
    // model in this pass. Left as a follow-up.
}

// ── Workspace-wide index ──────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct WorkspaceIndex {
    by_fqn: FxHashMap<SmolStr, Symbol>,
    by_file: FxHashMap<FileUri, FileSymbols>,
    /// URI -> file-alias name -> resolved file uri. Populated from each
    /// file's `using X = import "…";` aliases.
    file_aliases: FxHashMap<FileUri, FxHashMap<SmolStr, FileUri>>,
    /// URI -> type-alias name -> (resolved file uri, target path within it).
    /// Populated from `using X = import "…".Foo.Bar;` and
    /// `using import "…".Tag;` forms.
    type_aliases: FxHashMap<FileUri, FxHashMap<SmolStr, (FileUri, Vec<SmolStr>)>>,
    /// URI -> local-alias name -> target dotted path in the same file.
    /// Populated from plain `using SlotId = Int16;` / `using T = A.B;` forms.
    local_aliases: FxHashMap<FileUri, FxHashMap<SmolStr, Vec<SmolStr>>>,
    /// URI -> raw `import "…"` string -> resolved file uri. Lets inline
    /// `import "foo.capnp".Bar` expressions resolve at query time without
    /// needing the workspace.
    inline_imports: FxHashMap<FileUri, FxHashMap<String, FileUri>>,
    /// URI -> set of URIs it can see (itself + direct imports).
    visible_from: FxHashMap<FileUri, FxHashSet<FileUri>>,
}

impl WorkspaceIndex {
    pub fn build(ws: &Workspace) -> Self {
        let mut by_fqn: FxHashMap<SmolStr, Symbol> = FxHashMap::default();
        let mut by_file: FxHashMap<FileUri, FileSymbols> = FxHashMap::default();
        let mut file_aliases: FxHashMap<FileUri, FxHashMap<SmolStr, FileUri>> =
            FxHashMap::default();
        let mut type_aliases: FxHashMap<FileUri, FxHashMap<SmolStr, (FileUri, Vec<SmolStr>)>> =
            FxHashMap::default();
        let mut local_aliases: FxHashMap<FileUri, FxHashMap<SmolStr, Vec<SmolStr>>> =
            FxHashMap::default();
        let mut inline_imports: FxHashMap<FileUri, FxHashMap<String, FileUri>> =
            FxHashMap::default();
        let mut direct_imports: FxHashMap<FileUri, FxHashSet<FileUri>> = FxHashMap::default();

        for (uri, state) in ws.files() {
            let fs = index_file(uri, &state.analysis.file, &state.source);
            for (fqn, sym) in &fs.entries {
                by_fqn.entry(fqn.clone()).or_insert_with(|| sym.clone());
            }
            let mut aliases: FxHashMap<SmolStr, FileUri> = FxHashMap::default();
            let mut t_aliases: FxHashMap<SmolStr, (FileUri, Vec<SmolStr>)> = FxHashMap::default();
            let mut l_aliases: FxHashMap<SmolStr, Vec<SmolStr>> = FxHashMap::default();
            let mut deps: FxHashSet<FileUri> = FxHashSet::default();
            for a in &fs.aliases {
                match a {
                    UsingAlias::File { name, import_path, .. } => {
                        if let Some(target) = ws.resolve_import_path(uri, import_path) {
                            aliases.insert(name.clone(), target.clone());
                            deps.insert(target);
                        }
                    }
                    UsingAlias::ImportType { name, import_path, target, .. } => {
                        if let Some(file) = ws.resolve_import_path(uri, import_path) {
                            t_aliases.insert(name.clone(), (file.clone(), target.clone()));
                            deps.insert(file);
                        }
                    }
                    UsingAlias::Type { name, target, .. } => {
                        if !target.is_empty() {
                            l_aliases.insert(name.clone(), target.clone());
                        }
                    }
                }
            }
            // Also track inline `import "…"` expressions in type positions so
            // the importer can see the target file even without a `using`.
            let mut inlines: FxHashMap<String, FileUri> = FxHashMap::default();
            for site in collect_type_use_sites(&state.analysis.file) {
                if let Some(import_path) = &site.import_path {
                    if let Some(target) = ws.resolve_import_path(uri, import_path) {
                        deps.insert(target.clone());
                        inlines.insert(import_path.clone(), target);
                    }
                }
            }
            file_aliases.insert(uri.clone(), aliases);
            type_aliases.insert(uri.clone(), t_aliases);
            local_aliases.insert(uri.clone(), l_aliases);
            inline_imports.insert(uri.clone(), inlines);
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

        WorkspaceIndex {
            by_fqn,
            by_file,
            file_aliases,
            type_aliases,
            local_aliases,
            inline_imports,
            visible_from,
        }
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

    /// Resolve a dotted path (`Foo`, `Foo.Bar`, `Cxx.Baz`) appearing inside
    /// `enclosing_scope` in file `importer`.
    pub fn resolve_type(
        &self,
        importer: &FileUri,
        enclosing_scope: &str,
        path: &[Ident],
    ) -> Resolution {
        self.resolve_type_with_import(importer, enclosing_scope, None, path)
    }

    /// Like [`resolve_type`](Self::resolve_type), but also accepts an inline
    /// `import "…"` prefix. When `import_path` is `Some`, resolution goes
    /// directly through that file and ignores the scope walk.
    pub fn resolve_type_with_import(
        &self,
        importer: &FileUri,
        enclosing_scope: &str,
        import_path: Option<&str>,
        path: &[Ident],
    ) -> Resolution {
        self.resolve_impl(importer, enclosing_scope, import_path, path, 0)
    }

    fn resolve_impl(
        &self,
        importer: &FileUri,
        enclosing_scope: &str,
        import_path: Option<&str>,
        path: &[Ident],
        alias_depth: u8,
    ) -> Resolution {
        // Inline `import "foo.capnp"[.Path]` — resolve into the named file.
        if let Some(import_path) = import_path {
            let target_file = self
                .inline_imports
                .get(importer)
                .and_then(|m| m.get(import_path))
                .cloned();
            let Some(target_file) = target_file else {
                return Resolution::Unknown {
                    candidates: vec![format!("import \"{}\"", import_path)],
                };
            };
            if path.is_empty() {
                // Bare `import "…"` — resolves to the file itself.
                let span_source = path.first().map(|i| i.span).unwrap_or(ByteSpan::EMPTY);
                return Resolution::FileAlias { file: target_file, span_source };
            }
            let joined = path.iter().map(|i| i.text.as_str()).collect::<Vec<_>>().join(".");
            if let Some(fs) = self.by_file.get(&target_file) {
                if let Some(sym) = fs.entries.get(joined.as_str()) {
                    return Resolution::Found { symbol: sym.clone(), visibility_ok: true };
                }
            }
            return Resolution::Unknown {
                candidates: vec![format!("{} (in import \"{}\")", joined, import_path)],
            };
        }

        if path.is_empty() {
            return Resolution::Unknown { candidates: Vec::new() };
        }
        let head = path[0].text.as_str();
        let rest_names: Vec<&str> = path[1..].iter().map(|i| i.text.as_str()).collect();
        let rest_joined = rest_names.join(".");

        // File alias: `using Cxx = import "…";` then `Cxx.Foo` → look Foo up
        // in the aliased file's top-level scope.
        if let Some(aliases) = self.file_aliases.get(importer) {
            if let Some(target_file) = aliases.get(head) {
                if rest_names.is_empty() {
                    // Bare `Cxx` — resolve as the file itself (not a symbol).
                    return Resolution::FileAlias {
                        file: target_file.clone(),
                        span_source: path[0].span,
                    };
                }
                if let Some(fs) = self.by_file.get(target_file) {
                    if let Some(sym) = fs.entries.get(rest_joined.as_str()) {
                        return Resolution::Found {
                            symbol: sym.clone(),
                            visibility_ok: true,
                        };
                    }
                }
                return Resolution::Unknown {
                    candidates: vec![format!("{} (via alias {})", rest_joined, head)],
                };
            }
        }

        // Type alias: `using Tag = import "…".Tag;` then `Tag` (or `Tag.Sub`)
        // resolves to that type in the target file, possibly with an
        // additional suffix appended.
        if let Some(type_aliases) = self.type_aliases.get(importer) {
            if let Some((target_file, base)) = type_aliases.get(head) {
                let mut full: Vec<&str> = base.iter().map(|s| s.as_str()).collect();
                for r in &rest_names {
                    full.push(r);
                }
                let joined = full.join(".");
                if let Some(fs) = self.by_file.get(target_file) {
                    if let Some(sym) = fs.entries.get(joined.as_str()) {
                        return Resolution::Found {
                            symbol: sym.clone(),
                            visibility_ok: true,
                        };
                    }
                }
                return Resolution::Unknown {
                    candidates: vec![format!("{} (via alias {})", joined, head)],
                };
            }
        }

        // Local type alias: `using SlotId = Int16;` / `using T = Outer.Inner;`
        // — substitute the head with the alias target and resolve again from
        // the file's top-level scope (where the alias was declared). Bounded
        // so alias-to-alias cycles can't recurse forever.
        if let Some(local) = self.local_aliases.get(importer) {
            if let Some(target) = local.get(head) {
                const MAX_ALIAS_DEPTH: u8 = 8;
                if alias_depth >= MAX_ALIAS_DEPTH {
                    return Resolution::Unknown {
                        candidates: vec![format!("{} (alias cycle via {})", rest_joined, head)],
                    };
                }
                let mut expanded: Vec<Ident> = target
                    .iter()
                    .map(|s| Ident { text: s.clone(), span: path[0].span })
                    .collect();
                expanded.extend(path[1..].iter().cloned());
                return self.resolve_impl(importer, "", None, &expanded, alias_depth + 1);
            }
        }

        // Built-in types resolve as themselves, so an alias expansion that
        // lands on `Int16` (or a direct `List`, `Text`, …) is not "unknown".
        if rest_names.is_empty() && is_builtin(head) {
            return Resolution::Builtin { name: path[0].text.clone() };
        }

        let candidates = scope_candidates(enclosing_scope, path);
        let local = self.by_file.get(importer);
        for cand in &candidates {
            if let Some(sym) = local.and_then(|fs| fs.entries.get(cand.as_str())) {
                return Resolution::Found { symbol: sym.clone(), visibility_ok: true };
            }
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

    /// Files directly visible to `uri`.
    pub fn visible_files(&self, uri: &FileUri) -> Option<&FxHashSet<FileUri>> {
        self.visible_from.get(uri)
    }
}

#[derive(Debug, Clone)]
pub enum Resolution {
    Found {
        symbol: Symbol,
        /// When false, the symbol exists but isn't visible to the importer
        /// — typically because the relevant `using … = import "…";` is
        /// missing.
        visibility_ok: bool,
    },
    /// A bare reference to an imported file alias (e.g. `Cxx` from
    /// `using Cxx = import "/capnp/c++.capnp";`).
    FileAlias {
        file: FileUri,
        span_source: ByteSpan,
    },
    /// A built-in type, either referenced directly (`Int16`) or through a
    /// local alias (`using SlotId = Int16;`).
    Builtin {
        name: SmolStr,
    },
    Unknown {
        candidates: Vec<String>,
    },
}

pub fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "Void"
            | "Bool"
            | "Int8" | "Int16" | "Int32" | "Int64"
            | "UInt8" | "UInt16" | "UInt32" | "UInt64"
            | "Float32" | "Float64"
            | "Text" | "Data" | "List" | "AnyPointer" | "Capability"
    )
}

/// Given `scope = "Outer.Inner"` and `path = ["Foo", "Bar"]`, produce the
/// scope-walk candidates in innermost-out order:
/// ["Outer.Inner.Foo.Bar", "Outer.Foo.Bar", "Foo.Bar"].
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

    fn fu() -> FileUri { FileUri("file:///t.capnp".into()) }

    #[test]
    fn extracts_doc_comments() {
        let src = "# doc line 1\n# doc line 2\nstruct Foo {}\n";
        let parsed = parse(src);
        let fs = index_file(&fu(), &parsed.file, src);
        let sym = fs.entries.get("Foo").unwrap();
        assert_eq!(sym.doc.as_deref(), Some("doc line 1\ndoc line 2"));
    }

    #[test]
    fn collects_nested_symbols() {
        let src = "@0x1; struct Outer { struct Inner { id @0 :UInt32; } }";
        let parsed = parse(src);
        let fs = index_file(&fu(), &parsed.file, src);
        assert!(fs.entries.contains_key("Outer"));
        assert!(fs.entries.contains_key("Outer.Inner"));
        assert!(fs.entries.contains_key("Outer.Inner.id"));
    }

    #[test]
    fn resolves_across_import() {
        use crate::vfs::Workspace;
        let mut ws = Workspace::new();
        ws.update("file:///a.capnp", "@0x1; struct Foo { id @0 :UInt32; }".into());
        ws.update(
            "file:///b.capnp",
            "@0x2; using A = import \"a.capnp\"; struct Bar { f @0 :A.Foo; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let b_uri = FileUri("file:///b.capnp".into());
        let path = vec![
            Ident { text: "A".into(), span: ByteSpan::EMPTY },
            Ident { text: "Foo".into(), span: ByteSpan::EMPTY },
        ];
        match idx.resolve_type(&b_uri, "Bar", &path) {
            Resolution::Found { symbol, visibility_ok: true } => {
                assert_eq!(symbol.fqn.as_str(), "Foo");
            }
            other => panic!("expected cross-file Found, got {:?}", other),
        }
    }

    #[test]
    fn resolves_inner_out() {
        let cand = scope_candidates(
            "A.B",
            &[Ident { text: "C".into(), span: ByteSpan::EMPTY }],
        );
        assert_eq!(cand, vec!["A.B.C".to_string(), "A.C".into(), "C".into()]);
    }

    #[test]
    fn resolves_inline_import_type() {
        use crate::vfs::Workspace;
        let mut ws = Workspace::new();
        ws.update("file:///a.capnp", "@0x1; struct Foo { id @0 :UInt32; }".into());
        ws.update(
            "file:///b.capnp",
            "@0x2; struct Bar { f @0 :import \"a.capnp\".Foo; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let b_uri = FileUri("file:///b.capnp".into());
        let path = vec![Ident { text: "Foo".into(), span: ByteSpan::EMPTY }];
        match idx.resolve_type_with_import(&b_uri, "Bar", Some("a.capnp"), &path) {
            Resolution::Found { symbol, visibility_ok: true } => {
                assert_eq!(symbol.fqn.as_str(), "Foo");
            }
            other => panic!("expected Found via inline import, got {:?}", other),
        }
    }

    #[test]
    fn resolves_using_import_type_alias() {
        use crate::vfs::Workspace;
        let mut ws = Workspace::new();
        ws.update("file:///types.capnp", "@0x1; enum Tag { a @0; b @1; }".into());
        ws.update(
            "file:///b.capnp",
            "@0x2; using import \"types.capnp\".Tag; struct S { t @0 :Tag; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let b_uri = FileUri("file:///b.capnp".into());
        let path = vec![Ident { text: "Tag".into(), span: ByteSpan::EMPTY }];
        match idx.resolve_type(&b_uri, "S", &path) {
            Resolution::Found { symbol, visibility_ok: true } => {
                assert_eq!(symbol.fqn.as_str(), "Tag");
            }
            other => panic!("expected Found via using-import alias, got {:?}", other),
        }
    }

    #[test]
    fn resolves_interface_via_file_alias() {
        use crate::vfs::Workspace;
        let mut ws = Workspace::new();
        ws.update(
            "file:///rpc.capnp",
            "@0x1; interface UpdateSink { deliver @0 (x :UInt32) -> stream; end @1 (); }\n\
             interface Subscription { cancel @0 () -> (); }\n".into(),
        );
        ws.update(
            "file:///workspace.capnp",
            "@0x2; using Rpc = import \"rpc.capnp\";\n\
             interface Cap { subscribe @0 (sink :Rpc.UpdateSink) -> (sub :Rpc.Subscription); }\n"
                .into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let w_uri = FileUri("file:///workspace.capnp".into());
        let path = vec![
            Ident { text: "Rpc".into(), span: ByteSpan::EMPTY },
            Ident { text: "UpdateSink".into(), span: ByteSpan::EMPTY },
        ];
        match idx.resolve_type(&w_uri, "Cap", &path) {
            Resolution::Found { symbol, visibility_ok: true } => {
                assert_eq!(symbol.fqn.as_str(), "UpdateSink");
                assert_eq!(symbol.kind, SymbolKind::Interface);
            }
            other => panic!("expected Found UpdateSink, got {:?}", other),
        }
        let path = vec![
            Ident { text: "Rpc".into(), span: ByteSpan::EMPTY },
            Ident { text: "Subscription".into(), span: ByteSpan::EMPTY },
        ];
        match idx.resolve_type(&w_uri, "Cap", &path) {
            Resolution::Found { symbol, visibility_ok: true } => {
                assert_eq!(symbol.fqn.as_str(), "Subscription");
            }
            other => panic!("expected Found Subscription, got {:?}", other),
        }
    }

    #[test]
    fn resolves_local_alias_to_builtin() {
        use crate::vfs::Workspace;
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; using SlotId = Int16; struct S { slot @0 :SlotId; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.capnp".into());
        let path = vec![Ident { text: "SlotId".into(), span: ByteSpan::EMPTY }];
        match idx.resolve_type(&uri, "S", &path) {
            Resolution::Builtin { name } => assert_eq!(name.as_str(), "Int16"),
            other => panic!("expected Builtin Int16, got {:?}", other),
        }
    }

    #[test]
    fn resolves_local_alias_to_local_struct() {
        use crate::vfs::Workspace;
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; struct Point { x @0 :Int16; } using P = Point; struct S { p @0 :P; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.capnp".into());
        let path = vec![Ident { text: "P".into(), span: ByteSpan::EMPTY }];
        match idx.resolve_type(&uri, "S", &path) {
            Resolution::Found { symbol, visibility_ok: true } => {
                assert_eq!(symbol.fqn.as_str(), "Point");
            }
            other => panic!("expected Found Point via local alias, got {:?}", other),
        }
    }

    #[test]
    fn resolves_local_alias_chain() {
        use crate::vfs::Workspace;
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; using A = Int16; using B = A; struct S { x @0 :B; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.capnp".into());
        let path = vec![Ident { text: "B".into(), span: ByteSpan::EMPTY }];
        match idx.resolve_type(&uri, "S", &path) {
            Resolution::Builtin { name } => assert_eq!(name.as_str(), "Int16"),
            other => panic!("expected Builtin Int16 via alias chain, got {:?}", other),
        }
    }

    #[test]
    fn resolves_local_alias_to_nested_type_with_suffix() {
        use crate::vfs::Workspace;
        let mut ws = Workspace::new();
        ws.update(
            "file:///a.capnp",
            "@0x1; struct Outer { struct Inner { x @0 :Int16; } } \
             using O = Outer; struct S { i @0 :O.Inner; }"
                .into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.capnp".into());
        let path = vec![
            Ident { text: "O".into(), span: ByteSpan::EMPTY },
            Ident { text: "Inner".into(), span: ByteSpan::EMPTY },
        ];
        match idx.resolve_type(&uri, "S", &path) {
            Resolution::Found { symbol, visibility_ok: true } => {
                assert_eq!(symbol.fqn.as_str(), "Outer.Inner");
            }
            other => panic!("expected Found Outer.Inner, got {:?}", other),
        }
    }

    #[test]
    fn resolves_using_named_import_type_alias() {
        use crate::vfs::Workspace;
        let mut ws = Workspace::new();
        ws.update("file:///types.capnp", "@0x1; enum Tag { a @0; b @1; }".into());
        ws.update(
            "file:///b.capnp",
            "@0x2; using T = import \"types.capnp\".Tag; struct S { t @0 :T; }".into(),
        );
        let idx = WorkspaceIndex::build(&ws);
        let b_uri = FileUri("file:///b.capnp".into());
        let path = vec![Ident { text: "T".into(), span: ByteSpan::EMPTY }];
        match idx.resolve_type(&b_uri, "S", &path) {
            Resolution::Found { symbol, visibility_ok: true } => {
                assert_eq!(symbol.fqn.as_str(), "Tag");
            }
            other => panic!("expected Found via named import-type alias, got {:?}", other),
        }
    }
}
