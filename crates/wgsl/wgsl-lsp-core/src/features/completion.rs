//! `textDocument/completion`.
//!
//! The old in-process provider ignored the cursor entirely and returned one
//! flat word list. Everything interesting here is the opposite of that: what
//! is offered depends on where the cursor is, and the list is ordered so the
//! thing you meant is at the top.
//!
//! Context is read off the token stream, so it works on a file naga cannot
//! parse. naga is consulted for one thing only — the type of the value before
//! a `.` — and when it has no answer the fall-back is every field name in the
//! file, which is weaker but not nothing.

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse, Documentation,
    MarkupContent, MarkupKind,
};
use wgsl_syntax::builtins::{self, glsl, wgsl};
use wgsl_syntax::lexer::TokenKind;
use wgsl_syntax::{Language, Parsed, SymbolKind};

use crate::Server;
use crate::analysis::types;
use crate::features::{completion_chain, lsp_completion_kind};
use crate::state::Document;

/// Characters that make the client ask without the user pressing anything.
///
/// `(` is deliberately absent: signature help owns that position, and popping
/// a completion list over it fights with the parameter hints.
pub const TRIGGER_CHARACTERS: [&str; 4] = [".", "@", "#", "<"];

/// Sort buckets. LSP sorts `sort_text` lexically, so the prefix is the rank.
mod rank {
    pub const MEMBER: &str = "0";
    pub const LOCAL: &str = "1";
    pub const FILE: &str = "2";
    pub const BUILTIN: &str = "3";
    pub const KEYWORD: &str = "4";
}

/// Where the cursor is, and therefore what makes sense to offer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Context {
    /// After a `.`, or part-way through the member that follows one.
    Member,
    /// After `@`, naming a WGSL attribute.
    Attribute,
    /// Inside `@name(…)`.
    AttributeArgument(String),
    /// Inside `var<…>`.
    AddressSpace,
    /// After a `#` at the start of a GLSL line.
    Directive,
    /// Inside `layout(…)`.
    LayoutQualifier,
    /// After `:` or `->`, where only a type is legal.
    Type,
    /// Anywhere else.
    General,
}

impl Server {
    pub fn completion(&mut self, params: CompletionParams) -> Option<CompletionResponse> {
        let position = params.text_document_position;
        let (document, offset) = self.locate(&position.text_document.uri, position.position)?;
        if !self.settings().for_language(document.language).completion.enabled {
            return None;
        }

        let items = match context(document, offset) {
            Context::Member => members(document, offset),
            Context::Attribute => attributes(document.language),
            Context::AttributeArgument(name) => attribute_arguments(&name),
            Context::AddressSpace => address_spaces(),
            Context::Directive => directives(),
            Context::LayoutQualifier => layout_qualifiers(),
            Context::Type => types_only(document),
            Context::General => general(document, offset),
        };

        Some(CompletionResponse::Array(items))
    }
}

// ── Context detection ──────────────────────────────────────────────────────

fn context(document: &Document, offset: u32) -> Context {
    let parsed = document.parsed();
    let language = document.language;
    let Some(previous) = parsed.token_before(offset) else {
        return Context::General;
    };
    let previous_text = document.slice(previous.span);

    // Mid-word: the word under the cursor is what is being completed, so the
    // context is whatever precedes *it*.
    let anchor = if matches!(previous.kind, TokenKind::Ident | TokenKind::Type)
        && previous.span.contains(offset)
    {
        previous.span.start
    } else {
        offset
    };

    if previous_text == "." || is_member_position(parsed, document.text(), anchor) {
        return Context::Member;
    }
    if language == Language::Wgsl && previous.kind == TokenKind::Attribute {
        return Context::Attribute;
    }
    if language == Language::Glsl && previous.kind == TokenKind::Preprocessor {
        return Context::Directive;
    }

    if let Some(open) = enclosing_paren(document, offset) {
        match parsed.token_before(open) {
            Some(token) if token.kind == TokenKind::Attribute => {
                let name = document.slice(token.span).trim_start_matches('@').to_string();
                return Context::AttributeArgument(name);
            }
            Some(token) if document.slice(token.span) == "layout" => {
                return Context::LayoutQualifier;
            }
            _ => {}
        }
    }

    if language == Language::Wgsl {
        if in_var_template(document, offset) {
            return Context::AddressSpace;
        }
        if previous_text == ":" || is_return_arrow(document, previous.span.start) {
            return Context::Type;
        }
    }

    Context::General
}

