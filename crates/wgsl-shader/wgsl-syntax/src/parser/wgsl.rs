//! The WGSL walk.
//!
//! Module scope is a sequence of optionally-attributed declarations, each
//! introduced by a keyword, which makes dispatch a single `match` on the token
//! after the attributes. Anything unrecognised goes to
//! [`Cursor::recover`](super::Cursor::recover).
//!
//! WESL takes the same walk with three additions, each switched on by
//! [`Builder::language`]: `import` statements, `{ … }` groups of declarations
//! at module scope, and declarations an `@if`, `@elif` or `@else` governs.

use analyzer_core::spans::ByteSpan;

use super::{Builder, Cursor};
use crate::Language;
use crate::tree::{Import, PathRoot, SymbolKind};

/// Attributes that make a function an entry point.
const STAGE_ATTRIBUTES: [&str; 3] = ["vertex", "fragment", "compute"];

/// WESL's conditional-translation attributes.
const CONDITION_ATTRIBUTES: [&str; 3] = ["if", "elif", "else"];

pub(crate) fn parse(cursor: &mut Cursor, builder: &mut Builder) {
    let file = ByteSpan::new(0, cursor.source.len() as u32);
    while !cursor.at_end() {
        let before = cursor.pos;
        declaration(cursor, builder, file);
        // Every path must consume something, or module scope never ends.
        if cursor.pos == before {
            cursor.pos += 1;
        }
    }
}

fn declaration(cursor: &mut Cursor, builder: &mut Builder, file: ByteSpan) {
    let start = cursor.pos;
    let (keyword, attributes) = skip_attributes(cursor, start);
    let start_offset = cursor.span(start).start;
    let first_symbol = builder.symbols.len();
    let wesl = builder.language == Language::Wesl;

    match cursor.text(keyword) {
        "import" if wesl => import_statement(cursor, builder, keyword, start_offset),
        // `@if(feature) { … }` — a group of declarations sharing one
        // condition. Its members are module-scope declarations like any other.
        "{" if wesl => {
            let close = cursor.group_end(keyword).unwrap_or(cursor.toks.len());
            cursor.pos = keyword + 1;
            while cursor.pos < close {
                let before = cursor.pos;
                declaration(cursor, builder, file);
                if cursor.pos == before {
                    cursor.pos += 1;
                }
            }
            cursor.pos = close + 1;
        }
        "fn" => function(cursor, builder, keyword, start_offset, &attributes),
        "struct" => struct_declaration(cursor, builder, keyword, start_offset, file),
        "alias" => simple(cursor, builder, keyword, start_offset, file, SymbolKind::TypeAlias),
        "var" => variable(cursor, builder, keyword, start_offset, file),
        // `let` is not legal at module scope any more, but a file being ported
        // from an older WGSL still has them and still deserves an outline.
        "const" | "override" | "let" => {
            simple(cursor, builder, keyword, start_offset, file, SymbolKind::Constant)
        }
        _ => {
            cursor.pos = keyword;
            cursor.recover();
        }
    }

    if is_conditional(&attributes) {
        for index in first_symbol..builder.symbols.len() {
            if builder.symbols[index].parent.is_none() {
                builder.mark_conditional(index);
            }
        }
    }
}

/// Whether a run of attributes includes conditional translation.
fn is_conditional(attributes: &[String]) -> bool {
    attributes.iter().any(|a| CONDITION_ATTRIBUTES.contains(&a.as_str()))
}

/// Whether the statement starting at `i` carries conditional translation —
/// read backwards, since a function body is walked keyword by keyword and
/// the attributes have already been stepped over.
fn conditional_before(cursor: &Cursor, i: usize) -> bool {
    let mut j = i;
    while let Some(previous) = j.checked_sub(1) {
        let attribute = if cursor.is(previous, ")") {
            match cursor.matching.get(previous).copied().flatten() {
                Some(open) if open >= 1 => open - 1,
                _ => return false,
            }
        } else {
            previous
        };
        if cursor.kind(attribute) != Some(crate::lexer::TokenKind::Attribute) {
            return false;
        }
        if CONDITION_ATTRIBUTES.contains(&cursor.text(attribute).trim_start_matches('@')) {
            return true;
        }
        j = attribute;
    }
    false
}

