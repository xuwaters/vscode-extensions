//! Hierarchical document symbols — each `diesel::table!` becomes a Class
//! symbol whose children are its columns (Fields). Joinable and
//! allow-group invocations are flattened as top-level Method/Constant
//! symbols so users can jump to them from the outline view.

use crate::ast::SchemaFile;
use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    pub detail: String,
    pub kind: SymbolKind,
    pub range: ByteSpan,
    pub selection_range: ByteSpan,
    pub children: Vec<DocumentSymbol>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Table,
    Column,
    Joinable,
    AllowGroup,
}

pub fn document_symbols(file: &SchemaFile) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    for t in &file.tables {
        let mut children = Vec::with_capacity(t.columns.len());
        for c in &t.columns {
            children.push(DocumentSymbol {
                name: c.name.name.clone(),
                detail: c.sql_type.display.clone(),
                kind: SymbolKind::Column,
                range: ByteSpan::new(c.name.span.start, c.sql_type.span.end),
                selection_range: c.name.span,
                children: Vec::new(),
            });
        }
        let qualified = match &t.schema {
            Some(s) => format!("{}.{}", s.name, t.name.name),
            None => t.name.name.clone(),
        };
        let pk = if t.primary_keys.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> =
                t.primary_keys.iter().map(|p| p.name.as_str()).collect();
            format!("pk: {}", names.join(", "))
        };
        out.push(DocumentSymbol {
            name: qualified,
            detail: pk,
            kind: SymbolKind::Table,
            range: t.span,
            selection_range: t.name.span,
            children,
        });
    }
    for j in &file.joinables {
        out.push(DocumentSymbol {
            name: format!("{} → {}", j.child.name, j.parent.name),
            detail: format!("via `{}`", j.fk_column.name),
            kind: SymbolKind::Joinable,
            range: j.span,
            selection_range: j.child.span,
            children: Vec::new(),
        });
    }
    for (i, g) in file.allow_groups.iter().enumerate() {
        out.push(DocumentSymbol {
            name: format!("allow_tables_to_appear_in_same_query #{}", i + 1),
            detail: format!("{} tables", g.tables.len()),
            kind: SymbolKind::AllowGroup,
            range: g.span,
            selection_range: g.span,
            children: Vec::new(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    #[test]
    fn outlines_tables_and_columns() {
        let src = r#"
diesel::table! {
    users (id) {
        id -> Text,
        email -> Citext,
    }
}
"#;
        let pf = parse(FileUri::new("t"), src.to_string());
        let syms = document_symbols(&pf.ast);
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "users");
        assert_eq!(syms[0].children.len(), 2);
        assert_eq!(syms[0].children[0].name, "id");
        assert_eq!(syms[0].children[0].detail, "Text");
        assert_eq!(syms[0].children[1].detail, "Citext");
    }
}
