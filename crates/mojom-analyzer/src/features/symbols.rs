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
    Module,
    Struct,
    Union,
    Interface,
    Enum,
    EnumMember,
    Method,
    Field,
    Constant,
}

pub fn document_symbols(file: &File) -> Vec<Symbol> {
    let mut out = Vec::new();
    if let Some(m) = &file.module {
        out.push(Symbol {
            name: m.name.to_string(),
            detail: "module".into(),
            kind: SymbolKind::Module,
            range: m.span,
            selection_range: m.name_span,
            children: Vec::new(),
        });
    }
    for d in &file.decls {
        out.push(decl_symbol(d));
    }
    out
}

fn decl_symbol(d: &Decl) -> Symbol {
    match d {
        Decl::Struct(s) => struct_symbol(s),
        Decl::Union(u) => union_symbol(u),
        Decl::Interface(i) => interface_symbol(i),
        Decl::Enum(e) => enum_symbol(e),
        Decl::Const(c) => const_symbol(c),
    }
}

fn struct_symbol(s: &Struct) -> Symbol {
    let children = s
        .members
        .iter()
        .map(|m| match m {
            StructMember::Field(f) => field_symbol(f),
            StructMember::Const(c) => const_symbol(c),
            StructMember::Enum(e) => enum_symbol(e),
        })
        .collect();
    Symbol {
        name: s.name.text.to_string(),
        detail: "struct".into(),
        kind: SymbolKind::Struct,
        range: s.span,
        selection_range: s.name.span,
        children,
    }
}

fn union_symbol(u: &Union) -> Symbol {
    let children = u.fields.iter().map(field_symbol).collect();
    Symbol {
        name: u.name.text.to_string(),
        detail: "union".into(),
        kind: SymbolKind::Union,
        range: u.span,
        selection_range: u.name.span,
        children,
    }
}

fn interface_symbol(i: &Interface) -> Symbol {
    let children = i
        .members
        .iter()
        .map(|m| match m {
            InterfaceMember::Method(meth) => method_symbol(meth),
            InterfaceMember::Const(c) => const_symbol(c),
            InterfaceMember::Enum(e) => enum_symbol(e),
        })
        .collect();
    Symbol {
        name: i.name.text.to_string(),
        detail: "interface".into(),
        kind: SymbolKind::Interface,
        range: i.span,
        selection_range: i.name.span,
        children,
    }
}

fn enum_symbol(e: &EnumDecl) -> Symbol {
    let children = e
        .values
        .iter()
        .map(|v| Symbol {
            name: v.name.text.to_string(),
            detail: "enum value".into(),
            kind: SymbolKind::EnumMember,
            range: v.span,
            selection_range: v.name.span,
            children: Vec::new(),
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

fn field_symbol(f: &Field) -> Symbol {
    let name = match &f.ordinal {
        Some(o) => format!("{} @{}", f.name.text, o.value),
        None => f.name.text.to_string(),
    };
    Symbol {
        name,
        detail: f.ty.label.clone(),
        kind: SymbolKind::Field,
        range: f.span,
        selection_range: f.name.span,
        children: Vec::new(),
    }
}

fn method_symbol(m: &Method) -> Symbol {
    let name = match &m.ordinal {
        Some(o) => format!("{} @{}", m.name.text, o.value),
        None => m.name.text.to_string(),
    };
    Symbol {
        name,
        detail: method_signature(m),
        kind: SymbolKind::Method,
        range: m.span,
        selection_range: m.name.span,
        children: Vec::new(),
    }
}

fn const_symbol(c: &ConstDecl) -> Symbol {
    Symbol {
        name: c.name.text.to_string(),
        detail: format!("const {}", c.ty.label),
        kind: SymbolKind::Constant,
        range: c.span,
        selection_range: c.name.span,
        children: Vec::new(),
    }
}

fn method_signature(m: &Method) -> String {
    let params = m.params.iter().map(|p| p.ty.label.clone()).collect::<Vec<_>>().join(", ");
    match &m.response {
        Some(resp) => {
            let r = resp.iter().map(|p| p.ty.label.clone()).collect::<Vec<_>>().join(", ");
            format!("({}) => ({})", params, r)
        }
        None => format!("({})", params),
    }
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
        match d {
            Decl::Struct(s) => {
                out.push(FoldingRange { start: s.span, end: s.span, kind: "region" });
                for m in &s.members {
                    if let StructMember::Enum(e) = m {
                        out.push(FoldingRange { start: e.span, end: e.span, kind: "region" });
                    }
                }
            }
            Decl::Union(u) => out.push(FoldingRange { start: u.span, end: u.span, kind: "region" }),
            Decl::Interface(i) => {
                out.push(FoldingRange { start: i.span, end: i.span, kind: "region" });
                for m in &i.members {
                    if let InterfaceMember::Enum(e) = m {
                        out.push(FoldingRange { start: e.span, end: e.span, kind: "region" });
                    }
                }
            }
            Decl::Enum(e) => out.push(FoldingRange { start: e.span, end: e.span, kind: "region" }),
            Decl::Const(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn lists_top_level_symbols() {
        let src = "module m;\nstruct S { int32 id; };\ninterface I { Foo() => (); };";
        let file = parse(src).file;
        let syms = document_symbols(&file);
        // module + struct + interface
        assert_eq!(syms.len(), 3);
        assert!(matches!(syms[0].kind, SymbolKind::Module));
        let s = syms.iter().find(|s| s.name == "S").unwrap();
        assert_eq!(s.children.len(), 1);
    }
}
