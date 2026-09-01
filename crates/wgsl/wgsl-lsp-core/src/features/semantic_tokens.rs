//! `textDocument/semanticTokens/full` and its delta.
//!
//! TextMate grammars colour a word by its shape. This colours it by what it
//! *is*: `Light` in `uniform Light lights[4]` paints as a struct because the
//! symbol index says a struct was declared under that name, and `dot` paints
//! as a library function because the builtin table says so.
//!
//! Delta is not a nicety. A shader produces a few thousand tokens and a
//! keystroke changes a handful, so `full` on every edit would be the most
//! expensive thing the server does.

use analyzer_core::spans::ByteSpan;
use lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensDelta, SemanticTokensDeltaParams, SemanticTokensEdit,
    SemanticTokensFullDeltaResult, SemanticTokensLegend, SemanticTokensParams,
    SemanticTokensResult,
};
use wgsl_syntax::lexer::TokenKind;
use wgsl_syntax::{Reference, SymbolKind, builtins};

use crate::Server;
use crate::state::Document;

/// Token types, in the order the legend declares them.
const TYPES: [SemanticTokenType; 13] = [
    SemanticTokenType::KEYWORD,
    SemanticTokenType::TYPE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::MACRO,
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::OPERATOR,
    SemanticTokenType::DECORATOR,
];

mod ty {
    pub const KEYWORD: u32 = 0;
    pub const TYPE: u32 = 1;
    pub const STRUCT: u32 = 2;
    pub const FUNCTION: u32 = 3;
    pub const VARIABLE: u32 = 4;
    pub const PARAMETER: u32 = 5;
    pub const PROPERTY: u32 = 6;
    pub const MACRO: u32 = 7;
    pub const COMMENT: u32 = 8;
    pub const STRING: u32 = 9;
    pub const NUMBER: u32 = 10;
    pub const DECORATOR: u32 = 12;
}

/// Modifiers, as bit positions in the order the legend declares them.
mod modifier {
    pub const DECLARATION: u32 = 1 << 0;
    pub const READONLY: u32 = 1 << 1;
    pub const DEFAULT_LIBRARY: u32 = 1 << 2;
}

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TYPES.to_vec(),
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::READONLY,
            SemanticTokenModifier::DEFAULT_LIBRARY,
        ],
    }
}

impl Server {
    pub fn semantic_tokens_full(
        &mut self,
        params: SemanticTokensParams,
    ) -> Option<SemanticTokensResult> {
        let uri = params.text_document.uri;
        let document = self.document(&uri)?;
        if !self.settings().for_language(document.language).semantic_tokens {
            return None;
        }
        let tokens = tokens(document);

        let document = self.document_mut(&uri)?;
        let result_id = document.tokens.store(tokens.clone());
        Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: Some(result_id),
            data: tokens,
        }))
    }

    pub fn semantic_tokens_delta(
        &mut self,
        params: SemanticTokensDeltaParams,
    ) -> Option<SemanticTokensFullDeltaResult> {
        let uri = params.text_document.uri;
        let document = self.document(&uri)?;
        if !self.settings().for_language(document.language).semantic_tokens {
            return None;
        }
        let tokens = tokens(document);
        // The client quoted an id we no longer hold — it has been restarted,
        // or two deltas raced. Sending a full array resynchronises it.
        let known = document.tokens.matches(&params.previous_result_id);
        let previous = document.tokens.tokens.clone();

        let document = self.document_mut(&uri)?;
        let result_id = document.tokens.store(tokens.clone());

        if !known {
            return Some(SemanticTokensFullDeltaResult::Tokens(SemanticTokens {
                result_id: Some(result_id),
                data: tokens,
            }));
        }

        Some(SemanticTokensFullDeltaResult::TokensDelta(SemanticTokensDelta {
            result_id: Some(result_id),
            edits: diff(&previous, &tokens),
        }))
    }
}

