//! Hover: Markdown summary of the symbol under the cursor.
//!
//! Two shapes:
//!  - cursor on a type reference → look up the referenced declaration and
//!    show its kind, FQN, and doc comment.
//!  - cursor on a declared name (struct/interface/field/method/…) → show that
//!    declaration's own signature and doc.

use super::position::{span_contains, type_use_at};
use crate::resolve::{Resolution, Symbol, SymbolKind, WorkspaceIndex};
use crate::spans::ByteSpan;
use crate::vfs::{FileUri, Workspace};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Hover {
    pub markdown: String,
    pub range: ByteSpan,
}

pub fn hover(ws: &Workspace, index: &WorkspaceIndex, uri: &FileUri, offset: u32) -> Option<Hover> {
    let state = ws.file(uri)?;
    let file = &state.analysis.file;

    // 1. Cursor on a declared name in this file.
    if let Some(fs) = index.file_symbols(uri) {
        let mut best: Option<&Symbol> = None;
        for sym in fs.entries.values() {
            if span_contains(sym.name_span, offset)
                && best.is_none_or(|b| sym.name_span.len() < b.name_span.len())
            {
                best = Some(sym);
            }
        }
        if let Some(sym) = best {
            return Some(Hover { markdown: render_symbol(sym, true), range: sym.name_span });
        }
    }

    // 2. Cursor on a type reference.
    let site = type_use_at(file, offset)?;
    let markdown = match index.resolve_type(uri, site.enclosing_scope.as_str(), &site.path) {
        Resolution::Found { symbol, visibility_ok } => render_symbol(&symbol, visibility_ok),
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
    let mut md = format!("```mojom\n{} {}{}{}\n```", kind, s.fqn, sep, detail);
    if !visibility_ok {
        md.push_str(
            "\n\n> ⚠ Defined in a file that is not imported here. Add an `import \"…\";`.",
        );
    }
    if let Some(doc) = &s.doc {
        md.push_str("\n\n");
        md.push_str(doc);
    }
    md
}

fn kind_word(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Module => "module",
        SymbolKind::Struct => "struct",
        SymbolKind::Union => "union",
        SymbolKind::Interface => "interface",
        SymbolKind::Enum => "enum",
        SymbolKind::EnumValue => "enum value",
        SymbolKind::Const => "const",
        SymbolKind::Field => "field",
        SymbolKind::Method => "method",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::WorkspaceIndex;
    use crate::spans::{LineCol, SpanTable};
    use crate::vfs::Workspace;

    #[test]
    fn hovers_type_reference() {
        let mut ws = Workspace::new();
        let src = "// A foo.\nstruct Foo { int32 id; };\nstruct Bar { Foo f; };";
        ws.update("file:///a.mojom", src.into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.mojom".into());
        let offset = (src.find("Foo f").unwrap() + 1) as u32;
        let h = hover(&ws, &idx, &uri, offset).expect("hover");
        assert!(h.markdown.contains("struct Foo"), "{}", h.markdown);
        assert!(h.markdown.contains("A foo."), "{}", h.markdown);
    }

    #[test]
    fn hovers_declared_field() {
        let mut ws = Workspace::new();
        let src = "struct Foo { int32 my_id; };";
        ws.update("file:///a.mojom", src.into());
        let idx = WorkspaceIndex::build(&ws);
        let uri = FileUri("file:///a.mojom".into());
        let table = SpanTable::new(src);
        let offset = (src.find("my_id").unwrap() + 1) as u32;
        let lc = table.offset_to_line_col(src, offset);
        let back = table.line_col_to_offset(src, LineCol { line: lc.line, col: lc.col });
        let h = hover(&ws, &idx, &uri, back).expect("hover");
        assert!(h.markdown.contains("field"), "{}", h.markdown);
        assert!(h.markdown.contains("int32"), "{}", h.markdown);
    }
}
