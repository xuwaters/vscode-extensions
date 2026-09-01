//! The concrete syntax tree — flat preorder arrays, per decision 0006.
//!
//! A [`SyntaxTree`] is two vectors: [`Node`]s in preorder, and one contiguous
//! run of [`Child`] entries per node. A leaf is a [`TokenId`] into
//! [`Preprocessed::tokens`](crate::preprocessor::Preprocessed::tokens) — the
//! tree deliberately does not own its tokens, so a reparse costs no `String`
//! clones and every accessor that needs spelling asks for the `Preprocessed` it
//! was built from.
//!
//! Two invariants make "lossless" testable rather than decorative:
//!
//! - **Every token is a leaf exactly once, in stream order.** Whatever the
//!   parser could not understand sits inside an [`NodeKind::Error`] node; it is
//!   never dropped.
//! - **[`SyntaxTree::pieces`] tiles the source.** The leaves the preprocessor
//!   marked as written account for the bytes the user typed; everything between
//!   them — comments, directives, inactive branches, the `NAME(` … `)` of a
//!   macro invocation — comes back as a [`PieceKind::Gap`]. Concatenating the
//!   pieces returns the source byte for byte.
//!
//! Node ids are preorder, so a parent's id is always smaller than its
//! children's and a whole subtree is the id range `[id, node.end)`.

use analyzer_core::spans::ByteSpan;

use crate::diagnostics::SyntaxDiagnostic;
use crate::preprocessor::Preprocessed;

/// An index into [`SyntaxTree::nodes`]. Assigned in preorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    /// The root of every tree, which is always the [`NodeKind::SourceFile`].
    pub const ROOT: NodeId = NodeId(0);

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// An index into [`Preprocessed::tokens`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenId(pub u32);

impl TokenId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One entry in a node's child list: either a subtree or a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Child {
    Node(NodeId),
    Token(TokenId),
}

/// What a node is. The inventory is design/cst.md §3.
///
/// Kinds describe *syntax*, never meaning: a call and a constructor are both
/// [`NodeKind::CallExpr`], because telling them apart needs a symbol table and
/// guessing would put a lie in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    // -- structure ---------------------------------------------------------
    /// The whole translation unit. Always node 0.
    SourceFile,
    /// Tokens the parser could not place. Always paired with a diagnostic.
    Error,

    // -- declarations ------------------------------------------------------
    /// `layout(…) uniform mat4 a, b[2] = c;`
    Declaration,
    /// One `name [array] [= init]` inside a declaration.
    Declarator,
    /// `= expr`, or `= { … }`.
    Initializer,
    /// A braced initialiser list, GLSL 4.20+.
    InitializerList,
    /// A prototype or a definition; the `CompoundStmt` child is the difference.
    FunctionDecl,
    ParameterList,
    Parameter,
    /// `struct Name { … }` wherever a type specifier may appear.
    StructSpec,
    /// The `{ … }` of a struct or an interface block.
    FieldList,
    /// One `type name, name2;` line inside a [`NodeKind::FieldList`].
    FieldDecl,
    /// `layout(std140) uniform Camera { … } camera;`
    InterfaceBlock,
    /// `precision highp float;`
    PrecisionDecl,
    /// The type-less declarations: `invariant gl_Position;`, `layout(…) in;`.
    QualifierDecl,
    /// A stray `;` at file scope, which the grammar allows.
    EmptyDecl,
    /// A *declared* identifier. Uses are [`NodeKind::NameExpr`].
    Name,
    /// A `[[unroll]]`-style attribute group. Not core GLSL — it is
    /// `GL_EXT_control_flow_attributes` — but it prefixes ordinary statements
    /// and declarations, so the grammar has to step over it either way.
    Attribute,

    // -- types and qualifiers ----------------------------------------------
    QualifierList,
    /// `layout( … )`.
    LayoutQualifier,
    /// One `name` or `name = expr` inside a `layout`.
    LayoutItem,
    /// `subroutine` or `subroutine(typeName, …)`.
    SubroutineQualifier,
    /// A type token or a [`NodeKind::StructSpec`], plus any C-style array
    /// suffix — `float[4]` in `float[4] x`.
    TypeSpec,
    /// One `[ … ]`, sized or not.
    ArraySpec,

    // -- statements --------------------------------------------------------
    CompoundStmt,
    DeclStmt,
    ExprStmt,
    EmptyStmt,
    IfStmt,
    ElseClause,
    SwitchStmt,
    /// `case expr:` or `default:`.
    CaseLabel,
    WhileStmt,
    DoWhileStmt,
    ForStmt,
    /// An `if`/`while`/`for` header, which §9 lets declare a name.
    Condition,
    ReturnStmt,
    BreakStmt,
    ContinueStmt,
    DiscardStmt,

    // -- expressions -------------------------------------------------------
    /// `a, b` — the comma operator, not an argument list.
    CommaExpr,
    AssignExpr,
    /// `c ? a : b`.
    CondExpr,
    BinaryExpr,
    /// A prefix `++ -- + - ~ !`.
    UnaryExpr,
    /// A postfix `++` or `--`.
    PostfixExpr,
    /// A call, a constructor, or an array constructor. Phase 4 tells them apart.
    CallExpr,
    ArgumentList,
    IndexExpr,
    /// `base.member`, swizzles included.
    FieldExpr,
    ParenExpr,
    NameExpr,
    LiteralExpr,
}