/// One delta-encoded token array for a whole document.
pub fn tokens(document: &Document) -> Vec<SemanticToken> {
    let parsed = document.parsed();
    let mut absolute: Vec<(u32, u32, u32, u32, u32)> = Vec::new();
    // References are in source order, as are tokens, so one cursor over each
    // is enough — no lookup table.
    let mut next_reference = 0usize;

    for token in &parsed.tokens {
        while next_reference < parsed.references.len()
            && parsed.references[next_reference].span.end < token.span.start
        {
            next_reference += 1;
        }
        let reference = parsed
            .references
            .get(next_reference)
            .filter(|reference| reference.span == token.span);

        let Some((token_type, modifiers)) = classify(document, token.kind, token.span, reference)
        else {
            continue;
        };

        // LSP forbids a token from spanning lines, and a block comment does.
        for (line, start, length) in split_lines(document, token.span) {
            absolute.push((line, start, length, token_type, modifiers));
        }
    }

    let mut data = Vec::with_capacity(absolute.len());
    let (mut previous_line, mut previous_start) = (0, 0);
    for (line, start, length, token_type, modifiers) in absolute {
        let delta_line = line - previous_line;
        data.push(SemanticToken {
            delta_line,
            delta_start: if delta_line == 0 { start - previous_start } else { start },
            length,
            token_type,
            token_modifiers_bitset: modifiers,
        });
        previous_line = line;
        previous_start = start;
    }
    data
}

/// What to paint a token as, or `None` to leave it to the TextMate grammar.
fn classify(
    document: &Document,
    kind: TokenKind,
    span: ByteSpan,
    reference: Option<&Reference>,
) -> Option<(u32, u32)> {
    let language = document.language;
    match kind {
        TokenKind::Comment => Some((ty::COMMENT, 0)),
        TokenKind::Str => Some((ty::STRING, 0)),
        TokenKind::Number => Some((ty::NUMBER, 0)),
        TokenKind::Keyword => Some((ty::KEYWORD, 0)),
        TokenKind::Type => Some((ty::TYPE, modifier::DEFAULT_LIBRARY)),
        TokenKind::Preprocessor => Some((ty::MACRO, 0)),
        TokenKind::Attribute => Some((ty::DECORATOR, 0)),
        // Operators are left to the grammar: they would triple the token count
        // for colour that no common theme actually varies.
        TokenKind::Punct | TokenKind::Unknown => None,
        TokenKind::Ident => Some(identifier(document, span, reference, language)),
    }
}

fn identifier(
    document: &Document,
    span: ByteSpan,
    reference: Option<&Reference>,
    language: wgsl_syntax::Language,
) -> (u32, u32) {
    let parsed = document.parsed();
    let name = document.slice(span);

    if let Some(reference) = reference {
        if reference.is_declaration {
            if let Some(index) = parsed.symbol_declared_at(span.start) {
                let (token_type, modifiers) = from_symbol(parsed.symbols[index].kind);
                return (token_type, modifiers | modifier::DECLARATION);
            }
        }
        // A member is a property of whatever it belongs to; resolving it needs
        // a type, which semantic tokens are not worth a naga round trip for.
        if reference.is_member {
            return (ty::PROPERTY, 0);
        }
    }

    if let Some(index) = parsed.resolve_name(name, span.start) {
        return from_symbol(parsed.symbols[index].kind);
    }
    if builtins::function(language, name).is_some() {
        return (ty::FUNCTION, modifier::DEFAULT_LIBRARY);
    }
    if builtins::variable(language, name).is_some() {
        return (ty::VARIABLE, modifier::DEFAULT_LIBRARY | modifier::READONLY);
    }
    (ty::VARIABLE, 0)
}

fn from_symbol(kind: SymbolKind) -> (u32, u32) {
    match kind {
        SymbolKind::Function | SymbolKind::EntryPoint => (ty::FUNCTION, 0),
        SymbolKind::Struct | SymbolKind::Block => (ty::STRUCT, 0),
        SymbolKind::Field => (ty::PROPERTY, 0),
        SymbolKind::Variable | SymbolKind::Local => (ty::VARIABLE, 0),
        SymbolKind::Constant => (ty::VARIABLE, modifier::READONLY),
        SymbolKind::Parameter => (ty::PARAMETER, 0),
        SymbolKind::TypeAlias => (ty::TYPE, 0),
        SymbolKind::Macro => (ty::MACRO, 0),
    }
}

