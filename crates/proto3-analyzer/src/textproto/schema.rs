//! Schema-binding validation for textproto documents.
//!
//! Given a [`ParsedTextproto`] that carries `# proto-file:` and
//! `# proto-message:` header hints, this module resolves the referenced
//! message in the workspace's [`WorkspaceIndex`] and walks the textproto AST
//! to check:
//!
//!  * every field name exists on the enclosing message (or is a valid
//!    extension / Any type URL),
//!  * value kinds are compatible with declared field types (scalar vs
//!    message vs list),
//!  * enum-valued identifiers match a known enum value,
//!  * singular fields aren't set twice and oneof arms don't conflict.
//!
//! The checks are deliberately lightweight — the goal is "obvious mistakes",
//! not full wire-format compatibility, which a textproto parser itself would
//! enforce at runtime.

use super::ast::{self, FieldName, Value};
use super::parse::ParsedTextproto;
use crate::ast as proto_ast;
use crate::diagnostics::{DiagnosticCode, ProtoDiagnostic, Severity};
use crate::resolve::{Symbol, SymbolKind, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use rustc_hash::FxHashSet;
use smol_str::SmolStr;

/// Run schema validation over a parsed textproto. Returns diagnostics in
/// source order. If the document has no `# proto-message:` header, validation
/// is a no-op (we don't yet have a schema to check against).
pub fn validate(
    ws: &Workspace,
    index: &WorkspaceIndex,
    parsed: &ParsedTextproto,
) -> Vec<ProtoDiagnostic> {
    let mut out = Vec::new();
    let Some(message_hint) = parsed.header().proto_message.as_ref() else {
        return out;
    };
    // Proto-file is optional but, when present, scopes the message lookup to a
    // specific file so we catch "wrong file" mistakes early.
    let scope_file: Option<FileUri> = parsed
        .header()
        .proto_file
        .as_ref()
        .and_then(|h| resolve_header_file(ws, &h.value))
        .or_else(|| {
            // Emit a diagnostic only when proto-file was provided but didn't
            // resolve. Missing proto-file is allowed (not every team uses it).
            if let Some(h) = parsed.header().proto_file.as_ref() {
                out.push(ProtoDiagnostic::new(
                    DiagnosticCode::TextprotoSchemaFileUnresolved,
                    Severity::Error,
                    format!("Cannot resolve schema file `{}`", h.value),
                    h.value_span,
                ));
            }
            None
        });

    let Some(root_fqn) = resolve_message_fqn(index, scope_file.as_ref(), &message_hint.value) else {
        out.push(ProtoDiagnostic::new(
            DiagnosticCode::TextprotoSchemaMessageUnknown,
            Severity::Error,
            format!(
                "Unknown message `{}` — not found{}",
                message_hint.value,
                match &scope_file {
                    Some(f) => format!(" in `{}`", f.as_str()),
                    None => String::new(),
                }
            ),
            message_hint.value_span,
        ));
        return out;
    };

    let ctx = Ctx { ws, index };
    check_fields(&ctx, &root_fqn, &parsed.ast.fields, &mut out);
    out
}

struct Ctx<'a> {
    ws: &'a Workspace,
    index: &'a WorkspaceIndex,
}