/// Whether `::` starts at token `i`. It lexes as two `:`s, and only counts as
/// a path separator when nothing sits between them.
fn is_path_separator(cursor: &Cursor, i: usize) -> bool {
    cursor.is(i, ":")
        && cursor.is(i + 1, ":")
        && cursor.span(i).end == cursor.span(i + 1).start
}

/// `import [package:: | super::…] path ;`, where a path is
/// `name (:: path | :: { path, … } | as name)?`.
fn import_statement(cursor: &mut Cursor, builder: &mut Builder, keyword: usize, start_offset: u32) {
    let end = import_end(cursor, keyword + 1);
    let last = if cursor.is(end, ";") { end } else { end.saturating_sub(1).max(keyword) };
    let full_span = ByteSpan::new(start_offset, cursor.span(last).end);

    let mut i = keyword + 1;
    let mut root = PathRoot::Bare;
    let mut root_span = None;
    if cursor.is(i, "package") && is_path_separator(cursor, i + 1) {
        root = PathRoot::Package;
        root_span = Some(cursor.span(i));
        i += 3;
    } else {
        let first = i;
        let mut count = 0;
        while cursor.is(i, "super") && is_path_separator(cursor, i + 1) {
            count += 1;
            i += 3;
        }
        if count > 0 {
            root = PathRoot::Super(count);
            root_span = Some(ByteSpan::new(cursor.span(first).start, cursor.span(i - 3).end));
        }
    }

    let context = ImportContext { root, root_span, full_span };
    import_path(cursor, builder, &context, i, end, &[]);
    cursor.pos = if cursor.is(end, ";") { end + 1 } else { end.max(keyword + 1) };
}

/// What every name one import statement brings in shares.
struct ImportContext {
    root: PathRoot,
    root_span: Option<ByteSpan>,
    full_span: ByteSpan,
}

/// The `;` ending an import, or the token that starts whatever follows a
/// statement still being typed — so a missing `;` does not swallow the next
/// declaration.
fn import_end(cursor: &Cursor, from: usize) -> usize {
    let mut i = from;
    while i < cursor.toks.len() {
        if cursor.is(i, ";") {
            return i;
        }
        if cursor.is(i, "{") {
            i = cursor.past_group(i);
            continue;
        }
        let starts_declaration = cursor.kind(i) == Some(crate::lexer::TokenKind::Attribute)
            || matches!(
                cursor.text(i),
                "fn" | "struct" | "var" | "const" | "override" | "alias" | "import" | "let"
                    | "const_assert" | "enable" | "requires" | "diagnostic" | "}"
            );
        if starts_declaration {
            return i;
        }
        i += 1;
    }
    i
}

/// One path of an import, descending into `{…}` collections.
fn import_path(
    cursor: &Cursor,
    builder: &mut Builder,
    context: &ImportContext,
    at: usize,
    limit: usize,
    prefix: &[(String, ByteSpan)],
) {
    if at >= limit || !cursor.is_name(at) {
        return;
    }
    let mut segments = prefix.to_vec();
    segments.push((cursor.text(at).to_string(), cursor.span(at)));

    let next = at + 1;
    if next < limit && is_path_separator(cursor, next) {
        let after = next + 2;
        if cursor.is(after, "{") {
            let close = cursor.group_end(after).unwrap_or(limit).min(limit);
            for (from, to) in cursor.comma_chunks(after + 1, close) {
                import_path(cursor, builder, context, from, to, &segments);
            }
        } else {
            import_path(cursor, builder, context, after, limit, &segments);
        }
        return;
    }

    let alias = (cursor.is(next, "as") && cursor.is_name(next + 1))
        .then(|| (cursor.text(next + 1).to_string(), cursor.span(next + 1)));
    let (name, name_span) = alias.clone().unwrap_or_else(|| segments[segments.len() - 1].clone());

    let mut import = Import {
        root: context.root,
        root_span: context.root_span,
        segments,
        alias,
        symbol: 0,
    };
    let mut detail = format!("import {}", import.path());
    if let Some((alias, _)) = &import.alias {
        detail.push_str(&format!(" as {alias}"));
    }
    import.symbol = builder.add(
        None,
        &name,
        SymbolKind::Import,
        name_span,
        context.full_span,
        ByteSpan::new(0, cursor.source.len() as u32),
        detail,
    );
    builder.imports.push(import);
}

