//! Postfix completions — P4-13.
//!
//! Typing `x.rect` and getting `rect(x)` is a tinymist affordance worth having,
//! and it needs nothing upstream does not already give us: it is a pure syntax
//! transformation over what the cursor is sitting on.
//!
//! The rule for when to offer them matters as much as the transformation. After
//! `value.`, typst's own completion offers *fields and methods of that value* —
//! which is the right answer and must come first. Postfix entries are appended
//! after it, sorted below, so they are there when nothing else fits and out of
//! the way when something does.

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, MarkupContent,
    MarkupKind, Range, TextEdit,
};
use typst::syntax::{LinkedNode, Side, Source, SyntaxKind, ast};

use crate::convert::range_to_lsp;
use crate::{Ports, Server};

/// The functions offered as postfix completions.
///
/// Deliberately a short, curated list of things that genuinely read better
/// postfix — wrapping and formatting a value you have already written. Offering
/// the whole standard library here would bury the field completions that are
/// usually what you actually wanted.
const POSTFIX_FUNCTIONS: &[(&str, &str)] = &[
    ("rect", "Wrap in a rectangle"),
    ("box", "Wrap in an inline box"),
    ("block", "Wrap in a block"),
    ("circle", "Wrap in a circle"),
    ("emph", "Emphasize"),
    ("strong", "Embolden"),
    ("underline", "Underline"),
    ("text", "Apply text styling"),
    ("align", "Align"),
    ("pad", "Add padding"),
    ("repr", "Show the value's representation"),
    ("str", "Convert to a string"),
];

impl<Q: Ports> Server<Q> {
    /// Postfix items for a cursor sitting just after `expression.`.
    ///
    /// Returns nothing anywhere else, which is most places.
    pub(crate) fn postfix_completions(
        &self,
        source: &Source,
        cursor: usize,
    ) -> Vec<CompletionItem> {
        let Some((receiver, replace)) = postfix_context(source, cursor) else {
            return Vec::new();
        };
        let Some(text) = source.text().get(receiver.clone()) else {
            return Vec::new();
        };

        let range = range_to_lsp(source, replace);

        POSTFIX_FUNCTIONS
            .iter()
            .enumerate()
            .map(|(index, (name, description))| {
                item(name, description, text, range, index)
            })
            .collect()
    }

}

/// The receiver range and the range to replace, when the cursor is after a dot.
///
/// `#value.` → receiver is `value`, replacement covers `value.` so the item can
/// rewrite the whole thing.
///
/// The two shapes here are the ones upstream's own `complete_field_accesses`
/// distinguishes, and they are not obvious. A dot in *markup* is a `Text` node
/// holding `"."` — `#name.` is a value followed by a full stop, which is what
/// you usually want and is why typst parses it that way. Only once a field name
/// follows does it become a real `FieldAccess`.
fn postfix_context(
    source: &Source,
    cursor: usize,
) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let root = LinkedNode::new(source.root());
    let leaf = root.leaf_at(cursor, Side::Before)?;

    // `#value.` — a textual dot right after an expression.
    let textual_dot = matches!(leaf.kind(), SyntaxKind::Text | SyntaxKind::MathText)
        && leaf.leaf_text() == ".";
    if textual_dot || leaf.kind() == SyntaxKind::Dot {
        let previous = leaf.prev_sibling()?;
        // `[#x .]` — trivia between the value and the dot means the dot is
        // punctuation, not an access.
        if previous.range().end != leaf.range().start {
            return None;
        }
        if !previous.is::<ast::Expr>() {
            return None;
        }
        return Some((previous.range(), previous.range().start..leaf.range().end));
    }

    // `#value.re` — a started field access.
    if leaf.kind() == SyntaxKind::Ident || leaf.kind() == SyntaxKind::MathIdent {
        let dot = leaf.prev_sibling().filter(|node| node.kind() == SyntaxKind::Dot)?;
        let previous = dot.prev_sibling()?;
        if !previous.is::<ast::Expr>() {
            return None;
        }
        return Some((previous.range(), previous.range().start..leaf.range().end));
    }

    None
}

fn item(
    name: &str,
    description: &str,
    receiver: &str,
    range: Range,
    index: usize,
) -> CompletionItem {
    CompletionItem {
        label: name.to_string(),
        kind: Some(CompletionItemKind::FUNCTION),
        detail: Some(format!("{name}({receiver})")),
        documentation: Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("{description}.\n\n```typst\n{name}({receiver})\n```"),
        })),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
            range,
            new_text: format!("{name}({receiver})"),
        })),
        // `z` prefix: sorted after everything upstream returns, because a real
        // field on the value is nearly always the better answer.
        sort_text: Some(format!("z{index:03}")),
        ..CompletionItem::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(text: &str) -> Option<(String, String)> {
        let source = Source::detached(text);
        let cursor = text.len();
        let (receiver, replace) = postfix_context(&source, cursor)?;
        Some((text[receiver].to_string(), text[replace].to_string()))
    }

    #[test]
    fn a_cursor_after_a_dot_offers_the_receiver() {
        assert_eq!(
            context("#value."),
            Some(("value".to_string(), "value.".to_string()))
        );
    }

    #[test]
    fn a_partially_typed_name_still_resolves_the_receiver() {
        assert_eq!(
            context("#value.re"),
            Some(("value".to_string(), "value.re".to_string()))
        );
    }

    #[test]
    fn plain_markup_offers_nothing() {
        assert_eq!(context("Just some text."), None);
        assert_eq!(context("= Heading"), None);
    }

    #[test]
    fn a_bare_identifier_offers_nothing() {
        assert_eq!(context("#value"), None);
    }

    #[test]
    fn the_items_rewrite_the_whole_expression() {
        let source = Source::detached("#value.");
        let (_, replace) = postfix_context(&source, source.text().len()).unwrap();
        let range = range_to_lsp(&source, replace);
        let entry = item("rect", "Wrap in a rectangle", "value", range, 0);

        let Some(CompletionTextEdit::Edit(edit)) = entry.text_edit else {
            panic!("expected a text edit");
        };
        assert_eq!(edit.new_text, "rect(value)");
        // The edit must cover `value.`, not just the cursor.
        assert_eq!(edit.range.start.character, 1);
        assert_eq!(edit.range.end.character, 7);
    }

    #[test]
    fn every_offered_name_sorts_below_upstreams_items() {
        let source = Source::detached("#value.");
        let (_, replace) = postfix_context(&source, source.text().len()).unwrap();
        let range = range_to_lsp(&source, replace);

        for (index, (name, description)) in POSTFIX_FUNCTIONS.iter().enumerate() {
            let entry = item(name, description, "value", range, index);
            let sort = entry.sort_text.unwrap();
            assert!(sort.starts_with('z'), "{name} sorts as {sort}");
        }
    }
}
