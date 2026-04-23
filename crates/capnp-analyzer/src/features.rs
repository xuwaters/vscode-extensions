//! Document-symbol tree and folding-range extraction. Both are pure views on
//! the parsed [`File`] and do not perform name resolution.

use crate::ast::*;
use crate::spans::ByteSpan;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub name: String,
    pub detail: String,
    pub kind: SymbolKind,
    pub range: ByteSpan,
    pub selection_range: ByteSpan,
    pub children: Vec<Symbol>,
}

#[derive(Debug, Clone, Copy, Serialize)]
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
    Namespace,
}

pub fn document_symbols(file: &File) -> Vec<Symbol> {
    let mut out = Vec::new();
    for d in &file.decls {
        if let Some(sym) = decl_symbol(d) {
            out.push(sym);
        }
    }
    out
}

fn decl_symbol(d: &Decl) -> Option<Symbol> {
    match d {
        Decl::Using(u) => {
            let name = u
                .name
                .as_ref()
                .map(|i| i.text.to_string())
                .or_else(|| u.import_path.as_ref().map(|p| p.value.clone()))
                .unwrap_or_else(|| "using".into());
            let sel = u.name.as_ref().map(|i| i.span).unwrap_or(u.span);
            Some(Symbol {
                name,
                detail: "using".into(),
                kind: SymbolKind::Namespace,
                range: u.span,
                selection_range: sel,
                children: Vec::new(),
            })
        }
        Decl::Struct(s) => Some(struct_symbol(s)),
        Decl::Enum(e) => Some(enum_symbol(e)),
        Decl::Interface(i) => Some(interface_symbol(i)),
        Decl::Const(c) => Some(Symbol {
            name: c.name.text.to_string(),
            detail: "const".into(),
            kind: SymbolKind::Constant,
            range: c.span,
            selection_range: c.name.span,
            children: Vec::new(),
        }),
        Decl::Annotation(a) => Some(Symbol {
            name: a.name.text.to_string(),
            detail: "annotation".into(),
            kind: SymbolKind::Annotation,
            range: a.span,
            selection_range: a.name.span,
            children: Vec::new(),
        }),
        Decl::TopAnnotation(_) => None,
    }
}

fn struct_symbol(s: &Struct) -> Symbol {
    let mut children = Vec::new();
    for m in &s.members {
        match m {
            StructMember::Field(f) => children.push(field_symbol(f)),
            StructMember::AnonUnion(u) => children.push(union_symbol("union", u)),
            StructMember::Struct(s2) => children.push(struct_symbol(s2)),
            StructMember::Enum(e) => children.push(enum_symbol(e)),
            StructMember::Interface(i) => children.push(interface_symbol(i)),
            StructMember::Const(c) => children.push(Symbol {
                name: c.name.text.to_string(),
                detail: "const".into(),
                kind: SymbolKind::Constant,
                range: c.span,
                selection_range: c.name.span,
                children: Vec::new(),
            }),
            StructMember::Annotation(a) => children.push(Symbol {
                name: a.name.text.to_string(),
                detail: "annotation".into(),
                kind: SymbolKind::Annotation,
                range: a.span,
                selection_range: a.name.span,
                children: Vec::new(),
            }),
            StructMember::Using(u) => {
                let name = u.name.as_ref().map(|i| i.text.to_string()).unwrap_or_else(|| "using".into());
                let sel = u.name.as_ref().map(|i| i.span).unwrap_or(u.span);
                children.push(Symbol {
                    name,
                    detail: "using".into(),
                    kind: SymbolKind::Namespace,
                    range: u.span,
                    selection_range: sel,
                    children: Vec::new(),
                });
            }
        }
    }
    let detail = if s.type_params.is_empty() {
        "struct".into()
    } else {
        format!(
            "struct({})",
            s.type_params.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join(", ")
        )
    };
    Symbol {
        name: s.name.text.to_string(),
        detail,
        kind: SymbolKind::Struct,
        range: s.span,
        selection_range: s.name.span,
        children,
    }
}

