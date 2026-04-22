//! Semantic-token highlighting, layered over the TextMate grammar.
//! Distinguishes message types, enums, fields, and RPC methods based on
//! the resolved symbol rather than the bare regex grammar.

use crate::ast;
use crate::resolve::{collect_type_use_sites, Resolution, SymbolKind, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SemanticTokenType {
    Type,
    Enum,
    EnumMember,
    Property,
    Function,
    Namespace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticToken {
    pub span: ByteSpan,
    pub ty: SemanticTokenType,
}

pub fn semantic_tokens(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
) -> Vec<SemanticToken> {
    let Some(pf) = ws.file(uri) else { return Vec::new() };
    let mut out = Vec::new();

    // Package path — each segment is a namespace.
    if let Some(pkg) = &pf.ast.package {
        for part in &pkg.name.parts {
            out.push(SemanticToken { span: part.span, ty: SemanticTokenType::Namespace });
        }
    }

    // Type references — colored by the resolved kind.
    for site in collect_type_use_sites(&pf.ast) {
        let Resolution::Found { symbol, .. } =
            index.resolve_type(uri, site.enclosing_scope.as_str(), &site.name)
        else { continue };
        let ty = match symbol.kind {
            SymbolKind::Message => SemanticTokenType::Type,
            SymbolKind::Enum => SemanticTokenType::Enum,
            _ => continue,
        };
        // Token the tail identifier only — the namespace segments are
        // already namespace-colored via a separate pass below.
        if let Some(last) = site.name.parts.last() {
            out.push(SemanticToken { span: last.span, ty });
        }
        for part in site.name.parts.iter().take(site.name.parts.len().saturating_sub(1)) {
            out.push(SemanticToken { span: part.span, ty: SemanticTokenType::Namespace });
        }
    }

    // Field names and enum-value names at their declaration sites.
    for item in &pf.ast.items {
        match item {
            ast::TopLevelItem::Message(m) => color_message_fields(m, &mut out),
            ast::TopLevelItem::Enum(e) => color_enum_values(e, &mut out),
            _ => {}
        }
    }

    out.sort_by_key(|t| t.span.start);
    out
}

fn color_enum_values(e: &ast::EnumDecl, out: &mut Vec<SemanticToken>) {
    for v in &e.values {
        out.push(SemanticToken { span: v.name.span, ty: SemanticTokenType::EnumMember });
    }
}

fn color_message_fields(m: &ast::Message, out: &mut Vec<SemanticToken>) {
    for f in &m.fields {
        out.push(SemanticToken { span: f.name.span, ty: SemanticTokenType::Property });
    }
    for o in &m.oneofs {
        for f in &o.fields {
            out.push(SemanticToken { span: f.name.span, ty: SemanticTokenType::Property });
        }
    }
    for nm in &m.nested_messages {
        color_message_fields(nm, out);
    }
    for ne in &m.nested_enums {
        for v in &ne.values {
            out.push(SemanticToken { span: v.name.span, ty: SemanticTokenType::EnumMember });
        }
    }
}