impl NodeKind {
    /// A stable lowercase-ish name, used by the tree dumps the tests snapshot.
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::SourceFile => "SourceFile",
            NodeKind::Error => "Error",
            NodeKind::Declaration => "Declaration",
            NodeKind::Declarator => "Declarator",
            NodeKind::Initializer => "Initializer",
            NodeKind::InitializerList => "InitializerList",
            NodeKind::FunctionDecl => "FunctionDecl",
            NodeKind::ParameterList => "ParameterList",
            NodeKind::Parameter => "Parameter",
            NodeKind::StructSpec => "StructSpec",
            NodeKind::FieldList => "FieldList",
            NodeKind::FieldDecl => "FieldDecl",
            NodeKind::InterfaceBlock => "InterfaceBlock",
            NodeKind::PrecisionDecl => "PrecisionDecl",
            NodeKind::QualifierDecl => "QualifierDecl",
            NodeKind::EmptyDecl => "EmptyDecl",
            NodeKind::Name => "Name",
            NodeKind::Attribute => "Attribute",
            NodeKind::QualifierList => "QualifierList",
            NodeKind::LayoutQualifier => "LayoutQualifier",
            NodeKind::LayoutItem => "LayoutItem",
            NodeKind::SubroutineQualifier => "SubroutineQualifier",
            NodeKind::TypeSpec => "TypeSpec",
            NodeKind::ArraySpec => "ArraySpec",
            NodeKind::CompoundStmt => "CompoundStmt",
            NodeKind::DeclStmt => "DeclStmt",
            NodeKind::ExprStmt => "ExprStmt",
            NodeKind::EmptyStmt => "EmptyStmt",
            NodeKind::IfStmt => "IfStmt",
            NodeKind::ElseClause => "ElseClause",
            NodeKind::SwitchStmt => "SwitchStmt",
            NodeKind::CaseLabel => "CaseLabel",
            NodeKind::WhileStmt => "WhileStmt",
            NodeKind::DoWhileStmt => "DoWhileStmt",
            NodeKind::ForStmt => "ForStmt",
            NodeKind::Condition => "Condition",
            NodeKind::ReturnStmt => "ReturnStmt",
            NodeKind::BreakStmt => "BreakStmt",
            NodeKind::ContinueStmt => "ContinueStmt",
            NodeKind::DiscardStmt => "DiscardStmt",
            NodeKind::CommaExpr => "CommaExpr",
            NodeKind::AssignExpr => "AssignExpr",
            NodeKind::CondExpr => "CondExpr",
            NodeKind::BinaryExpr => "BinaryExpr",
            NodeKind::UnaryExpr => "UnaryExpr",
            NodeKind::PostfixExpr => "PostfixExpr",
            NodeKind::CallExpr => "CallExpr",
            NodeKind::ArgumentList => "ArgumentList",
            NodeKind::IndexExpr => "IndexExpr",
            NodeKind::FieldExpr => "FieldExpr",
            NodeKind::ParenExpr => "ParenExpr",
            NodeKind::NameExpr => "NameExpr",
            NodeKind::LiteralExpr => "LiteralExpr",
        }
    }

    /// Whether this node declares something the outline should show.
    pub fn is_declaration(self) -> bool {
        matches!(
            self,
            NodeKind::Declaration
                | NodeKind::FunctionDecl
                | NodeKind::InterfaceBlock
                | NodeKind::StructSpec
                | NodeKind::FieldDecl
                | NodeKind::Parameter
                | NodeKind::QualifierDecl
                | NodeKind::PrecisionDecl
        )
    }

    /// Whether this node is one of the expression forms.
    pub fn is_expression(self) -> bool {
        matches!(
            self,
            NodeKind::CommaExpr
                | NodeKind::AssignExpr
                | NodeKind::CondExpr
                | NodeKind::BinaryExpr
                | NodeKind::UnaryExpr
                | NodeKind::PostfixExpr
                | NodeKind::CallExpr
                | NodeKind::IndexExpr
                | NodeKind::FieldExpr
                | NodeKind::ParenExpr
                | NodeKind::NameExpr
                | NodeKind::LiteralExpr
        )
    }
}

