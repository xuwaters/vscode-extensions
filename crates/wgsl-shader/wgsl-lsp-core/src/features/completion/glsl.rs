//! The GLSL half of `textDocument/completion` (P5-04).
//!
//! Two things the old flat word list could not do, and both are the point:
//!
//! - **Members come from a type.** `light.` offers `colour` and `intensity`
//!   because analysis typed `light` as `Light`; `base.` offers swizzles
//!   because it typed `base` as a `vec4`. When it could not type the base —
//!   mid-edit, which is when completion is always asked — the fall-back is
//!   every field name in the file, which is weaker and not nothing.
//! - **Everything else is filtered by the dialect.** A `#version 300 es`
//!   shader is not offered `texture1D`, `dmat4` or `gl_ClipVertex`; a
//!   `#version 110` one is not offered `texture` or `gl_VertexID`. Stage
//!   filtering only applies when the stage was *declared*, since a guessed
//!   stage must never hide a name.

use lsp_types::{CompletionItem, CompletionItemKind};

use glsl_analysis::{Scalar, Type};
use glsl_spec::{Availability, Predeclared};
use wgsl_syntax::SymbolKind;

use super::{documented, item, rank};
use crate::features::lsp_completion_kind;
use crate::glsl::GlslDocument;
use crate::state::Document;

/// Directive names, without the `#`. The set
/// [`glsl_syntax::preprocessor`] implements.
const DIRECTIVES: [&str; 14] = [
    "version", "define", "undef", "if", "ifdef", "ifndef", "else", "elif", "endif",
    "error", "pragma", "extension", "line", "include",
];

/// The `layout(…)` keys, GLSL 4.60 §4.4 plus the ES subset. Not in
/// `glsl-spec`: the reference pages document builtins, and these are grammar.
const LAYOUT_QUALIFIERS: [&str; 44] = [
    "location", "binding", "set", "offset", "align", "component", "index",
    "input_attachment_index", "push_constant", "constant_id", "std140", "std430",
    "packed", "shared", "row_major", "column_major", "local_size_x", "local_size_y",
    "local_size_z", "points", "lines", "lines_adjacency", "triangles",
    "triangles_adjacency", "line_strip", "triangle_strip", "max_vertices",
    "invocations", "stream", "vertices", "quads", "isolines", "equal_spacing",
    "fractional_even_spacing", "fractional_odd_spacing", "cw", "ccw", "point_mode",
    "origin_upper_left", "pixel_center_integer", "early_fragment_tests", "depth_any",
    "depth_greater", "depth_unchanged",
];

pub fn directives() -> Vec<CompletionItem> {
    DIRECTIVES
        .iter()
        .map(|name| {
            item(
                name,
                CompletionItemKind::KEYWORD,
                "preprocessor directive".to_string(),
                rank::BUILTIN,
            )
        })
        .collect()
}

pub fn layout_qualifiers() -> Vec<CompletionItem> {
    LAYOUT_QUALIFIERS
        .iter()
        .map(|name| {
            item(name, CompletionItemKind::PROPERTY, "layout qualifier".to_string(), rank::BUILTIN)
        })
        .collect()
}

/// What follows a `.`.
pub fn members(document: &Document, offset: u32) -> Vec<CompletionItem> {
    let Some(glsl) = document.glsl() else {
        return Vec::new();
    };
    if let Some(ty) = base_type(document, glsl, offset) {
        let items = members_of(glsl, &ty);
        if !items.is_empty() {
            return items;
        }
    }
    // Nothing could type the base. Every field declared in the file is a
    // weaker answer than the right struct's, and a much better one than none.
    document
        .parsed()
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Field)
        .map(|symbol| {
            item(&symbol.name, CompletionItemKind::FIELD, symbol.detail.clone(), rank::MEMBER)
        })
        .collect()
}

/// The type of the expression the `.` before `offset` selects from.
fn base_type(document: &Document, glsl: &GlslDocument, offset: u32) -> Option<Type> {
    let parsed = document.parsed();
    // Completion arrives either just after the `.` or part-way through the
    // member being typed; in the second case the `.` is one token further back.
    let token = parsed.token_before(offset)?;
    let dot = if document.slice(token.span) == "." {
        token
    } else {
        parsed.token_before(token.span.start)?
    };
    if document.slice(dot.span) != "." {
        return None;
    }
    let base = parsed.token_before(dot.span.start)?;
    glsl.type_at(base.span.end.saturating_sub(1)).cloned()
}

/// What a type offers after a `.`: a struct's fields, a vector's components,
/// or an array's one method.
fn members_of(glsl: &GlslDocument, ty: &Type) -> Vec<CompletionItem> {
    match ty {
        Type::Struct(id) => match glsl.analysis.structs.get(*id) {
            Some(def) => def
                .fields
                .iter()
                .map(|field| {
                    item(
                        &field.name,
                        CompletionItemKind::FIELD,
                        glsl.type_name(&field.ty),
                        rank::MEMBER,
                    )
                })
                .collect(),
            None => Vec::new(),
        },
        Type::Vector(scalar, size) => swizzles(*scalar, *size),
        // `a.length()` is the one thing an array answers.
        Type::Array(..) => vec![item(
            "length",
            CompletionItemKind::METHOD,
            "int length()".to_string(),
            rank::MEMBER,
        )],
        _ => Vec::new(),
    }
}

