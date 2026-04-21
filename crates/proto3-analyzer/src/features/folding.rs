//! Folding ranges — collapse message/enum/service bodies, oneof/extend/extensions.

use crate::ast;
use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldingRange {
    pub span: ByteSpan,
    pub kind: FoldingKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FoldingKind {
    Region,
    Comment,
}

pub fn folding_ranges(file: &ast::File) -> Vec<FoldingRange> {
    let mut out = Vec::new();
    for item in &file.items {
        match item {
            ast::TopLevelItem::Message(m) => visit_message(m, &mut out),
            ast::TopLevelItem::Enum(e) => {
                out.push(FoldingRange { span: e.span, kind: FoldingKind::Region });
            }
            ast::TopLevelItem::Service(s) => {
                out.push(FoldingRange { span: s.span, kind: FoldingKind::Region });
                for m in &s.methods {
                    if !m.options.is_empty() {
                        out.push(FoldingRange { span: m.span, kind: FoldingKind::Region });
                    }
                }
            }
            ast::TopLevelItem::Extend(e) => {
                out.push(FoldingRange { span: e.span, kind: FoldingKind::Region });
            }
        }
    }
    out
}

fn visit_message(m: &ast::Message, out: &mut Vec<FoldingRange>) {
    out.push(FoldingRange { span: m.span, kind: FoldingKind::Region });
    for o in &m.oneofs {
        out.push(FoldingRange { span: o.span, kind: FoldingKind::Region });
    }
    for nm in &m.nested_messages {
        visit_message(nm, out);
    }
    for ne in &m.nested_enums {
        out.push(FoldingRange { span: ne.span, kind: FoldingKind::Region });
    }
}