/// Step over a run of `@attr` / `@attr(args)`, returning the index of the
/// token that follows and the attribute names.
fn skip_attributes(cursor: &Cursor, mut i: usize) -> (usize, Vec<String>) {
    let mut names = Vec::new();
    while cursor.kind(i) == Some(crate::lexer::TokenKind::Attribute) {
        names.push(cursor.text(i).trim_start_matches('@').to_string());
        i += 1;
        if cursor.is(i, "(") {
            i = cursor.past_group(i);
        }
    }
    (i, names)
}

fn function(
    cursor: &mut Cursor,
    builder: &mut Builder,
    keyword: usize,
    start_offset: u32,
    attributes: &[String],
) {
    let name = keyword + 1;
    if !cursor.is_name(name) {
        cursor.pos = keyword + 1;
        cursor.recover();
        return;
    }

    let params_open = name + 1;
    let params_close = cursor.group_end(params_open);
    let after_params = params_close.map_or(params_open + 1, |close| close + 1);

    // The return type sits between `)` and `{`; skip to whichever comes first.
    let body_open = find_body(cursor, after_params);
    let body_close = body_open.and_then(|open| cursor.group_end(open));

    let end_offset = match (body_close, params_close) {
        (Some(close), _) => cursor.span(close).end,
        (None, Some(close)) => cursor.span(close).end,
        (None, None) => cursor.span(name).end,
    };
    let full_span = ByteSpan::new(start_offset, end_offset);
    let detail_end = body_open.map_or(end_offset, |open| cursor.span(open).start);
    let kind = if attributes.iter().any(|a| STAGE_ATTRIBUTES.contains(&a.as_str())) {
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

    cursor.pos = body_close.or(params_close).map_or(name + 1, |i| i + 1);
}

/// The `{` that opens a function body, if the declaration has one. A prototype
/// ends at `;` instead and has no body to look inside.
fn find_body(cursor: &Cursor, from: usize) -> Option<usize> {
    let mut i = from;
    while i < cursor.toks.len() {
        match cursor.text(i) {
            "{" => return Some(i),
            ";" => return None,
            _ => i += 1,
        }
    }
    None
}

/// `[@attr] name : type` repeated, comma-separated.
fn parameters(
    cursor: &mut Cursor,
    builder: &mut Builder,
    open: usize,
    close: usize,
    parent: usize,
    scope: ByteSpan,
) {
    for (from, to) in cursor.comma_chunks(open + 1, close) {
        let (name, attributes) = skip_attributes(cursor, from);
        if !cursor.is_name(name) || !cursor.is(name + 1, ":") {
            continue;
        }
        let span = ByteSpan::new(cursor.span(from).start, cursor.span(to - 1).end);
        let index = builder.add(
            Some(parent),
            cursor.text(name),
            SymbolKind::Parameter,
            cursor.span(name),
            span,
            scope,
            builder.detail(span),
        );
        if is_conditional(&attributes) {
            builder.mark_conditional(index);
        }
    }
}

fn struct_declaration(
    cursor: &mut Cursor,
    builder: &mut Builder,
    keyword: usize,
    start_offset: u32,
    file: ByteSpan,
) {
    let name = keyword + 1;
    if !cursor.is_name(name) {
        cursor.pos = keyword + 1;
        cursor.recover();
        return;
    }
    let open = name + 1;
    let close = cursor.group_end(open);
    let end_offset = close.map_or(cursor.span(name).end, |c| cursor.span(c).end);
    let full_span = ByteSpan::new(start_offset, end_offset);

    let index = builder.add(
        None,
        cursor.text(name),
        SymbolKind::Struct,
        cursor.span(name),
        full_span,
        file,
        builder.detail(ByteSpan::new(start_offset, cursor.span(name).end)),
    );

    if let Some(close) = close {
        for (from, to) in cursor.comma_chunks(open + 1, close) {
            let (field, attributes) = skip_attributes(cursor, from);
            if !cursor.is_name(field) || !cursor.is(field + 1, ":") {
                continue;
            }
            let span = ByteSpan::new(cursor.span(from).start, cursor.span(to - 1).end);
            let member = builder.add(
                Some(index),
                cursor.text(field),
                SymbolKind::Field,
                cursor.span(field),
                span,
                full_span,
                builder.detail(span),
            );
            if is_conditional(&attributes) {
                builder.mark_conditional(member);
            }
        }
        cursor.pos = close + 1;
        // A struct may be followed by a stray `;`, which is legal and which the
        // next iteration would otherwise treat as a declaration.
        if cursor.is(cursor.pos, ";") {
            cursor.pos += 1;
        }
    } else {
        cursor.pos = open;
        cursor.recover();
    }
}

/// `var<space, access> name : type = init;`
fn variable(
    cursor: &mut Cursor,
    builder: &mut Builder,
    keyword: usize,
    start_offset: u32,
    file: ByteSpan,
) {
    let name = cursor.past_template(keyword + 1);
    declare(cursor, builder, name, start_offset, file, SymbolKind::Variable);
}

/// `keyword name …;` — the shape `alias`, `const`, `override` and `let` share.
fn simple(
    cursor: &mut Cursor,
    builder: &mut Builder,
    keyword: usize,
    start_offset: u32,
    file: ByteSpan,
    kind: SymbolKind,
) {
    declare(cursor, builder, keyword + 1, start_offset, file, kind);
}

fn declare(
    cursor: &mut Cursor,
    builder: &mut Builder,
    name: usize,
    start_offset: u32,
    scope: ByteSpan,
    kind: SymbolKind,
) {
    if !cursor.is_name(name) {
        cursor.pos = name;
        cursor.recover();
        return;
    }
    let end = cursor.statement_end(name + 1);
    let full_span = ByteSpan::new(start_offset, cursor.span(end).end);
    // The detail stops at the initialiser: `var<uniform> camera: Camera`, not
    // the forty lines of struct literal that may follow.
    let detail_end = initialiser_start(cursor, name + 1, end)
        .map_or(cursor.span(end).start, |eq| cursor.span(eq).start);
    builder.add(
        None,
        cursor.text(name),
        kind,
        cursor.span(name),
        full_span,
        scope,
        builder.detail(ByteSpan::new(start_offset, detail_end)),
    );
    cursor.pos = end + 1;
}

/// The index of the `=` introducing an initialiser, if there is one.
fn initialiser_start(cursor: &Cursor, from: usize, to: usize) -> Option<usize> {
    (from..to).find(|&i| cursor.is(i, "="))
}

/// Locals declared inside a function body.
fn body(cursor: &mut Cursor, builder: &mut Builder, open: usize, close: usize, parent: usize) {
    // Close indices of the blocks we are inside, innermost last. A local's
    // scope runs from its declaration to the end of the block holding it.
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
            // The loop variable of a `for` outlives the header but not the
            // statement, so it is declared here rather than by the arm below.
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
                local_declarations(cursor, builder, header + 1, header_close, parent, scope);
                i = header_close + 1;
            }
            "let" | "var" | "const" => {
                let scope_end = cursor.span(*enclosing.last().unwrap_or(&close)).end;
                let name = if cursor.is(i, "var") {
                    cursor.past_template(i + 1)
                } else {
                    i + 1
                };
                let scope = ByteSpan::new(cursor.span(i).start, scope_end);
                i = add_local(cursor, builder, i, name, parent, scope);
            }
            _ => i += 1,
        }
    }
}

