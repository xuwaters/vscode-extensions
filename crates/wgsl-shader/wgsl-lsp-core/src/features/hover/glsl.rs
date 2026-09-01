//! The GLSL half of `textDocument/hover` (P5-03).
//!
//! Everything here comes from what the analyzer *resolved* the occurrence
//! under the cursor to, rather than from a name lookup in a flat table. That is
//! the whole difference: `texture` in a `#version 100` shader and `texture` in
//! a `#version 300 es` one are the same six letters and not the same answer.
//!
//! Four things a hover can add that the old one could not:
//!
//! - **Every overload**, filtered to the version the file declares, with the
//!   spec's own generic notation (`gvec4 texture(gsampler2D, vec2)`).
//! - **The stages** a `gl_*` variable belongs to, and the versions it exists
//!   in.
//! - **A resolved type** for a user symbol, a field or a swizzle — GLSL
//!   declares its types, but `light.colour.rg` does not declare its own.
//! - **A macro's definition**, which is not in the token stream at all.

use glsl_analysis::{Target, Type};
use glsl_spec::{BuiltinFunction, BuiltinVariable, Version};
use wgsl_syntax::Language;

use super::code_block;
use crate::features::doc_comment;
use crate::glsl::GlslDocument;
use crate::state::Document;

/// What to show over the identifier at `offset`.
pub fn hover(document: &Document, offset: u32, name: &str) -> Option<String> {
    let glsl = document.glsl()?;

    // A `#define` is not in the expanded stream, so it is looked up by name
    // rather than through a resolved reference.
    if let Some(markdown) = macro_hover(document, glsl, name) {
        return Some(markdown);
    }

    match glsl.analysis.reference_at(offset).map(|r| r.target.clone()) {
        Some(Target::Symbol(id)) => symbol(document, glsl, id, offset),
        Some(Target::BuiltinFunction(f)) => Some(function(f, glsl.version())),
        Some(Target::LegacyFunction(f)) => {
            Some(with_doc(&signatures(&f.function, glsl.version()), f.doc))
        }
        Some(Target::BuiltinVariable(v)) => Some(variable(v, glsl.version())),
        Some(Target::LegacyVariable(v)) => {
            Some(with_doc(&code_block(Language::Glsl, &v.variable.declaration()), v.doc))
        }
        Some(Target::Field { owner, index }) => field(glsl, owner, index),
        Some(Target::Type(ty)) => Some(type_hover(glsl, &ty)),
        Some(Target::Swizzle) => swizzle(glsl, offset, name),
        _ => keyword(name),
    }
}

/// A name the file declares: the declaration as written, plus its type when
/// the declaration does not already say it, plus its doc comment.
fn symbol(
    document: &Document,
    glsl: &GlslDocument,
    id: glsl_analysis::SymbolId,
    offset: u32,
) -> Option<String> {
    let symbol = glsl.analysis.symbols.get(id)?;
    // The outline's detail is the declaration the user actually wrote, array
    // suffix and qualifiers included, which is what a reader wants to see.
    let written = glsl
        .outline
        .symbols
        .iter()
        .find(|s| s.name_span == symbol.name_span)
        .map(|s| s.detail.clone())
        .filter(|detail| !detail.is_empty())
        .unwrap_or_else(|| symbol.name.clone());

    let mut declaration = written;
    let ty = glsl.type_name(&symbol.ty);
    if !symbol.ty.is_unknown() && !declaration.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| word == ty)
    {
        declaration.push_str(&format!("  // {ty}"));
    }

    let mut markdown = code_block(Language::Glsl, &declaration);
    if let Some(value) = symbol.const_value {
        markdown.push_str(&format!("\n\nA compile-time constant: `{value}`."));
    }
    if let Some(doc) =
        doc_comment(document.parsed(), document.text(), symbol.full_span)
    {
        markdown.push_str("\n\n");
        markdown.push_str(&doc);
    }
    let _ = offset;
    Some(markdown)
}

/// A builtin function: the overloads this `#version` has, then the prose.
fn function(builtin: &'static BuiltinFunction, version: Version) -> String {
    with_doc(&signatures(builtin, version), builtin.doc.text())
}

/// The overload set as a fenced block.
///
/// Filtered to the declared version — that is the point of the spec tables —
/// but never filtered to *nothing*: a name that exists here with no overload
/// this version has is better shown in full than shown empty.
fn signatures(builtin: &'static BuiltinFunction, version: Version) -> String {
    let mut lines: Vec<String> =
        builtin.overloads_in(version).map(|o| o.signature(builtin.name)).collect();
    if lines.is_empty() {
        lines = builtin.overloads.iter().map(|o| o.signature(builtin.name)).collect();
    }
    // The reference pages repeat a prototype across families; a hover showing
    // the same line twice looks like a bug.
    lines.dedup();
    let mut markdown = code_block(Language::Glsl, &lines.join("\n"));
    if !available(builtin.availability(), version) {
        markdown.push_str(&format!(
            "\n\n*Not available in GLSL {}.*",
            version.label()
        ));
    }
    markdown
}