fn check_fields(
    ctx: &Ctx<'_>,
    message_fqn: &str,
    fields: &[ast::Field],
    out: &mut Vec<ProtoDiagnostic>,
) {
    let message = match lookup_message(ctx, message_fqn) {
        Some(m) => m,
        None => return,
    };

    // Track singular fields that have already been set so we can warn on
    // duplicates. Oneof conflicts: once any member of a oneof has been seen,
    // every other member in the same oneof is a conflict.
    let mut seen_singular: FxHashSet<SmolStr> = FxHashSet::default();
    let mut active_oneofs: FxHashSet<SmolStr> = FxHashSet::default();

    for f in fields {
        match &f.name {
            FieldName::Ident(ident) => {
                check_named_field(
                    ctx,
                    &message,
                    ident,
                    f,
                    &mut seen_singular,
                    &mut active_oneofs,
                    out,
                );
            }
            FieldName::Extension { name, span } => {
                // Extensions are declared via `extend X { … }` in proto2.
                // We can't verify much without a full extension index; for
                // now, emit a hint if the extension is unresolved but don't
                // block the caller — the user may be editing an as-yet-missing
                // proto file.
                let fqn = name.to_display();
                if ctx.index.lookup(&fqn).is_none() {
                    out.push(ProtoDiagnostic::new(
                        DiagnosticCode::TextprotoFieldUnknown,
                        Severity::Warning,
                        format!("Unknown extension `[{}]`", fqn),
                        *span,
                    ));
                }
                // Recurse into message payloads generically — no schema to check.
                descend_unknown(ctx, &f.value, out);
            }
            FieldName::Any { url, span } => {
                // Any fields: look up the referenced message FQN directly.
                let target_fqn = url.type_name.to_display();
                if let Some(sym) = ctx.index.lookup(&target_fqn) {
                    if sym.kind != SymbolKind::Message {
                        out.push(ProtoDiagnostic::new(
                            DiagnosticCode::TextprotoSchemaMessageUnknown,
                            Severity::Error,
                            format!("`{}` is not a message type", target_fqn),
                            url.type_name.span,
                        ));
                    } else if let Value::Message { fields: inner, .. } = &f.value {
                        check_fields(ctx, &target_fqn, inner, out);
                    } else if !matches!(f.value, Value::Missing(_)) {
                        out.push(ProtoDiagnostic::new(
                            DiagnosticCode::TextprotoFieldTypeMismatch,
                            Severity::Error,
                            "Any field payload must be a message literal".into(),
                            f.value.span(),
                        ));
                    }
                } else {
                    out.push(ProtoDiagnostic::new(
                        DiagnosticCode::TextprotoAnyUnsupported,
                        Severity::Warning,
                        format!("Unknown Any target message `{}`", target_fqn),
                        *span,
                    ));
                    descend_unknown(ctx, &f.value, out);
                }
            }
        }
    }
}

fn check_named_field(
    ctx: &Ctx<'_>,
    message: &ResolvedMessage<'_>,
    ident: &ast::Ident,
    field: &ast::Field,
    seen_singular: &mut FxHashSet<SmolStr>,
    active_oneofs: &mut FxHashSet<SmolStr>,
    out: &mut Vec<ProtoDiagnostic>,
) {
    let Some(fdecl) = message.field_by_name(&ident.name) else {
        out.push(ProtoDiagnostic::new(
            DiagnosticCode::TextprotoFieldUnknown,
            Severity::Error,
            format!(
                "Unknown field `{}` on message `{}`",
                ident.name,
                message.fqn
            ),
            ident.span,
        ));
        // Even if unknown, walk any nested message to surface obvious errors.
        descend_unknown(ctx, &field.value, out);
        return;
    };

    // Oneof / singular tracking — skip for repeated fields which are
    // explicitly allowed to appear multiple times.
    let is_repeated = matches!(fdecl.label, proto_ast::FieldLabel::Repeated)
        || matches!(&fdecl.ty, proto_ast::TypeRef::Map(_));
    if !is_repeated {
        if let Some(oneof) = message.oneof_of(&ident.name) {
            if !active_oneofs.insert(oneof.clone()) || seen_singular.iter().any(|n| message.oneof_of(n).as_deref() == Some(oneof.as_str()) && n != &ident.name) {
                // Find any already-set member of the same oneof to emit a
                // precise message; fall back to a generic one.
                let other = seen_singular
                    .iter()
                    .find(|n| message.oneof_of(n).as_deref() == Some(oneof.as_str()) && *n != &ident.name)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "another member".into());
                out.push(ProtoDiagnostic::new(
                    DiagnosticCode::TextprotoOneofConflict,
                    Severity::Error,
                    format!(
                        "Field `{}` conflicts with `{}` — both belong to oneof `{}`",
                        ident.name, other, oneof
                    ),
                    ident.span,
                ));
            }
        }
        if !seen_singular.insert(ident.name.clone()) {
            out.push(ProtoDiagnostic::new(
                DiagnosticCode::TextprotoDuplicateSingular,
                Severity::Warning,
                format!(
                    "Singular field `{}` set more than once (later value wins)",
                    ident.name
                ),
                ident.span,
            ));
        }
    }

    check_field_value(ctx, message, fdecl, field, ident.span, out);
}

