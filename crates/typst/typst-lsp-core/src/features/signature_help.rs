//! Signature help from `Func` metadata.
//!
//! Declared parameters, their types, and their defaults — but **not** evaluated
//! argument values. Showing live values is what tinymist's compiler patches buy,
//! and buying them means carrying a fork (decision 0001). Declared signatures
//! cover the overwhelming majority of uses.

use lsp_types::{
    Documentation, MarkupContent, MarkupKind, ParameterInformation, ParameterLabel,
    SignatureHelp, SignatureHelpParams, SignatureInformation,
};
use typst::foundations::{CastInfo, Func, ParamInfo, Repr};
use typst::syntax::{LinkedNode, Side, SyntaxKind, ast};

use crate::docs::DocComment;
use crate::{Ports, Server};

impl<Q: Ports> Server<Q> {
    /// `textDocument/signatureHelp`.
    pub fn signature_help(&mut self, params: SignatureHelpParams) -> Option<SignatureHelp> {
        let position = params.text_document_position_params;
        let (id, source, cursor) =
            self.locate(&position.text_document.uri, position.position)?;
        // There are no function calls in a bibliography.
        if super::bibtex::is_bib(id) {
            return None;
        }

        let root = LinkedNode::new(source.root());
        let leaf = root
            .leaf_at(cursor, Side::Before)
            .or_else(|| root.leaf_at(cursor, Side::After))?;

        // Walk out to the enclosing call, if the cursor is inside one.
        let mut node = Some(leaf.clone());
        let call_node = loop {
            let current = node?;
            if current.kind() == SyntaxKind::FuncCall {
                break current;
            }
            node = current.parent().cloned();
        };

        let call = call_node.cast::<ast::FuncCall>()?;
        let func = self.resolve_callee(&call_node, &call)?;
        let active = active_parameter(&call_node, cursor);

        let mut signature = signature_of(&func);
        // A closure carries no documentation of its own; the comment above it
        // does, and that is where a package's parameter docs live.
        if let Some(docs) = self.doc_comment_of(&func) {
            document_signature(&mut signature, &docs);
        }

        Some(SignatureHelp {
            signatures: vec![signature],
            active_signature: Some(0),
            active_parameter: active.map(|index| index as u32),
        })
    }
}

/// Attach a doc comment's prose to a signature and its entries to the
/// parameters they describe.
fn document_signature(signature: &mut SignatureInformation, docs: &DocComment) {
    if signature.documentation.is_none() {
        signature.documentation = Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: docs.markdown().to_string(),
        }));
    }

    for parameter in signature.parameters.iter_mut().flatten() {
        if parameter.documentation.is_some() {
            continue;
        }
        let ParameterLabel::Simple(label) = &parameter.label else { continue };
        // The label is `..name`, `name`, `name: types`, or `name = default`.
        let name = label.trim_start_matches('.').split([':', ' ']).next().unwrap_or("");
        let Some(entry) = docs.param(name) else { continue };
        if entry.docs.is_empty() {
            continue;
        }

        parameter.documentation = Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: entry.docs.to_string(),
        }));
    }
}

fn signature_of(func: &Func) -> SignatureInformation {
    let name = func.name().unwrap_or("function");
    let params: Vec<ParamInfo> = func.params().collect();

    let rendered: Vec<String> = params.iter().map(render_param).collect();
    let label = format!("{name}({})", rendered.join(", "));

    let parameters = params
        .iter()
        .zip(&rendered)
        .map(|(param, rendered)| ParameterInformation {
            label: ParameterLabel::Simple(rendered.clone()),
            // Only native functions carry parameter documentation here; a
            // closure's comes from the doc comment above it, which
            // `document_signature` fills in afterwards.
            documentation: param
                .to_native()
                .filter(|native| !native.docs.is_empty())
                .map(|native| {
                    Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: native.docs.to_string(),
                    })
                }),
        })
        .collect();

    SignatureInformation {
        label,
        documentation: func.docs().map(|docs| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: docs.to_string(),
            })
        }),
        parameters: Some(parameters),
        active_parameter: None,
    }
}

fn render_param(param: &ParamInfo) -> String {
    let mut out = String::new();
    if param.variadic() {
        out.push_str("..");
    }
    out.push_str(param.name().unwrap_or("_"));

    // The accepted types are only described for native functions.
    if let Some(native) = param.to_native() {
        let types = describe_types(&native.input);
        if !types.is_empty() {
            out.push_str(": ");
            out.push_str(&types);
        }
    }
    if !param.required()
        && let Some(default) = param.default()
    {
        out.push_str(" = ");
        out.push_str(&Repr::repr(&default));
    }
    out
}

/// The types a parameter accepts, rendered the way typst's own docs do.
///
/// `CastInfo` has no `Display`; `walk` flattens unions, which is the only part
/// that matters for a one-line signature.
fn describe_types(info: &CastInfo) -> String {
    let mut parts: Vec<String> = Vec::new();
    info.walk(|leaf| match leaf {
        CastInfo::Any => parts.push("any".into()),
        CastInfo::Type(ty) => parts.push(ty.to_string()),
        CastInfo::Value(value, _) => parts.push(Repr::repr(value).to_string()),
        CastInfo::Union(_) => {}
    });

    parts.dedup();
    // A long union is noise in a signature; name a few and stop.
    if parts.len() > 4 {
        parts.truncate(3);
        parts.push("…".into());
    }
    parts.join(" | ")
}

/// Which positional slot the cursor is in, counted by the commas before it.
fn active_parameter(call: &LinkedNode, cursor: usize) -> Option<usize> {
    let args = call.children().find(|child| child.kind() == SyntaxKind::Args)?;

    let mut index = 0;
    for child in args.children() {
        if child.range().start >= cursor {
            break;
        }
        if child.kind() == SyntaxKind::Comma {
            index += 1;
        }
    }
    Some(index)
}