/// Whether a `.` sits immediately before the word being completed.
fn is_member_position(parsed: &Parsed, source: &str, anchor: u32) -> bool {
    parsed
        .token_before(anchor)
        .is_some_and(|token| parsed.text(source, token.span) == ".")
}

/// The offset of the `(` opening the innermost parenthesised group around
/// `offset`.
fn enclosing_paren(document: &Document, offset: u32) -> Option<u32> {
    document
        .parsed()
        .blocks
        .iter()
        .filter(|block| {
            block.kind == wgsl_syntax::BlockKind::Paren && block.span.contains(offset)
        })
        .min_by_key(|block| block.span.len())
        .map(|block| block.span.start)
}

/// Whether the cursor is between `var<` and its `>`.
///
/// Angle brackets are not paired by the parser — they are comparisons as often
/// as templates — so this looks back for the opening `<` by hand and gives up
/// at any token that cannot appear inside a `var<…>`.
fn in_var_template(document: &Document, offset: u32) -> bool {
    let parsed = document.parsed();
    let mut end = offset;
    for _ in 0..8 {
        let Some(token) = parsed.token_before(end) else {
            return false;
        };
        match document.slice(token.span) {
            "<" => {
                return parsed
                    .token_before(token.span.start)
                    .is_some_and(|before| document.slice(before.span) == "var");
            }
            "," | "read" | "write" | "read_write" => end = token.span.start,
            _ if token.kind == TokenKind::Ident => end = token.span.start,
            _ => return false,
        }
    }
    false
}

/// Whether the token ending at `start` is the `>` of a `->`.
fn is_return_arrow(document: &Document, start: u32) -> bool {
    let parsed = document.parsed();
    parsed
        .token_before(start + 1)
        .is_some_and(|token| document.slice(token.span) == ">")
        && parsed
            .token_before(start)
            .is_some_and(|token| document.slice(token.span) == "-")
}

// ── Item builders ──────────────────────────────────────────────────────────

fn item(label: &str, kind: CompletionItemKind, detail: String, rank: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: (!detail.is_empty()).then_some(detail),
        sort_text: Some(format!("{rank}{label}")),
        ..CompletionItem::default()
    }
}

fn documented(mut item: CompletionItem, doc: &str) -> CompletionItem {
    if !doc.is_empty() {
        item.documentation = Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc.to_string(),
        }));
    }
    item
}

/// What follows a `.`.
fn members(document: &Document, offset: u32) -> Vec<CompletionItem> {
    let parsed = document.parsed();

    if let Some(module) = document.module() {
        if let Some((base, fields)) = completion_chain(parsed, document.text(), offset) {
            let function = document.naga_function_at(&module, offset);
            if let Some(resolution) =
                types::type_of_chain(&module, function, base, &fields)
            {
                let members =
                    types::members(&module, resolution.inner_with(&module.types), document.language);
                if !members.is_empty() {
                    return members
                        .into_iter()
                        .map(|member| {
                            item(
                                &member.name,
                                match member.detail {
                                    "field" => CompletionItemKind::FIELD,
                                    _ => CompletionItemKind::PROPERTY,
                                },
                                member.type_name,
                                rank::MEMBER,
                            )
                        })
                        .collect();
                }
            }
        }
    }

    // naga could not type the base — mid-edit, or a dialect it does not
    // implement. Every field declared in the file is a weaker answer than the
    // right struct's fields, and a much better one than nothing.
    parsed
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Field)
        .map(|symbol| {
            item(&symbol.name, CompletionItemKind::FIELD, symbol.detail.clone(), rank::MEMBER)
        })
        .collect()
}

