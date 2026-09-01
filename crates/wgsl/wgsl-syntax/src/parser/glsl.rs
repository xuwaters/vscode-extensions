//! The GLSL walk.
//!
//! GLSL has no declaration keyword: a global is `qualifiers type name …` and a
//! function is `type name ( … )`. What makes both findable is that two adjacent
//! identifiers cannot occur in a GLSL *expression* — so "name followed by name"
//! is a reliable declaration signal, at file scope and inside a body alike.

use analyzer_core::spans::ByteSpan;

use super::{Builder, Cursor};
use crate::lexer::TokenKind;
use crate::tree::SymbolKind;

/// Keywords that may precede the type in a declaration.
const QUALIFIERS: [&str; 26] = [
    "const", "uniform", "buffer", "shared", "attribute", "varying", "in", "out",
    "inout", "centroid", "flat", "smooth", "noperspective", "patch", "sample",
    "invariant", "precise", "coherent", "volatile", "restrict", "readonly",
    "writeonly", "highp", "mediump", "lowp", "subroutine",
];

pub(crate) fn parse(cursor: &mut Cursor, builder: &mut Builder) {
    let file = ByteSpan::new(0, cursor.source.len() as u32);
    while !cursor.at_end() {
        let before = cursor.pos;
        declaration(cursor, builder, file);
        if cursor.pos == before {
            cursor.pos += 1;
        }
    }
}

fn declaration(cursor: &mut Cursor, builder: &mut Builder, file: ByteSpan) {
    let start = cursor.pos;

    if cursor.kind(start) == Some(TokenKind::Preprocessor) {
        directive(cursor, builder, start, file);
        return;
    }
    // `precision mediump float;` declares nothing.
    if cursor.is(start, "precision") {
        cursor.pos = cursor.statement_end(start) + 1;
        return;
    }
    if cursor.is(start, "struct") {
        structure(cursor, builder, start, file);
        return;
    }

    let start_offset = cursor.span(start).start;
    let after_qualifiers = skip_qualifiers(cursor, start);

    // `uniform Camera { … } camera;` — a name immediately followed by a brace
    // is an interface block, not a variable.
    if cursor.is_name(after_qualifiers) && cursor.is(after_qualifiers + 1, "{") {
        interface_block(cursor, builder, start, after_qualifiers, file);
        return;
    }

    let Some((type_end, name)) = type_then_name(cursor, after_qualifiers) else {
        cursor.pos = start;
        cursor.recover();
        return;
    };

    if cursor.is(name + 1, "(") {
        function(cursor, builder, start_offset, name);
    } else {
        globals(cursor, builder, start, type_end, name, file, is_const(cursor, start));
    }
}

/// Step over `layout(…)` and the qualifier keywords, returning the index of
/// the type that follows.
fn skip_qualifiers(cursor: &Cursor, mut i: usize) -> usize {
    loop {
        if cursor.is(i, "layout") {
            i += 1;
            if cursor.is(i, "(") {
                i = cursor.past_group(i);
            }
            continue;
        }
        if QUALIFIERS.contains(&cursor.text(i)) {
            i += 1;
            continue;
        }
        return i;
    }
}

fn is_const(cursor: &Cursor, from: usize) -> bool {
    let mut i = from;
    while i < cursor.toks.len() {
        if cursor.is(i, "const") {
            return true;
        }
        if cursor.is(i, "layout") {
            i = cursor.past_group(i + 1);
            continue;
        }
        if !QUALIFIERS.contains(&cursor.text(i)) {
            return false;
        }
        i += 1;
    }
    false
}

/// A type — possibly `float[4]` — followed by a name.
///
/// Returns the last index of the type and the index of the name.
fn type_then_name(cursor: &Cursor, i: usize) -> Option<(usize, usize)> {
    if !cursor.is_name(i) {
        return None;
    }
    let mut j = i + 1;
    while cursor.is(j, "[") {
        j = cursor.past_group(j);
    }
    cursor.is_name(j).then(|| (j - 1, j))
}

fn function(cursor: &mut Cursor, builder: &mut Builder, start_offset: u32, name: usize) {
    let params_open = name + 1;
    let params_close = cursor.group_end(params_open);
    let after = params_close.map_or(params_open + 1, |close| close + 1);

    // `type name(…);` is a prototype: worth an outline entry, but it has no
    // body to look inside and the definition will overwrite nothing.
    let body_open = cursor.is(after, "{").then_some(after);
    let body_close = body_open.and_then(|open| cursor.group_end(open));

    let end = body_close.or(params_close).unwrap_or(name);
    let full_span = ByteSpan::new(start_offset, cursor.span(end).end);
    let detail_end = body_open.map_or(cursor.span(end).end, |open| cursor.span(open).start);
    let kind = if cursor.text(name) == "main" {
        SymbolKind::EntryPoint
    } else {
        SymbolKind::Function
    };

    let index = builder.add(
        None,
        cursor.text(name),
        kind,
        cursor.span(name),
        full_span,
        ByteSpan::new(0, cursor.source.len() as u32),
        builder.detail(ByteSpan::new(start_offset, detail_end)),
    );

    if let Some(close) = params_close {
        parameters(cursor, builder, params_open, close, index, full_span);
    }
    if let (Some(open), Some(close)) = (body_open, body_close) {
        body(cursor, builder, open, close, index);
    }

    cursor.pos = body_close.map_or_else(
        || cursor.statement_end(after) + 1,
        |close| close + 1,
    );
}