/// The component sets a vector answers to, single components then prefixes.
///
/// GLSL has three interchangeable spellings and forbids mixing them, so they
/// are offered as three runs rather than one shuffled list.
fn swizzles(scalar: Scalar, size: u8) -> Vec<CompletionItem> {
    const SETS: [&str; 3] = ["xyzw", "rgba", "stpq"];
    let mut items = Vec::new();
    for set in SETS {
        let letters: Vec<char> = set.chars().take(size as usize).collect();
        for (index, letter) in letters.iter().enumerate() {
            let detail = scalar.name().to_string();
            items.push(item(
                &letter.to_string(),
                CompletionItemKind::PROPERTY,
                detail,
                rank::MEMBER,
            ));
            // The prefixes — `xy`, `xyz` — which are the selections a reader
            // reaches for and the ones that are always legal.
            if index >= 1 {
                let prefix: String = letters[..=index].iter().collect();
                let ty = Type::Vector(scalar, index as u8 + 1);
                items.push(item(
                    &prefix,
                    CompletionItemKind::PROPERTY,
                    ty.name(&glsl_analysis::StructTable::default()),
                    rank::MEMBER,
                ));
            }
        }
    }
    items
}

/// Positions where only a type name is legal.
pub fn types_only(document: &Document) -> Vec<CompletionItem> {
    let Some(glsl) = document.glsl() else {
        return Vec::new();
    };
    let mut items = builtin_types(glsl);
    for symbol in &document.parsed().symbols {
        if symbol.kind == SymbolKind::Struct {
            items.push(item(
                &symbol.name,
                CompletionItemKind::STRUCT,
                symbol.detail.clone(),
                rank::FILE,
            ));
        }
    }
    items
}

/// Everywhere else: what is in scope, then the file, then the language — with
/// the language's half filtered to the dialect the file declares.
pub fn general(document: &Document, offset: u32) -> Vec<CompletionItem> {
    let Some(glsl) = document.glsl() else {
        return Vec::new();
    };
    let parsed = document.parsed();
    let mut items = Vec::new();

    for &index in &parsed.visible_at(offset) {
        let symbol = &parsed.symbols[index];
        let rank = if symbol.kind.is_local() { rank::LOCAL } else { rank::FILE };
        items.push(item(
            &symbol.name,
            lsp_completion_kind(symbol.kind),
            symbol.detail.clone(),
            rank,
        ));
    }

    let ctx = glsl.context();
    let version = ctx.version;
    for predeclared in glsl_spec::visible_in(version) {
        match predeclared {
            Predeclared::Function(function) => {
                let signature = function
                    .overloads_in(version)
                    .next()
                    .map(|o| o.signature(function.name))
                    .unwrap_or_else(|| function.name.to_string());
                items.push(documented(
                    item(function.name, CompletionItemKind::FUNCTION, signature, rank::BUILTIN),
                    function.doc.text(),
                ));
            }
            Predeclared::Variable(variable) => {
                // A stage we were *told* filters the list; a stage we guessed
                // must not hide a name the shader may legitimately use.
                if glsl.stage_known && !variable.stages.contains(ctx.stage) {
                    continue;
                }
                items.push(documented(
                    item(
                        variable.name,
                        CompletionItemKind::VARIABLE,
                        variable.declaration(),
                        rank::BUILTIN,
                    ),
                    variable.doc.text(),
                ));
            }
        }
    }

    // The compatibility and ES 1.00 surface (decision 0007), which the
    // generated tables do not carry.
    for legacy in glsl_spec::LEGACY_FUNCTIONS {
        if !ctx.available(legacy.function.availability(), legacy.compatibility) {
            continue;
        }
        let signature = legacy
            .function
            .overloads
            .first()
            .map(|o| o.signature(legacy.function.name))
            .unwrap_or_else(|| legacy.function.name.to_string());
        items.push(documented(
            item(legacy.function.name, CompletionItemKind::FUNCTION, signature, rank::BUILTIN),
            legacy.doc,
        ));
    }
    for legacy in glsl_spec::LEGACY_VARIABLES {
        if !ctx.available(legacy.variable.availability(), legacy.compatibility) {
            continue;
        }
        if glsl.stage_known
            && !legacy.variable.stages.is_empty()
            && !legacy.variable.stages.contains(ctx.stage)
        {
            continue;
        }
        items.push(documented(
            item(
                legacy.variable.name,
                CompletionItemKind::VARIABLE,
                legacy.variable.declaration(),
                rank::BUILTIN,
            ),
            legacy.doc,
        ));
    }

    items.extend(builtin_types(glsl));
    for keyword in glsl_spec::KEYWORDS {
        if !ctx.available(Availability::new(keyword.desktop, keyword.es), keyword.compatibility) {
            continue;
        }
        items.push(item(keyword.word, CompletionItemKind::KEYWORD, String::new(), rank::KEYWORD));
    }
    items
}

/// The basic types this version has.
fn builtin_types(glsl: &GlslDocument) -> Vec<CompletionItem> {
    let ctx = glsl.context();
    glsl_spec::BASIC_TYPES
        .iter()
        .filter(|ty| ctx.available(Availability::new(ty.desktop, ty.es), false))
        .map(|ty| {
            item(ty.name, CompletionItemKind::STRUCT, "built-in type".to_string(), rank::BUILTIN)
        })
        .collect()
}
