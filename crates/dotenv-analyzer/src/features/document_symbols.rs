//! Flat document-symbol list — one entry per assignment.

use crate::ast::{Entry, File};
use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    pub detail: String,
    pub kind: SymbolKind,
    pub range: ByteSpan,
    pub selection_range: ByteSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Variable,
}

pub fn document_symbols(file: &File, source: &str) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    for entry in &file.entries {
        let Entry::Assignment(a) = entry else { continue };
        if a.name.name.is_empty() {
            continue;
        }
        let detail = preview(&source[a.value_span.start as usize..a.value_span.end as usize]);
        out.push(DocumentSymbol {
            name: a.name.name.clone(),
            detail,
            kind: SymbolKind::Variable,
            range: a.span,
            selection_range: a.name.span,
        });
    }
    out
}

fn preview(s: &str) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 60;
    if collapsed.chars().count() <= MAX {
        collapsed
    } else {
        let mut out: String = collapsed.chars().take(MAX).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    #[test]
    fn lists_each_assignment() {
        let src = "FOO=1\n# comment\nBAR=hello\n";
        let pf = parse(FileUri::new("t"), src.to_string());
        let syms = document_symbols(&pf.ast, &pf.source);
        assert_eq!(syms.len(), 2);
        assert_eq!(syms[0].name, "FOO");
        assert_eq!(syms[1].name, "BAR");
    }
}