fn field_symbol(f: &Field) -> Symbol {
    let (kind, detail) = match &f.body {
        FieldBody::Slot { ty, .. } => (SymbolKind::Field, type_label(ty)),
        FieldBody::NamedUnion(_) => (SymbolKind::Union, "union".into()),
        FieldBody::NamedGroup(_) => (SymbolKind::Group, "group".into()),
    };
    let children = match &f.body {
        FieldBody::NamedUnion(ub) => ub.members.iter().map(field_symbol).collect(),
        FieldBody::NamedGroup(gb) => gb
            .members
            .iter()
            .filter_map(|m| match m {
                StructMember::Field(f2) => Some(field_symbol(f2)),
                StructMember::Struct(s) => Some(struct_symbol(s)),
                StructMember::Enum(e) => Some(enum_symbol(e)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let name = match &f.ordinal {
        Some(o) => format!("{} @{}", f.name.text, o.value),
        None => f.name.text.to_string(),
    };
    Symbol {
        name,
        detail,
        kind,
        range: f.span,
        selection_range: f.name.span,
        children,
    }
}

fn union_symbol(name: &str, u: &UnionBlock) -> Symbol {
    let children = u.members.iter().map(field_symbol).collect();
    Symbol {
        name: name.to_string(),
        detail: "union".into(),
        kind: SymbolKind::Union,
        range: u.span,
        selection_range: u.span,
        children,
    }
}

fn enum_symbol(e: &EnumDecl) -> Symbol {
    let children = e
        .enumerants
        .iter()
        .map(|en| {
            let name = match &en.ordinal {
                Some(o) => format!("{} @{}", en.name.text, o.value),
                None => en.name.text.to_string(),
            };
            Symbol {
                name,
                detail: "enumerant".into(),
                kind: SymbolKind::EnumMember,
                range: en.span,
                selection_range: en.name.span,
                children: Vec::new(),
            }
        })
        .collect();
    Symbol {
        name: e.name.text.to_string(),
        detail: "enum".into(),
        kind: SymbolKind::Enum,
        range: e.span,
        selection_range: e.name.span,
        children,
    }
}

fn interface_symbol(i: &Interface) -> Symbol {
    let mut children: Vec<Symbol> = Vec::new();
    for m in &i.methods {
        let name = match &m.ordinal {
            Some(o) => format!("{} @{}", m.name.text, o.value),
            None => m.name.text.to_string(),
        };
        children.push(Symbol {
            name,
            detail: "method".into(),
            kind: SymbolKind::Method,
            range: m.span,
            selection_range: m.name.span,
            children: Vec::new(),
        });
    }
    for n in &i.nested {
        match n {
            StructMember::Struct(s) => children.push(struct_symbol(s)),
            StructMember::Enum(e) => children.push(enum_symbol(e)),
            StructMember::Interface(i2) => children.push(interface_symbol(i2)),
            _ => {}
        }
    }
    Symbol {
        name: i.name.text.to_string(),
        detail: "interface".into(),
        kind: SymbolKind::Interface,
        range: i.span,
        selection_range: i.name.span,
        children,
    }
}

fn type_label(t: &TypeRef) -> String {
    let mut s = t.path.iter().map(|i| i.text.as_str()).collect::<Vec<_>>().join(".");
    if !t.args.is_empty() {
        let args = t.args.iter().map(type_label).collect::<Vec<_>>().join(", ");
        s.push('(');
        s.push_str(&args);
        s.push(')');
    }
    s
}

#[derive(Debug, Clone, Serialize)]
pub struct FoldingRange {
    pub start: ByteSpan,
    pub end: ByteSpan,
    pub kind: &'static str,
}

pub fn folding_ranges(file: &File) -> Vec<FoldingRange> {
    let mut out = Vec::new();
    for d in &file.decls {
        collect_folds_decl(d, &mut out);
    }
    out
}

fn collect_folds_decl(d: &Decl, out: &mut Vec<FoldingRange>) {
    match d {
        Decl::Struct(s) => {
            out.push(FoldingRange { start: s.span, end: s.span, kind: "region" });
            for m in &s.members {
                collect_folds_member(m, out);
            }
        }
        Decl::Enum(e) => out.push(FoldingRange { start: e.span, end: e.span, kind: "region" }),
        Decl::Interface(i) => {
            out.push(FoldingRange { start: i.span, end: i.span, kind: "region" });
            for n in &i.nested {
                collect_folds_member(n, out);
            }
        }
        _ => {}
    }
}

fn collect_folds_member(m: &StructMember, out: &mut Vec<FoldingRange>) {
    match m {
        StructMember::Struct(s) => {
            out.push(FoldingRange { start: s.span, end: s.span, kind: "region" });
            for c in &s.members { collect_folds_member(c, out); }
        }
        StructMember::Enum(e) => out.push(FoldingRange { start: e.span, end: e.span, kind: "region" }),
        StructMember::Interface(i) => {
            out.push(FoldingRange { start: i.span, end: i.span, kind: "region" });
            for n in &i.nested { collect_folds_member(n, out); }
        }
        StructMember::AnonUnion(u) => out.push(FoldingRange { start: u.span, end: u.span, kind: "region" }),
        StructMember::Field(f) => {
            if let FieldBody::NamedUnion(u) = &f.body {
                out.push(FoldingRange { start: u.span, end: u.span, kind: "region" });
            }
            if let FieldBody::NamedGroup(g) = &f.body {
                out.push(FoldingRange { start: g.span, end: g.span, kind: "region" });
                for c in &g.members { collect_folds_member(c, out); }
            }
        }
        _ => {}
    }
}
