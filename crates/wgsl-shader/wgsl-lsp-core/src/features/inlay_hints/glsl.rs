//! The GLSL half of `textDocument/inlayHint` (P5-08).
//!
//! GLSL writes its types down, so the WGSL hint — "this binding declares no
//! type, here is the inferred one" — has nothing to say in almost every
//! declaration. There is exactly one exception, and it is worth a hint: an
//! implicitly sized array takes its size from its initialiser, and counting
//! `float[](0.1, 0.2, 0.3, 0.4, 0.5)` by eye is exactly the sort of thing an
//! editor should do for you.
//!
//! Parameter-name hints have more to work on than before. The names come from
//! the overload the call actually matched, in the reference pages' own
//! spelling, rather than from one hand-written signature per function.

use analyzer_core::spans::ByteSpan;
use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel};

use glsl_syntax::{NodeId, NodeKind, SyntaxTree};

use crate::state::Document;

/// `float weights[]` → `float weights[3]`.
///
/// Counted off the CST rather than asked of the analyzer: the type model
/// records an implicitly sized array as unsized (which it is, until the
/// initialiser is read), and counting the initialiser's elements is a
/// structural question, not a semantic one.
pub fn array_size_hints(document: &Document, range: ByteSpan) -> Vec<InlayHint> {
    let Some(glsl) = document.glsl() else {
        return Vec::new();
    };
    let tree = &glsl.tree;
    let mut hints = Vec::new();
    for (id, node) in tree.nodes() {
        if node.kind != NodeKind::Declarator || !range.contains(node.span.start) {
            continue;
        }
        let Some(spec) = tree.child_of_kind(id, NodeKind::ArraySpec) else {
            continue;
        };
        // `[4]` already says how many; only `[]` leaves it to the reader.
        if tree.child_nodes(spec).next().is_some() {
            continue;
        }
        let Some(count) = initialiser_length(tree, id) else {
            continue;
        };
        hints.push(InlayHint {
            position: document.position(tree.span(spec).start + 1),
            label: InlayHintLabel::String(count.to_string()),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: Some(false),
            padding_right: Some(false),
            data: None,
        });
    }
    hints
}

/// How many elements a declarator's initialiser supplies.
///
/// GLSL spells an array initialiser two ways — `float[](a, b)` and `{a, b}` —
/// and both are one node with one expression per element.
fn initialiser_length(tree: &SyntaxTree, declarator: NodeId) -> Option<usize> {
    let initializer = tree.child_of_kind(declarator, NodeKind::Initializer)?;
    for child in tree.child_nodes(initializer) {
        let list = match tree.kind(child) {
            NodeKind::InitializerList => Some(child),
            NodeKind::CallExpr => tree.child_of_kind(child, NodeKind::ArgumentList),
            _ => None,
        };
        if let Some(list) = list {
            return Some(tree.child_nodes(list).count());
        }
    }
    None
}

/// A signature to read parameter names out of: the file's own function, or the
/// first builtin overload that could take this call.
pub fn signature_of(document: &Document, name: &str) -> Option<String> {
    let glsl = document.glsl()?;
    let structs = &glsl.analysis.structs;
    for (_, symbol) in glsl.analysis.symbols.iter() {
        if symbol.name != name {
            continue;
        }
        if let Some(signature) = &symbol.signature {
            let params: Vec<String> = signature
                .params
                .iter()
                .map(|param| format!("{} {}", param.ty.name(structs), param.name))
                .collect();
            return Some(format!(
                "{} {name}({})",
                signature.ret.name(structs),
                params.join(", ")
            ));
        }
    }

    let found = glsl_analysis::lookup_function(name)?;
    let function = found.function();
    function
        .overloads_in(glsl.version())
        .next()
        .or_else(|| function.overloads.first())
        .map(|overload| overload.signature(function.name))
}
