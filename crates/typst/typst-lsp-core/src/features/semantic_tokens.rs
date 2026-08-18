//! Semantic tokens — the real syntax highlighting.
//!
//! `typst_syntax::highlight` gives 22 tags from the actual parser, so colouring
//! is correct by construction rather than by regex approximation. Because the
//! TextMate grammar is deliberately minimal (decision 0007), this is not a
//! garnish: it is the primary colouring mechanism, and
//! `typstUltra.semanticTokens: "disable"` means "fall back to approximate
//! colouring".
//!
//! Two details that are easy to get wrong:
//!
//! * **Tags nest.** Upstream reports `= *AB*` as `0..6 Heading` *and*
//!   `2..6 Strong`. LSP tokens may not overlap, so we tag leaves with the
//!   innermost tag on their ancestor chain.
//! * **Tokens may not span lines.** A raw block does, so it is split.

use lsp_types::{
    SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensDelta,
    SemanticTokensDeltaParams, SemanticTokensEdit, SemanticTokensFullDeltaResult,
    SemanticTokensLegend, SemanticTokensParams, SemanticTokensResult,
};
use typst::syntax::{LinkedNode, Source, Tag, highlight};

use crate::settings::SemanticTokensMode;
use crate::{Ports, Server};

/// The legend, in the order the encoded indices refer to.
///
/// The first six are LSP standard types every theme already colours. The rest
/// are ours, and `contributes.semanticTokenScopes` in `package.json` gives each
/// one a TextMate scope so themes without explicit support still look right.
pub const TOKEN_TYPES: &[&str] = &[
    "comment",
    "keyword",
    "operator",
    "number",
    "string",
    "function",
    // Custom, with grammar-scope fallbacks declared by the extension.
    "punct",
    "escape",
    "strong",
    "emph",
    "link",
    "raw",
    "label",
    "ref",
    "heading",
    "listMarker",
    "listTerm",
    "mathDelimiter",
    "interpolated",
    "error",
    // Standard LSP types, used by the BibTeX colouring in `features::bibtex`.
    // Every theme already understands both, so they need no scope mapping.
    "property",
    "variable",
];

/// The legend the server advertises at initialize time.
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.iter().map(|name| SemanticTokenType::new(name)).collect(),
        token_modifiers: Vec::new(),
    }
}

/// A token type's index in [`TOKEN_TYPES`], which is what the wire format
/// carries. An unknown name would be a bug here, not in the client, so it falls
/// back to the first entry rather than dropping the token.
pub(crate) fn index_of(name: &str) -> u32 {
    TOKEN_TYPES.iter().position(|candidate| *candidate == name).unwrap_or(0) as u32
}

/// Tag → index into [`TOKEN_TYPES`].
fn token_index(tag: Tag) -> u32 {
    let name = match tag {
        Tag::Comment => "comment",
        Tag::Keyword => "keyword",
        Tag::Operator | Tag::MathOperator => "operator",
        Tag::Number => "number",
        Tag::String => "string",
        Tag::Function => "function",
        Tag::Punctuation | Tag::MathGroupingParens => "punct",
        Tag::Escape => "escape",
        Tag::Strong => "strong",
        Tag::Emph => "emph",
        Tag::Link => "link",
        Tag::Raw => "raw",
        Tag::Label => "label",
        Tag::Ref => "ref",
        Tag::Heading => "heading",
        Tag::ListMarker => "listMarker",
        Tag::ListTerm => "listTerm",
        Tag::MathDelimiter => "mathDelimiter",
        Tag::Interpolated => "interpolated",
        Tag::Error => "error",
    };
    index_of(name)
}

/// One absolute token, before delta encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AbsoluteToken {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
}

