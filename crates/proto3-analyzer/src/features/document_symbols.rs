//! Hierarchical document-symbol tree for an outline / breadcrumb view.

use crate::ast;
use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

/// Mirrors VSCode's `DocumentSymbol` shape, kept as a plain data struct so
/// serialization across the WASM boundary is trivial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    pub detail: String,
    pub kind: SymbolKind,
    pub range: ByteSpan,
    pub selection_range: ByteSpan,
    pub children: Vec<DocumentSymbol>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SymbolKind {
    Package,
    Message,
    Enum,
    EnumMember,
    Field,
    Oneof,
    Service,
    Method,
}

pub fn document_symbols(file: &ast::File) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    for item in &file.items {
        match item {
            ast::TopLevelItem::Message(m) => out.push(message_symbol(m)),
            ast::TopLevelItem::Enum(e) => out.push(enum_symbol(e)),
            ast::TopLevelItem::Service(s) => out.push(service_symbol(s)),
            ast::TopLevelItem::Extend(_) => {}
        }
    }
    out
}

fn message_symbol(m: &ast::Message) -> DocumentSymbol {
    let mut children = Vec::new();
    for f in &m.fields {
        children.push(field_symbol(f));
    }
    for o in &m.oneofs {
        let mut oc = Vec::new();
        for f in &o.fields {
            oc.push(field_symbol(f));
        }
        children.push(DocumentSymbol {
            name: o.name.name.to_string(),
            detail: "oneof".into(),
            kind: SymbolKind::Oneof,
            range: o.span,
            selection_range: o.name.span,
            children: oc,
        });
    }
    for nm in &m.nested_messages {
        children.push(message_symbol(nm));
    }
    for ne in &m.nested_enums {
        children.push(enum_symbol(ne));
    }
    DocumentSymbol {
        name: m.name.name.to_string(),
        detail: "message".into(),
        kind: SymbolKind::Message,
        range: m.span,
        selection_range: m.name.span,
        children,
    }
}

fn enum_symbol(e: &ast::EnumDecl) -> DocumentSymbol {
    let children = e
        .values
        .iter()
        .map(|v| DocumentSymbol {
            name: v.name.name.to_string(),
            detail: v
                .number
                .as_i64()
                .map(|n| format!("= {}", n))
                .unwrap_or_default(),
            kind: SymbolKind::EnumMember,
            range: v.span,
            selection_range: v.name.span,
            children: Vec::new(),
        })
        .collect();
    DocumentSymbol {
        name: e.name.name.to_string(),
        detail: "enum".into(),
        kind: SymbolKind::Enum,
        range: e.span,
        selection_range: e.name.span,
        children,
    }
}

fn service_symbol(s: &ast::Service) -> DocumentSymbol {
    let children = s
        .methods
        .iter()
        .map(|m| DocumentSymbol {
            name: m.name.name.to_string(),
            detail: format!(
                "rpc ({}) returns ({})",
                m.input.ty.to_display(),
                m.output.ty.to_display()
            ),
            kind: SymbolKind::Method,
            range: m.span,
            selection_range: m.name.span,
            children: Vec::new(),
        })
        .collect();
    DocumentSymbol {
        name: s.name.name.to_string(),
        detail: "service".into(),
        kind: SymbolKind::Service,
        range: s.span,
        selection_range: s.name.span,
        children,
    }
}

fn field_symbol(f: &ast::FieldDecl) -> DocumentSymbol {
    let ty = match &f.ty {
        ast::TypeRef::Scalar(s, _) => s.as_str().to_string(),
        ast::TypeRef::Named(q) => q.to_display(),
        ast::TypeRef::Map(m) => format!(
            "map<{}, {}>",
            type_label(&m.key),
            type_label(&m.value)
        ),
        ast::TypeRef::Missing(_) => "?".into(),
    };
    let label = match f.label {
        ast::FieldLabel::Repeated => "repeated ",
        ast::FieldLabel::Optional => "optional ",
        ast::FieldLabel::Required => "required ",
        ast::FieldLabel::None => "",
    };
    let number = f
        .number
        .as_i64()
        .map(|n| format!(" = {}", n))
        .unwrap_or_default();
    DocumentSymbol {
        name: f.name.name.to_string(),
        detail: format!("{}{}{}", label, ty, number),
        kind: SymbolKind::Field,
        range: f.span,
        selection_range: f.name.span,
        children: Vec::new(),
    }
}

fn type_label(t: &ast::TypeRef) -> String {
    match t {
        ast::TypeRef::Scalar(s, _) => s.as_str().to_string(),
        ast::TypeRef::Named(q) => q.to_display(),
        ast::TypeRef::Map(m) => format!("map<{}, {}>", type_label(&m.key), type_label(&m.value)),
        ast::TypeRef::Missing(_) => "?".into(),
    }
}
