//! LSP-style feature providers for textproto documents.
//!
//! Every provider binds against the `# proto-message:` header hint to surface
//! schema-aware intellisense — document symbols, folding ranges, hover,
//! go-to-definition, and field completion. When the header is absent or the
//! referenced message is unresolved, providers degrade gracefully: structure
//! features (symbols, folds) still work; semantic features (hover, definition,
//! completion of field names) are no-ops because there's no schema to bind to.

use super::ast::{self, Value};
use super::parse::ParsedTextproto;
use super::schema::{
    lookup_message, resolve_header_file, resolve_message_fqn, resolve_scope_aware, Ctx,
};
use crate::ast as proto_ast;
use crate::resolve::{Symbol, SymbolKind, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};

use crate::features::completion::{CompletionItem, CompletionKind};
use crate::features::definition::Location;
use crate::features::document_symbols::{DocumentSymbol, SymbolKind as DocKind};
use crate::features::folding::{FoldingKind, FoldingRange};
use crate::features::hover::Hover;

// ─────────────────────────── document symbols ───────────────────────────

pub fn document_symbols(pt: &ParsedTextproto) -> Vec<DocumentSymbol> {
    pt.ast
        .fields
        .iter()
        .map(|f| field_symbol(f, ""))
        .collect()
}

fn field_symbol(f: &ast::Field, parent: &str) -> DocumentSymbol {
    let name = f.name.display_name();
    let detail = match &f.value {
        Value::Message { .. } => String::from("message"),
        Value::List { elements, .. } => format!("list ({})", elements.len()),
        other => other.kind_label().to_string(),
    };
    let children = match &f.value {
        Value::Message { fields, .. } => fields.iter().map(|cf| field_symbol(cf, &name)).collect(),
        Value::List { elements, .. } => elements
            .iter()
            .enumerate()
            .filter_map(|(i, el)| list_element_symbol(el, i, &name))
            .collect(),
        _ => Vec::new(),
    };
    let _ = parent; // reserved for future fully-qualified detail rendering.
    DocumentSymbol {
        name,
        detail,
        kind: if matches!(f.value, Value::Message { .. }) {
            DocKind::Message
        } else {
            DocKind::Field
        },
        range: f.span,
        selection_range: f.name.span(),
        children,
    }
}

fn list_element_symbol(v: &Value, i: usize, parent: &str) -> Option<DocumentSymbol> {
    // Only message-valued list elements get a symbol — scalars and idents are
    // visible enough at their line and would just clutter the outline.
    let Value::Message { fields, span, .. } = v else {
        return None;
    };
    let children = fields.iter().map(|cf| field_symbol(cf, parent)).collect();
    Some(DocumentSymbol {
        name: format!("[{}]", i),
        detail: "message".into(),
        kind: DocKind::Message,
        range: *span,
        selection_range: *span,
        children,
    })
}

// ───────────────────────────── folding ranges ───────────────────────────

pub fn folding_ranges(pt: &ParsedTextproto) -> Vec<FoldingRange> {
    let mut out = Vec::new();
    for f in &pt.ast.fields {
        collect_folds(&f.value, &mut out);
    }
    out
}

fn collect_folds(v: &Value, out: &mut Vec<FoldingRange>) {
    match v {
        Value::Message { fields, span, .. } => {
            out.push(FoldingRange { span: *span, kind: FoldingKind::Region });
            for f in fields {
                collect_folds(&f.value, out);
            }
        }
        Value::List { elements, span } => {
            out.push(FoldingRange { span: *span, kind: FoldingKind::Region });
            for el in elements {
                collect_folds(el, out);
            }
        }
        _ => {}
    }
}

// ────────────────────────── position helpers ────────────────────────────

fn root_message_fqn(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
) -> Option<String> {
    let hint = pt.header().proto_message.as_ref()?;
    let scope_file: Option<FileUri> = pt
        .header()
        .proto_file
        .as_ref()
        .and_then(|h| resolve_header_file(ws, &h.value));
    resolve_message_fqn(index, scope_file.as_ref(), &hint.value)
}