/// One node of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    /// The source bytes this node covers, joined over the written tokens under
    /// it. A node that covers only macro-body tokens carries the invocation's
    /// span, which is text the user can see.
    pub span: ByteSpan,
    /// The enclosing node. The root is its own parent.
    pub parent: NodeId,
    /// The half-open range of the token stream this node covers.
    pub tokens: (TokenId, TokenId),
    /// One past the last descendant: the subtree is the id range `[id, end)`.
    pub end: NodeId,
    /// Range into [`SyntaxTree::children`].
    children: (u32, u32),
}

impl Node {
    /// Whether this node has no children at all — an empty `Error`, say.
    pub fn is_empty(&self) -> bool {
        self.children.0 == self.children.1
    }

    /// How many tokens of the stream this node covers.
    pub fn token_count(&self) -> u32 {
        self.tokens.1.0 - self.tokens.0.0
    }
}

/// What a byte range of the source is accounted for by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceKind {
    /// A token the tree holds as a leaf, spelled at these bytes.
    Token(TokenId),
    /// Bytes no leaf claims: whitespace, comments, directive lines, inactive
    /// branches, and the scaffolding of a macro invocation.
    Gap,
}

/// One byte range of the source, and what accounts for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    pub kind: PieceKind,
    pub span: ByteSpan,
}

/// A parsed source.
#[derive(Debug, Clone)]
pub struct SyntaxTree {
    nodes: Vec<Node>,
    children: Vec<Child>,
    /// How many tokens the stream held, so the tree can be checked against it
    /// without the `Preprocessed` in hand.
    token_count: u32,
    pub diagnostics: Vec<SyntaxDiagnostic>,
}

impl SyntaxTree {
    pub(crate) fn new(
        nodes: Vec<Node>,
        children: Vec<Child>,
        token_count: u32,
        diagnostics: Vec<SyntaxDiagnostic>,
    ) -> Self {
        SyntaxTree { nodes, children, token_count, diagnostics }
    }

    pub fn root(&self) -> NodeId {
        NodeId::ROOT
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn token_count(&self) -> u32 {
        self.token_count
    }

    /// Every node, in preorder.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().enumerate().map(|(i, n)| (NodeId(i as u32), n))
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    pub fn kind(&self, id: NodeId) -> NodeKind {
        self.nodes[id.index()].kind
    }

    pub fn span(&self, id: NodeId) -> ByteSpan {
        self.nodes[id.index()].span
    }

    /// This node's children — subtrees and tokens interleaved, in source order.
    pub fn children(&self, id: NodeId) -> &[Child] {
        let node = &self.nodes[id.index()];
        &self.children[node.children.0 as usize..node.children.1 as usize]
    }

    /// Just the subtrees among this node's children.
    pub fn child_nodes(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(id).iter().filter_map(|c| match c {
            Child::Node(id) => Some(*id),
            Child::Token(_) => None,
        })
    }

    /// Just the tokens directly under this node — not those under its subtrees.
    pub fn child_tokens(&self, id: NodeId) -> impl Iterator<Item = TokenId> + '_ {
        self.children(id).iter().filter_map(|c| match c {
            Child::Token(id) => Some(*id),
            Child::Node(_) => None,
        })
    }