fn attributes(language: Language) -> Vec<CompletionItem> {
    if language != Language::Wgsl {
        return Vec::new();
    }
    wgsl::ATTRIBUTES
        .iter()
        .map(|attribute| {
            documented(
                item(
                    attribute.name,
                    CompletionItemKind::KEYWORD,
                    attribute.signature.to_string(),
                    rank::BUILTIN,
                ),
                attribute.doc,
            )
        })
        .collect()
}

fn attribute_arguments(name: &str) -> Vec<CompletionItem> {
    match name {
        "builtin" => wgsl::BUILTIN_VALUES
            .iter()
            .map(|value| {
                documented(
                    item(
                        value.name,
                        CompletionItemKind::ENUM_MEMBER,
                        value.signature.to_string(),
                        rank::BUILTIN,
                    ),
                    value.doc,
                )
            })
            .collect(),
        "interpolate" => wgsl::INTERPOLATE_ARGS
            .iter()
            .map(|arg| {
                item(arg, CompletionItemKind::ENUM_MEMBER, String::new(), rank::BUILTIN)
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn address_spaces() -> Vec<CompletionItem> {
    wgsl::ADDRESS_SPACES
        .iter()
        .map(|space| {
            documented(
                item(
                    space.name,
                    CompletionItemKind::KEYWORD,
                    space.signature.to_string(),
                    rank::BUILTIN,
                ),
                space.doc,
            )
        })
        .chain(wgsl::ACCESS_MODES.iter().map(|mode| {
            item(mode, CompletionItemKind::KEYWORD, "access mode".to_string(), rank::BUILTIN)
        }))
        .collect()
}

fn directives() -> Vec<CompletionItem> {
    glsl::DIRECTIVES
        .iter()
        .map(|directive| {
            item(
                directive,
                CompletionItemKind::KEYWORD,
                "preprocessor directive".to_string(),
                rank::BUILTIN,
            )
        })
        .collect()
}

fn layout_qualifiers() -> Vec<CompletionItem> {
    glsl::LAYOUT_QUALIFIERS
        .iter()
        .map(|qualifier| {
            item(
                qualifier,
                CompletionItemKind::PROPERTY,
                "layout qualifier".to_string(),
                rank::BUILTIN,
            )
        })
        .collect()
}

/// Positions where only a type name is legal.
fn types_only(document: &Document) -> Vec<CompletionItem> {
    let language = document.language;
    let mut items: Vec<CompletionItem> = builtins::type_names(language)
        .into_iter()
        .map(|name| {
            item(name, CompletionItemKind::STRUCT, "built-in type".to_string(), rank::BUILTIN)
        })
        .collect();

    for symbol in &document.parsed().symbols {
        if matches!(symbol.kind, SymbolKind::Struct | SymbolKind::TypeAlias) {
            items.push(item(
                &symbol.name,
                lsp_completion_kind(symbol.kind),
                symbol.detail.clone(),
                rank::FILE,
            ));
        }
    }
    items
}

/// Everywhere else: what is in scope, then the file, then the language.
fn general(document: &Document, offset: u32) -> Vec<CompletionItem> {
    let language = document.language;
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

    for builtin in builtins::functions(language) {
        items.push(documented(
            item(
                builtin.name,
                CompletionItemKind::FUNCTION,
                builtin.signature.to_string(),
                rank::BUILTIN,
            ),
            builtin.doc,
        ));
    }
    for builtin in builtins::variables(language) {
        items.push(documented(
            item(
                builtin.name,
                CompletionItemKind::VARIABLE,
                builtin.signature.to_string(),
                rank::BUILTIN,
            ),
            builtin.doc,
        ));
    }
    for name in builtins::type_names(language) {
        items.push(item(
            name,
            CompletionItemKind::STRUCT,
            "built-in type".to_string(),
            rank::BUILTIN,
        ));
    }
    for keyword in builtins::keywords(language) {
        items.push(item(keyword, CompletionItemKind::KEYWORD, String::new(), rank::KEYWORD));
    }

    items
}
