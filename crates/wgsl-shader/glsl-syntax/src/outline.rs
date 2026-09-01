//! The outline: symbols, scopes and references read off the CST.
//!
//! These are the shapes `wgsl-lsp-core` already consumes for WGSL — a flat
//! symbol arena with parent/child indices, a scope span per symbol, and one
//! reference per identifier occurrence. The types live here so Phase 3 can
//! prove parity with the heuristic walk before Phase 5 swaps the server over;
//! the wiring itself is P5-01, and nothing in `wgsl-syntax` is touched.
//!
//! Scoping is the same trick the old walk used and for the same reason: every
//! symbol carries the span its name is visible over, so resolution is "the
//! visible symbol with that name whose scope is smallest" and shadowing needs
//! no scope tree.

use analyzer_core::spans::ByteSpan;

use crate::cst::{NodeId, NodeKind, SyntaxTree};
use crate::lexer::{Punct, TokenKind};
use crate::preprocessor::Preprocessed;

/// What a declaration declares. Mirrors `wgsl_syntax::tree::SymbolKind` name
/// for name, minus `TypeAlias`, which GLSL has no spelling for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// `float lambert(vec3 n, vec3 l) { … }`.
    Function,
    /// `main` — worth its own kind so the outline can lead with it.
    EntryPoint,
    Struct,
    /// A member of a struct, or of an interface block that has an instance
    /// name.
    Field,
    /// A file-scope `uniform`/`in`/`out`/`buffer`, or a block's members when
    /// the block has no instance name and they land in global scope.
    Variable,
    /// A file-scope `const`.
    Constant,
    Parameter,
    /// Anything declared inside a function body.
    Local,
    /// A `#define`.
    Macro,
    /// The interface name of a `uniform Camera { … }` — what the API binds
    /// against, not a value.
    Block,
}

impl SymbolKind {
    /// Whether the symbol lives inside a function body, and so does not belong
    /// in a document outline.
    pub fn is_local(self) -> bool {
        matches!(self, SymbolKind::Local | SymbolKind::Parameter)
    }
}

/// A declared name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// The identifier alone — an LSP selection range.
    pub name_span: ByteSpan,
    /// The whole declarator, initialiser and body included — an LSP range.
    pub full_span: ByteSpan,
    /// Where the name is visible. File-scope names get the whole file.
    pub scope: ByteSpan,
    /// The declaration as one line: `layout(location = 0) in vec3 v_normal`.
    pub detail: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

/// An identifier occurrence, declaration sites included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    pub span: ByteSpan,
    /// Whether this occurrence is a declaration's own name.
    pub is_declaration: bool,
    /// Whether a `.` immediately precedes it, making it a member or a swizzle
    /// rather than a name to resolve in scope.
    pub is_member: bool,
}

/// Everything the outline layer knows about a file.
#[derive(Debug, Clone, Default)]
pub struct Outline {
    pub symbols: Vec<Symbol>,
    /// Indices of the symbols with no parent, in source order.
    pub roots: Vec<usize>,
    pub references: Vec<Reference>,
}

impl Outline {
    /// The innermost symbol named `name` that is visible at `offset`.
    pub fn resolve(&self, name: &str, offset: u32) -> Option<usize> {
        self.symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.name == name && s.kind != SymbolKind::Field && s.scope.contains(offset)
            })
            // The smallest scope wins, which is what shadowing means.
            .min_by_key(|(_, s)| s.scope.len())
            .map(|(i, _)| i)
    }

    /// The identifier occurrence at `offset`, if the cursor is on one.
    pub fn reference_at(&self, offset: u32) -> Option<&Reference> {
        self.references.iter().find(|r| r.span.contains(offset))
    }
}

/// Read the outline off a parsed source.
pub fn outline(tree: &SyntaxTree, pp: &Preprocessed, source: &str) -> Outline {
    let file = ByteSpan::new(0, source.len() as u32);
    let mut builder = Builder { source, symbols: Vec::new(), roots: Vec::new() };

    // `#define`s come from the macro table rather than a token scan: the
    // preprocessor already knows every one, including the ones a conditional
    // switched off, and where each was written.
    for def in pp.macros.all() {
        if def.predefined || def.span.is_empty() {
            continue;
        }
        builder.add(
            None,
            def.name.clone(),
            SymbolKind::Macro,
            def.name_span,
            def.span,
            // A macro is in force from its definition to the end of the file.
            ByteSpan::new(def.span.start, file.end),
            builder.collapse(def.span),
        );
    }

    for child in tree.child_nodes(tree.root()) {
        match tree.kind(child) {
            NodeKind::Declaration => {
                declaration(&mut builder, tree, child, None, file, false);
            }
            NodeKind::FunctionDecl => function(&mut builder, tree, child, file),
            NodeKind::InterfaceBlock => interface_block(&mut builder, tree, child, file),
            _ => {}
        }
    }

    builder.roots.sort_by_key(|&i| builder.symbols[i].full_span.start);
    let references = references(pp, source, &builder.symbols);
    Outline { symbols: builder.symbols, roots: builder.roots, references }
}

