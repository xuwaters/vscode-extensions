//! Smart select: from the token under the cursor out through each entry
//! and container to the whole value.

use crate::ast::EntryKey;
use crate::features::locate::steps_to;
use crate::spans::{ByteSpan, LineCol};
use crate::workspace::ParsedFile;

/// Innermost first; every span contains the one before it.
pub fn selection_ranges(pf: &ParsedFile, pos: LineCol) -> Vec<ByteSpan> {
    let offset = pf.spans.line_col_to_offset(&pf.source, pos);
    let steps = steps_to(&pf.ast, offset, false);
    let mut chain: Vec<ByteSpan> = Vec::new();
    let mut add = |span: ByteSpan| {
        let grows = chain.last().is_none_or(|last: &ByteSpan| {
            span != *last && span.start <= last.start && span.end >= last.end
        });
        if grows {
            chain.push(span);
        }
    };
    for step in steps.iter().rev() {
        if let Some((entry, _)) = step.entry {
            match &entry.key {
                Some(EntryKey::Field(id)) if id.span.contains(offset) => add(id.span),
                Some(EntryKey::Map(k)) if k.span.contains(offset) => add(k.span),
                _ => {}
            }
        }
        if let Some(name) = step.value.name().filter(|n| n.span.contains(offset)) {
            add(name.span);
        }
        add(step.value.span);
        if let Some((entry, _)) = step.entry {
            add(entry.span());
        }
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{parse, FileUri};
    use pretty_assertions::assert_eq;

    #[test]
    fn expands_outward() {
        let src = "Config(size: (800, 600), name: \"x\")";
        let pf = parse(FileUri::new("t"), src.to_string());
        let offset = src.find("600").unwrap() as u32 + 1;
        let chain = selection_ranges(&pf, pf.spans.offset_to_line_col(src, offset));
        let texts: Vec<&str> = chain.iter().map(|s| &src[s.start as usize..s.end as usize]).collect();
        assert_eq!(texts, vec!["600", "(800, 600)", "size: (800, 600)", src]);
    }

    #[test]
    fn starts_from_a_field_name() {
        let src = "(size: 1)";
        let pf = parse(FileUri::new("t"), src.to_string());
        let chain = selection_ranges(&pf, LineCol { line: 0, col: 2 });
        let texts: Vec<&str> = chain.iter().map(|s| &src[s.start as usize..s.end as usize]).collect();
        assert_eq!(texts, vec!["size", "size: 1", src]);
    }
}
