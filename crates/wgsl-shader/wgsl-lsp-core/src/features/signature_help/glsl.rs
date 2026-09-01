//! The GLSL half of `textDocument/signatureHelp` (P5-05).
//!
//! GLSL overloads, so the answer is a *set*, not a signature. Three sources,
//! and a call can only be one of them:
//!
//! 1. **The file's own functions.** GLSL lets a file declare `float f(float)`
//!    and `vec2 f(vec2)`; both are shown, with their parameter names.
//! 2. **The builtin tables**, filtered to the declared `#version` and printed
//!    in the reference pages' own generic notation — `genType mix(genType x,
//!    genType y, float a)` is what the spec says and what a reader recognises,
//!    and expanding it to the nine concrete signatures would be a wall.
//! 3. **The legacy table** (decision 0007), for the compatibility and ES 1.00
//!    surface.
//!
//! The active signature is the first whose arity accepts the argument being
//! typed, which is what makes an overload set usable: type the second comma in
//! `clamp(x, ` and the three-parameter form is the one highlighted.

use lsp_types::{SignatureHelp, SignatureInformation};

use glsl_spec::{BuiltinFunction, Version};

use super::{Call, information};
use crate::glsl::GlslDocument;
use crate::state::Document;

pub fn signature_help(document: &Document, call: &Call) -> Option<SignatureHelp> {
    let glsl = document.glsl()?;
    let signatures = user_functions(glsl, &call.name, call.argument)
        .or_else(|| builtin(glsl, &call.name, call.argument))?;
    if signatures.0.is_empty() {
        return None;
    }
    Some(SignatureHelp {
        signatures: signatures.0,
        active_signature: Some(signatures.1),
        active_parameter: None,
    })
}

/// Every declaration of `name` the file makes, in source order.
///
/// A file function shadows a builtin of the same name — that is what GLSL
/// scoping says — so finding one stops the search.
fn user_functions(
    glsl: &GlslDocument,
    name: &str,
    argument: u32,
) -> Option<(Vec<SignatureInformation>, u32)> {
    let mut labels: Vec<String> = Vec::new();
    let mut arities: Vec<(usize, usize)> = Vec::new();
    for (_, symbol) in glsl.analysis.symbols.iter() {
        if symbol.name != name {
            continue;
        }
        let Some(signature) = &symbol.signature else {
            continue;
        };
        let structs = &glsl.analysis.structs;
        let params: Vec<String> = signature
            .params
            .iter()
            .map(|param| {
                let flow = if param.writes { "out " } else { "" };
                format!("{flow}{} {}", param.ty.name(structs), param.name)
            })
            .collect();
        let label = format!(
            "{} {name}({})",
            signature.ret.name(structs),
            params.join(", ")
        );
        // A prototype and its definition are the same signature written twice.
        if labels.contains(&label) {
            continue;
        }
        arities.push((signature.params.len(), signature.params.len()));
        labels.push(label);
    }
    if labels.is_empty() {
        return None;
    }
    let active = active_for(&arities, argument);
    Some((
        labels.iter().map(|label| information(label, "", &[], argument)).collect(),
        active,
    ))
}

/// The builtin of that name, from either table.
fn builtin(
    glsl: &GlslDocument,
    name: &str,
    argument: u32,
) -> Option<(Vec<SignatureInformation>, u32)> {
    let found = glsl_analysis::lookup_function(name)?;
    Some(overloads(found.function(), found.doc(), glsl.version(), argument))
}

/// The overload set as LSP signatures, availability-filtered.
///
/// Never filtered to *nothing*: a name that exists with no overload this
/// version has is better shown in full than shown empty — the availability
/// diagnostic is what says it is the wrong version.
fn overloads(
    function: &'static BuiltinFunction,
    doc: &str,
    version: Version,
    argument: u32,
) -> (Vec<SignatureInformation>, u32) {
    let mut chosen: Vec<&'static glsl_spec::Overload> =
        function.overloads_in(version).collect();
    if chosen.is_empty() {
        chosen = function.overloads.iter().collect();
    }

    let mut labels: Vec<String> = Vec::new();
    let mut kept: Vec<&'static glsl_spec::Overload> = Vec::new();
    for overload in chosen {
        let label = overload.signature(function.name);
        if labels.contains(&label) {
            continue;
        }
        labels.push(label);
        kept.push(overload);
    }

    let arities: Vec<(usize, usize)> = kept.iter().map(|o| o.arity()).collect();
    let active = active_for(&arities, argument);
    let signatures = kept
        .iter()
        .zip(&labels)
        .map(|(overload, label)| {
            let docs: Vec<String> = overload
                .params
                .iter()
                .map(|param| {
                    function
                        .param_doc(param.name)
                        .map(|doc| doc.text().to_string())
                        .unwrap_or_default()
                })
                .collect();
            information(label, doc, &docs, argument)
        })
        .collect();
    (signatures, active)
}

/// The first signature that can still accept the argument being typed.
///
/// Falls back to the first, which is the spec's own order and therefore the
/// simplest form of the call.
fn active_for(arities: &[(usize, usize)], argument: u32) -> u32 {
    let wanted = argument as usize + 1;
    arities
        .iter()
        .position(|&(min, max)| wanted >= min.min(1) && wanted <= max)
        .unwrap_or(0) as u32
}
