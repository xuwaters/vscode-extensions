//! Hover: render a Markdown summary of the symbol under the cursor.
//!
//! Two shapes:
//!  - cursor on a type reference → look up the referenced message/enum,
//!    show `<fqn>` + kind + doc comment.
//!  - cursor on a field name → show `label type name = number` plus doc.

use super::position::{field_at_name, type_use_at};
use crate::ast;
use crate::resolve::{Resolution, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    let pf = ws.file(uri)?;

    if let Some((field, parent_fqn)) = field_at_name(&pf.ast, offset) {
        return Some(render_field_hover(field, &parent_fqn, ws, index, uri));
    }

    let site = type_use_at(&pf.ast, offset)?;
    let markdown = match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.name) {
        Resolution::Found { symbol, .. } => {
            let mut s = String::new();
            s.push_str(&format!("```proto3\n{:?} .{}\n```", symbol.kind, symbol.fqn));
            if let Some(doc) = &symbol.doc {
                s.push_str("\n\n");
                s.push_str(doc);
            }
            s
        }
        Resolution::Unknown { candidates } => {
            format!(
                "Unresolved type `{}`\n\nTried: {}",
                site.name.to_display(),
                candidates
                    .iter()
                    .map(|c| format!("`{}`", c))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        }
    };
    Some(Hover { markdown, range: site.span })
}

fn render_field_hover(
    field: &ast::FieldDecl,
    parent_fqn: &str,
    _ws: &Workspace,
    _index: &WorkspaceIndex,
    _uri: &FileUri,
) -> Hover {
    let label = match field.label {
        ast::FieldLabel::Repeated => "repeated ",
        ast::FieldLabel::Optional => "optional ",
        ast::FieldLabel::Required => "required ",
        ast::FieldLabel::None => "",
    };
    let ty_txt = render_type_for_hover(&field.ty);
    let num = field.number.as_i64().map(|n| n.to_string()).unwrap_or_else(|| "?".into());
    let mut md = format!(
        "```proto3\n{}{} {} = {};\n```\n\n*field of* `.{}`",
        label, ty_txt, field.name.name, num, parent_fqn,
    );
    if let Some(doc) = ast::doc_comment_text(&field.leading_comments) {
        md.push_str("\n\n");
        md.push_str(&doc);
    }
    Hover { markdown: md, range: field.name.span }
}

fn render_type_for_hover(t: &ast::TypeRef) -> String {
    match t {
        ast::TypeRef::Scalar(s, _) => s.as_str().to_string(),
        ast::TypeRef::Named(q) => q.to_display(),
        ast::TypeRef::Map(m) => format!(
            "map<{}, {}>",
            render_type_for_hover(&m.key),
            render_type_for_hover(&m.value)
        ),
        ast::TypeRef::Missing(_) => "?".into(),
    }
}
