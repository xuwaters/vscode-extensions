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
    let new_text = apply.map(|a| a.to_string()).unwrap_or_else(|| completion.label.to_string());

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
            // `apply` is snippet syntax — `${lhs} + ${rhs}` and friends.
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
