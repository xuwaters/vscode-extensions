//! Helpers for "what's at this cursor position" queries.

use crate::ast;
use crate::resolve::{collect_type_use_sites, TypeUseSite};
use crate::spans::ByteSpan;

/// Find the type-use site whose span contains `offset`, if any.
pub fn type_use_at(file: &ast::File, offset: u32) -> Option<TypeUseSite> {
    collect_type_use_sites(file)
        .into_iter()
        .find(|s| s.span.start <= offset && offset <= s.span.end)
}

/// Find the AST field declaration whose *name* span contains `offset`.
pub fn field_at_name<'a>(file: &'a ast::File, offset: u32) -> Option<(&'a ast::FieldDecl, String)> {
    let pkg = file.package.as_ref().map(|p| p.name.to_display()).unwrap_or_default();
    for item in &file.items {
        if let ast::TopLevelItem::Message(m) = item {
            if let Some(hit) = find_field_in_message(&pkg, m, offset) {
                return Some(hit);
            }
        }
    }
    None
}

fn find_field_in_message<'a>(
    scope: &str,
    m: &'a ast::Message,
    offset: u32,
) -> Option<(&'a ast::FieldDecl, String)> {
    let fqn = join(scope, &m.name.name);
    for f in &m.fields {
        if span_contains(f.name.span, offset) {
            return Some((f, fqn.clone()));
        }
    }
    for o in &m.oneofs {
        for f in &o.fields {
            if span_contains(f.name.span, offset) {
                return Some((f, fqn.clone()));
            }
        }
    }
    for nm in &m.nested_messages {
        if let Some(hit) = find_field_in_message(&fqn, nm, offset) {
            return Some(hit);
        }
    }
    None
}

pub fn span_contains(span: ByteSpan, offset: u32) -> bool {
    span.start <= offset && offset <= span.end
}

fn join(a: &str, b: &str) -> String {
    if a.is_empty() { b.to_string() } else { format!("{}.{}", a, b) }
}

/// Find which message FQN encloses `offset` (innermost wins). Used by
/// completion to compute the resolver's starting scope.
pub fn enclosing_scope_at(file: &ast::File, offset: u32) -> String {
    let pkg = file.package.as_ref().map(|p| p.name.to_display()).unwrap_or_default();
    let mut scope = pkg.clone();
    for item in &file.items {
        match item {
            ast::TopLevelItem::Message(m) if span_contains(m.span, offset) => {
                scope = descend_message(&pkg, m, offset);
                return scope;
            }
            ast::TopLevelItem::Service(s) if span_contains(s.span, offset) => {
                // Services don't host types; keep scope at package for RPC type refs.
                scope = pkg.clone();
                return scope;
            }
            _ => {}
        }
    }
    scope
}

fn descend_message(scope: &str, m: &ast::Message, offset: u32) -> String {
    let fqn = join(scope, &m.name.name);
    for nm in &m.nested_messages {
        if span_contains(nm.span, offset) {
            return descend_message(&fqn, nm, offset);
        }
    }
    fqn
}