fn check_field_value(
    ctx: &Ctx<'_>,
    message: &ResolvedMessage<'_>,
    fdecl: &proto_ast::FieldDecl,
    field: &ast::Field,
    ident_span: ByteSpan,
    out: &mut Vec<ProtoDiagnostic>,
) {
    let is_repeated = matches!(fdecl.label, proto_ast::FieldLabel::Repeated);
    let is_map = matches!(&fdecl.ty, proto_ast::TypeRef::Map(_));

    match &field.value {
        Value::List { elements, .. } => {
            if !is_repeated && !is_map {
                out.push(ProtoDiagnostic::new(
                    DiagnosticCode::TextprotoFieldTypeMismatch,
                    Severity::Error,
                    format!(
                        "List value is not allowed here — field `{}` is singular",
                        fdecl.name.name
                    ),
                    field.value.span(),
                ));
            }
            for el in elements {
                check_single_value(ctx, message, fdecl, el, ident_span, out);
            }
        }
        _ => check_single_value(ctx, message, fdecl, &field.value, ident_span, out),
    }
}

fn check_single_value(
    ctx: &Ctx<'_>,
    message: &ResolvedMessage<'_>,
    fdecl: &proto_ast::FieldDecl,
    value: &Value,
    ident_span: ByteSpan,
    out: &mut Vec<ProtoDiagnostic>,
) {
    use proto_ast::ScalarType::*;
    let expected = match &fdecl.ty {
        proto_ast::TypeRef::Scalar(s, _) => *s,
        proto_ast::TypeRef::Named(name) => {
            // Resolve either via workspace index or by trying the nearest
            // nested candidate inside `message.fqn`.
            let name_str = name.to_display();
            let name_trimmed = name_str.trim_start_matches('.').to_string();
            let sym = resolve_scope_aware(ctx.index, &message.fqn, &name_trimmed);
            match sym {
                Some(s) if s.kind == SymbolKind::Message => {
                    check_message_payload(ctx, &s.fqn, value, out);
                    return;
                }
                Some(s) if s.kind == SymbolKind::Enum => {
                    check_enum_value(ctx, &s.fqn, value, out);
                    return;
                }
                _ => {
                    // Type unresolved at proto level — diagnostics from resolve_checks
                    // should already flag this. Don't double-report.
                    return;
                }
            }
        }
        proto_ast::TypeRef::Map(_) => {
            // Map values come as message literals with `key:` / `value:`
            // fields — we let the anonymous-field walk handle it structurally.
            if let Value::Message { fields, .. } = value {
                descend_unknown_fields(ctx, fields, out);
            } else {
                out.push(ProtoDiagnostic::new(
                    DiagnosticCode::TextprotoFieldTypeMismatch,
                    Severity::Error,
                    format!(
                        "Map field `{}` expects a `{{ key: …, value: … }}` literal",
                        fdecl.name.name
                    ),
                    value.span(),
                ));
            }
            return;
        }
        proto_ast::TypeRef::Missing(_) => return,
    };

    let ok = match expected {
        Double | Float => matches!(value, Value::Float { .. } | Value::Integer { .. } | Value::Ident(_) | Value::SignedIdent { .. }),
        Int32 | Int64 | Uint32 | Uint64 | Sint32 | Sint64 | Fixed32 | Fixed64 | Sfixed32 | Sfixed64 => {
            matches!(value, Value::Integer { .. })
        }
        Bool => matches!(value, Value::Ident(_) | Value::Integer { .. }),
        String | Bytes => matches!(value, Value::String { .. }),
    };

    if !ok && !matches!(value, Value::Missing(_)) {
        out.push(ProtoDiagnostic::new(
            DiagnosticCode::TextprotoFieldTypeMismatch,
            Severity::Error,
            format!(
                "Field `{}` expects `{}`, got {}",
                fdecl.name.name,
                expected.as_str(),
                value.kind_label()
            ),
            value.span(),
        ));
        let _ = ident_span; // retained for possible future related-info link
    }
}

fn check_message_payload(
    ctx: &Ctx<'_>,
    message_fqn: &str,
    value: &Value,
    out: &mut Vec<ProtoDiagnostic>,
) {
    match value {
        Value::Message { fields, .. } => check_fields(ctx, message_fqn, fields, out),
        Value::Missing(_) => {}
        other => out.push(ProtoDiagnostic::new(
            DiagnosticCode::TextprotoFieldTypeMismatch,
            Severity::Error,
            format!(
                "Expected a `{{ … }}` or `< … >` message literal, got {}",
                other.kind_label()
            ),
            other.span(),
        )),
    }
}

