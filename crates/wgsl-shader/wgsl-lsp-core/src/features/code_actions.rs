//! `textDocument/codeAction`.
//!
//! Three actions, each for something the server knows that the source does not
//! say out loud: the GLSL version, the GLSL stage, and which of WGSL's two
//! spellings of a vector type is in use.

use std::collections::HashMap;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse,
    Position, Range, TextEdit, WorkspaceEdit,
};
use wgsl_syntax::Language;

use crate::Server;
use crate::analysis::stage::{StageHint, hint_from_extension, stage_from_pragma};
use crate::state::Document;

impl Server {
    pub fn code_actions(&mut self, params: CodeActionParams) -> Option<CodeActionResponse> {
        let document = self.document(&params.text_document.uri)?;
        let cursor = document.span(params.range);

        let mut actions = Vec::new();
        match document.language {
            Language::Glsl => {
                actions.extend(add_version(document));
                actions.extend(pin_stage(document));
            }
            Language::Wgsl => actions.extend(switch_type_spelling(document, cursor.start)),
        }

        Some(actions.into_iter().map(CodeActionOrCommand::CodeAction).collect())
    }
}

// `Uri`'s interior mutability is a parse cache inside fluent-uri that takes
// no part in `Hash` or `Eq`, and `WorkspaceEdit::changes` is keyed by `Uri`
// upstream — there is no other map to reach for.
#[allow(clippy::mutable_key_type)]
fn action(title: String, document: &Document, edits: Vec<TextEdit>) -> CodeAction {
    let mut changes = HashMap::new();
    changes.insert(document.uri.clone(), edits);
    CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit { changes: Some(changes), ..WorkspaceEdit::default() }),
        ..CodeAction::default()
    }
}

/// A GLSL source with no `#version` is GLSL 1.10 by the specification (or
/// `glsl.defaultVersion`, when the workspace sets one), which is rarely what
/// the author meant. The offer is a fixed `450` rather than the setting's
/// value: the point is to make the file say which GLSL it is, and a file that
/// states its version is right whatever the setting later becomes.
fn add_version(document: &Document) -> Option<CodeAction> {
    if document.text().lines().any(|line| line.trim_start().starts_with("#version")) {
        return None;
    }
    let top = Range { start: Position { line: 0, character: 0 }, ..Range::default() };
    Some(action(
        "Add `#version 450`".to_string(),
        document,
        vec![TextEdit { range: top, new_text: "#version 450\n".to_string() }],
    ))
}

/// When a GLSL file's stage was *guessed* from the built-ins it happens to
/// use, offer to write the guess down. `#pragma shader_stage` is what `glslc`
/// reads, so the file then means the same thing to the compiler as to us.
fn pin_stage(document: &Document) -> Option<CodeAction> {
    let text = document.text();
    if stage_from_pragma(text).is_some() {
        return None;
    }
    // An extension that already names the stage is documentation enough.
    if hint_from_extension(document.extension()) != StageHint::Unknown {
        return None;
    }
    let label = document.glsl()?.stage_label();

    // After the `#version` line if there is one, since it must come first.
    let line = text
        .lines()
        .position(|line| line.trim_start().starts_with("#version"))
        .map(|index| index as u32 + 1)
        .unwrap_or(0);
    let at = Range {
        start: Position { line, character: 0 },
        end: Position { line, character: 0 },
    };

    Some(action(
        format!("Add `#pragma shader_stage({label})`"),
        document,
        vec![TextEdit {
            range: at,
            new_text: format!("#pragma shader_stage({label})\n"),
        }],
    ))
}

/// WGSL spells `vec4<f32>` and `vec4f` for the same type. Offer the other one.
fn switch_type_spelling(document: &Document, offset: u32) -> Option<CodeAction> {
    let parsed = document.parsed();
    let index = parsed.tokens.iter().position(|token| token.span.contains(offset))?;
    let text = document.slice(parsed.tokens[index].span);

    // Short → long: one token becomes four.
    if let Some((long, scalar)) = expand(text) {
        let span = parsed.tokens[index].span;
        return Some(action(
            format!("Use `{long}<{scalar}>`"),
            document,
            vec![TextEdit {
                range: document.range(span),
                new_text: format!("{long}<{scalar}>"),
            }],
        ));
    }

    // Long → short: `vec4` `<` `f32` `>` becomes one token. The `>` may be
    // several tokens away in `array<vec2<f32>>`, so only the exact shape here
    // is converted.
    let following: Vec<&str> = (1..4)
        .filter_map(|step| parsed.tokens.get(index + step))
        .map(|token| document.slice(token.span))
        .collect();
    if following.len() < 3 || following[0] != "<" || following[2] != ">" {
        return None;
    }
    let short = contract(text, following[1])?;
    let span = analyzer_core::spans::ByteSpan::new(
        parsed.tokens[index].span.start,
        parsed.tokens[index + 3].span.end,
    );
    Some(action(
        format!("Use `{short}`"),
        document,
        vec![TextEdit { range: document.range(span), new_text: short }],
    ))
}

/// The suffix each scalar type takes in WGSL's short spellings.
const SUFFIXES: [(char, &str); 4] =
    [('f', "f32"), ('h', "f16"), ('i', "i32"), ('u', "u32")];

/// `vec4f` → `("vec4", "f32")`.
fn expand(name: &str) -> Option<(&str, &'static str)> {
    let (base, last) = name.split_at(name.len().checked_sub(1)?);
    let suffix = last.chars().next()?;
    let scalar = SUFFIXES.iter().find(|(c, _)| *c == suffix).map(|(_, s)| *s)?;
    is_parameterised(base).then_some((base, scalar))
}

/// `("vec4", "f32")` → `vec4f`.
fn contract(base: &str, scalar: &str) -> Option<String> {
    if !is_parameterised(base) {
        return None;
    }
    let suffix = SUFFIXES.iter().find(|(_, s)| *s == scalar).map(|(c, _)| *c)?;
    Some(format!("{base}{suffix}"))
}

/// Whether a WGSL type name takes a scalar parameter, and so has both
/// spellings. Matrices and vectors do; `array` and the textures do not.
fn is_parameterised(base: &str) -> bool {
    matches!(base, "vec2" | "vec3" | "vec4")
        || (base.starts_with("mat")
            && base.len() == 6
            && base.as_bytes()[4] == b'x'
            && base[3..4].parse::<u8>().is_ok()
            && base[5..6].parse::<u8>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_and_long_wgsl_spellings_convert_both_ways() {
        assert_eq!(expand("vec4f"), Some(("vec4", "f32")));
        assert_eq!(expand("mat3x3h"), Some(("mat3x3", "f16")));
        assert_eq!(contract("vec2", "u32").as_deref(), Some("vec2u"));
        assert_eq!(contract("mat4x2", "f32").as_deref(), Some("mat4x2f"));
    }

    #[test]
    fn types_without_a_short_spelling_are_left_alone() {
        // `array` and the textures take a type argument but have no alias.
        assert_eq!(expand("array"), None);
        assert_eq!(contract("array", "f32"), None);
        assert_eq!(contract("texture_2d", "f32"), None);
        // `f32` itself is not a vector.
        assert_eq!(expand("f32"), None);
        // A user's own type ending in `f` must not be mistaken for one.
        assert_eq!(expand("Stuff"), None);
    }
}