    /// The first child subtree of the given kind, if there is one.
    pub fn child_of_kind(&self, id: NodeId, kind: NodeKind) -> Option<NodeId> {
        self.child_nodes(id).find(|c| self.kind(*c) == kind)
    }

    /// Every node in the subtree rooted at `id`, itself included, in preorder.
    pub fn descendants(&self, id: NodeId) -> impl Iterator<Item = NodeId> {
        (id.0..self.nodes[id.index()].end.0).map(NodeId)
    }

    /// Whether `inner` sits inside `outer` — an id-range check, because ids are
    /// preorder.
    pub fn contains(&self, outer: NodeId, inner: NodeId) -> bool {
        outer <= inner && inner.0 < self.nodes[outer.index()].end.0
    }

    /// The chain from `id` up to the root, `id` first.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut current = Some(id);
        std::iter::from_fn(move || {
            let this = current?;
            let parent = self.nodes[this.index()].parent;
            current = if parent == this { None } else { Some(parent) };
            Some(this)
        })
    }

    /// The innermost node whose span covers `offset`.
    ///
    /// Ties go to the deepest node, which is what an editor query wants: the
    /// cursor is on the identifier, not on the function that contains it.
    pub fn node_at(&self, offset: u32) -> NodeId {
        let mut best = NodeId::ROOT;
        let mut current = NodeId::ROOT;
        loop {
            let next = self
                .child_nodes(current)
                .find(|c| self.nodes[c.index()].span.contains(offset));
            match next {
                Some(child) => {
                    best = child;
                    current = child;
                }
                None => return best,
            }
        }
    }

    /// The nearest ancestor of `id` (itself included) of one of these kinds.
    pub fn enclosing(&self, id: NodeId, kinds: &[NodeKind]) -> Option<NodeId> {
        self.ancestors(id).find(|n| kinds.contains(&self.kind(*n)))
    }

    /// Every leaf token in the subtree rooted at `id`, in stream order.
    pub fn leaves(&self, id: NodeId) -> impl Iterator<Item = TokenId> {
        let node = &self.nodes[id.index()];
        (node.tokens.0.0..node.tokens.1.0).map(TokenId)
    }

    /// The source text a node covers, as the user wrote it.
    ///
    /// Only meaningful for a node whose tokens were written rather than
    /// substituted; a macro-derived node returns the invocation's text.
    pub fn text<'a>(&self, source: &'a str, id: NodeId) -> &'a str {
        let span = self.nodes[id.index()].span;
        source.get(span.start as usize..span.end as usize).unwrap_or("")
    }

    /// The spelling of a node's tokens, joined with single spaces.
    ///
    /// This reads the *expanded* stream, so a node built out of a macro
    /// invocation spells its replacement — which is what a hover wants to show.
    pub fn spelling(&self, pp: &Preprocessed, id: NodeId) -> String {
        let mut out = String::new();
        for token in self.leaves(id) {
            let Some(token) = pp.tokens.get(token.index()) else {
                continue;
            };
            if !out.is_empty() && token.leading_space {
                out.push(' ');
            }
            out.push_str(&token.text);
        }
        out
    }

    /// Every byte of the source, in order, split into what the tree accounts
    /// for and what lies between.
    ///
    /// Concatenating the pieces returns the source byte for byte — the
    /// losslessness contract of decision 0006. See design/cst.md §2.1 for why
    /// nothing needs to be stored to make that true.
    pub fn pieces(&self, pp: &Preprocessed, source: &str) -> Vec<Piece> {
        let len = source.len() as u32;
        let mut pieces: Vec<Piece> = Vec::with_capacity(self.token_count as usize + 16);
        let mut at = 0u32;
        for index in 0..self.token_count {
            let Some(token) = pp.tokens.get(index as usize) else {
                break;
            };
            // A synthesised token spells bytes that exist nowhere; the
            // invocation that produced it is covered by the gap around it.
            if !token.origin.is_written() {
                continue;
            }
            let span = token.span;
            // An argument used twice, or used out of order, arrives with a span
            // the surrounding gap already covers.
            if span.start < at || span.end > len {
                continue;
            }
            if span.start > at {
                pieces.push(Piece { kind: PieceKind::Gap, span: ByteSpan::new(at, span.start) });
            }
            pieces.push(Piece { kind: PieceKind::Token(TokenId(index)), span });
            at = span.end;
        }
        if at < len {
            pieces.push(Piece { kind: PieceKind::Gap, span: ByteSpan::new(at, len) });
        }
        pieces
    }

    /// The source rebuilt from [`SyntaxTree::pieces`]. Equal to the source it
    /// was parsed from; the round-trip gate is exactly this assertion.
    pub fn reconstruct(&self, pp: &Preprocessed, source: &str) -> String {
        let mut out = String::with_capacity(source.len());
        for piece in self.pieces(pp, source) {
            out.push_str(&source[piece.span.start as usize..piece.span.end as usize]);
        }
        out
    }

    /// An indented dump of the tree, for fixtures and for debugging.
    ///
    /// Node lines are `Kind "text"`; token lines are `· spelling`.
    pub fn dump(&self, pp: &Preprocessed, source: &str) -> String {
        let mut out = String::new();
        self.dump_node(pp, source, NodeId::ROOT, 0, &mut out);
        out
    }

    fn dump_node(
        &self,
        pp: &Preprocessed,
        source: &str,
        id: NodeId,
        depth: usize,
        out: &mut String,
    ) {
        for _ in 0..depth {
            out.push_str("  ");
        }
        out.push_str(self.kind(id).as_str());
        out.push('\n');
        for child in self.children(id) {
            match *child {
                Child::Node(node) => self.dump_node(pp, source, node, depth + 1, out),
                Child::Token(token) => {
                    for _ in 0..depth + 1 {
                        out.push_str("  ");
                    }
                    out.push_str("· ");
                    match pp.tokens.get(token.index()) {
                        Some(t) => out.push_str(&t.text),
                        None => out.push_str("<gone>"),
                    }
                    let _ = source;
                    out.push('\n');
                }
            }
        }
    }
}

