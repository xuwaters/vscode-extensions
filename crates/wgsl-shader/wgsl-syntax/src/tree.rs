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
    /// A name a WESL `import` brings into scope — the item or module the path
    /// ends at, or its `as` rename. [`Parsed::imports`] holds the path.
    Import,
}

impl SymbolKind {
    /// Whether the symbol lives inside a function body. Used to decide what
    /// belongs in the document outline.
    pub fn is_local(self) -> bool {
        matches!(self, SymbolKind::Local | SymbolKind::Parameter)
    }

    /// Whether the symbol is something this file declares, rather than a
    /// local or a name it imports — what an outline or a workspace-wide
    /// symbol picker lists.
    pub fn is_declaration(self) -> bool {
        !self.is_local() && self != SymbolKind::Import
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
    /// rather than a name to resolve in scope — or a WESL `::`, making it a
    /// segment of a module path, which is resolved through the path.
    pub is_member: bool,
}

/// Where a WESL module path starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRoot {
    /// `package::` — the root of the package this file belongs to.
    Package,
    /// `super::`, repeated this many times — the module above this file's.
    Super(u32),
    /// Neither: the first segment is a name, which in an import is a
    /// dependency package and in code is something an import brought in.
    Bare,
}

/// One name a WESL `import` statement brings into scope. A collection —
/// `import package::lights::{point, spot as cone};` — produces one apiece.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub root: PathRoot,
    /// `package` or the run of `super`s, when the path has one. A diagnostic
    /// about the root lands here.
    pub root_span: Option<ByteSpan>,
    /// The path after its root, each segment with its span; the imported name
    /// is the last.
    pub segments: Vec<(String, ByteSpan)>,
    /// The `as` rename, if there is one.
    pub alias: Option<(String, ByteSpan)>,
    /// The [`SymbolKind::Import`] symbol this produced.
    pub symbol: usize,
}

impl Import {
    /// The path as written: `package::lights::point`.
    pub fn path(&self) -> String {
        let mut out = match self.root {
            PathRoot::Package => "package::".to_string(),
            PathRoot::Super(n) => "super::".repeat(n as usize),
            PathRoot::Bare => String::new(),
        };
        let names: Vec<&str> = self.segments.iter().map(|(name, _)| name.as_str()).collect();
        out.push_str(&names.join("::"));
        out
    }
}

/// The qualified path written in front of a name: `package::lights` before
/// `point` in `package::lights::point`, or `utils` before `helper` in
/// `utils::helper()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPrefix {
    pub root: PathRoot,
    pub root_span: Option<ByteSpan>,
    /// The segments between the root and the name, outermost first.
    pub segments: Vec<(String, ByteSpan)>,
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
    /// What each WESL `import` brings in, in source order. Empty for every
    /// other language.
    pub imports: Vec<Import>,
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

    /// The import that produced symbol `index`, when it is an import.
    pub fn import_for(&self, index: usize) -> Option<&Import> {
        self.imports.iter().find(|import| import.symbol == index)
    }

    /// The WESL path written in front of whatever starts at `offset`.
    ///
    /// Read off the tokens rather than out of [`Parsed::imports`], because it
    /// has to answer for a half-typed path too — `package::lights::|` is what
    /// completion is asked about. Inside an import's `{…}` collection the
    /// prefix includes the path the collection hangs off, so in
    /// `import package::lights::{point, |}` it is `package::lights`.
    ///
    /// `None` when nothing qualifies the position: a plain name.
    pub fn path_prefix(&self, source: &str, offset: u32) -> Option<PathPrefix> {
        let toks: Vec<&Token> = self.tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
        let text = |i: usize| self.text(source, toks[i].span);
        // `::` lexes as two `:` tokens; they only form a path separator when
        // nothing sits between them.
        let colons = |i: usize| {
            i >= 1
                && text(i) == ":"
                && text(i - 1) == ":"
                && toks[i - 1].span.end == toks[i].span.start
        };

        // The token immediately before the position.
        let mut j = toks.iter().rposition(|t| t.span.end <= offset)?;
        let mut segments: Vec<(String, ByteSpan)> = Vec::new();
        let mut root = PathRoot::Bare;
        let mut root_span = None;

        loop {
            if colons(j) {
                let segment = j.checked_sub(2)?;
                match (toks[segment].kind, text(segment)) {
                    (TokenKind::Keyword, "package") => {
                        root = PathRoot::Package;
                        root_span = Some(toks[segment].span);
                        break;
                    }
                    (TokenKind::Keyword, "super") => {
                        let (mut first, mut count) = (segment, 1);
                        while first >= 3 && colons(first - 1) && text(first - 3) == "super" {
                            first -= 3;
                            count += 1;
                        }
                        root = PathRoot::Super(count);
                        root_span =
                            Some(ByteSpan::new(toks[first].span.start, toks[segment].span.end));
                        break;
                    }
                    (TokenKind::Ident | TokenKind::Type, name) => {
                        segments.push((name.to_string(), toks[segment].span));
                        match segment.checked_sub(1) {
                            Some(before) => j = before,
                            None => break,
                        }
                    }
                    _ => return None,
                }
                continue;
            }
            // Inside an import collection the path continues from the `::`
            // in front of the `{`.
            if matches!(text(j), "," | "{") {
                if let Some(open) = collection_open(&toks, source, j) {
                    if open >= 1 && colons(open - 1) {
                        j = open - 1;
                        continue;
                    }
                }
            }
            break;
        }

        if root == PathRoot::Bare && segments.is_empty() {
            return None;
        }
        segments.reverse();
        Some(PathPrefix { root, root_span, segments })
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

/// The `{` that opens the collection token `at` sits in, or `at` itself when
/// it is one. Gives up at a `;`, which no import collection contains.
fn collection_open(toks: &[&Token], source: &str, at: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = at;
    loop {
        match source.get(toks[i].span.start as usize..toks[i].span.end as usize)? {
            "}" => depth += 1,
            "{" if depth == 0 => return Some(i),
            "{" => depth -= 1,
            ";" => return None,
            _ => {}
        }
        i = i.checked_sub(1)?;
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