/// A file-scope or member declaration: `layout(…) uniform vec3 a, b[2];`.
///
/// `local` switches the kind to [`SymbolKind::Local`] and is what a body
/// declaration passes; `scope` is where the declared names are visible.
fn declaration(
    b: &mut Builder,
    tree: &SyntaxTree,
    node: NodeId,
    parent: Option<usize>,
    scope: ByteSpan,
    local: bool,
) {
    let type_spec = tree.child_of_kind(node, NodeKind::TypeSpec);
    let struct_spec = type_spec.and_then(|t| tree.child_of_kind(t, NodeKind::StructSpec));

    // `struct S { … } s;` declares the struct as well as the variable, and the
    // variable's type is spelled `S`, not the whole body.
    let mut prefix = match type_spec {
        Some(type_spec) => {
            b.collapse(ByteSpan::new(tree.span(node).start, tree.span(type_spec).end))
        }
        None => String::new(),
    };
    if let Some(struct_spec) = struct_spec {
        let index = structure(b, tree, struct_spec, tree.span(node), scope);
        prefix = match index.map(|i| b.symbols[i].name.clone()) {
            Some(name) => name,
            None => "struct".to_string(),
        };
    }

    let kind = if local {
        SymbolKind::Local
    } else if is_const(b, tree, node) {
        SymbolKind::Constant
    } else {
        SymbolKind::Variable
    };
    declarators(b, tree, node, parent, scope, kind, &prefix);
}

/// One symbol per [`NodeKind::Declarator`] under `node`.
#[allow(clippy::too_many_arguments)]
fn declarators(
    b: &mut Builder,
    tree: &SyntaxTree,
    node: NodeId,
    parent: Option<usize>,
    scope: ByteSpan,
    kind: SymbolKind,
    prefix: &str,
) {
    for declarator in tree.child_nodes(node) {
        if tree.kind(declarator) != NodeKind::Declarator {
            continue;
        }
        let Some((name, name_span)) = b.name_of(tree, declarator) else {
            continue;
        };
        // The detail keeps any array suffix and stops at the initialiser.
        let detail_end = tree
            .child_of_kind(declarator, NodeKind::Initializer)
            .map_or(tree.span(declarator).end, |i| tree.span(i).start);
        let written =
            b.collapse(ByteSpan::new(tree.span(declarator).start, detail_end));
        let detail =
            if prefix.is_empty() { written.clone() } else { format!("{prefix} {written}") };
        b.add(parent, name, kind, name_span, tree.span(declarator), scope, detail);
    }
}

/// `struct Name { … }` — the struct symbol and its fields. Returns the index.
fn structure(
    b: &mut Builder,
    tree: &SyntaxTree,
    node: NodeId,
    full_span: ByteSpan,
    scope: ByteSpan,
) -> Option<usize> {
    let (name, name_span) = b.name_of(tree, node)?;
    let detail = b.collapse(ByteSpan::new(tree.span(node).start, name_span.end));
    let index =
        b.add(None, name, SymbolKind::Struct, name_span, full_span, scope, detail);
    if let Some(fields) = tree.child_of_kind(node, NodeKind::FieldList) {
        members(b, tree, fields, Some(index), full_span, SymbolKind::Field);
    }
    Some(index)
}

/// The `type name, name2;` lines inside a struct or an interface block.
fn members(
    b: &mut Builder,
    tree: &SyntaxTree,
    fields: NodeId,
    parent: Option<usize>,
    scope: ByteSpan,
    kind: SymbolKind,
) {
    for field in tree.child_nodes(fields) {
        if tree.kind(field) != NodeKind::FieldDecl {
            continue;
        }
        let prefix = match tree.child_of_kind(field, NodeKind::TypeSpec) {
            Some(type_spec) => {
                b.collapse(ByteSpan::new(tree.span(field).start, tree.span(type_spec).end))
            }
            None => String::new(),
        };
        declarators(b, tree, field, parent, scope, kind, &prefix);
    }
}

