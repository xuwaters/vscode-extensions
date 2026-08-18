//! Completion — `typst_ide::autocomplete`.
//!
//! Upstream does the semantic work (181 items at a bare cursor, context-aware
//! across code, markup, math, params, imports, labels, and packages); this
//! module is the LSP shape around it.
//!
//! The one thing worth being careful about is the edit. Typst completions
//! frequently replace back *past* the cursor — `#` triggers a whole-expression
//! completion — so every item carries a `textEdit` spanning from the returned
//! replacement offset. Using `insertText` would duplicate the trigger character.

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
    CompletionTextEdit, Documentation, InsertTextFormat, MarkupContent, MarkupKind,
    TextEdit,
};
use typst_ide::{Completion, CompletionKind};

use crate::convert::range_to_lsp;
use crate::{Ports, Server};

/// The characters that make VSCode ask for completions without being told.
pub const TRIGGER_CHARACTERS: &[&str] = &["#", ".", "@", "/", "\"", ":", "$"];

impl<Q: Ports> Server<Q> {
    /// `textDocument/completion`.
    pub fn completion(&mut self, params: CompletionParams) -> Option<CompletionResponse> {
        let position = params.text_document_position;
        let (_, source, cursor) = self.locate(&position.text_document.uri, position.position)?;

        // `explicit` tells upstream how aggressive to be: a deliberate
        // ctrl+space wants everything, a typed `#` wants what fits.
        let explicit = params
            .context
            .as_ref()
            .map(|context| {
                context.trigger_kind == lsp_types::CompletionTriggerKind::INVOKED
            })
            .unwrap_or(true);

        let Some((from, completions)) = typst_ide::autocomplete(
            self.session().world(),
            self.last_good(),
            &source,
            cursor,
            explicit,
        ) else {
            // Upstream declined — a comment, say. Postfix items may still apply.
            let items = self.postfix_completions(&source, cursor);
            if items.is_empty() {
                return None;
            }
            return Some(CompletionResponse::List(lsp_types::CompletionList {
                is_incomplete: true,
                items,
            }));
        };

        let range = range_to_lsp(&source, from..cursor);
        let mut items: Vec<CompletionItem> = completions
            .iter()
            .enumerate()
            .map(|(index, completion)| to_item(completion, range, index))
            .collect();

        // Postfix entries come last and sort last: a real field on the value is
        // nearly always the better answer (P4-13).
        items.extend(self.postfix_completions(&source, cursor));

        Some(CompletionResponse::List(lsp_types::CompletionList {
            // Typst's completions depend on the cursor's syntactic context, so
            // the list is not a stable prefix filter — the client must ask
            // again as the user types.
            is_incomplete: true,
            items,
        }))
    }
}

fn to_item(completion: &Completion, range: lsp_types::Range, index: usize) -> CompletionItem {
    let apply = completion.apply.as_ref();
    let new_text = apply
        .map(|a| to_lsp_snippet(a))
        .unwrap_or_else(|| completion.label.to_string());

    CompletionItem {
        label: completion.label.to_string(),
        kind: Some(kind_of(&completion.kind)),
        detail: detail_of(completion),
        documentation: completion.detail.as_ref().map(|detail| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: detail.to_string(),
            })
        }),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text })),
        insert_text_format: Some(if apply.is_some() {
            InsertTextFormat::SNIPPET
        } else {
            InsertTextFormat::PLAIN_TEXT
        }),
        // Upstream returns items in a deliberate relevance order; preserve it
        // rather than letting the client re-sort alphabetically.
        sort_text: Some(format!("{index:06}")),
        ..CompletionItem::default()
    }
}

/// Translate typst's `apply` string into LSP snippet syntax.
///
/// The two look alike and are not the same. Typst writes a hole as `${hint}`,
/// where the hint is prose for the reader and often empty — `page(${})`. LSP
/// requires every tab stop to be numbered: `${1:hint}` or a bare `$1`. An
/// unnumbered `${}` is not a tab stop at all, so VSCode inserts it as text,
/// which is exactly what the user sees as `#page(${})`.
///
/// So: number the holes left to right, drop the braces when the hint is empty,
/// and escape every `$` and `\` that was meant literally — typst's math
/// snippets (`$${x}$`) are full of them.
fn to_lsp_snippet(apply: &str) -> String {
    let mut out = String::with_capacity(apply.len());
    let mut rest = apply;
    let mut stop = 0;

    while let Some(offset) = rest.find('$') {
        escape_literal(&rest[..offset], &mut out);
        rest = &rest[offset..];

        match rest.strip_prefix("${").and_then(|inner| {
            inner.find('}').map(|end| (&inner[..end], &inner[end + 1..]))
        }) {
            Some((hint, tail)) => {
                stop += 1;
                // Typst occasionally pre-numbers a hole (`${2:2}` in the math
                // sub/superscript snippets). Our own numbering is authoritative
                // — strip theirs rather than emitting two competing schemes.
                let hint = strip_stop_number(hint);
                if hint.is_empty() {
                    out.push_str(&format!("${stop}"));
                } else {
                    out.push_str(&format!("${{{stop}:"));
                    escape_placeholder(hint, &mut out);
                    out.push('}');
                }
                rest = tail;
            }
            // A lone `$` (math delimiter) or an unterminated `${`: literal.
            None => {
                out.push_str("\\$");
                rest = &rest['$'.len_utf8()..];
            }
        }
    }

    escape_literal(rest, &mut out);
    out
}