/// The flat event list the parser emits, which [`build`] turns into the arena.
///
/// Events rather than direct construction because recovery needs to close a
/// stack of open nodes without knowing what they will contain, and a `Vec` of
/// events is the cheapest structure that allows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    Open(NodeKind),
    Token(TokenId),
    Close,
}

/// Turn an event list into the arena.
///
/// The event list is well formed by construction — the parser closes every node
/// it opens — but this is defensive anyway: a stray `Close` is ignored and any
/// node still open at the end is closed here. A malformed list must not panic.
///
/// The children of the nodes still open are staged in **one** buffer rather
/// than a `Vec` per open node. An open node's children are always the top of
/// that buffer — nothing can be added to an ancestor while a descendant is open
/// — so closing a node is "take the tail from here", and the tree costs one
/// allocation instead of one per node (RFC 012 P5-10).
pub(crate) fn build(
    events: &[Event],
    pp: &Preprocessed,
    token_count: u32,
    diagnostics: Vec<SyntaxDiagnostic>,
) -> SyntaxTree {
    let mut nodes: Vec<Node> = Vec::with_capacity(events.len() / 2 + 1);
    let mut children: Vec<Child> = Vec::with_capacity(events.len());
    // The children of every open node, outermost first, contiguously.
    let mut pending: Vec<Child> = Vec::with_capacity(64);
    // One entry per open node: the id it will land in, and where its own
    // children start in `pending`.
    let mut stack: Vec<(NodeId, usize)> = Vec::new();
    // Tokens arrive in order, so this is where the next one will land.
    let mut consumed = 0u32;

    for event in events {
        match *event {
            Event::Open(kind) => {
                let id = NodeId(nodes.len() as u32);
                let parent = stack.last().map_or(id, |(parent, _)| *parent);
                let first = TokenId(consumed.min(token_count));
                nodes.push(Node {
                    kind,
                    span: ByteSpan::EMPTY,
                    parent,
                    tokens: (first, first),
                    end: id,
                    children: (0, 0),
                });
                stack.push((id, pending.len()));
            }
            Event::Token(token) => {
                consumed = token.0 + 1;
                if !stack.is_empty() {
                    pending.push(Child::Token(token));
                }
            }
            Event::Close => close_top(&mut stack, &mut pending, &mut nodes, &mut children, pp),
        }
    }
    // Anything still open at the end — only reachable from a malformed list.
    while !stack.is_empty() {
        close_top(&mut stack, &mut pending, &mut nodes, &mut children, pp);
    }
    if nodes.is_empty() {
        nodes.push(Node {
            kind: NodeKind::SourceFile,
            span: ByteSpan::EMPTY,
            parent: NodeId::ROOT,
            tokens: (TokenId(0), TokenId(0)),
            end: NodeId(1),
            children: (0, 0),
        });
    }
    SyntaxTree::new(nodes, children, token_count, diagnostics)
}