/// `layout(std140) uniform Camera { … } camera;`
///
/// The block name is not a variable — it is the interface name the API binds
/// against. What the members are reached through depends on the instance name:
/// with one they are `camera.view`, without one GLSL puts them in global scope,
/// so they become ordinary variables.
fn interface_block(b: &mut Builder, tree: &SyntaxTree, node: NodeId, file: ByteSpan) {
    let Some((name, name_span)) = b.name_of(tree, node) else {
        return;
    };
    let full_span = tree.span(node);
    let detail = b.collapse(ByteSpan::new(full_span.start, name_span.end));
    let index =
        b.add(None, name.clone(), SymbolKind::Block, name_span, full_span, file, detail);

    let fields = tree.child_of_kind(node, NodeKind::FieldList);
    let instance = tree.child_of_kind(node, NodeKind::Declarator);
    match instance {
        Some(instance) => {
            if let Some(fields) = fields {
                members(b, tree, fields, Some(index), full_span, SymbolKind::Field);
            }
            if let Some((instance_name, instance_span)) = b.name_of(tree, instance) {
                let detail = format!("{name} {instance_name}");
                b.add(
                    None,
                    instance_name,
                    SymbolKind::Variable,
                    instance_span,
                    ByteSpan::new(tree.span(instance).start, full_span.end),
                    file,
                    detail,
                );
            }
        }
        // No instance name: the members *are* the globals.
        None => {
            if let Some(fields) = fields {
                members(b, tree, fields, None, file, SymbolKind::Variable);
            }
        }
    }
}

/// A function prototype or definition, its parameters, and its locals.
fn function(b: &mut Builder, tree: &SyntaxTree, node: NodeId, file: ByteSpan) {
    let Some((name, name_span)) = b.name_of(tree, node) else {
        return;
    };
    let full_span = tree.span(node);
    let body = tree.child_of_kind(node, NodeKind::CompoundStmt);
    let detail_end = match body {
        Some(body) => tree.span(body).start,
        None => tree
            .child_of_kind(node, NodeKind::ParameterList)
            .map_or(full_span.end, |p| tree.span(p).end),
    };
    let kind =
        if name == "main" { SymbolKind::EntryPoint } else { SymbolKind::Function };
    let detail = b.collapse(ByteSpan::new(full_span.start, detail_end));
    let index = b.add(None, name, kind, name_span, full_span, file, detail);

    if let Some(params) = tree.child_of_kind(node, NodeKind::ParameterList) {
        for param in tree.child_nodes(params) {
            if tree.kind(param) != NodeKind::Parameter {
                continue;
            }
            let Some(declarator) = tree.child_of_kind(param, NodeKind::Declarator) else {
                // `void main(void)` and `float f(float)` name no parameter.
                continue;
            };
            let Some((param_name, param_span)) = b.name_of(tree, declarator) else {
                continue;
            };
            let span = tree.span(param);
            let detail = b.collapse(span);
            b.add(
                Some(index),
                param_name,
                SymbolKind::Parameter,
                param_span,
                span,
                full_span,
                detail,
            );
        }
    }

    if let Some(body) = body {
        locals(b, tree, body, index, tree.span(body));
    }
}

/// Walk a function body, recording every declaration it makes.
///
/// `block` is the enclosing braces, whose end is where a name declared here
/// stops being visible.
fn locals(
    b: &mut Builder,
    tree: &SyntaxTree,
    node: NodeId,
    function: usize,
    block: ByteSpan,
) {
    for child in tree.child_nodes(node) {
        match tree.kind(child) {
            NodeKind::CompoundStmt => locals(b, tree, child, function, tree.span(child)),
            // A `for` header's names are visible over the whole loop, its body
            // included, and from the `for` keyword rather than from the type.
            NodeKind::ForStmt => {
                let loop_span = tree.span(child);
                for part in tree.child_nodes(child) {
                    match tree.kind(part) {
                        NodeKind::DeclStmt => {
                            declare_locals(b, tree, part, function, loop_span, loop_span)
                        }
                        NodeKind::CompoundStmt => {
                            locals(b, tree, part, function, tree.span(part))
                        }
                        _ => locals(b, tree, part, function, loop_span),
                    }
                }
            }
            NodeKind::DeclStmt => {
                declare_locals(b, tree, child, function, block, tree.span(child))
            }
            _ => locals(b, tree, child, function, block),
        }
    }
}

/// One body declaration. `visible` is the span the names it declares live over.
fn declare_locals(
    b: &mut Builder,
    tree: &SyntaxTree,
    node: NodeId,
    function: usize,
    block: ByteSpan,
    from: ByteSpan,
) {
    let scope = ByteSpan::new(from.start.min(block.end), block.end.max(from.end));
    for child in tree.child_nodes(node) {
        if tree.kind(child) == NodeKind::Declaration {
            declaration(b, tree, child, Some(function), scope, true);
        }
    }
}

/// Whether a declaration's qualifiers include `const`.
fn is_const(b: &Builder, tree: &SyntaxTree, node: NodeId) -> bool {
    let Some(qualifiers) = tree.child_of_kind(node, NodeKind::QualifierList) else {
        return false;
    };
    let span = tree.span(qualifiers);
    let text = b.source.get(span.start as usize..span.end as usize).unwrap_or("");
    text.split(|c: char| !c.is_alphanumeric() && c != '_').any(|word| word == "const")
}