fn check_enum_value(
    ctx: &Ctx<'_>,
    enum_fqn: &str,
    value: &Value,
    out: &mut Vec<ProtoDiagnostic>,
) {
    let enum_file = match ctx.index.lookup(enum_fqn) {
        Some(s) => s.file.clone(),
        None => return,
    };
    // Name-based: identifier must be a known value on this enum.
    let check_ident = |name: &str, span: ByteSpan, out: &mut Vec<ProtoDiagnostic>| {
        let vfqn = format!("{}.{}", enum_fqn, name);
        if ctx
            .index
            .lookup(&vfqn)
            .map(|s| s.kind == SymbolKind::EnumValue)
            .unwrap_or(false)
        {
            return;
        }
        out.push(ProtoDiagnostic::new(
            DiagnosticCode::TextprotoEnumValueUnknown,
            Severity::Error,
            format!("Unknown value `{}` for enum `{}`", name, enum_fqn),
            span,
        ));
    };
    let _ = enum_file; // placeholder for future cross-file diagnostics
    match value {
        Value::Ident(i) => check_ident(&i.name, i.span, out),
        Value::SignedIdent { ident, span } => {
            // `-MAX` etc. Treat the identifier portion as the enum value name.
            check_ident(&ident.name, *span, out);
        }
        Value::Integer { .. } => {
            // Numeric enum assignment is legal per text-format spec.
        }
        Value::Missing(_) => {}
        other => out.push(ProtoDiagnostic::new(
            DiagnosticCode::TextprotoFieldTypeMismatch,
            Severity::Error,
            format!(
                "Expected an enum value for `{}`, got {}",
                enum_fqn,
                other.kind_label()
            ),
            other.span(),
        )),
    }
}

fn descend_unknown(ctx: &Ctx<'_>, value: &Value, out: &mut Vec<ProtoDiagnostic>) {
    match value {
        Value::Message { fields, .. } => descend_unknown_fields(ctx, fields, out),
        Value::List { elements, .. } => {
            for el in elements {
                descend_unknown(ctx, el, out);
            }
        }
        _ => {}
    }
}

fn descend_unknown_fields(_ctx: &Ctx<'_>, _fields: &[ast::Field], _out: &mut Vec<ProtoDiagnostic>) {
    // Nothing to do — without schema we can't diagnose inner fields. A future
    // pass might emit syntactic consistency checks here (e.g. duplicate keys
    // in map literals).
}

// ───────────────────────── workspace resolution ─────────────────────────

fn resolve_header_file(ws: &Workspace, hint: &str) -> Option<FileUri> {
    // The header may be "pkg/foo.proto" (relative to include path) or an
    // absolute file URI. Try the workspace's regular import router first.
    let dummy_importer = FileUri::new("<textproto>");
    if let Some(u) = ws.resolve_import_path(&dummy_importer, hint) {
        return Some(u);
    }
    // Last-chance: exact match on URI string.
    for (u, _) in ws.files() {
        if u.as_str() == hint {
            return Some(u.clone());
        }
    }
    None
}

fn resolve_message_fqn(
    index: &WorkspaceIndex,
    scope_file: Option<&FileUri>,
    name: &str,
) -> Option<String> {
    let trimmed = name.trim_start_matches('.').to_string();
    // Fast path: exact match.
    if let Some(sym) = index.lookup(&trimmed) {
        if sym.kind == SymbolKind::Message {
            if scope_file.map_or(true, |f| &sym.file == f) {
                return Some(sym.fqn.to_string());
            }
        }
    }
    // Fallback: search per-file for a matching short or dotted suffix within
    // `scope_file`'s package scope.
    if let Some(f) = scope_file {
        if let Some(fs) = index.file_symbols(f) {
            let pkg = fs.package.clone();
            let with_pkg = if pkg.is_empty() { trimmed.clone() } else { format!("{}.{}", pkg, trimmed) };
            if let Some(sym) = index.lookup(&with_pkg) {
                if sym.kind == SymbolKind::Message {
                    return Some(sym.fqn.to_string());
                }
            }
        }
    }
    None
}

fn resolve_scope_aware(index: &WorkspaceIndex, scope_fqn: &str, name: &str) -> Option<Symbol> {
    // Imitate proto3's innermost-out scope walk.
    let mut cur = scope_fqn.to_string();
    loop {
        let cand = if cur.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", cur, name)
        };
        if let Some(sym) = index.lookup(&cand) {
            return Some(sym.clone());
        }
        if cur.is_empty() {
            return None;
        }
        match cur.rfind('.') {
            Some(i) => cur.truncate(i),
            None => cur.clear(),
        }
    }
}

// ─────────────────────── per-message resolution ─────────────────────────

/// Cache of a resolved message schema — every field by name and every
/// oneof → field mapping for quick lookup.
struct ResolvedMessage<'a> {
    fqn: String,
    fields: Vec<&'a proto_ast::FieldDecl>,
    oneofs: Vec<(SmolStr, Vec<SmolStr>)>,
}