fn parameters(
    cursor: &mut Cursor,
    builder: &mut Builder,
    open: usize,
    close: usize,
    parent: usize,
    scope: ByteSpan,
) {
    for (from, to) in cursor.comma_chunks(open + 1, close) {
        let type_start = skip_qualifiers(cursor, from);
        // `void main(void)` and `float f(float)` name no parameter.
        let Some((_, name)) = type_then_name(cursor, type_start) else {
            continue;
        };
        let span = ByteSpan::new(cursor.span(from).start, cursor.span(to - 1).end);
        builder.add(
            Some(parent),
            cursor.text(name),
            SymbolKind::Parameter,
            cursor.span(name),
            span,
            scope,
            builder.detail(span),
        );
    }
}

/// `struct Name { … } instance;`
fn structure(cursor: &mut Cursor, builder: &mut Builder, keyword: usize, file: ByteSpan) {
    let name = keyword + 1;
    if !cursor.is_name(name) || !cursor.is(name + 1, "{") {
        cursor.pos = keyword;
        cursor.recover();
        return;
    }
    let open = name + 1;
    let Some(close) = cursor.group_end(open) else {
        cursor.pos = open;
        cursor.recover();
        return;
    };

    let end = cursor.statement_end(close + 1);
    let full_span = ByteSpan::new(cursor.span(keyword).start, cursor.span(end).end);
    let index = builder.add(
        None,
        cursor.text(name),
        SymbolKind::Struct,
        cursor.span(name),
        full_span,
        file,
        builder.detail(ByteSpan::new(cursor.span(keyword).start, cursor.span(name).end)),
    );

    members(cursor, builder, open, close, Some(index), full_span, SymbolKind::Field);

    // `} instance;` declares a variable of the struct type.
    if cursor.is_name(close + 1) {
        let instance = close + 1;
        builder.add(
            None,
            cursor.text(instance),
            SymbolKind::Variable,
            cursor.span(instance),
            ByteSpan::new(cursor.span(instance).start, cursor.span(end).end),
            file,
            format!("{} {}", cursor.text(name), cursor.text(instance)),
        );
    }
    cursor.pos = end + 1;
}

/// `layout(std140) uniform Camera { mat4 view; } camera;`
///
/// The block name is not a variable — it is the interface name the API binds
/// against. What the fields are reached through depends on the instance name:
/// with one, they are `camera.view`; without, GLSL puts them in global scope,
/// so they are recorded as ordinary variables.
fn interface_block(
    cursor: &mut Cursor,
    builder: &mut Builder,
    start: usize,
    name: usize,
    file: ByteSpan,
) {
    let open = name + 1;
    let Some(close) = cursor.group_end(open) else {
        cursor.pos = open;
        cursor.recover();
        return;
    };
    let end = cursor.statement_end(close + 1);
    let full_span = ByteSpan::new(cursor.span(start).start, cursor.span(end).end);
    let instance = cursor.is_name(close + 1).then_some(close + 1);

    let index = builder.add(
        None,
        cursor.text(name),
        SymbolKind::Block,
        cursor.span(name),
        full_span,
        file,
        builder.detail(ByteSpan::new(cursor.span(start).start, cursor.span(name).end)),
    );

    match instance {
        Some(instance) => {
            members(cursor, builder, open, close, Some(index), full_span, SymbolKind::Field);
            builder.add(
                None,
                cursor.text(instance),
                SymbolKind::Variable,
                cursor.span(instance),
                ByteSpan::new(cursor.span(instance).start, cursor.span(end).end),
                file,
                format!("{} {}", cursor.text(name), cursor.text(instance)),
            );
        }
        // No instance name: the members *are* the globals.
        None => members(cursor, builder, open, close, None, file, SymbolKind::Variable),
    }

    cursor.pos = end + 1;
}

/// The `type name, name2;` statements inside a struct or interface block.
fn members(
    cursor: &mut Cursor,
    builder: &mut Builder,
    open: usize,
    close: usize,
    parent: Option<usize>,
    scope: ByteSpan,
    kind: SymbolKind,
) {
    let mut i = open + 1;
    while i < close {
        let statement = cursor.statement_end_before(i, close);
        let type_start = skip_qualifiers(cursor, i);
        if let Some((type_end, name)) = type_then_name(cursor, type_start) {
            let prefix = builder.detail(ByteSpan::new(
                cursor.span(i).start,
                cursor.span(type_end).end,
            ));
            declarators(cursor, builder, name, statement, parent, scope, kind, &prefix);
        }
        i = statement + 1;
    }
}

