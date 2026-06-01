//! "What's at this cursor position" queries.

use crate::ast::*;
use crate::resolve::{collect_type_use_sites, TypeUseSite};
use crate::spans::ByteSpan;
use smol_str::SmolStr;

pub fn span_contains(span: ByteSpan, offset: u32) -> bool {
    span.start <= offset && offset <= span.end
}

/// Find the innermost type-use site whose span contains `offset`.
pub fn type_use_at(file: &File, offset: u32) -> Option<TypeUseSite> {
    let mut best: Option<TypeUseSite> = None;
    for site in collect_type_use_sites(file) {
        if span_contains(site.span, offset) {
            // Prefer the narrowest enclosing span.
            if best.as_ref().is_none_or(|b| site.span.len() < b.span.len()) {
                best = Some(site);
            }
        }
    }
    best
}

/// If `offset` falls inside an `import "…"` path string, return that path.
pub fn import_at(file: &File, offset: u32) -> Option<&StringLit> {
    file.imports.iter().map(|i| &i.path).find(|p| span_contains(p.span, offset))
}

/// Return the FQN of the innermost struct/union/interface body containing
/// `offset`. Used by completion to seed the resolver's starting scope.
pub fn enclosing_scope_at(file: &File, offset: u32) -> String {
    let module = file.module.as_ref().map(|m| m.name.as_str()).unwrap_or("");
    for d in &file.decls {
        match d {
            Decl::Struct(s) if span_contains(s.span, offset) => return join(module, &s.name.text),
            Decl::Union(u) if span_contains(u.span, offset) => return join(module, &u.name.text),
            Decl::Interface(i) if span_contains(i.span, offset) => {
                return join(module, &i.name.text)
            }
            _ => {}
        }
    }
    module.to_string()
}

/// The module name in scope at `offset` (always the file's module — Mojom has
/// one module per file). Kept as a helper for symmetry with [`enclosing_scope_at`].
pub fn module_of(file: &File) -> SmolStr {
    file.module.as_ref().map(|m| m.name.clone()).unwrap_or_default()
}

fn join(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.into()
    } else {
        format!("{}.{}", scope, name)
    }
}
