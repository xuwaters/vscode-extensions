//! "What's at this cursor position" queries.

use crate::ast::*;
use crate::resolve::{collect_type_use_sites, TypeUseSite};
use crate::spans::ByteSpan;

pub fn span_contains(span: ByteSpan, offset: u32) -> bool {
    span.start <= offset && offset <= span.end
}

/// Find the innermost type-use site whose span contains `offset`.
pub fn type_use_at(file: &File, offset: u32) -> Option<TypeUseSite> {
    let mut best: Option<TypeUseSite> = None;
    for site in collect_type_use_sites(file) {
        if span_contains(site.span, offset) {
            // Prefer the narrowest enclosing span so `List(Foo)` hovered on
            // `Foo` lands on `Foo`, not `List(Foo)`.
            if best
                .as_ref()
                .map_or(true, |b| site.span.end - site.span.start < b.span.end - b.span.start)
            {
                best = Some(site);
            }
        }
    }
    best
}

/// Find the AST field whose *name* span contains `offset`, plus the FQN of
/// its enclosing struct.
pub fn field_at_name<'a>(file: &'a File, offset: u32) -> Option<(&'a Field, String)> {
    for d in &file.decls {
        if let Decl::Struct(s) = d {
            if let Some(hit) = find_field_in_struct("", s, offset) {
                return Some(hit);
            }
        }
        if let Decl::Interface(i) = d {
            if let Some(hit) = find_field_in_interface("", i, offset) {
                return Some(hit);
            }
        }
    }
    None
}

fn find_field_in_struct<'a>(
    scope: &str,
    s: &'a Struct,
    offset: u32,
) -> Option<(&'a Field, String)> {
    let fqn = join(scope, &s.name.text);
    for m in &s.members {
        if let Some(hit) = find_field_in_member(&fqn, m, offset) {
            return Some(hit);
        }
    }
    None
}

fn find_field_in_interface<'a>(
    scope: &str,
    i: &'a Interface,
    offset: u32,
) -> Option<(&'a Field, String)> {
    let fqn = join(scope, &i.name.text);
    for n in &i.nested {
        if let Some(hit) = find_field_in_member(&fqn, n, offset) {
            return Some(hit);
        }
    }
    None
}

fn find_field_in_member<'a>(
    scope: &str,
    m: &'a StructMember,
    offset: u32,
) -> Option<(&'a Field, String)> {
    match m {
        StructMember::Field(f) => {
            if span_contains(f.name.span, offset) {
                return Some((f, scope.to_string()));
            }
            match &f.body {
                FieldBody::NamedUnion(ub) => {
                    for inner in &ub.members {
                        if span_contains(inner.name.span, offset) {
                            return Some((inner, scope.to_string()));
                        }
                    }
                }
                FieldBody::NamedGroup(gb) => {
                    for inner in &gb.members {
                        if let Some(hit) = find_field_in_member(scope, inner, offset) {
                            return Some(hit);
                        }
                    }
                }
                _ => {}
            }
            None
        }
        StructMember::AnonUnion(ub) => {
            for inner in &ub.members {
                if span_contains(inner.name.span, offset) {
                    return Some((inner, scope.to_string()));
                }
            }
            None
        }
        StructMember::Struct(s2) => find_field_in_struct(scope, s2, offset),
        StructMember::Interface(i) => find_field_in_interface(scope, i, offset),
        _ => None,
    }
}

/// Return the FQN of the innermost struct/interface body containing
/// `offset`. Used by completion to set the resolver's starting scope.
pub fn enclosing_scope_at(file: &File, offset: u32) -> String {
    let mut scope = String::new();
    for d in &file.decls {
        match d {
            Decl::Struct(s) if span_contains(s.span, offset) => {
                return descend_struct(&scope, s, offset);
            }
            Decl::Interface(i) if span_contains(i.span, offset) => {
                return descend_interface(&scope, i, offset);
            }
            _ => {}
        }
    }
    scope.clear();
    scope
}

fn descend_struct(scope: &str, s: &Struct, offset: u32) -> String {
    let fqn = join(scope, &s.name.text);
    for m in &s.members {
        if let StructMember::Struct(inner) = m {
            if span_contains(inner.span, offset) {
                return descend_struct(&fqn, inner, offset);
            }
        }
        if let StructMember::Interface(inner) = m {
            if span_contains(inner.span, offset) {
                return descend_interface(&fqn, inner, offset);
            }
        }
    }
    fqn
}

fn descend_interface(scope: &str, i: &Interface, offset: u32) -> String {
    let fqn = join(scope, &i.name.text);
    for n in &i.nested {
        match n {
            StructMember::Struct(s) if span_contains(s.span, offset) => {
                return descend_struct(&fqn, s, offset);
            }
            StructMember::Interface(inner) if span_contains(inner.span, offset) => {
                return descend_interface(&fqn, inner, offset);
            }
            _ => {}
        }
    }
    fqn
}

fn join(scope: &str, name: &str) -> String {
    if scope.is_empty() { name.into() } else { format!("{}.{}", scope, name) }
}