impl<Q: Ports> Server<Q> {
    /// `textDocument/semanticTokens/full`.
    pub fn semantic_tokens_full(
        &mut self,
        params: SemanticTokensParams,
    ) -> Option<SemanticTokensResult> {
        if self.settings().semantic_tokens == SemanticTokensMode::Disable {
            return None;
        }
        let (id, source) = self.source_of(&params.text_document.uri)?;
        let tokens = match self.bib_of(&params.text_document.uri) {
            Some((_, source, bib)) => self.bib_tokens(&source, &bib),
            None => tokens_for(&source),
        };

        let cache = self.tokens.entry(id).or_default();
        let result_id = cache.store(tokens.clone());

        Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: Some(result_id),
            data: tokens,
        }))
    }

    /// `textDocument/semanticTokens/full/delta`.
    ///
    /// Not optional: a document produces thousands of tokens and re-sending
    /// them on every keystroke is the difference between smooth and not.
    pub fn semantic_tokens_delta(
        &mut self,
        params: SemanticTokensDeltaParams,
    ) -> Option<SemanticTokensFullDeltaResult> {
        if self.settings().semantic_tokens == SemanticTokensMode::Disable {
            return None;
        }
        let (id, source) = self.source_of(&params.text_document.uri)?;
        let tokens = match self.bib_of(&params.text_document.uri) {
            Some((_, source, bib)) => self.bib_tokens(&source, &bib),
            None => tokens_for(&source),
        };

        let cache = self.tokens.entry(id).or_default();
        if !cache.matches(&params.previous_result_id) {
            // We no longer hold what the client is diffing against — send the
            // whole array rather than an edit it cannot apply.
            let result_id = cache.store(tokens.clone());
            return Some(SemanticTokensFullDeltaResult::Tokens(SemanticTokens {
                result_id: Some(result_id),
                data: tokens,
            }));
        }

        let edits = diff_tokens(&cache.tokens, &tokens);
        let result_id = cache.store(tokens);

        Some(SemanticTokensFullDeltaResult::TokensDelta(SemanticTokensDelta {
            result_id: Some(result_id),
            edits,
        }))
    }
}

/// Delta-encoded semantic tokens for a whole document.
pub fn tokens_for(source: &Source) -> Vec<SemanticToken> {
    let mut absolute = Vec::new();
    collect(&LinkedNode::new(source.root()), None, source, &mut absolute);
    absolute.sort_by_key(|token| (token.line, token.start));
    encode(&absolute)
}

/// Walk to the leaves, carrying the innermost tag seen so far.
fn collect(
    node: &LinkedNode,
    inherited: Option<Tag>,
    source: &Source,
    out: &mut Vec<AbsoluteToken>,
) {
    // A tag on this node overrides one from an ancestor, so `*bold*` inside a
    // heading is bold rather than heading-coloured.
    let tag = highlight(node).or(inherited);

    if node.children().len() == 0 {
        let range = node.range();
        if range.is_empty() {
            return;
        }
        if let Some(tag) = tag {
            push_split_by_line(range, token_index(tag), source, out);
        }
        return;
    }

    for child in node.children() {
        collect(&child, tag, source, out);
    }
}

/// LSP tokens may not span lines, so a multi-line leaf (a raw block, a block
/// comment) becomes one token per line.
pub(crate) fn push_split_by_line(
    range: std::ops::Range<usize>,
    token_type: u32,
    source: &Source,
    out: &mut Vec<AbsoluteToken>,
) {
    let lines = source.lines();
    let mut cursor = range.start;

    while cursor < range.end {
        let line = lines.byte_to_line(cursor).unwrap_or(0);
        let line_range = lines.line_to_range(line).unwrap_or(cursor..range.end);
        let stop = range.end.min(line_range.end);

        // Trim the newline itself, which has no visible extent.
        let text = source.text();
        let mut end = stop;
        while end > cursor && matches!(text.as_bytes().get(end - 1), Some(b'\n' | b'\r')) {
            end -= 1;
        }

        if end > cursor {
            let start_utf16 = lines.byte_to_utf16(cursor).unwrap_or(0);
            let line_start_utf16 = lines.byte_to_utf16(line_range.start).unwrap_or(0);
            let end_utf16 = lines.byte_to_utf16(end).unwrap_or(start_utf16);

            out.push(AbsoluteToken {
                line: line as u32,
                start: (start_utf16 - line_start_utf16) as u32,
                length: (end_utf16 - start_utf16) as u32,
                token_type,
            });
        }

        cursor = stop.max(cursor + 1);
    }
}