/// Every identifier occurrence in the file, tagged the way rename and
/// go-to-definition need it.
///
/// Three sources, and one rule behind all of them — **a reference is bytes the
/// user wrote**:
///
/// - tokens the user wrote, straight or through a macro argument;
/// - one reference per *macro invocation*, spanning the macro's name rather
///   than the replacement it stood for. The replacement tokens all carry the
///   invocation's span (decision 0003), so the run collapses to one;
/// - the name in each `#define`, which never reaches the token stream at all
///   because the preprocessor consumed the directive.
fn references(pp: &Preprocessed, source: &str, symbols: &[Symbol]) -> Vec<Reference> {
    // Sorted and searched rather than scanned: this runs once per token, and a
    // linear `contains` over every declaration made a thousand-line shader
    // quadratic in its own outline (measured at P5-10).
    let mut declarations: Vec<ByteSpan> = symbols.iter().map(|s| s.name_span).collect();
    declarations.sort_unstable_by_key(|span| (span.start, span.end));
    let is_declaration = |span: ByteSpan| {
        declarations.binary_search_by_key(&(span.start, span.end), |s| (s.start, s.end)).is_ok()
    };

    let mut references: Vec<Reference> = Vec::with_capacity(pp.tokens.len() / 3 + 1);
    for (i, token) in pp.tokens.iter().enumerate() {
        if token.origin.is_written() {
            if token.kind != TokenKind::Ident {
                continue;
            }
            let is_member = i > 0 && pp.tokens[i - 1].kind == TokenKind::Punct(Punct::Dot);
            references.push(Reference {
                span: token.span,
                is_declaration: is_declaration(token.span),
                is_member,
            });
        } else if let Some(span) = invocation_name(source, token.span) {
            references.push(Reference { span, is_declaration: false, is_member: false });
        }
    }
    for def in pp.macros.all() {
        if def.predefined || def.name_span.is_empty() {
            continue;
        }
        references.push(Reference {
            span: def.name_span,
            is_declaration: true,
            is_member: false,
        });
    }
    // Source order, with the repeats a macro produces collapsed: every token
    // of one replacement carries one span, and an argument used twice arrives
    // twice.
    references.sort_by_key(|r| (r.span.start, r.span.end));
    references.dedup_by_key(|r| r.span);
    references
}

/// The macro name at the head of an invocation span.
///
/// A body token is attributed to the whole invocation — `SCALE` for an
/// object-like macro, `SCALE(x)` for a function-like one. Only the name is a
/// reference to the macro; the rest is the call.
fn invocation_name(source: &str, span: ByteSpan) -> Option<ByteSpan> {
    if span.is_empty() {
        return None;
    }
    let text = source.get(span.start as usize..span.end as usize)?;
    let len = text
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count() as u32;
    if len == 0 {
        return None;
    }
    Some(ByteSpan::new(span.start, span.start + len))
}

/// Accumulates symbols and keeps the parent/child links consistent.
struct Builder<'a> {
    source: &'a str,
    symbols: Vec<Symbol>,
    roots: Vec<usize>,
}

impl Builder<'_> {
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        parent: Option<usize>,
        name: String,
        kind: SymbolKind,
        name_span: ByteSpan,
        full_span: ByteSpan,
        scope: ByteSpan,
        detail: String,
    ) -> usize {
        let index = self.symbols.len();
        self.symbols.push(Symbol {
            name,
            kind,
            name_span,
            full_span,
            scope,
            detail,
            parent,
            children: Vec::new(),
        });
        match parent {
            Some(parent) => self.symbols[parent].children.push(index),
            None => self.roots.push(index),
        }
        index
    }

    /// The [`NodeKind::Name`] under `node`, as text and span.
    fn name_of(&self, tree: &SyntaxTree, node: NodeId) -> Option<(String, ByteSpan)> {
        let name = tree.child_of_kind(node, NodeKind::Name)?;
        let span = tree.span(name);
        let text = self.source.get(span.start as usize..span.end as usize)?;
        Some((text.to_string(), span))
    }

    /// The source a span covers, with runs of whitespace collapsed so it fits
    /// the one line a hover or an outline row gives it.
    fn collapse(&self, span: ByteSpan) -> String {
        let text = self
            .source
            .get(span.start as usize..span.end as usize)
            .unwrap_or("")
            .trim();
        let mut out = String::with_capacity(text.len());
        let mut space = false;
        for ch in text.chars() {
            if ch.is_whitespace() {
                space = !out.is_empty();
            } else {
                if space {
                    out.push(' ');
                }
                space = false;
                out.push(ch);
            }
        }
        out
    }
}
