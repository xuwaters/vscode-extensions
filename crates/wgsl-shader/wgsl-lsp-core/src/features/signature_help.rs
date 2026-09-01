//! `textDocument/signatureHelp`.
//!
//! WGSL gets one signature, because that is all there is: a builtin's entry in
//! [`wgsl_syntax::builtins`], or a file function's `detail`, which the parser
//! captured verbatim from the source. Parameter ranges are then found by
//! scanning the string, which sounds fragile and is not — both sources are
//! `name(a, b) -> c` by construction.
//!
//! GLSL gets the whole **overload set** ([`glsl`]), because GLSL overloads:
//! `texture` has thirty-odd signatures and `mix` has nine, and the file's own
//! functions can be overloaded too. The set is filtered to the declared
//! `#version`, printed in the spec's own generic notation, and the one whose
//! arity fits what has been typed so far is the active one.

use lsp_types::{
    Documentation, MarkupContent, MarkupKind, ParameterInformation, ParameterLabel,
    SignatureHelp, SignatureHelpParams, SignatureInformation,
};
use wgsl_syntax::builtins;

use crate::Server;
use crate::state::Document;

mod glsl;

impl Server {
    pub fn signature_help(&mut self, params: SignatureHelpParams) -> Option<SignatureHelp> {
        let position = params.text_document_position_params;
        let (document, offset) = self.locate(&position.text_document.uri, position.position)?;

        let call = enclosing_call(document, offset)?;
        if document.glsl().is_some() {
            return glsl::signature_help(document, &call);
        }

        let (signature, doc) = lookup(document, &call.name)?;
        Some(SignatureHelp {
            signatures: vec![information(&signature, &doc, &[], call.argument)],
            active_signature: Some(0),
            active_parameter: None,
        })
    }
}

/// One signature, with its parameter ranges resolved and the cursor's
/// argument marked.
///
/// `docs` supplies per-parameter prose where there is any, positionally; a
/// short slice simply leaves the rest undocumented.
pub(crate) fn information(
    signature: &str,
    doc: &str,
    docs: &[String],
    argument: u32,
) -> SignatureInformation {
    let parameters = parameter_ranges(signature);
    SignatureInformation {
        label: signature.to_string(),
        documentation: (!doc.is_empty()).then(|| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc.to_string(),
            })
        }),
        parameters: Some(
            parameters
                .iter()
                .enumerate()
                .map(|(index, &(start, end))| ParameterInformation {
                    label: ParameterLabel::LabelOffsets([start, end]),
                    documentation: docs
                        .get(index)
                        .filter(|text| !text.is_empty())
                        .map(|text| {
                            Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: text.clone(),
                            })
                        }),
                })
                .collect(),
        ),
        active_parameter: Some(argument.min(parameters.len().saturating_sub(1) as u32)),
    }
}

/// The call the cursor is inside.
pub(crate) struct Call {
    pub name: String,
    /// Zero-based index of the argument the cursor is in.
    pub argument: u32,
}

/// The innermost `name(…)` containing `offset`.
pub(crate) fn enclosing_call(document: &Document, offset: u32) -> Option<Call> {
    let parsed = document.parsed();
    let block = parsed
        .blocks
        .iter()
        .filter(|block| {
            block.kind == wgsl_syntax::BlockKind::Paren && block.span.contains(offset)
        })
        .min_by_key(|block| block.span.len())?;

    let name = parsed.token_before(block.span.start)?;
    if !matches!(
        name.kind,
        wgsl_syntax::lexer::TokenKind::Ident | wgsl_syntax::lexer::TokenKind::Type
    ) {
        return None;
    }

    Some(Call {
        name: document.slice(name.span).to_string(),
        argument: argument_index(document, block.span.start, offset),
    })
}

/// How many top-level commas lie between the `(` and the cursor.
fn argument_index(document: &Document, open: u32, offset: u32) -> u32 {
    let parsed = document.parsed();
    let mut depth = 0i32;
    let mut commas = 0u32;

    for token in &parsed.tokens {
        if token.span.start <= open || token.span.start >= offset {
            continue;
        }
        match document.slice(token.span) {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            "," if depth == 0 => commas += 1,
            _ => {}
        }
    }
    commas
}

/// The signature and documentation for a callable name.
fn lookup(document: &Document, name: &str) -> Option<(String, String)> {
    if let Some(builtin) = builtins::function(document.language, name) {
        return Some((builtin.signature.to_string(), builtin.doc.to_string()));
    }

    let parsed = document.parsed();
    let symbol = parsed.symbols.iter().find(|symbol| {
        symbol.name == name
            && matches!(
                symbol.kind,
                wgsl_syntax::SymbolKind::Function | wgsl_syntax::SymbolKind::EntryPoint
            )
    })?;
    let doc = crate::features::doc_comment(parsed, document.text(), symbol.full_span);
    Some((symbol.detail.clone(), doc.unwrap_or_default()))
}

/// UTF-16 offsets of each parameter within the signature string.
///
/// Offsets rather than substrings, because the same word appears twice in
/// `min(x: T, y: T)` and a client matching by text would highlight the wrong
/// one.
pub(crate) fn parameter_ranges(signature: &str) -> Vec<(u32, u32)> {
    let Some(open) = signature.find('(') else {
        return Vec::new();
    };
    let Some(close) = matching_paren(signature, open) else {
        return Vec::new();
    };

    let mut ranges = Vec::new();
    let mut depth = 0i32;
    let mut start = open + 1;

    for (index, ch) in signature.char_indices().skip(open + 1) {
        if index >= close {
            break;
        }
        match ch {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => depth -= 1,
            ',' if depth == 0 => {
                push_range(&mut ranges, signature, start, index);
                start = index + 1;
            }
            _ => {}
        }
    }
    push_range(&mut ranges, signature, start, close);
    ranges
}

fn push_range(ranges: &mut Vec<(u32, u32)>, signature: &str, start: usize, end: usize) {
    let text = &signature[start..end];
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let lead = text.len() - text.trim_start().len();
    let from = utf16_len(&signature[..start + lead]);
    ranges.push((from, from + utf16_len(trimmed)));
}

fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (index, ch) in text.char_indices().skip(open) {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn utf16_len(text: &str) -> u32 {
    text.chars().map(|c| c.len_utf16() as u32).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(signature: &str) -> Vec<&str> {
        parameter_ranges(signature)
            .into_iter()
            .map(|(start, end)| &signature[start as usize..end as usize])
            .collect()
    }

    #[test]
    fn parameters_are_split_at_top_level_commas_only() {
        assert_eq!(labels("mix(e1: T, e2: T, e3: T) -> T"), ["e1: T", "e2: T", "e3: T"]);
        // The comma inside `array<f32, 4>` does not start a parameter.
        assert_eq!(
            labels("f(a: array<f32, 4>, b: u32) -> f32"),
            ["a: array<f32, 4>", "b: u32"]
        );
        assert_eq!(labels("barrier()"), Vec::<&str>::new());
        assert_eq!(labels("no parens at all"), Vec::<&str>::new());
    }

    /// The return type comes after the closing paren and is not a parameter.
    #[test]
    fn the_return_type_is_not_a_parameter() {
        assert_eq!(labels("dot(a: vecN<T>, b: vecN<T>) -> T").len(), 2);
    }
}