/// LSP's relative encoding: each token is offset from the previous one.
pub(crate) fn encode(tokens: &[AbsoluteToken]) -> Vec<SemanticToken> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut previous_line = 0;
    let mut previous_start = 0;

    for token in tokens {
        let delta_line = token.line - previous_line;
        let delta_start =
            if delta_line == 0 { token.start - previous_start } else { token.start };

        out.push(SemanticToken {
            delta_line,
            delta_start,
            length: token.length,
            token_type: token.token_type,
            token_modifiers_bitset: 0,
        });

        previous_line = token.line;
        previous_start = token.start;
    }

    out
}

/// The smallest single edit that turns `old` into `new`.
///
/// One replaced span is what a keystroke actually produces, and it is what the
/// protocol is shaped for: the arrays are compared as flat `u32` quintuples, so
/// a common prefix and suffix bound the change.
fn diff_tokens(old: &[SemanticToken], new: &[SemanticToken]) -> Vec<SemanticTokensEdit> {
    if old == new {
        return Vec::new();
    }

    let prefix = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let max_suffix = (old.len() - prefix).min(new.len() - prefix);
    let suffix = (0..max_suffix)
        .take_while(|i| old[old.len() - 1 - i] == new[new.len() - 1 - i])
        .count();

    let deleted = old.len() - prefix - suffix;
    let inserted: Vec<SemanticToken> = new[prefix..new.len() - suffix].to_vec();

    vec![SemanticTokensEdit {
        // Offsets are in `u32`s, not tokens: five per token.
        start: (prefix * 5) as u32,
        delete_count: (deleted * 5) as u32,
        data: Some(inserted),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::TokenCache;

    fn absolute(source: &Source) -> Vec<AbsoluteToken> {
        let mut out = Vec::new();
        collect(&LinkedNode::new(source.root()), None, source, &mut out);
        out.sort_by_key(|token| (token.line, token.start));
        out
    }

    fn type_name(index: u32) -> &'static str {
        TOKEN_TYPES[index as usize]
    }

    #[test]
    fn every_tag_maps_to_a_declared_token_type() {
        for tag in Tag::LIST {
            let index = token_index(*tag);
            assert!(
                (index as usize) < TOKEN_TYPES.len(),
                "{tag:?} maps outside the legend"
            );
        }
    }

    #[test]
    fn tokens_never_overlap_and_run_in_order() {
        let source = Source::detached(
            "= A *bold* heading <intro>\n\nSee @intro and `raw`.\n\n#let f(x) = x + 1\n",
        );
        let tokens = absolute(&source);

        for pair in tokens.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                (a.line, a.start) <= (b.line, b.start),
                "tokens out of order: {a:?} then {b:?}"
            );
            if a.line == b.line {
                assert!(
                    a.start + a.length <= b.start,
                    "tokens overlap: {a:?} and {b:?}"
                );
            }
        }
    }

    #[test]
    fn the_innermost_tag_wins_over_an_enclosing_one() {
        // Upstream reports the heading as 0..6 and the strong as 2..6; the
        // strong text must come back strong, not heading-coloured.
        let source = Source::detached("= *AB*\n");
        let tokens = absolute(&source);

        let strong = tokens
            .iter()
            .find(|token| type_name(token.token_type) == "strong")
            .expect("expected a strong token");
        assert_eq!(strong.start, 2);
    }

    #[test]
    fn a_multi_line_raw_block_becomes_one_token_per_line() {
        let source = Source::detached("```\nline one\nline two\n```\n");
        let tokens = absolute(&source);

        let raw_lines: Vec<u32> = tokens
            .iter()
            .filter(|token| type_name(token.token_type) == "raw")
            .map(|token| token.line)
            .collect();

        assert!(raw_lines.len() > 1, "a raw block must be split across lines");
        assert!(
            raw_lines.windows(2).all(|pair| pair[0] != pair[1] || pair.is_empty()),
            "each line should carry its own token"
        );
    }

    #[test]
    fn delta_encoding_is_relative_to_the_previous_token() {
        let tokens = encode(&[
            AbsoluteToken { line: 0, start: 0, length: 1, token_type: 3 },
            AbsoluteToken { line: 0, start: 5, length: 2, token_type: 4 },
            AbsoluteToken { line: 2, start: 4, length: 3, token_type: 5 },
        ]);

        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        assert_eq!(tokens[1].delta_line, 0);
        assert_eq!(tokens[1].delta_start, 5, "same line: relative to the previous start");
        assert_eq!(tokens[2].delta_line, 2);
        assert_eq!(tokens[2].delta_start, 4, "new line: absolute again");
    }

    #[test]
    fn an_identical_document_produces_no_edits() {
        let tokens = tokens_for(&Source::detached("= Heading\n"));
        assert!(diff_tokens(&tokens, &tokens).is_empty());
    }

    /// P2-17: the delta must survive a scripted edit sequence — applying the
    /// edits to the old array has to reproduce the new one exactly, or the
    /// client's colours drift away from the document.
    #[test]
    fn deltas_round_trip_over_an_edit_sequence() {
        let mut text = String::from("= Heading\n\nBody text.\n");
        let mut previous = tokens_for(&Source::detached(text.as_str()));

        for insertion in ["*bold* ", "#let x = 1\n", "`raw` ", "@ref ", "// note\n"] {
            text.push_str(insertion);
            let next = tokens_for(&Source::detached(text.as_str()));

            let edits = diff_tokens(&previous, &next);
            let applied = apply_edits(&previous, &edits);
            assert_eq!(applied, next, "delta did not reproduce the new token array");

            previous = next;
        }
    }

    /// The client's side of `SemanticTokensDelta`, for the test above.
    fn apply_edits(
        tokens: &[SemanticToken],
        edits: &[SemanticTokensEdit],
    ) -> Vec<SemanticToken> {
        let mut flat: Vec<u32> = tokens
            .iter()
            .flat_map(|token| {
                [
                    token.delta_line,
                    token.delta_start,
                    token.length,
                    token.token_type,
                    token.token_modifiers_bitset,
                ]
            })
            .collect();

        // Apply back to front so earlier offsets stay valid.
        for edit in edits.iter().rev() {
            let start = edit.start as usize;
            let end = start + edit.delete_count as usize;
            let inserted: Vec<u32> = edit
                .data
                .as_deref()
                .unwrap_or_default()
                .iter()
                .flat_map(|token| {
                    [
                        token.delta_line,
                        token.delta_start,
                        token.length,
                        token.token_type,
                        token.token_modifiers_bitset,
                    ]
                })
                .collect();
            flat.splice(start..end, inserted);
        }

        flat.as_chunks::<5>()
            .0
            .iter()
            .map(|chunk| SemanticToken {
                delta_line: chunk[0],
                delta_start: chunk[1],
                length: chunk[2],
                token_type: chunk[3],
                token_modifiers_bitset: chunk[4],
            })
            .collect()
    }

    #[test]
    fn a_stale_result_id_falls_back_to_the_full_array() {
        let mut cache = TokenCache::default();
        let first = cache.store(tokens_for(&Source::detached("= A\n")));
        assert!(cache.matches(&first));
        cache.store(tokens_for(&Source::detached("= B\n")));
        assert!(!cache.matches(&first));
    }
}
