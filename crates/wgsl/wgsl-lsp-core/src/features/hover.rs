//! `textDocument/hover`.
//!
//! The hover is assembled from three sources, in whatever combination is
//! available: the declaration as written (always, from the syntax layer), the
//! type naga inferred (when naga parsed the file), and the `//` comment above
//! the declaration or the builtin's documentation line.
//!
//! Nothing here requires naga. A hover over a local in a `#version 300 es`
//! shader still shows the declaration and its doc comment; it just cannot add
//! an inferred type, because nothing inferred one.

use lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind};
use wgsl_syntax::{Language, builtins};

use crate::Server;
use crate::analysis::types;
use crate::features::{definition, doc_comment, member_chain};
use crate::state::Document;

impl Server {
    pub fn hover(&mut self, params: HoverParams) -> Option<Hover> {
        let position = params.text_document_position_params;
        let (document, offset) = self.locate(&position.text_document.uri, position.position)?;
        let reference = document.parsed().reference_at(offset)?;
        let name = document.slice(reference.span);

        let markdown = member_hover(document, offset, name)
            .or_else(|| symbol_hover(document, offset))
            .or_else(|| builtin_hover(document, name))?;

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: Some(document.range(reference.span)),
        })
    }
}

/// `camera.view` — the type comes from naga, the declaration from the struct.
fn member_hover(document: &Document, offset: u32, field: &str) -> Option<String> {
    let parsed = document.parsed();
    if !parsed.reference_at(offset)?.is_member {
        return None;
    }

    let module = document.module()?;
    let (base, fields) = member_chain(parsed, document.text(), offset)?;
    let function = document.naga_function_at(&module, offset);
    let owner = types::type_of_chain(&module, function, base, &fields)?;

    let members = types::members(&module, owner.inner_with(&module.types), document.language);
    let member = members.iter().find(|member| member.name == field)?;

    let declaration = match document.language {
        Language::Wgsl => format!("{}: {}", member.name, member.type_name),
        Language::Glsl => format!("{} {}", member.type_name, member.name),
    };
    let mut markdown = code_block(document.language, &declaration);

    // If the field has a declaration in this file, its doc comment applies.
    if let Some(index) = definition::target(document, offset) {
        if let Some(doc) = doc_comment(parsed, document.text(), parsed.symbols[index].full_span)
        {
            markdown.push_str("\n\n");
            markdown.push_str(&doc);
        }
    } else if member.detail != "field" {
        markdown.push_str(&format!("\n\nVector {}.", member.detail));
    }
    Some(markdown)
}

/// A name declared in this file.
fn symbol_hover(document: &Document, offset: u32) -> Option<String> {
    let parsed = document.parsed();
    let index = definition::target(document, offset)?;
    let symbol = &parsed.symbols[index];

    let mut declaration = symbol.detail.clone();
    if declaration.is_empty() {
        declaration = symbol.name.clone();
    }

    // A WGSL `let` declares no type; naga is the only way to know it. Adding
    // the inferred type here is the difference between `let squared` and
    // `let squared: f32`.
    if let Some(inferred) = inferred_type(document, offset, &symbol.name) {
        if !declaration.contains(&inferred) {
            declaration = match document.language {
                Language::Wgsl => format!("{declaration}: {inferred}"),
                Language::Glsl => format!("{declaration}  // {inferred}"),
            };
        }
    }

    let mut markdown = code_block(document.language, &declaration);
    if let Some(doc) = doc_comment(parsed, document.text(), symbol.full_span) {
        markdown.push_str("\n\n");
        markdown.push_str(&doc);
    }
    Some(markdown)
}

/// A name the language predefines.
fn builtin_hover(document: &Document, name: &str) -> Option<String> {
    let language = document.language;
    let builtin = builtins::function(language, name).or_else(|| builtins::variable(language, name));

    if let Some(builtin) = builtin {
        let mut markdown = code_block(language, builtin.signature);
        if !builtin.doc.is_empty() {
            markdown.push_str("\n\n");
            markdown.push_str(builtin.doc);
        }
        return Some(markdown);
    }

    if builtins::is_type(language, name) {
        return Some(format!(
            "{}\n\nA built-in {} type.",
            code_block(language, name),
            language.label()
        ));
    }
    None
}

/// The type naga inferred for a name, if it inferred one.
fn inferred_type(document: &Document, offset: u32, name: &str) -> Option<String> {
    let module = document.module()?;
    let function = document.naga_function_at(&module, offset);
    let resolution = types::type_of_name(&module, function, name)?;
    Some(types::render_resolution(&module, &resolution, document.language))
}

/// A fenced block tagged with the document's language, so the client applies
/// the same grammar the editor does.
fn code_block(language: Language, code: &str) -> String {
    format!("```{}\n{code}\n```", language.id())
}
