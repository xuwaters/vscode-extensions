//! Collect every type reference in a file with its enclosing scope, so
//! feature providers can ask "what symbol does the cursor land on" and
//! diagnostics can report unresolved-type errors.

use crate::ast;
use crate::spans::ByteSpan;
use smol_str::SmolStr;

#[derive(Debug, Clone)]
pub struct TypeUseSite {
    /// The qualified name as written in source (without implicit resolution).
    pub name: ast::QualifiedName,
    /// Enclosing scope FQN (e.g. `pkg.Outer.Inner` for a use inside
    /// `Inner`'s body). Drives the resolver's scope walk.
    pub enclosing_scope: SmolStr,
    /// Span of the identifier as written (for go-to-def click targets).
    pub span: ByteSpan,
}

pub fn collect_type_use_sites(file: &ast::File) -> Vec<TypeUseSite> {
    let mut out = Vec::new();
    let pkg = file.package.as_ref().map(|p| p.name.to_display()).unwrap_or_default();
    for item in &file.items {
        match item {
            ast::TopLevelItem::Message(m) => visit_message(&pkg, m, &mut out),
            ast::TopLevelItem::Enum(_) => {}
            ast::TopLevelItem::Service(s) => visit_service(&pkg, s, &mut out),
            ast::TopLevelItem::Extend(e) => {
                // Extend's target type reference, at package scope.
                out.push(TypeUseSite {
                    name: e.ty.clone(),
                    enclosing_scope: SmolStr::new(&pkg),
                    span: e.ty.span,
                });
                for f in &e.fields {
                    collect_field_type(&pkg, f, &mut out);
                }
            }
        }
    }
    out
}

fn visit_message(scope: &str, m: &ast::Message, out: &mut Vec<TypeUseSite>) {
    let fqn = join(scope, &m.name.name);
    for f in &m.fields {
        collect_field_type(&fqn, f, out);
    }
    for o in &m.oneofs {
        for f in &o.fields {
            collect_field_type(&fqn, f, out);
        }
    }
    for nm in &m.nested_messages {
        visit_message(&fqn, nm, out);
    }
}

fn visit_service(scope: &str, s: &ast::Service, out: &mut Vec<TypeUseSite>) {
    for m in &s.methods {
        out.push(TypeUseSite {
            name: m.input.ty.clone(),
            enclosing_scope: SmolStr::new(scope),
            span: m.input.ty.span,
        });
        out.push(TypeUseSite {
            name: m.output.ty.clone(),
            enclosing_scope: SmolStr::new(scope),
            span: m.output.ty.span,
        });
    }
}

fn collect_field_type(scope: &str, f: &ast::FieldDecl, out: &mut Vec<TypeUseSite>) {
    collect_type_ref(scope, &f.ty, out);
}

fn collect_type_ref(scope: &str, t: &ast::TypeRef, out: &mut Vec<TypeUseSite>) {
    match t {
        ast::TypeRef::Named(q) => out.push(TypeUseSite {
            name: q.clone(),
            enclosing_scope: SmolStr::new(scope),
            span: q.span,
        }),
        ast::TypeRef::Map(m) => {
            collect_type_ref(scope, &m.key, out);
            collect_type_ref(scope, &m.value, out);
        }
        _ => {}
    }
}

fn join(scope: &str, name: &str) -> String {
    if scope.is_empty() { name.into() } else { format!("{}.{}", scope, name) }
}