impl<'a> ResolvedMessage<'a> {
    fn field_by_name(&self, name: &str) -> Option<&proto_ast::FieldDecl> {
        self.fields
            .iter()
            .find(|f| f.name.name == name)
            .copied()
    }

    fn oneof_of(&self, field_name: &str) -> Option<SmolStr> {
        for (oneof, members) in &self.oneofs {
            if members.iter().any(|m| m.as_str() == field_name) {
                return Some(oneof.clone());
            }
        }
        None
    }
}

fn lookup_message<'a>(ctx: &'a Ctx<'_>, fqn: &str) -> Option<ResolvedMessage<'a>> {
    let sym = ctx.index.lookup(fqn)?;
    if sym.kind != SymbolKind::Message {
        return None;
    }
    let pf = ctx.ws.file(&sym.file)?;
    let message = find_message_in_ast(&pf.ast, &sym.file, fqn)?;
    let mut fields: Vec<&proto_ast::FieldDecl> = message.fields.iter().collect();
    let mut oneofs: Vec<(SmolStr, Vec<SmolStr>)> = Vec::new();
    for o in &message.oneofs {
        let members: Vec<SmolStr> = o.fields.iter().map(|f| f.name.name.clone()).collect();
        oneofs.push((o.name.name.clone(), members));
        for f in &o.fields {
            fields.push(f);
        }
    }
    Some(ResolvedMessage { fqn: fqn.to_string(), fields, oneofs })
}

fn find_message_in_ast<'a>(
    file: &'a proto_ast::File,
    _file_uri: &FileUri,
    fqn: &str,
) -> Option<&'a proto_ast::Message> {
    // Strip any package prefix and walk the AST.
    let package_prefix = match &file.package {
        Some(p) => p.name.to_display(),
        None => String::new(),
    };
    let stripped = if package_prefix.is_empty() {
        fqn.to_string()
    } else {
        let pfx = format!("{}.", package_prefix);
        if let Some(rest) = fqn.strip_prefix(&pfx) {
            rest.to_string()
        } else {
            return None;
        }
    };
    let parts: Vec<&str> = stripped.split('.').collect();
    for item in &file.items {
        if let proto_ast::TopLevelItem::Message(m) = item {
            if m.name.name == parts[0] {
                return find_nested(m, &parts[1..]);
            }
        }
    }
    None
}