/// Given a field on `parent_fqn`, resolve the message FQN that the field's
/// value inhabits. Returns `None` for scalar/enum fields.
fn child_message_fqn(
    ws: &Workspace,
    index: &WorkspaceIndex,
    parent_fqn: &str,
    f: &ast::Field,
) -> Option<String> {
    match &f.name {
        ast::FieldName::Any { url, .. } => {
            let target = url.type_name.to_display();
            let sym = index.lookup(&target)?;
            if sym.kind == SymbolKind::Message {
                Some(sym.fqn.to_string())
            } else {
                None
            }
        }
        ast::FieldName::Extension { .. } => None,
        ast::FieldName::Ident(ident) => {
            let ctx = Ctx { ws, index };
            let resolved = lookup_message(&ctx, parent_fqn)?;
            let fdecl = resolved.field_by_name(&ident.name)?;
            match &fdecl.ty {
                proto_ast::TypeRef::Named(q) => {
                    let name_str = q.to_display();
                    let trimmed = name_str.trim_start_matches('.').to_string();
                    let sym = resolve_scope_aware(index, parent_fqn, &trimmed)?;
                    if sym.kind == SymbolKind::Message {
                        Some(sym.fqn.to_string())
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
    }
}

/// Resolve the FQN of the innermost message whose body contains `offset`.
/// Returns `None` when there's no header binding or the root is unresolved.
fn enclosing_message_fqn(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
    offset: u32,
) -> Option<String> {
    let root = root_message_fqn(ws, index, pt)?;
    Some(descend_to_offset(ws, index, &root, &pt.ast.fields, offset))
}

fn descend_to_offset(
    ws: &Workspace,
    index: &WorkspaceIndex,
    current_fqn: &str,
    fields: &[ast::Field],
    offset: u32,
) -> String {
    for f in fields {
        if !f.span.contains(offset) {
            continue;
        }
        let sub_fqn = child_message_fqn(ws, index, current_fqn, f);
        let Some(sub) = sub_fqn else {
            return current_fqn.to_string();
        };
        match &f.value {
            Value::Message { fields: inner, span, .. } if span.contains(offset) => {
                return descend_to_offset(ws, index, &sub, inner, offset);
            }
            Value::List { elements, .. } => {
                for el in elements {
                    if let Value::Message { fields: inner, span, .. } = el {
                        if span.contains(offset) {
                            return descend_to_offset(ws, index, &sub, inner, offset);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    current_fqn.to_string()
}

/// The `(field, parent_fqn)` whose *name* token contains `offset`. Walks
/// the AST just like `descend_to_offset` but returns the leaf field whose
/// name span hits.
fn field_name_at(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
    offset: u32,
) -> Option<(ast::Field, String)> {
    let root = root_message_fqn(ws, index, pt)?;
    find_field_name(ws, index, &root, &pt.ast.fields, offset)
}

fn find_field_name(
    ws: &Workspace,
    index: &WorkspaceIndex,
    current_fqn: &str,
    fields: &[ast::Field],
    offset: u32,
) -> Option<(ast::Field, String)> {
    for f in fields {
        if f.name.span().contains(offset) {
            return Some((f.clone(), current_fqn.to_string()));
        }
        if !f.span.contains(offset) {
            continue;
        }
        if let Some(sub) = child_message_fqn(ws, index, current_fqn, f) {
            match &f.value {
                Value::Message { fields: inner, .. } => {
                    if let Some(hit) = find_field_name(ws, index, &sub, inner, offset) {
                        return Some(hit);
                    }
                }
                Value::List { elements, .. } => {
                    for el in elements {
                        if let Value::Message { fields: inner, .. } = el {
                            if let Some(hit) = find_field_name(ws, index, &sub, inner, offset) {
                                return Some(hit);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// The enclosing field of `offset`, along with the FQN of its enclosing
/// message. Unlike `field_name_at`, this matches anywhere inside the field's
/// *value* span too — used to detect "cursor on an enum-valued ident".
fn enclosing_field_at(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
    offset: u32,
) -> Option<(ast::Field, String)> {
    let root = root_message_fqn(ws, index, pt)?;
    find_enclosing_field(ws, index, &root, &pt.ast.fields, offset)
}

fn find_enclosing_field(
    ws: &Workspace,
    index: &WorkspaceIndex,
    current_fqn: &str,
    fields: &[ast::Field],
    offset: u32,
) -> Option<(ast::Field, String)> {
    for f in fields {
        if !f.span.contains(offset) {
            continue;
        }
        // Prefer a deeper hit inside nested messages.
        if let Some(sub) = child_message_fqn(ws, index, current_fqn, f) {
            match &f.value {
                Value::Message { fields: inner, .. } => {
                    if let Some(hit) = find_enclosing_field(ws, index, &sub, inner, offset) {
                        return Some(hit);
                    }
                }
                Value::List { elements, .. } => {
                    for el in elements {
                        if let Value::Message { fields: inner, .. } = el {
                            if let Some(hit) = find_enclosing_field(ws, index, &sub, inner, offset)
                            {
                                return Some(hit);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        return Some((f.clone(), current_fqn.to_string()));
    }
    None
}

// ──────────────────────────────── hover ─────────────────────────────────

pub fn hover(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
    offset: u32,
) -> Option<Hover> {
    // Header annotations first — cursor on `proto-message:` or `proto-file:`.
    if let Some(h) = header_hover(ws, index, pt, offset) {
        return Some(h);
    }
    // Field name → schema field.
    if let Some((field, parent_fqn)) = field_name_at(ws, index, pt, offset) {
        return render_field_hover(ws, index, &field, &parent_fqn);
    }
    // Enum value (cursor on a bare ident value).
    if let Some((field, parent_fqn)) = enclosing_field_at(ws, index, pt, offset) {
        return render_enum_value_hover(ws, index, &field, &parent_fqn, offset);
    }
    None
}

fn render_field_hover(
    ws: &Workspace,
    index: &WorkspaceIndex,
    field: &ast::Field,
    parent_fqn: &str,
) -> Option<Hover> {
    let ast::FieldName::Ident(ident) = &field.name else {
        return None;
    };
    let ctx = Ctx { ws, index };
    let resolved = lookup_message(&ctx, parent_fqn)?;
    let fdecl = resolved.field_by_name(&ident.name)?;
    let label = match fdecl.label {
        proto_ast::FieldLabel::Repeated => "repeated ",
        proto_ast::FieldLabel::Optional => "optional ",
        proto_ast::FieldLabel::Required => "required ",
        proto_ast::FieldLabel::None => "",
    };
    let ty_txt = type_label(&fdecl.ty);
    let num = fdecl
        .number
        .as_i64()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".into());
    let mut md = format!(
        "```proto3\n{}{} {} = {};\n```\n\n*field of* `.{}`",
        label, ty_txt, fdecl.name.name, num, parent_fqn,
    );
    if let Some(doc) = proto_ast::doc_comment_text(&fdecl.leading_comments) {
        md.push_str("\n\n");
        md.push_str(&doc);
    }
    Some(Hover { markdown: md, range: ident.span })
}

fn render_enum_value_hover(
    ws: &Workspace,
    index: &WorkspaceIndex,
    field: &ast::Field,
    parent_fqn: &str,
    offset: u32,
) -> Option<Hover> {
    let ast::FieldName::Ident(name) = &field.name else {
        return None;
    };
    let ident_span = match &field.value {
        Value::Ident(i) if i.span.contains(offset) => i.span,
        Value::SignedIdent { ident, span } if span.contains(offset) => ident.span,
        _ => return None,
    };
    let ident_name = match &field.value {
        Value::Ident(i) => i.name.as_str(),
        Value::SignedIdent { ident, .. } => ident.name.as_str(),
        _ => return None,
    };
    let ctx = Ctx { ws, index };
    let resolved = lookup_message(&ctx, parent_fqn)?;
    let fdecl = resolved.field_by_name(&name.name)?;
    let proto_ast::TypeRef::Named(q) = &fdecl.ty else {
        return None;
    };
    let name_str = q.to_display();
    let trimmed = name_str.trim_start_matches('.').to_string();
    let sym = resolve_scope_aware(index, parent_fqn, &trimmed)?;
    if sym.kind != SymbolKind::Enum {
        return None;
    }
    let value_fqn = format!("{}.{}", sym.fqn, ident_name);
    let md = match index.lookup(&value_fqn) {
        Some(vsym) => {
            let mut s = format!(
                "```proto3\n{} = (enum value of .{})\n```",
                vsym.name, sym.fqn
            );
            if let Some(doc) = &vsym.doc {
                s.push_str("\n\n");
                s.push_str(doc);
            }
            s
        }
        None => format!("Unknown value `{}` for enum `.{}`", ident_name, sym.fqn),
    };
    Some(Hover { markdown: md, range: ident_span })
}

fn header_hover(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
    offset: u32,
) -> Option<Hover> {
    let hints = pt.header();
    if let Some(ann) = &hints.proto_message {
        if ann.value_span.contains(offset) {
            let md = match resolve_message_fqn(
                index,
                hints
                    .proto_file
                    .as_ref()
                    .and_then(|h| resolve_header_file(ws, &h.value))
                    .as_ref(),
                &ann.value,
            ) {
                Some(fqn) => format!(
                    "```proto3\nmessage .{}\n```\n\n*bound schema for this file*",
                    fqn
                ),
                None => format!("Unknown message `{}`", ann.value),
            };
            return Some(Hover { markdown: md, range: ann.value_span });
        }
    }
    if let Some(ann) = &hints.proto_file {
        if ann.value_span.contains(offset) {
            let md = match resolve_header_file(ws, &ann.value) {
                Some(uri) => format!("```\n{}\n```", uri.as_str()),
                None => format!("Cannot resolve schema file `{}`", ann.value),
            };
            return Some(Hover { markdown: md, range: ann.value_span });
        }
    }
    None
}

// ──────────────────────────── definition ────────────────────────────────

pub fn definition(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
    offset: u32,
) -> Option<Location> {
    // Header annotations.
    if let Some(loc) = header_definition(ws, index, pt, offset) {
        return Some(loc);
    }
    // Field name → proto field definition.
    if let Some((field, parent_fqn)) = field_name_at(ws, index, pt, offset) {
        if let Some(loc) = field_definition(ws, index, &field, &parent_fqn) {
            return Some(loc);
        }
    }
    // Enum value → proto enum value definition.
    if let Some((field, parent_fqn)) = enclosing_field_at(ws, index, pt, offset) {
        return enum_value_definition(ws, index, &field, &parent_fqn, offset);
    }
    None
}

fn field_definition(
    ws: &Workspace,
    index: &WorkspaceIndex,
    field: &ast::Field,
    parent_fqn: &str,
) -> Option<Location> {
    let ast::FieldName::Ident(ident) = &field.name else {
        return None;
    };
    let ctx = Ctx { ws, index };
    let resolved = lookup_message(&ctx, parent_fqn)?;
    let fdecl = resolved.field_by_name(&ident.name)?;
    // Locate the defining symbol via the workspace index.
    let sym_fqn = format!("{}.{}", resolved.fqn, fdecl.name.name);
    let sym = index.lookup(&sym_fqn)?;
    Some(Location {
        file: sym.file.as_str().to_string(),
        range: sym.name_span,
    })
}

fn enum_value_definition(
    ws: &Workspace,
    index: &WorkspaceIndex,
    field: &ast::Field,
    parent_fqn: &str,
    offset: u32,
) -> Option<Location> {
    let ast::FieldName::Ident(name) = &field.name else {
        return None;
    };
    let ident_name = match &field.value {
        Value::Ident(i) if i.span.contains(offset) => i.name.as_str(),
        Value::SignedIdent { ident, span } if span.contains(offset) => ident.name.as_str(),
        _ => return None,
    };
    let ctx = Ctx { ws, index };
    let resolved = lookup_message(&ctx, parent_fqn)?;
    let fdecl = resolved.field_by_name(&name.name)?;
    let proto_ast::TypeRef::Named(q) = &fdecl.ty else {
        return None;
    };
    let name_str = q.to_display();
    let trimmed = name_str.trim_start_matches('.').to_string();
    let sym: Symbol = resolve_scope_aware(index, parent_fqn, &trimmed)?;
    if sym.kind != SymbolKind::Enum {
        return None;
    }
    let value_fqn = format!("{}.{}", sym.fqn, ident_name);
    let vsym = index.lookup(&value_fqn)?;
    Some(Location {
        file: vsym.file.as_str().to_string(),
        range: vsym.name_span,
    })
}

fn header_definition(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
    offset: u32,
) -> Option<Location> {
    let hints = pt.header();
    if let Some(ann) = &hints.proto_message {
        if ann.value_span.contains(offset) {
            let fqn = resolve_message_fqn(
                index,
                hints
                    .proto_file
                    .as_ref()
                    .and_then(|h| resolve_header_file(ws, &h.value))
                    .as_ref(),
                &ann.value,
            )?;
            let sym = index.lookup(&fqn)?;
            return Some(Location {
                file: sym.file.as_str().to_string(),
                range: sym.name_span,
            });
        }
    }
    if let Some(ann) = &hints.proto_file {
        if ann.value_span.contains(offset) {
            let uri = resolve_header_file(ws, &ann.value)?;
            return Some(Location {
                file: uri.as_str().to_string(),
                range: ByteSpan::EMPTY,
            });
        }
    }
    None
}

// ──────────────────────────── completion ────────────────────────────────

pub fn completion(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
    offset: u32,
) -> Vec<CompletionItem> {
    // Cursor on a `#` comment line — offer header keys/values.
    if let Some(items) = header_completion(ws, index, pt, offset) {
        return items;
    }
    let Some(fqn) = enclosing_message_fqn(ws, index, pt, offset) else {
        // No schema binding — still offer header-key completion for a new `#`
        // line so the user can start typing `# proto-message: …`.
        return Vec::new();
    };
    let ctx = Ctx { ws, index };
    let Some(resolved) = lookup_message(&ctx, &fqn) else {
        return Vec::new();
    };
    let out: Vec<CompletionItem> = resolved
        .fields
        .iter()
        .map(|fd| completion_for_field(fd, &fqn, index))
        .collect();

    // Cursor on an enum-valued field's value: prepend value completions.
    if let Some((field, parent_fqn)) = enclosing_field_at(ws, index, pt, offset) {
        if let ast::FieldName::Ident(name) = &field.name {
            if let Some(parent) = lookup_message(&ctx, &parent_fqn) {
                if let Some(fdecl) = parent.field_by_name(&name.name) {
                    if let proto_ast::TypeRef::Named(q) = &fdecl.ty {
                        let name_str = q.to_display();
                        let trimmed = name_str.trim_start_matches('.').to_string();
                        if let Some(sym) = resolve_scope_aware(index, &parent_fqn, &trimmed) {
                            if sym.kind == SymbolKind::Enum {
                                let mut enum_items = enum_value_items(index, &sym.fqn);
                                enum_items.extend(out);
                                return enum_items;
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

// ─────────────────────────── header completion ─────────────────────────

const HEADER_KEYS: &[&str] = &["proto-file", "proto-message", "proto-import", "proto-syntax"];

/// If `offset` lies on a `#` comment line, return either key or value
/// suggestions for the recognised header annotations. Returns `None` when
/// the cursor isn't on a `#` line so the caller can fall back to field
/// completion.
fn header_completion(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
    offset: u32,
) -> Option<Vec<CompletionItem>> {
    let src = pt.source.as_str();
    let off = (offset as usize).min(src.len());
    let line_start = src[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = src[off..]
        .find('\n')
        .map(|i| off + i)
        .unwrap_or(src.len());
    let line = &src[line_start..line_end];
    let trimmed_start = line.trim_start();
    let leading_ws = line.len() - trimmed_start.len();
    let after_ws_col = line_start + leading_ws;
    // Only treat as a header line if `#` is the first non-whitespace char and
    // the cursor sits at-or-after that `#`.
    if !trimmed_start.starts_with('#') || off < after_ws_col {
        return None;
    }
    let body_start = after_ws_col + 1; // skip `#`
    if off < body_start {
        // Cursor is literally on the `#` character — offer header keys.
        return Some(header_key_items(""));
    }
    let body = &src[body_start..line_end];
    let body_rel = off - body_start;
    // Split at the first `:` to distinguish key from value region.
    match body.find(':') {
        None => {
            // No colon yet — we're in the key part. Offer header keys.
            let key_text = body[..body_rel].trim_start();
            Some(header_key_items(key_text))
        }
        Some(colon) if body_rel <= colon => {
            let key_text = body[..body_rel].trim_start();
            Some(header_key_items(key_text))
        }
        Some(colon) => {
            // Cursor is on the value side.
            let key = body[..colon].trim();
            let key = key.trim_start_matches('#').trim();
            match key {
                "proto-message" => Some(message_fqn_items(ws, index, pt)),
                "proto-file" | "proto-import" => Some(proto_file_items(ws)),
                "proto-syntax" => Some(proto_syntax_items()),
                _ => None,
            }
        }
    }
}

fn header_key_items(prefix: &str) -> Vec<CompletionItem> {
    let prefix = prefix.trim_start_matches('#').trim();
    HEADER_KEYS
        .iter()
        .filter(|k| prefix.is_empty() || k.starts_with(prefix))
        .map(|k| CompletionItem {
            label: (*k).into(),
            insert_text: format!("{}: $0", k),
            kind: CompletionKind::Keyword,
            detail: "header annotation".into(),
        })
        .collect()
}

fn message_fqn_items(
    ws: &Workspace,
    index: &WorkspaceIndex,
    pt: &ParsedTextproto,
) -> Vec<CompletionItem> {
    // If a `# proto-file:` hint is present and resolves, restrict to messages
    // defined in that file. Otherwise list every message in the workspace.
    let scope_file: Option<FileUri> = pt
        .header()
        .proto_file
        .as_ref()
        .and_then(|h| resolve_header_file(ws, &h.value));
    let mut out = Vec::new();
    for sym in index.all_symbols() {
        if sym.kind != SymbolKind::Message {
            continue;
        }
        if let Some(f) = &scope_file {
            if &sym.file != f {
                continue;
            }
        }
        out.push(CompletionItem {
            label: sym.fqn.to_string(),
            insert_text: sym.fqn.to_string(),
            kind: CompletionKind::Message,
            detail: sym
                .detail
                .clone()
                .unwrap_or_else(|| "message".into()),
        });
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

fn proto_file_items(ws: &Workspace) -> Vec<CompletionItem> {
    let mut out = Vec::new();
    for (uri, _) in ws.files() {
        let path = short_path_for(ws, uri);
        if path.is_empty() {
            continue;
        }
        out.push(CompletionItem {
            label: path.clone(),
            insert_text: path,
            kind: CompletionKind::Keyword,
            detail: uri.as_str().to_string(),
        });
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out.dedup_by(|a, b| a.label == b.label);
    out
}

fn short_path_for(ws: &Workspace, uri: &FileUri) -> String {
    let s = uri.as_str();
    if let Some(rest) = s.strip_prefix("proto3-wkt:/") {
        return rest.to_string();
    }
    // Prefer the include-path-relative form, matching the resolver.
    for inc in ws.include_paths() {
        let prefix = format!("{}/", inc.0.trim_end_matches('/'));
        if let Some(rest) = s.strip_prefix(&prefix) {
            return rest.to_string();
        }
    }
    // Fall back to the URI's last path component — a best-effort label the
    // user can edit before accepting.
    s.rsplit('/').next().unwrap_or(s).to_string()
}

fn proto_syntax_items() -> Vec<CompletionItem> {
    ["proto2", "proto3", "editions"]
        .iter()
        .map(|v| CompletionItem {
            label: (*v).into(),
            insert_text: (*v).into(),
            kind: CompletionKind::Keyword,
            detail: "syntax".into(),
        })
        .collect()
}

fn completion_for_field(
    fd: &proto_ast::FieldDecl,
    parent_fqn: &str,
    index: &WorkspaceIndex,
) -> CompletionItem {
    let insert = insert_snippet_for(fd, parent_fqn, index);
    let detail = format!("{}{}", label_str(fd.label), type_label(&fd.ty));
    CompletionItem {
        label: fd.name.name.to_string(),
        insert_text: insert,
        kind: CompletionKind::Field,
        detail,
    }
}

fn enum_value_items(index: &WorkspaceIndex, enum_fqn: &str) -> Vec<CompletionItem> {
    let prefix = format!("{}.", enum_fqn);
    let mut out = Vec::new();
    for sym in index.all_symbols() {
        if sym.kind == SymbolKind::EnumValue && sym.fqn.starts_with(&prefix) {
            out.push(CompletionItem {
                label: sym.name.to_string(),
                insert_text: sym.name.to_string(),
                kind: CompletionKind::EnumValue,
                detail: sym.fqn.to_string(),
            });
        }
    }
    out
}

/// Pick an insert string for a field completion. Scalars get a plain
/// `name: ` so the cursor naturally lands at the end; strings/bytes wrap the
/// value site in quotes with the cursor between them; messages open a
/// `{ … }` block with the cursor inside. Snippets (`$0`, `$1`) are only used
/// where a non-trailing cursor position is required — callers detect them on
/// the TS side and wrap in `SnippetString`.
fn insert_snippet_for(
    fd: &proto_ast::FieldDecl,
    parent_fqn: &str,
    index: &WorkspaceIndex,
) -> String {
    use proto_ast::ScalarType::*;
    let name = &fd.name.name;
    let is_repeated = matches!(fd.label, proto_ast::FieldLabel::Repeated);

    match &fd.ty {
        proto_ast::TypeRef::Map(_) => {
            format!("{}: {{ key: $1, value: $2 }}", name)
        }
        proto_ast::TypeRef::Named(q) => {
            let s = q.to_display();
            let trimmed = s.trim_start_matches('.').to_string();
            let kind = resolve_scope_aware(index, parent_fqn, &trimmed).map(|s| s.kind);
            match kind {
                Some(SymbolKind::Enum) => {
                    // Enum: `name: ` — cursor at end; enum-value completion
                    // kicks in once the user types or re-triggers.
                    format!("{}: ", name)
                }
                _ => {
                    // Default to message form — `name {\n\t$0\n}`.
                    format!("{} {{\n\t$0\n}}", name)
                }
            }
        }
        proto_ast::TypeRef::Scalar(scalar, _) => {
            let is_string = matches!(scalar, String | Bytes);
            if is_repeated {
                if is_string {
                    format!("{}: [\"$0\"]", name)
                } else {
                    format!("{}: [$0]", name)
                }
            } else if is_string {
                format!("{}: \"$0\"", name)
            } else {
                format!("{}: ", name)
            }
        }
        proto_ast::TypeRef::Missing(_) => format!("{}: ", name),
    }
}

fn label_str(l: proto_ast::FieldLabel) -> &'static str {
    match l {
        proto_ast::FieldLabel::Repeated => "repeated ",
        proto_ast::FieldLabel::Optional => "optional ",
        proto_ast::FieldLabel::Required => "required ",
        proto_ast::FieldLabel::None => "",
    }
}

fn type_label(t: &proto_ast::TypeRef) -> String {
    match t {
        proto_ast::TypeRef::Scalar(s, _) => s.as_str().to_string(),
        proto_ast::TypeRef::Named(q) => q.to_display(),
        proto_ast::TypeRef::Map(m) => {
            format!("map<{}, {}>", type_label(&m.key), type_label(&m.value))
        }
        proto_ast::TypeRef::Missing(_) => "?".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::Workspace;

    fn make_ws(proto: &str, proto_uri: &str, tp: &str) -> (Workspace, FileUri) {
        let mut ws = Workspace::new();
        ws.update_file(FileUri::new(proto_uri), proto.to_string());
        let tp_uri = FileUri::new("mem://doc.textproto");
        ws.update_textproto_file(tp_uri.clone(), tp.to_string());
        (ws, tp_uri)
    }

    fn offset_of(ws: &Workspace, uri: &FileUri, needle: &str) -> u32 {
        let pt = ws.textproto_file(uri).unwrap();
        pt.source.find(needle).unwrap() as u32
    }

    #[test]
    fn document_symbols_emit_fields_and_nested() {
        let proto = r#"syntax = "proto3"; package pkg;
            message Person { string name = 1; Address address = 2; }
            message Address { string city = 1; }
        "#;
        let tp = "# proto-file: pkg/p.proto\n# proto-message: pkg.Person\nname: \"A\"\naddress { city: \"NY\" }\n";
        let (ws, uri) = make_ws(proto, "pkg/p.proto", tp);
        let pt = ws.textproto_file(&uri).unwrap();
        let syms = document_symbols(pt);
        assert_eq!(syms.len(), 2);
        assert_eq!(syms[0].name, "name");
        assert_eq!(syms[1].name, "address");
        assert_eq!(syms[1].children.len(), 1);
        assert_eq!(syms[1].children[0].name, "city");
    }

    #[test]
    fn folding_ranges_include_nested_braces() {
        let proto = r#"syntax = "proto3"; message M { M inner = 1; }"#;
        let tp = "# proto-message: M\ninner {\n  inner {\n  }\n}\n";
        let (ws, uri) = make_ws(proto, "m.proto", tp);
        let pt = ws.textproto_file(&uri).unwrap();
        let folds = folding_ranges(pt);
        assert!(folds.len() >= 2, "got folds: {:?}", folds);
    }

    #[test]
    fn hover_on_field_name_shows_schema() {
        let proto = r#"syntax = "proto3"; package pkg;
            message Person { string name = 1; }
        "#;
        let tp = "# proto-file: pkg/p.proto\n# proto-message: pkg.Person\nname: \"A\"\n";
        let (ws, uri) = make_ws(proto, "pkg/p.proto", tp);
        let idx = ws.build_index();
        let pt = ws.textproto_file(&uri).unwrap();
        let off = offset_of(&ws, &uri, "name: ");
        let h = hover(&ws, &idx, pt, off + 1).expect("hover");
        assert!(h.markdown.contains("string name = 1"), "md: {}", h.markdown);
    }

    #[test]
    fn definition_on_field_jumps_to_proto() {
        let proto = r#"syntax = "proto3"; package pkg;
            message Person { string name = 1; }
        "#;
        let tp = "# proto-file: pkg/p.proto\n# proto-message: pkg.Person\nname: \"A\"\n";
        let (ws, uri) = make_ws(proto, "pkg/p.proto", tp);
        let idx = ws.build_index();
        let pt = ws.textproto_file(&uri).unwrap();
        let off = offset_of(&ws, &uri, "name: ");
        let loc = definition(&ws, &idx, pt, off + 1).expect("definition");
        assert_eq!(loc.file, "pkg/p.proto");
    }

    #[test]
    fn completion_lists_fields_of_enclosing_message() {
        let proto = r#"syntax = "proto3"; package pkg;
            message Person { string name = 1; int32 age = 2; }
        "#;
        let tp = "# proto-file: pkg/p.proto\n# proto-message: pkg.Person\n";
        let (ws, uri) = make_ws(proto, "pkg/p.proto", tp);
        let idx = ws.build_index();
        let pt = ws.textproto_file(&uri).unwrap();
        let off = pt.source.len() as u32;
        let items = completion(&ws, &idx, pt, off);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"name"), "labels: {:?}", labels);
        assert!(labels.contains(&"age"), "labels: {:?}", labels);
    }

    #[test]
    fn completion_for_enum_valued_field_offers_enum_values() {
        let proto = r#"syntax = "proto3"; package pkg;
            enum Kind { UNKNOWN = 0; DOG = 1; CAT = 2; }
            message Pet { Kind kind = 1; }
        "#;
        let tp = "# proto-file: pkg/p.proto\n# proto-message: pkg.Pet\nkind: D\n";
        let (ws, uri) = make_ws(proto, "pkg/p.proto", tp);
        let idx = ws.build_index();
        let pt = ws.textproto_file(&uri).unwrap();
        // Place the cursor on the `D` ident.
        let off = offset_of(&ws, &uri, "kind: D") + 6;
        let items = completion(&ws, &idx, pt, off);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"DOG"), "labels: {:?}", labels);
        assert!(labels.contains(&"CAT"), "labels: {:?}", labels);
    }

    #[test]
    fn completion_descends_into_nested_message_literal() {
        let proto = r#"syntax = "proto3"; package pkg;
            message Address { string city = 1; string zip = 2; }
            message Person { Address address = 1; }
        "#;
        let tp = "# proto-file: pkg/p.proto\n# proto-message: pkg.Person\naddress {\n  \n}\n";
        let (ws, uri) = make_ws(proto, "pkg/p.proto", tp);
        let idx = ws.build_index();
        let pt = ws.textproto_file(&uri).unwrap();
        // Cursor sits inside the `{ ... }` block.
        let off = offset_of(&ws, &uri, "{\n  ") + 3;
        let items = completion(&ws, &idx, pt, off);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"city"), "labels: {:?}", labels);
        assert!(labels.contains(&"zip"), "labels: {:?}", labels);
        assert!(!labels.contains(&"address"), "should not list parent fields");
    }

    #[test]
    fn completion_snippets_per_field_type() {
        let proto = r#"syntax = "proto3"; package pkg;
            enum Kind { K0 = 0; }
            message Inner { string s = 1; }
            message M {
                string s = 1;
                int64 n = 2;
                bool b = 3;
                Kind k = 4;
                Inner inner = 5;
                repeated string tags = 6;
                repeated int32 xs = 7;
                map<string, int32> counts = 8;
            }
        "#;
        let tp = "# proto-file: pkg/m.proto\n# proto-message: pkg.M\n";
        let (ws, uri) = make_ws(proto, "pkg/m.proto", tp);
        let idx = ws.build_index();
        let pt = ws.textproto_file(&uri).unwrap();
        let off = pt.source.len() as u32;
        let items = completion(&ws, &idx, pt, off);
        let by_label: std::collections::HashMap<_, _> = items
            .iter()
            .map(|i| (i.label.as_str(), i.insert_text.as_str()))
            .collect();
        assert_eq!(by_label.get("s").copied(), Some("s: \"$0\""));
        assert_eq!(by_label.get("n").copied(), Some("n: "));
        assert_eq!(by_label.get("b").copied(), Some("b: "));
        assert_eq!(by_label.get("k").copied(), Some("k: "));
        assert_eq!(by_label.get("inner").copied(), Some("inner {\n\t$0\n}"));
        assert_eq!(by_label.get("tags").copied(), Some("tags: [\"$0\"]"));
        assert_eq!(by_label.get("xs").copied(), Some("xs: [$0]"));
        assert_eq!(
            by_label.get("counts").copied(),
            Some("counts: { key: $1, value: $2 }")
        );
    }

    #[test]
    fn completion_on_proto_message_header_lists_messages() {
        let proto = r#"syntax = "proto3"; package pkg;
            message Person { string name = 1; }
            message Dog { string breed = 1; }
        "#;
        let tp = "# proto-message: \n";
        let (ws, uri) = make_ws(proto, "pkg/p.proto", tp);
        let idx = ws.build_index();
        let pt = ws.textproto_file(&uri).unwrap();
        // Cursor on the value side, just after the colon+space.
        let off = offset_of(&ws, &uri, ": ") + 2;
        let items = completion(&ws, &idx, pt, off);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"pkg.Person"), "labels: {:?}", labels);
        assert!(labels.contains(&"pkg.Dog"), "labels: {:?}", labels);
    }

    #[test]
    fn completion_on_proto_message_header_restricts_to_proto_file() {
        let mut ws = Workspace::new();
        ws.update_file(
            FileUri::new("pkg/a.proto"),
            "syntax = \"proto3\"; package pkg; message A { string x = 1; }".into(),
        );
        ws.update_file(
            FileUri::new("pkg/b.proto"),
            "syntax = \"proto3\"; package pkg; message B { string y = 1; }".into(),
        );
        let tp_uri = FileUri::new("mem://doc.textproto");
        let tp = "# proto-file: pkg/a.proto\n# proto-message: \n";
        ws.update_textproto_file(tp_uri.clone(), tp.to_string());
        let idx = ws.build_index();
        let pt = ws.textproto_file(&tp_uri).unwrap();
        let off = (pt.source.rfind(": ").unwrap() + 2) as u32;
        let items = completion(&ws, &idx, pt, off);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"pkg.A"), "labels: {:?}", labels);
        assert!(!labels.contains(&"pkg.B"), "labels: {:?}", labels);
    }

    #[test]
    fn completion_on_proto_file_header_lists_paths() {
        let mut ws = Workspace::new();
        ws.update_file(
            FileUri::new("/ws/pkg/a.proto"),
            "syntax = \"proto3\"; package pkg;".into(),
        );
        ws.set_include_paths(vec!["/ws".into()]);
        let tp_uri = FileUri::new("mem://doc.textproto");
        let tp = "# proto-file: \n";
        ws.update_textproto_file(tp_uri.clone(), tp.to_string());
        let idx = ws.build_index();
        let pt = ws.textproto_file(&tp_uri).unwrap();
        let off = (pt.source.find(": ").unwrap() + 2) as u32;
        let items = completion(&ws, &idx, pt, off);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| l == &"pkg/a.proto"),
            "labels: {:?}",
            labels
        );
    }

    #[test]
    fn completion_on_header_key_offers_keys() {
        let proto = r#"syntax = "proto3"; message M {}"#;
        let tp = "# proto-\n";
        let (ws, uri) = make_ws(proto, "m.proto", tp);
        let idx = ws.build_index();
        let pt = ws.textproto_file(&uri).unwrap();
        let off = offset_of(&ws, &uri, "proto-") + 6;
        let items = completion(&ws, &idx, pt, off);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"proto-file"));
        assert!(labels.contains(&"proto-message"));
    }

    #[test]
    fn hover_on_proto_message_header_resolves() {
        let proto = r#"syntax = "proto3"; package pkg; message Person { string name = 1; }"#;
        let tp = "# proto-file: pkg/p.proto\n# proto-message: pkg.Person\nname: \"A\"\n";
        let (ws, uri) = make_ws(proto, "pkg/p.proto", tp);
        let idx = ws.build_index();
        let pt = ws.textproto_file(&uri).unwrap();
        let off = offset_of(&ws, &uri, "pkg.Person") + 2;
        let h = hover(&ws, &idx, pt, off).expect("hover");
        assert!(h.markdown.contains("message .pkg.Person"), "md: {}", h.markdown);
    }
}

