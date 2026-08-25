//! Folding ranges: every container, plus block comments.

use crate::ast::{Ast, Comment, CommentKind, Value, ValueKind};
use crate::spans::ByteSpan;
use crate::workspace::ParsedFile;

const MAX_RANGES: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldKind {
    Region,
    Comment,
}

#[derive(Debug, Clone, Copy)]
pub struct FoldRange {
    pub span: ByteSpan,
    pub kind: FoldKind,
}

pub fn folding_ranges(pf: &ParsedFile) -> Vec<FoldRange> {
    let mut out = Vec::new();
    match &pf.ast {
        Ast::Single(root) => {
            for comment in &root.leading {
                push_comment(&mut out, comment);
            }
            if let Some(value) = &root.value {
                walk(value, &mut out);
            }
            for comment in &root.trailing {
                push_comment(&mut out, comment);
            }
        }
        // JSON Lines records are single lines by construction — nothing
        // multi-line to fold.
        Ast::Lines(_) => {}
    }
    out
}

fn push_comment(out: &mut Vec<FoldRange>, comment: &Comment) {
    if comment.kind == CommentKind::Block && out.len() < MAX_RANGES {
        out.push(FoldRange { span: comment.span, kind: FoldKind::Comment });
    }
}

fn walk(value: &Value, out: &mut Vec<FoldRange>) {
    if out.len() >= MAX_RANGES {
        return;
    }
    match &value.kind {
        ValueKind::Object(object) => {
            out.push(FoldRange { span: value.span, kind: FoldKind::Region });
            for member in &object.members {
                for comment in &member.leading {
                    push_comment(out, comment);
                }
                walk(&member.value, out);
            }
            for comment in &object.dangling {
                push_comment(out, comment);
            }
        }
        ValueKind::Array(array) => {
            out.push(FoldRange { span: value.span, kind: FoldKind::Region });
            for element in &array.elements {
                for comment in &element.leading {
                    push_comment(out, comment);
                }
                walk(&element.value, out);
            }
            for comment in &array.dangling {
                push_comment(out, comment);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flavor::Flavor;
    use crate::workspace::{parse, FileUri};

    #[test]
    fn containers_and_block_comments_fold() {
        let src = "/* head\ncomment */\n{\n  \"a\": [\n    1\n  ]\n}\n";
        let pf = parse(FileUri::new("t"), src.to_string(), Flavor::Jsonc);
        let ranges = folding_ranges(&pf);
        assert_eq!(ranges.len(), 3);
        assert!(ranges.iter().any(|r| r.kind == FoldKind::Comment));
        assert_eq!(ranges.iter().filter(|r| r.kind == FoldKind::Region).count(), 2);
    }

    #[test]
    fn jsonl_has_no_folds() {
        let pf = parse(FileUri::new("t"), "{\"a\": 1}\n".to_string(), Flavor::Jsonl);
        assert!(folding_ranges(&pf).is_empty());
    }
}