/// A predeclared `gl_*` variable: its declaration, its prose, and where it
/// exists — which is the half a flat table could never carry.
fn variable(builtin: &'static BuiltinVariable, version: Version) -> String {
    let mut markdown =
        with_doc(&code_block(Language::Glsl, &builtin.declaration()), builtin.doc.text());
    let stages: Vec<String> = builtin.stages.stages().map(capitalise).collect();
    if !stages.is_empty() && stages.len() < glsl_spec::Stage::ALL.len() {
        markdown.push_str(&format!("\n\n*Stages: {}.*", stages.join(", ")));
    }
    if !available(builtin.availability(), version) {
        markdown.push_str(&format!("\n\n*Not available in GLSL {}.*", version.label()));
    }
    markdown
}

/// A member of a struct or an interface block.
fn field(
    glsl: &GlslDocument,
    owner: glsl_analysis::StructId,
    index: usize,
) -> Option<String> {
    let def = glsl.analysis.structs.get(owner)?;
    let field = def.fields.get(index)?;
    let ty = glsl.type_name(&field.ty);
    let mut markdown = code_block(Language::Glsl, &format!("{ty} {}", field.name));
    markdown.push_str(&format!("\n\nA member of `{}`.", def.name));
    Some(markdown)
}

/// A type specifier, or a constructor's callee.
fn type_hover(glsl: &GlslDocument, ty: &Type) -> String {
    let name = glsl.type_name(ty);
    match glsl_spec::basic_type(&name) {
        Some(basic) => format!(
            "{}\n\nA built-in GLSL {} type.",
            code_block(Language::Glsl, &name),
            kind_label(basic.kind)
        ),
        None => format!("{}\n\nA type this shader declares.", code_block(Language::Glsl, &name)),
    }
}

/// `light.colour.rg` — the components name no declaration, only a type.
fn swizzle(glsl: &GlslDocument, offset: u32, name: &str) -> Option<String> {
    let ty = glsl.type_at(offset)?;
    Some(format!(
        "{}\n\nA {}-component selection.",
        code_block(Language::Glsl, &format!("{} {name}", glsl.type_name(ty))),
        name.chars().count()
    ))
}

/// A `#define`, shown as written — the one thing the expanded token stream
/// cannot show, because the preprocessor consumed the directive.
fn macro_hover(document: &Document, glsl: &GlslDocument, name: &str) -> Option<String> {
    let def = glsl.macro_at(name)?;
    let text = document.slice(def.span).trim();
    let mut markdown = code_block(Language::Glsl, text);
    if let Some(undefined) = def.undefined_at {
        let line = document.position(undefined.start).line + 1;
        markdown.push_str(&format!("\n\nUndefined again at line {line}."));
    }
    Some(markdown)
}

/// A reserved word — what it is for, and which versions have it.
fn keyword(name: &str) -> Option<String> {
    let keyword = glsl_spec::keyword(name)?;
    let purpose = match keyword.kind {
        glsl_spec::KeywordKind::Storage => "a storage qualifier",
        glsl_spec::KeywordKind::Qualifier => "a qualifier",
        glsl_spec::KeywordKind::Control => "a control-flow keyword",
        glsl_spec::KeywordKind::Precision => "a precision qualifier",
        glsl_spec::KeywordKind::Other => "a GLSL keyword",
    };
    Some(format!("{}\n\n{purpose}.", code_block(Language::Glsl, name)))
}

fn with_doc(head: &str, doc: &str) -> String {
    if doc.is_empty() {
        return head.to_string();
    }
    format!("{head}\n\n{doc}")
}

fn available(availability: glsl_spec::Availability, version: Version) -> bool {
    // An entry with nothing recorded for this profile says "not documented
    // here", not "not in the language" — the same rule the diagnostics use.
    let covered = if version.is_es() {
        !availability.es.is_empty()
    } else {
        !availability.desktop.is_empty()
    };
    !covered || availability.contains(version)
}

fn capitalise(stage: glsl_spec::Stage) -> String {
    let label = stage.label();
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn kind_label(kind: glsl_spec::TypeKind) -> &'static str {
    match kind {
        glsl_spec::TypeKind::Void => "void",
        glsl_spec::TypeKind::Scalar => "scalar",
        glsl_spec::TypeKind::Vector => "vector",
        glsl_spec::TypeKind::Matrix => "matrix",
        glsl_spec::TypeKind::Sampler => "sampler",
        glsl_spec::TypeKind::Image => "image",
        glsl_spec::TypeKind::AtomicCounter => "atomic counter",
    }
}
