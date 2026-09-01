//! What a parse produces.
//!
//! A flat [`Symbol`] arena with parent/child indices rather than a nested tree:
//! document symbols want the hierarchy, name resolution wants a flat scan, and
//! indices keep both cheap without a second structure.
//!
//! Every symbol carries a [`Symbol::scope`] — the span of source over which its
//! name is visible. Resolution is then "the visible symbol with that name whose
//! scope is smallest", which handles shadowing without a scope tree.

use analyzer_core::spans::ByteSpan;

use crate::Language;
use crate::lexer::{Token, TokenKind};

/// What a declaration declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// A function. WGSL `fn`, GLSL `type name(…) {…}`.
    Function,
    /// A function carrying `@vertex`, `@fragment` or `@compute`, or a GLSL
    /// `main`. Worth its own kind so the outline can lead with it.
    EntryPoint,
    Struct,
    /// A member of a struct or a GLSL interface block.
    Field,
    /// A module-scope variable: WGSL `var`, GLSL `uniform`/`in`/`out`/`buffer`.
    Variable,
    /// WGSL `const`/`override`, GLSL `const`.
    Constant,
    /// A function parameter.
    Parameter,
    /// A `let`/`var` inside a function body.
    Local,
    /// WGSL `alias`.
    TypeAlias,
    /// A GLSL `#define`.
    Macro,
    /// A GLSL interface block name: `uniform Camera { … }`.
    Block,
}

impl SymbolKind {
    /// Whether the symbol lives inside a function body. Used to decide what
    /// belongs in the document outline.
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
    /// The whole declaration, body included — an LSP range.
    pub full_span: ByteSpan,
    /// Where the name is visible. Module-scope names get the whole file.
    pub scope: ByteSpan,
    /// The declaration as written, minus its body: `fn scale(v: vec3f) -> vec3f`.
    pub detail: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

/// An identifier occurrence, declaration sites included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    pub span: ByteSpan,
    /// Whether this occurrence is the declaration's own name.
    pub is_declaration: bool,
    /// Whether a `.` immediately precedes it, making it a field or swizzle
    /// rather than a name to resolve in scope.
    pub is_member: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Brace,
    Paren,
    Bracket,
    /// A `/* … */` comment, or a run of adjacent `//` lines.
    Comment,
}

/// A foldable region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    /// Delimiters included.
    pub span: ByteSpan,
}

/// A problem the syntax layer found on its own, without naga.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxDiagnostic {
    pub span: ByteSpan,
    pub message: String,
}

/// A parsed source.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub language: Language,
    /// Every token, comments included, in source order.
    pub tokens: Vec<Token>,
    pub symbols: Vec<Symbol>,
    /// Indices of the symbols with no parent, in source order.
    pub roots: Vec<usize>,
    pub references: Vec<Reference>,
    pub blocks: Vec<Block>,
    pub diagnostics: Vec<SyntaxDiagnostic>,
}

impl Parsed {
    /// The identifier occurrence at `offset`, if the cursor is on one.
    pub fn reference_at(&self, offset: u32) -> Option<&Reference> {
        self.references.iter().find(|r| r.span.contains(offset))
    }

    /// The token at `offset`. The trailing edge counts as inside, so a cursor
    /// just past the last character of a word still finds it.
    pub fn token_at(&self, offset: u32) -> Option<&Token> {
        self.tokens.iter().find(|t| t.span.contains(offset))
    }

    /// The last token that ends at or before `offset`, skipping comments.
    ///
    /// This is what completion asks to find the `.` or `@` it was triggered by.
    pub fn token_before(&self, offset: u32) -> Option<&Token> {
        self.tokens
            .iter()
            .rev()
            .find(|t| !t.kind.is_trivia() && t.span.end <= offset)
    }

    /// The symbol whose *name* is at `offset` — i.e. the cursor is on a
    /// declaration rather than a use.
    pub fn symbol_declared_at(&self, offset: u32) -> Option<usize> {
        self.symbols.iter().position(|s| s.name_span.contains(offset))
    }

    /// Every symbol visible at `offset`, innermost scope first.
    ///
    /// Members of structs and interface blocks are left out: they are reached
    /// through a value, not by bare name.
    pub fn visible_at(&self, offset: u32) -> Vec<usize> {
        let mut visible: Vec<usize> = self
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind != SymbolKind::Field && s.scope.contains(offset))
            .map(|(i, _)| i)
            .collect();
        visible.sort_by_key(|&i| self.symbols[i].scope.len());
        visible
    }

    /// The symbol the name at `offset` refers to.
    ///
    /// Returns `None` for a member access (`camera.view`) — the base's type
    /// decides that, and this layer does not know types.
    pub fn resolve_at(&self, source: &str, offset: u32) -> Option<usize> {
        let reference = self.reference_at(offset)?;
        if reference.is_member {
            return None;
        }
        let name = self.text(source, reference.span);
        self.resolve_name(name, offset)
    }

    /// The innermost symbol named `name` that is visible at `offset`.
    pub fn resolve_name(&self, name: &str, offset: u32) -> Option<usize> {
        self.symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.name == name && s.kind != SymbolKind::Field && s.scope.contains(offset)
            })
            // Smallest scope wins, which is what shadowing means.
            .min_by_key(|(_, s)| s.scope.len())
            .map(|(i, _)| i)
    }

    /// Every occurrence of `name`, wherever it appears.
    ///
    /// Deliberately name-based rather than scope-resolved: shaders are single
    /// files with few names, and a rename that misses a use is worse than one
    /// that catches a shadowed homonym the user can see in the preview.
    pub fn occurrences<'a>(
        &'a self,
        source: &'a str,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Reference> {
        self.references.iter().filter(move |r| self.text(source, r.span) == name)
    }

    /// The source text a span covers.
    pub fn text<'a>(&self, source: &'a str, span: ByteSpan) -> &'a str {
        source.get(span.start as usize..span.end as usize).unwrap_or("")
    }

    /// The innermost function containing `offset`, if any.
    pub fn enclosing_function(&self, offset: u32) -> Option<usize> {
        self.symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                matches!(s.kind, SymbolKind::Function | SymbolKind::EntryPoint)
                    && s.full_span.contains(offset)
            })
            .min_by_key(|(_, s)| s.full_span.len())
            .map(|(i, _)| i)
    }
}

/// Comment blocks, for folding: every multi-line `/* … */`, and every run of
/// two or more `//` lines with nothing but comments between them.
pub(crate) fn comment_blocks(tokens: &[Token], source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let comments: Vec<&Token> = tokens.iter().filter(|t| t.kind == TokenKind::Comment).collect();

    let mut i = 0;
    while i < comments.len() {
        let start = comments[i];
        let text = &source[start.span.start as usize..start.span.end as usize];
        if !text.starts_with("//") {
            // A block comment folds whenever it spans more than one line.
            if text.contains('\n') {
                blocks.push(Block { kind: BlockKind::Comment, span: start.span });
            }
            i += 1;
            continue;
        }

        // Gather the run of consecutive line comments.
        let mut end = i;
        while end + 1 < comments.len()
            && comments[end + 1].line == comments[end].line + 1
            && source[comments[end + 1].span.start as usize..].starts_with("//")
        {
            end += 1;
        }
        if end > i {
            blocks.push(Block {
                kind: BlockKind::Comment,
                span: ByteSpan::new(start.span.start, comments[end].span.end),
            });
        }
        i = end + 1;
    }

    blocks
}
