//! Folding ranges — one per `diesel::table!` body and one per
//! multi-line `allow_tables_to_appear_in_same_query!` invocation.

use crate::ast::SchemaFile;
use crate::spans::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldingRange {
    pub span: ByteSpan,
    pub kind: FoldKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldKind {
    Region,
}

pub fn folding_ranges(file: &SchemaFile, source: &str) -> Vec<FoldingRange> {
    let mut out = Vec::new();
    for t in &file.tables {
        if has_multiple_lines(t.body_span, source) {
            out.push(FoldingRange { span: t.body_span, kind: FoldKind::Region });
        }
    }
    for g in &file.allow_groups {
        if has_multiple_lines(g.span, source) {
            out.push(FoldingRange { span: g.span, kind: FoldKind::Region });
        }
    }
    out
}

fn has_multiple_lines(span: ByteSpan, source: &str) -> bool {
    let bytes = source.as_bytes();
    let s = (span.start as usize).min(bytes.len());
    let e = (span.end as usize).min(bytes.len());
    bytes[s..e].iter().filter(|&&b| b == b'\n').count() >= 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    #[test]
    fn folds_table_body() {
        let src = "diesel::table! {\n  users (id) {\n    id -> Text,\n  }\n}\n";
        let pf = parse(FileUri::new("t"), src.to_string());
        let ranges = folding_ranges(&pf.ast, &pf.source);
        assert!(!ranges.is_empty());
    }
}