/// `2:x` → `x`. Only a leading run of digits followed by `:` counts.
fn strip_stop_number(hint: &str) -> &str {
    let digits = hint.len() - hint.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    match (digits > 0).then(|| hint[digits..].strip_prefix(':')).flatten() {
        Some(rest) => rest,
        None => hint,
    }
}

/// Outside a placeholder only `$` and `\` are special; a bare `}` is literal.
fn escape_literal(text: &str, out: &mut String) {
    for character in text.chars() {
        if matches!(character, '$' | '\\') {
            out.push('\\');
        }
        out.push(character);
    }
}

/// Inside `${n:…}` a `}` would close the placeholder early, so it escapes too.
fn escape_placeholder(text: &str, out: &mut String) {
    for character in text.chars() {
        if matches!(character, '$' | '\\' | '}') {
            out.push('\\');
        }
        out.push(character);
    }
}

fn kind_of(kind: &CompletionKind) -> CompletionItemKind {
    match kind {
        CompletionKind::Syntax => CompletionItemKind::SNIPPET,
        CompletionKind::Func => CompletionItemKind::FUNCTION,
        CompletionKind::Type => CompletionItemKind::CLASS,
        CompletionKind::Param => CompletionItemKind::FIELD,
        CompletionKind::Constant => CompletionItemKind::CONSTANT,
        CompletionKind::Path => CompletionItemKind::FILE,
        CompletionKind::Package => CompletionItemKind::MODULE,
        CompletionKind::Label => CompletionItemKind::REFERENCE,
        CompletionKind::Font => CompletionItemKind::TEXT,
        CompletionKind::Symbol(_) => CompletionItemKind::TEXT,
    }
}

/// The glyph itself is the most useful thing to show beside a `sym.*` entry.
fn detail_of(completion: &Completion) -> Option<String> {
    match &completion.kind {
        CompletionKind::Symbol(glyph) => Some(glyph.to_string()),
        _ => completion.detail.as_ref().map(|detail| first_line(detail)),
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::to_lsp_snippet;

    /// The reported bug: `#page(${})` showed up verbatim in the document.
    #[test]
    fn an_empty_hole_becomes_a_bare_tab_stop() {
        assert_eq!(to_lsp_snippet("page(${})"), "page($1)");
        assert_eq!(to_lsp_snippet("set ${}"), "set $1");
    }

    #[test]
    fn a_hinted_hole_keeps_its_hint_and_gains_a_number() {
        assert_eq!(to_lsp_snippet("*${strong}*"), "*${1:strong}*");
        assert_eq!(
            to_lsp_snippet("let ${name} = ${value}"),
            "let ${1:name} = ${2:value}"
        );
    }

    #[test]
    fn holes_number_left_to_right_across_a_multiline_snippet() {
        assert_eq!(
            to_lsp_snippet("for ${value} in ${(1, 2, 3)} {\n\t${}\n}"),
            "for ${1:value} in ${2:(1, 2, 3)} {\n\t$3\n}"
        );
    }

    /// Math snippets carry `$` as a delimiter, not as a tab stop.
    #[test]
    fn a_lone_dollar_is_escaped() {
        assert_eq!(to_lsp_snippet("$${x}$"), "\\$${1:x}\\$");
        assert_eq!(to_lsp_snippet("$ ${sum_x^2} $"), "\\$ ${1:sum_x^2} \\$");
    }

    /// Typst pre-numbers the sub/superscript holes; ours wins.
    #[test]
    fn a_pre_numbered_hint_is_renumbered() {
        assert_eq!(to_lsp_snippet("${x}_${2:2}"), "${1:x}_${2:2}");
        assert_eq!(to_lsp_snippet("${a}/${3:b}"), "${1:a}/${2:b}");
    }

    #[test]
    fn text_without_holes_passes_through() {
        assert_eq!(to_lsp_snippet("emph"), "emph");
        assert_eq!(to_lsp_snippet("\"${text}\": ${}"), "\"${1:text}\": $2");
    }

    #[test]
    fn a_backslash_survives_as_a_backslash() {
        assert_eq!(to_lsp_snippet("\\\n${}"), "\\\\\n$1");
    }

    #[test]
    fn an_unterminated_hole_is_literal_text() {
        assert_eq!(to_lsp_snippet("page(${"), "page(\\${");
    }
}
