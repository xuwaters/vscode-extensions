//! Hover: Markdown summary of the symbol under the cursor.
//!
//! Two shapes:
//!  - cursor on a type reference → look up the referenced declaration and
//!    show its kind, FQN, and doc comment.
//!  - cursor on a field name → show `name @N :Type` plus doc.

use super::position::{field_at_name, type_use_at};
use crate::ast::*;
use crate::resolve::{type_label, Resolution, Symbol, SymbolKind, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Hover {
    pub markdown: String,
    pub range: ByteSpan,
}

pub fn hover(
    ws: &Workspace,
    index: &WorkspaceIndex,
    uri: &FileUri,
    offset: u32,
) -> Option<Hover> {
    let state = ws.file(uri)?;
    let file = &state.analysis.file;

    if let Some((field, parent_fqn)) = field_at_name(file, offset) {
        return Some(render_field_hover(field, &parent_fqn, &state.source));
    }

    let site = type_use_at(file, offset)?;
    let markdown = match index.resolve_type_with_import(
        uri,
        site.enclosing_scope.as_str(),
        site.import_path.as_deref(),
        &site.path,
    ) {
        Resolution::Found { symbol, visibility_ok } => render_symbol(&symbol, visibility_ok),
        Resolution::FileAlias { file, .. } => {
            format!("```capnp\nimport \"…\"\n```\n\nFile alias → `{}`", file.as_str())
        }
        Resolution::Unknown { candidates } => {
            let tried = candidates.iter().map(|c| format!("`{}`", c)).collect::<Vec<_>>().join(", ");
            let shown = site.path.iter().map(|i| i.text.as_str()).collect::<Vec<_>>().join(".");
            format!("Unresolved type `{}`\n\nTried: {}", shown, tried)
        }
    };
    Some(Hover { markdown, range: site.span })
}

fn render_symbol(s: &Symbol, visibility_ok: bool) -> String {
    let kind = kind_word(s.kind);
    let detail = s.detail.as_deref().unwrap_or("");
    let sep = if detail.is_empty() { "" } else { " " };
    let mut md = format!("```capnp\n{} {}{}{}\n```", kind, s.fqn, sep, detail);
    if !visibility_ok {
        md.push_str(
            "\n\n> ⚠ Defined in a file that is not imported here. Add a `using … = import \"…\";`.",
        );
    }
    if let Some(doc) = &s.doc {
        md.push_str("\n\n");
        md.push_str(doc);
    }
    md
}

fn render_field_hover(field: &Field, parent_fqn: &str, source: &str) -> Hover {
    let ordinal = field
        .ordinal
        .as_ref()
        .map(|o| format!("@{} ", o.value))
        .unwrap_or_default();
    let ty = match &field.body {
        FieldBody::Slot { ty, .. } => format!(":{}", type_label(ty)),
        FieldBody::NamedUnion(_) => ":union".into(),
        FieldBody::NamedGroup(_) => ":group".into(),
    };
    let mut md = format!(
        "```capnp\n{} {}{}\n```\n\n*field of* `{}`",
        field.name.text, ordinal, ty, parent_fqn,
    );
    if let Some(doc) = crate::resolve::extract_doc_comment(source, field.span.start) {
        md.push_str("\n\n");
        md.push_str(&doc);
    }
    Hover { markdown: md, range: field.name.span }
}

fn kind_word(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::EnumMember => "enumerant",
        SymbolKind::Interface => "interface",
        SymbolKind::Method => "method",
        SymbolKind::Field => "field",
        SymbolKind::Union => "union",
        SymbolKind::Group => "group",
        SymbolKind::Constant => "const",
        SymbolKind::Annotation => "annotation",
        SymbolKind::Alias => "using",
    }
}