/// Split a span into one `(line, start, length)` per line it covers.
fn split_lines(document: &Document, span: ByteSpan) -> Vec<(u32, u32, u32)> {
    let start = document.position(span.start);
    let end = document.position(span.end);
    if start.line == end.line {
        return vec![(start.line, start.character, end.character - start.character)];
    }

    let text = document.slice(span);
    let mut pieces = Vec::new();
    let mut line = start.line;
    let mut character = start.character;
    for piece in text.split_inclusive('\n') {
        let content = piece.trim_end_matches(['\n', '\r']);
        let length: u32 = content.chars().map(|c| c.len_utf16() as u32).sum();
        if length > 0 {
            pieces.push((line, character, length));
        }
        if piece.ends_with('\n') {
            line += 1;
            character = 0;
        }
    }
    pieces
}

/// The edits that turn `previous` into `current`.
///
/// One replacement of the middle, found by trimming the common prefix and
/// suffix. A finer diff would save bytes on a paste and cost time on every
/// keystroke, which is the wrong trade for the case that actually happens.
pub fn diff(previous: &[SemanticToken], current: &[SemanticToken]) -> Vec<SemanticTokensEdit> {
    let mut prefix = 0;
    while prefix < previous.len()
        && prefix < current.len()
        && previous[prefix] == current[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0;
    while suffix < previous.len() - prefix
        && suffix < current.len() - prefix
        && previous[previous.len() - 1 - suffix] == current[current.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let delete = previous.len() - prefix - suffix;
    let insert = &current[prefix..current.len() - suffix];
    if delete == 0 && insert.is_empty() {
        return Vec::new();
    }

    // The protocol counts in `u32`s, not tokens, and a token is five of them.
    vec![SemanticTokensEdit {
        start: prefix as u32 * 5,
        delete_count: delete as u32 * 5,
        data: Some(insert.to_vec()),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(delta_line: u32, delta_start: u32) -> SemanticToken {
        SemanticToken {
            delta_line,
            delta_start,
            length: 1,
            token_type: 0,
            token_modifiers_bitset: 0,
        }
    }

    #[test]
    fn the_legend_covers_every_type_the_classifier_emits() {
        // The `ty` constants index into `TYPES`; a mismatch paints tokens the
        // wrong colour with no error anywhere.
        assert_eq!(TYPES.len(), 13);
        assert_eq!(TYPES[ty::DECORATOR as usize], SemanticTokenType::DECORATOR);
        assert_eq!(TYPES[ty::PROPERTY as usize], SemanticTokenType::PROPERTY);
        assert_eq!(TYPES[ty::MACRO as usize], SemanticTokenType::MACRO);
    }

    #[test]
    fn an_unchanged_array_produces_no_edits() {
        let tokens = vec![token(0, 0), token(1, 4)];
        assert!(diff(&tokens, &tokens).is_empty());
    }

    #[test]
    fn a_changed_middle_becomes_one_replacement_counted_in_u32s() {
        let previous = vec![token(0, 0), token(1, 4), token(1, 8)];
        let current = vec![token(0, 0), token(1, 5), token(1, 8)];
        let edits = diff(&previous, &current);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].start, 5);
        assert_eq!(edits[0].delete_count, 5);
        assert_eq!(edits[0].data.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn an_append_deletes_nothing() {
        let previous = vec![token(0, 0)];
        let current = vec![token(0, 0), token(1, 4)];
        let edits = diff(&previous, &current);
        assert_eq!(edits[0].delete_count, 0);
        assert_eq!(edits[0].start, 5);
    }

    #[test]
    fn emptying_the_array_deletes_everything() {
        let previous = vec![token(0, 0), token(1, 4)];
        let edits = diff(&previous, &[]);
        assert_eq!(edits[0].start, 0);
        assert_eq!(edits[0].delete_count, 10);
        assert!(edits[0].data.as_ref().unwrap().is_empty());
    }
}
