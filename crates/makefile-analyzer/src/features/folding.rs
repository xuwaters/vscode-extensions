//! Folding ranges for the editor: collapse rule bodies, define blocks,
//! and conditional branches.

use crate::ast;
use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldingRange {
    pub span: ByteSpan,
    pub kind: FoldKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FoldKind {
    Region,
    Comment,
}

pub fn folding_ranges(file: &ast::File) -> Vec<FoldingRange> {
    let mut out = Vec::new();
    collect(&file.items, &mut out);
    out
}

fn collect(items: &[ast::Item], out: &mut Vec<FoldingRange>) {
    for item in items {
        match item {
            ast::Item::Rule(r) if !r.recipe_lines.is_empty() => {
                out.push(FoldingRange { span: r.span, kind: FoldKind::Region });
            }
            ast::Item::Define(d) => {
                out.push(FoldingRange { span: d.span, kind: FoldKind::Region });
            }
            ast::Item::Conditional(c) => {
                out.push(FoldingRange { span: c.span, kind: FoldKind::Region });
                collect(&c.then_branch, out);
                collect(&c.else_branch, out);
            }
            _ => {}
        }
    }
}