fn find_nested<'a>(m: &'a proto_ast::Message, rest: &[&str]) -> Option<&'a proto_ast::Message> {
    if rest.is_empty() {
        return Some(m);
    }
    for nm in &m.nested_messages {
        if nm.name.name == rest[0] {
            return find_nested(nm, &rest[1..]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::Workspace;

    fn make_ws(proto_source: &str, proto_uri: &str, textproto_source: &str) -> (Workspace, FileUri) {
        let mut ws = Workspace::new();
        ws.update_file(FileUri::new(proto_uri), proto_source.to_string());
        let tp_uri = FileUri::new("mem://doc.textproto");
        ws.update_textproto_file(tp_uri.clone(), textproto_source.to_string());
        (ws, tp_uri)
    }

    fn diagnostics(ws: &Workspace, uri: &FileUri) -> Vec<ProtoDiagnostic> {
        ws.textproto_diagnostics_for(uri)
    }

    #[test]
    fn field_exists_check_succeeds_for_known_field() {
        let proto = r#"
            syntax = "proto3";
            package pkg;
            message Person {
                string name = 1;
                int32 age = 2;
            }
        "#;
        let tp = "# proto-file: pkg/person.proto\n# proto-message: pkg.Person\nname: \"Alice\"\nage: 30\n";
        let (ws, uri) = make_ws(proto, "pkg/person.proto", tp);
        let diags = diagnostics(&ws, &uri);
        let relevant: Vec<_> = diags
            .iter()
            .filter(|d| matches!(
                d.code,
                DiagnosticCode::TextprotoFieldUnknown | DiagnosticCode::TextprotoFieldTypeMismatch
            ))
            .collect();
        assert!(relevant.is_empty(), "unexpected schema diagnostics: {:?}", relevant);
    }

    #[test]
    fn unknown_field_is_reported() {
        let proto = r#"
            syntax = "proto3";
            package pkg;
            message Person { string name = 1; }
        "#;
        let tp =
            "# proto-file: pkg/person.proto\n# proto-message: pkg.Person\nname: \"A\"\nwat: 1\n";
        let (ws, uri) = make_ws(proto, "pkg/person.proto", tp);
        let diags = diagnostics(&ws, &uri);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::TextprotoFieldUnknown
                    && d.message.contains("wat")),
            "diags: {:?}",
            diags
        );
    }

    #[test]
    fn scalar_type_mismatch_is_reported() {
        let proto = r#"
            syntax = "proto3";
            package pkg;
            message Person { int32 age = 1; }
        "#;
        let tp = "# proto-file: pkg/person.proto\n# proto-message: pkg.Person\nage: \"not a number\"\n";
        let (ws, uri) = make_ws(proto, "pkg/person.proto", tp);
        let diags = diagnostics(&ws, &uri);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::TextprotoFieldTypeMismatch),
            "diags: {:?}",
            diags
        );
    }

    #[test]
    fn nested_message_fields_are_checked() {
        let proto = r#"
            syntax = "proto3";
            package pkg;
            message Address { string city = 1; }
            message Person {
                string name = 1;
                Address address = 2;
            }
        "#;
        let tp = "# proto-file: pkg/person.proto\n# proto-message: pkg.Person\nname: \"A\"\naddress { wrong_key: \"NY\" }\n";
        let (ws, uri) = make_ws(proto, "pkg/person.proto", tp);
        let diags = diagnostics(&ws, &uri);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::TextprotoFieldUnknown
                    && d.message.contains("wrong_key")),
            "diags: {:?}",
            diags
        );
    }

    #[test]
    fn enum_value_name_is_validated() {
        let proto = r#"
            syntax = "proto3";
            package pkg;
            enum Kind { UNKNOWN = 0; DOG = 1; CAT = 2; }
            message Pet { Kind kind = 1; }
        "#;
        let tp_ok = "# proto-file: pkg/pet.proto\n# proto-message: pkg.Pet\nkind: DOG\n";
        let tp_bad = "# proto-file: pkg/pet.proto\n# proto-message: pkg.Pet\nkind: MOUSE\n";
        let (ws_ok, u_ok) = make_ws(proto, "pkg/pet.proto", tp_ok);
        let (ws_bad, u_bad) = make_ws(proto, "pkg/pet.proto", tp_bad);
        assert!(
            diagnostics(&ws_ok, &u_ok)
                .iter()
                .all(|d| d.code != DiagnosticCode::TextprotoEnumValueUnknown)
        );
        assert!(
            diagnostics(&ws_bad, &u_bad)
                .iter()
                .any(|d| d.code == DiagnosticCode::TextprotoEnumValueUnknown)
        );
    }

    #[test]
    fn repeated_list_is_allowed_singular_list_is_flagged() {
        let proto = r#"
            syntax = "proto3";
            package pkg;
            message M {
                repeated int32 xs = 1;
                int32 y = 2;
            }
        "#;
        let tp = "# proto-file: pkg/m.proto\n# proto-message: pkg.M\nxs: [1, 2]\ny: [1, 2]\n";
        let (ws, uri) = make_ws(proto, "pkg/m.proto", tp);
        let diags = diagnostics(&ws, &uri);
        let mismatches: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::TextprotoFieldTypeMismatch)
            .collect();
        assert_eq!(mismatches.len(), 1, "expected exactly one mismatch, got: {:?}", diags);
    }

    #[test]
    fn missing_proto_message_skips_validation() {
        let proto = r#"
            syntax = "proto3";
            message M { string x = 1; }
        "#;
        let tp = "wat: 1\nfoo: \"bar\"\n";
        let (ws, uri) = make_ws(proto, "pkg/m.proto", tp);
        let diags = diagnostics(&ws, &uri);
        // No header → no schema-binding checks.
        assert!(
            diags
                .iter()
                .all(|d| !matches!(
                    d.code,
                    DiagnosticCode::TextprotoFieldUnknown
                        | DiagnosticCode::TextprotoFieldTypeMismatch
                )),
            "diags: {:?}",
            diags
        );
    }

    #[test]
    fn unresolved_header_message_reports() {
        let proto = "syntax = \"proto3\"; message M { string x = 1; }";
        let tp = "# proto-message: pkg.DoesNotExist\nx: \"a\"\n";
        let (ws, uri) = make_ws(proto, "pkg/m.proto", tp);
        let diags = diagnostics(&ws, &uri);
        assert!(diags
            .iter()
            .any(|d| d.code == DiagnosticCode::TextprotoSchemaMessageUnknown));
    }
}