/// Finish the innermost open node: give it its children, its extent, and a
/// place in its parent's child list.
fn close_top(
    stack: &mut Vec<(NodeId, usize)>,
    pending: &mut Vec<Child>,
    nodes: &mut [Node],
    children: &mut Vec<Child>,
    pp: &Preprocessed,
) {
    let Some((id, from)) = stack.pop() else {
        return;
    };
    let start = children.len() as u32;
    children.extend_from_slice(&pending[from..]);
    let (tokens, span) = extent(&pending[from..], nodes, pp, nodes[id.index()].tokens.0);
    pending.truncate(from);
    let count = nodes.len() as u32;
    let node = &mut nodes[id.index()];
    node.children = (start, children.len() as u32);
    node.end = NodeId(count);
    node.tokens = tokens;
    node.span = span;
    // The parent's own children are what is left at the top of `pending`, so
    // this lands in exactly the place the `Vec`-per-node version put it.
    if !stack.is_empty() {
        pending.push(Child::Node(id));
    }
}

/// The token range and source span a finished node covers.
///
/// Both come from the children, which already know their own — so this is
/// `O(children)` rather than `O(tokens)`, and the whole build stays linear.
fn extent(
    pending: &[Child],
    nodes: &[Node],
    pp: &Preprocessed,
    fallback: TokenId,
) -> ((TokenId, TokenId), ByteSpan) {
    let mut first: Option<u32> = None;
    let mut last: Option<u32> = None;
    let mut span: Option<ByteSpan> = None;
    for child in pending {
        let (start, end, child_span) = match *child {
            Child::Token(token) => {
                let span = pp.tokens.get(token.index()).map(|t| t.span);
                (token.0, token.0 + 1, span)
            }
            Child::Node(node) => {
                let node = &nodes[node.index()];
                (node.tokens.0.0, node.tokens.1.0, Some(node.span))
            }
        };
        first = Some(first.map_or(start, |f: u32| f.min(start)));
        last = Some(last.map_or(end, |l: u32| l.max(end)));
        // An empty span belongs to a node that covered nothing written; letting
        // it into the join would drag the whole span back to byte zero.
        if let Some(child_span) = child_span.filter(|s| !s.is_empty()) {
            span = Some(match span {
                Some(existing) => existing.join(child_span),
                None => child_span,
            });
        }
    }
    // An empty node still has a place in the stream, which is where a
    // diagnostic about a missing construct should point.
    let start = first.unwrap_or(fallback.0);
    let end = last.unwrap_or(fallback.0);
    let span = span.unwrap_or_else(|| {
        pp.tokens
            .get(start as usize)
            .map(|t| ByteSpan::new(t.span.start, t.span.start))
            .or_else(|| {
                start
                    .checked_sub(1)
                    .and_then(|i| pp.tokens.get(i as usize))
                    .map(|t| ByteSpan::new(t.span.end, t.span.end))
            })
            .unwrap_or(ByteSpan::EMPTY)
    });
    ((TokenId(start), TokenId(end)), span)
}
