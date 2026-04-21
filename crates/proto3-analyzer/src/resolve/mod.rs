//! Workspace-wide symbol index and name resolution.
//!
//! Phase-1 scope: build a fully-qualified-name → definition map per file, so
//! the document-symbol feature can emit a hierarchy and the (future)
//! definition/hover/completion features have a table to query. Full
//! cross-file resolution lands in Phase 2.

use crate::ast;
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use indexmap::IndexMap;
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

#[derive(Debug, Clone)]
pub struct Symbol {
    pub fqn: SmolStr,
    pub name: SmolStr,
    pub kind: SymbolKind,
    pub file: FileUri,
    pub name_span: ByteSpan,
    pub full_span: ByteSpan,
}

#[derive(Debug, Default, Clone)]
pub struct FileSymbols {
    /// FQN -> Symbol. Preserves source order to drive document-symbol output.
    pub entries: IndexMap<SmolStr, Symbol>,
}

pub fn index_file(file_uri: &FileUri, file: &ast::File) -> FileSymbols {
    let mut out = FileSymbols::default();
    let pkg_prefix = match &file.package {
        Some(p) => p.name.to_display(),
        None => String::new(),
    };
    for item in &file.items {
        match item {
            ast::TopLevelItem::Message(m) => index_message(&pkg_prefix, m, file_uri, &mut out),
            ast::TopLevelItem::Enum(e) => index_enum(&pkg_prefix, e, file_uri, &mut out),
            ast::TopLevelItem::Service(s) => index_service(&pkg_prefix, s, file_uri, &mut out),
            ast::TopLevelItem::Extend(_) => {}
        }
    }
    out
}

fn index_message(scope: &str, m: &ast::Message, file_uri: &FileUri, out: &mut FileSymbols) {
    let fqn = join(scope, &m.name.name);
    out.entries.insert(
        SmolStr::new(&fqn),
        Symbol {
            fqn: SmolStr::new(&fqn),
            name: m.name.name.clone(),
            kind: SymbolKind::Message,
            file: file_uri.clone(),
            name_span: m.name.span,
            full_span: m.span,
        },
    );
    for f in &m.fields {
        let field_fqn = join(&fqn, &f.name.name);
        out.entries.insert(
            SmolStr::new(&field_fqn),
            Symbol {
                fqn: SmolStr::new(&field_fqn),
                name: f.name.name.clone(),
                kind: SymbolKind::Field,
                file: file_uri.clone(),
                name_span: f.name.span,
                full_span: f.span,
            },
        );
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
            },
        );
        for f in &o.fields {
            let ff = join(&fqn, &f.name.name);
            out.entries.insert(
                SmolStr::new(&ff),
                Symbol {
                    fqn: SmolStr::new(&ff),
                    name: f.name.name.clone(),
                    kind: SymbolKind::Field,
                    file: file_uri.clone(),
                    name_span: f.name.span,
                    full_span: f.span,
                },
            );
        }
    }
    for nm in &m.nested_messages {
        index_message(&fqn, nm, file_uri, out);
    }
    for ne in &m.nested_enums {
        index_enum(&fqn, ne, file_uri, out);
    }
}

fn index_enum(scope: &str, e: &ast::EnumDecl, file_uri: &FileUri, out: &mut FileSymbols) {
    let fqn = join(scope, &e.name.name);
    out.entries.insert(
        SmolStr::new(&fqn),
        Symbol {
            fqn: SmolStr::new(&fqn),
            name: e.name.name.clone(),
            kind: SymbolKind::Enum,
            file: file_uri.clone(),
            name_span: e.name.span,
            full_span: e.span,
        },
    );
    for v in &e.values {
        let vf = join(&fqn, &v.name.name);
        out.entries.insert(
            SmolStr::new(&vf),
            Symbol {
                fqn: SmolStr::new(&vf),
                name: v.name.name.clone(),
                kind: SymbolKind::EnumValue,
                file: file_uri.clone(),
                name_span: v.name.span,
                full_span: v.span,
            },
        );
    }
}

fn index_service(scope: &str, s: &ast::Service, file_uri: &FileUri, out: &mut FileSymbols) {
    let fqn = join(scope, &s.name.name);
    out.entries.insert(
        SmolStr::new(&fqn),
        Symbol {
            fqn: SmolStr::new(&fqn),
            name: s.name.name.clone(),
            kind: SymbolKind::Service,
            file: file_uri.clone(),
            name_span: s.name.span,
            full_span: s.span,
        },
    );
    for m in &s.methods {
        let mf = join(&fqn, &m.name.name);
        out.entries.insert(
            SmolStr::new(&mf),
            Symbol {
                fqn: SmolStr::new(&mf),
                name: m.name.name.clone(),
                kind: SymbolKind::Rpc,
                file: file_uri.clone(),
                name_span: m.name.span,
                full_span: m.span,
            },
        );
    }
}

fn join(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", scope, name)
    }
}

pub fn workspace_symbols(ws: &Workspace) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (uri, pf) in ws.files() {
        let fs = index_file(uri, &pf.ast);
        out.extend(fs.entries.into_iter().map(|(_, v)| v));
    }
    out
}
