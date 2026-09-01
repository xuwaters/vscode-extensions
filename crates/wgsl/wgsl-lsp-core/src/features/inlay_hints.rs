//! `textDocument/inlayHint`.
//!
//! Two hints, both off by default, both about something the source does not
//! say:
//!
//! - **Types.** WGSL's `let squared = distance * distance;` declares no type,
//!   and the only thing that knows it is `f32` is naga. GLSL always declares
//!   its types, so this hint never fires there — correctly.
//! - **Parameter names.** `mix(a, b, t)` reads as three anonymous arguments
//!   until you know that the third is the blend factor.

use analyzer_core::spans::ByteSpan;
use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams};
use wgsl_syntax::SymbolKind;

use crate::Server;
use crate::analysis::types;
use crate::features::signature_help::parameter_ranges;
use crate::state::Document;

impl Server {
    pub fn inlay_hints(&mut self, params: InlayHintParams) -> Option<Vec<InlayHint>> {
        let document = self.document(&params.text_document.uri)?;
        let settings = &self.settings().for_language(document.language).inlay_hints;
        if !settings.enabled {
            return None;
        }

        let range = document.span(params.range);
        let mut hints = Vec::new();
        if settings.types {
            hints.extend(type_hints(document, range));
        }
        if settings.parameter_names {
            hints.extend(parameter_hints(document, range));
        }
        Some(hints)
    }
}

/// `let squared` → `let squared: f32`.
fn type_hints(document: &Document, range: ByteSpan) -> Vec<InlayHint> {
    let Some(module) = document.module() else {
        return Vec::new();
    };
    let parsed = document.parsed();

    parsed
        .symbols
        .iter()
        .filter(|symbol| {
            matches!(symbol.kind, SymbolKind::Local | SymbolKind::Variable)
                && range.contains(symbol.name_span.start)
                // Already annotated: the whole point is the ones that are not.
                && !symbol.detail.contains(':')
        })
        .filter_map(|symbol| {
            let function = document.naga_function_at(&module, symbol.name_span.start);
            let resolution = types::type_of_name(&module, function, &symbol.name)?;
            let rendered = types::render_resolution(&module, &resolution, document.language);
            Some(InlayHint {
                position: document.position(symbol.name_span.end),
                label: InlayHintLabel::String(format!(": {rendered}")),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: Some(false),
                padding_right: Some(false),
                data: None,
            })
        })
        .collect()
}

/// `mix(a, b, t)` → `mix(e1: a, e2: b, e3: t)`.
fn parameter_hints(document: &Document, range: ByteSpan) -> Vec<InlayHint> {
    let parsed = document.parsed();
    let mut hints = Vec::new();

    for block in &parsed.blocks {
        if block.kind != wgsl_syntax::BlockKind::Paren {
            continue;
        }
        if block.span.start < range.start || block.span.start > range.end {
            continue;
        }
        let Some(name) = parsed.token_before(block.span.start) else {
            continue;
        };
        if !matches!(
            name.kind,
            wgsl_syntax::lexer::TokenKind::Ident | wgsl_syntax::lexer::TokenKind::Type
        ) {
            continue;
        }
        let Some(signature) = signature_of(document, document.slice(name.span)) else {
            continue;
        };

        let labels = parameter_names(&signature);
        for (index, start) in argument_starts(document, block.span).into_iter().enumerate() {
            let Some(label) = labels.get(index) else {
                break;
            };
            hints.push(InlayHint {
                position: document.position(start),
                label: InlayHintLabel::String(format!("{label}:")),
                kind: Some(InlayHintKind::PARAMETER),
                text_edits: None,
                tooltip: None,
                padding_left: Some(false),
                padding_right: Some(true),
                data: None,
            });
        }
    }

    hints
}

/// The signature of a callable name, builtin or declared in this file.
fn signature_of(document: &Document, name: &str) -> Option<String> {
    if let Some(builtin) = wgsl_syntax::builtins::function(document.language, name) {
        return Some(builtin.signature.to_string());
    }
    document
        .parsed()
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == name
                && matches!(symbol.kind, SymbolKind::Function | SymbolKind::EntryPoint)
        })
        .map(|symbol| symbol.detail.clone())
}

/// The parameter *names* out of a signature string.
///
/// Both spellings appear: WGSL-shaped `name: type`, GLSL-shaped `type name`.
/// A parameter that is only a type — `float f(float)` — has no name to show.
pub(crate) fn parameter_names(signature: &str) -> Vec<String> {
    parameter_ranges(signature)
        .into_iter()
        .map(|(start, end)| {
            let text = signature[start as usize..end as usize].trim();
            match text.split_once(':') {
                // `e1: T`
                Some((name, _)) => name.trim().to_string(),
                // `in vec3 position` — the last word is the name, unless the
                // whole thing is one word, which makes it a bare type.
                None => {
                    let words: Vec<&str> = text.split_whitespace().collect();
                    if words.len() < 2 {
                        String::new()
                    } else {
                        // `float values[4]` — the array suffix belongs to the
                        // type, not to the name the hint shows.
                        let last = words[words.len() - 1];
                        last.split('[').next().unwrap_or(last).to_string()
                    }
                }
            }
        })
        .collect()
}

/// The offset each top-level argument starts at.
fn argument_starts(document: &Document, parens: ByteSpan) -> Vec<u32> {
    let parsed = document.parsed();
    let mut starts = Vec::new();
    let mut depth = 0i32;
    let mut expecting = true;

    for token in &parsed.tokens {
        if token.span.start <= parens.start || token.span.end >= parens.end {
            continue;
        }
        if token.kind.is_trivia() {
            continue;
        }
        match document.slice(token.span) {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            "," if depth == 0 => {
                expecting = true;
                continue;
            }
            _ => {}
        }
        if expecting {
            starts.push(token.span.start);
            expecting = false;
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_out_of_both_signature_spellings() {
        assert_eq!(parameter_names("mix(e1: T, e2: T, e3: T) -> T"), ["e1", "e2", "e3"]);
        assert_eq!(
            parameter_names("float attenuate(float distance, float falloff)"),
            ["distance", "falloff"]
        );
        // A bare type names nothing.
        assert_eq!(parameter_names("float f(float)"), [""]);
        assert!(parameter_names("barrier()").is_empty());
    }

    #[test]
    fn an_array_suffix_is_not_part_of_the_name() {
        assert_eq!(parameter_names("void f(float values[4])"), ["values"]);
    }
}