/// Declarations inside a `for` header, which may hold more than one.
fn local_declarations(
    cursor: &mut Cursor,
    builder: &mut Builder,
    from: usize,
    to: usize,
    parent: usize,
    scope: ByteSpan,
) {
    let mut i = from;
    while i < to {
        if matches!(cursor.text(i), "let" | "var" | "const") {
            let name = if cursor.is(i, "var") { cursor.past_template(i + 1) } else { i + 1 };
            i = add_local(cursor, builder, i, name, parent, scope);
        } else {
            i += 1;
        }
    }
}

/// Record one local and return the index to continue from.
fn add_local(
    cursor: &Cursor,
    builder: &mut Builder,
    keyword: usize,
    name: usize,
    parent: usize,
    scope: ByteSpan,
) -> usize {
    if !cursor.is_name(name) {
        return keyword + 1;
    }
    let end = cursor.statement_end(name + 1);
    let detail_end = initialiser_start(cursor, name + 1, end)
        .map_or(cursor.span(end).start, |eq| cursor.span(eq).start);
    let detail = builder.detail(ByteSpan::new(cursor.span(keyword).start, detail_end));
    let index = builder.add(
        Some(parent),
        cursor.text(name),
        SymbolKind::Local,
        cursor.span(name),
        ByteSpan::new(cursor.span(keyword).start, cursor.span(end).end),
        scope,
        detail,
    );
    if conditional_before(cursor, keyword) {
        builder.mark_conditional(index);
    }
    name + 1
}