/// Module-scope variables: `uniform float a, b[4];`
#[allow(clippy::too_many_arguments)]
fn globals(
    cursor: &mut Cursor,
    builder: &mut Builder,
    start: usize,
    type_end: usize,
    name: usize,
    file: ByteSpan,
    constant: bool,
) {
    let end = cursor.statement_end(name);
    let prefix =
        builder.detail(ByteSpan::new(cursor.span(start).start, cursor.span(type_end).end));
    let kind = if constant { SymbolKind::Constant } else { SymbolKind::Variable };
    declarators(cursor, builder, name, end, None, file, kind, &prefix);
    cursor.pos = end + 1;
}

/// One symbol per comma-separated declarator in `[from, to)`.
#[allow(clippy::too_many_arguments)]
fn declarators(
    cursor: &Cursor,
    builder: &mut Builder,
    from: usize,
    to: usize,
    parent: Option<usize>,
    scope: ByteSpan,
    kind: SymbolKind,
    prefix: &str,
) {
    for (chunk_start, chunk_end) in cursor.comma_chunks(from, to) {
        if !cursor.is_name(chunk_start) {
            continue;
        }
        // Stop the detail at the initialiser, but keep any array suffix.
        let detail_end = (chunk_start..chunk_end)
            .find(|&i| cursor.is(i, "="))
            .map_or(cursor.span(chunk_end - 1).end, |eq| cursor.span(eq).start);
        let declarator =
            builder.detail(ByteSpan::new(cursor.span(chunk_start).start, detail_end));
        builder.add(
            parent,
            cursor.text(chunk_start),
            kind,
            cursor.span(chunk_start),
            ByteSpan::new(cursor.span(chunk_start).start, cursor.span(chunk_end - 1).end),
            scope,
            format!("{prefix} {declarator}"),
        );
    }
}

fn directive(cursor: &mut Cursor, builder: &mut Builder, start: usize, file: ByteSpan) {
    let line = cursor.get(start).map(|t| t.line).unwrap_or(0);
    let mut end = start;
    while end + 1 < cursor.toks.len() && cursor.get(end + 1).map(|t| t.line) == Some(line) {
        end += 1;
    }

    // `#define NAME …` introduces a name; every other directive does not.
    let directive = cursor.text(start).trim_start_matches('#').trim();
    if directive == "define" && cursor.is_name(start + 1) {
        let name = start + 1;
        let span = ByteSpan::new(cursor.span(start).start, cursor.span(end).end);
        builder.add(
            None,
            cursor.text(name),
            SymbolKind::Macro,
            cursor.span(name),
            span,
            // A macro is in force from its definition to the end of the file.
            ByteSpan::new(cursor.span(start).start, file.end),
            builder.detail(span),
        );
    }

    cursor.pos = end + 1;
}

/// Locals declared inside a function body.
fn body(cursor: &mut Cursor, builder: &mut Builder, open: usize, close: usize, parent: usize) {
    let mut enclosing = vec![close];
    let mut i = open + 1;

    while i < close {
        match cursor.text(i) {
            "{" => {
                enclosing.push(cursor.group_end(i).unwrap_or(close));
                i += 1;
            }
            "}" => {
                if enclosing.len() > 1 {
                    enclosing.pop();
                }
                i += 1;
            }
            "for" => {
                let header = i + 1;
                let Some(header_close) = cursor.group_end(header) else {
                    i += 1;
                    continue;
                };
                let after = header_close + 1;
                let statement_close = if cursor.is(after, "{") {
                    cursor.group_end(after).unwrap_or(header_close)
                } else {
                    header_close
                };
                let scope =
                    ByteSpan::new(cursor.span(i).start, cursor.span(statement_close).end);
                local(cursor, builder, header + 1, header_close, parent, scope);
                i = header_close + 1;
            }
            _ => {
                let scope_end = cursor.span(*enclosing.last().unwrap_or(&close)).end;
                let scope = ByteSpan::new(cursor.span(i).start, scope_end);
                match local(cursor, builder, i, close, parent, scope) {
                    Some(next) => i = next,
                    None => i += 1,
                }
            }
        }
    }
}

/// Try to read a local declaration starting at `from`, returning the index to
/// continue from when one was found.
fn local(
    cursor: &Cursor,
    builder: &mut Builder,
    from: usize,
    limit: usize,
    parent: usize,
    scope: ByteSpan,
) -> Option<usize> {
    let type_start = skip_qualifiers(cursor, from);
    let (type_end, name) = type_then_name(cursor, type_start)?;
    // A name followed by `(` is a call, not a declaration — and a declaration
    // cannot be nested inside a function body anyway.
    if cursor.is(name + 1, "(") {
        return None;
    }
    let end = cursor.statement_end_before(name, limit);
    let prefix =
        builder.detail(ByteSpan::new(cursor.span(from).start, cursor.span(type_end).end));
    declarators(cursor, builder, name, end, Some(parent), scope, SymbolKind::Local, &prefix);
    Some(end + 1)
}
