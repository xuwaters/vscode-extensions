//! One module per request.
//!
//! Every handler is an inherent method on [`crate::Server`], so
//! [`crate::dispatch`] stays a flat table and a feature can be read start to
//! finish in one file.

pub mod code_actions;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod folding;
pub mod formatting;
pub mod hover;
pub mod inlay_hints;
pub mod lifecycle;
pub mod references;
pub mod rename;
pub mod semantic_tokens;
pub mod shader_info;
pub mod signature_help;
pub mod symbols;

use analyzer_core::spans::ByteSpan;
use wgsl_syntax::{Parsed, SymbolKind};

/// The dotted chain the cursor sits in, as `(base, fields)`.
///
/// `camera.view.x` with the cursor on `x` yields `("camera", ["view"])` — the
/// base to start a type lookup from, and the fields to walk before reaching
/// the cursor. Returns `None` when the cursor is not on a member access.
///
/// Shared by completion, hover and definition, which all need the same answer
/// and used to be three subtly different implementations of it.
pub fn member_chain<'a>(
    parsed: &Parsed,
    source: &'a str,
    offset: u32,
) -> Option<(&'a str, Vec<&'a str>)> {
    // Walk back over `name .` pairs from the token before the cursor.
    let mut names: Vec<&'a str> = Vec::new();
    let mut end = offset;

    // Every exit is a `break`, not a `?`: running out of tokens part-way
    // means the chain started at the top of the file, not that there is none.
    while let Some(dot) = parsed.token_before(end) {
        if parsed.text(source, dot.span) != "." {
            break;
        }
        let Some(name) = parsed.token_before(dot.span.start) else { break };
        // `f(x).y` is a member access, but on a value this layer cannot name.
        if !matches!(
            name.kind,
            wgsl_syntax::lexer::TokenKind::Ident | wgsl_syntax::lexer::TokenKind::Type
        ) {
            break;
        }
        names.push(parsed.text(source, name.span));
        end = name.span.start;
    }

    // `names` came off the source backwards, and the last one pushed is the
    // base the chain starts from.
    let base = names.pop()?;
    names.reverse();
    Some((base, names))
}

/// The chain a *completion* is being requested for.
///
/// Differs from [`member_chain`] in where it starts: completion is triggered
/// with the cursor after a `.`, or part-way through the member being typed,
/// so the partial word under the cursor is not part of the chain.
pub fn completion_chain<'a>(
    parsed: &Parsed,
    source: &'a str,
    offset: u32,
) -> Option<(&'a str, Vec<&'a str>)> {
    let token = parsed.token_before(offset)?;
    let start = if parsed.text(source, token.span) == "." {
        offset
    } else {
        // Mid-word: rewind to just before the partial member.
        token.span.start
    };
    member_chain(parsed, source, start)
}

/// How a symbol kind shows up in an outline.
pub fn lsp_symbol_kind(kind: SymbolKind) -> lsp_types::SymbolKind {
    use lsp_types::SymbolKind as Lsp;
    match kind {
        SymbolKind::Function => Lsp::FUNCTION,
        // An entry point is the thing a reader is looking for in the outline,
        // and a distinct icon is how they find it.
        SymbolKind::EntryPoint => Lsp::METHOD,
        SymbolKind::Struct => Lsp::STRUCT,
        SymbolKind::Field => Lsp::FIELD,
        SymbolKind::Variable => Lsp::VARIABLE,
        SymbolKind::Constant => Lsp::CONSTANT,
        SymbolKind::Parameter => Lsp::VARIABLE,
        SymbolKind::Local => Lsp::VARIABLE,
        SymbolKind::TypeAlias => Lsp::TYPE_PARAMETER,
        SymbolKind::Macro => Lsp::CONSTANT,
        SymbolKind::Block => Lsp::INTERFACE,
    }
}

/// How a symbol kind shows up in a completion list.
pub fn lsp_completion_kind(kind: SymbolKind) -> lsp_types::CompletionItemKind {
    use lsp_types::CompletionItemKind as Lsp;
    match kind {
        SymbolKind::Function | SymbolKind::EntryPoint => Lsp::FUNCTION,
        SymbolKind::Struct | SymbolKind::Block => Lsp::STRUCT,
        SymbolKind::Field => Lsp::FIELD,
        SymbolKind::Variable | SymbolKind::Local => Lsp::VARIABLE,
        SymbolKind::Constant => Lsp::CONSTANT,
        SymbolKind::Parameter => Lsp::VARIABLE,
        SymbolKind::TypeAlias => Lsp::CLASS,
        SymbolKind::Macro => Lsp::CONSTANT,
    }
}

/// The run of `//` comments immediately above a declaration, as markdown.
///
/// Read off the token stream rather than out of naga's `doc_comments`, so it
/// works for both languages and for a file naga could not parse.
pub fn doc_comment(parsed: &Parsed, source: &str, declaration: ByteSpan) -> Option<String> {
    let mut lines: Vec<&str> = Vec::new();
    // The line the declaration starts on; comments must run up to it.
    let mut expected = parsed
        .tokens
        .iter()
        .find(|t| t.span.start == declaration.start)
        .map(|t| t.line)?;

    for token in parsed.tokens.iter().rev() {
        if token.span.start >= declaration.start {
            continue;
        }
        if token.kind != wgsl_syntax::lexer::TokenKind::Comment || token.line + 1 != expected {
            break;
        }
        let text = parsed.text(source, token.span);
        let Some(body) = text.strip_prefix("//") else {
            break;
        };
        lines.push(body.trim_start_matches('/').trim());
        expected = token.line;
    }

    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}
