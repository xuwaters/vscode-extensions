//! The outline-and-scope parser.
//!
//! This is not a grammar. It is a walk over the token stream that finds
//! declarations, the scopes they are visible in, and the identifier
//! occurrences that might refer to them — and gives up gracefully on anything
//! it does not recognise by skipping to the next `;` or `}`.
//!
//! That trade is the whole point. A grammar-complete parser answers precisely
//! or not at all, and "not at all" is the state a file is in for most of the
//! keystrokes an editor asks about. naga supplies the precise answers when the
//! source happens to be valid; this layer supplies useful ones always.

mod glsl;
mod wgsl;

use analyzer_core::spans::ByteSpan;

use crate::Language;
use crate::lexer::{Token, TokenKind};
use crate::tree::{Block, BlockKind, Parsed, Symbol, SymbolKind, SyntaxDiagnostic};

/// Parse a source that has already been lexed.
pub fn parse(source: &str, language: Language, tokens: Vec<Token>) -> Parsed {
    let significant: Vec<Token> =
        tokens.iter().copied().filter(|t| !t.kind.is_trivia()).collect();
    let (mut blocks, matching, mut diagnostics) = delimiters(&significant, source);
    blocks.extend(crate::tree::comment_blocks(&tokens, source));
    blocks.sort_by_key(|b| (b.span.start, std::cmp::Reverse(b.span.end)));

    let mut cursor = Cursor { source, toks: significant, matching, pos: 0 };
    let mut builder = Builder { source, symbols: Vec::new(), roots: Vec::new() };

    match language {
        Language::Wgsl => wgsl::parse(&mut cursor, &mut builder),
        Language::Glsl => glsl::parse(&mut cursor, &mut builder),
    }

    let references = references(&cursor.toks, &builder.symbols, source);
    diagnostics.extend(builder.diagnostics());

    Parsed {
        language,
        tokens,
        symbols: builder.symbols,
        roots: builder.roots,
        references,
        blocks,
        diagnostics,
    }
}

/// Pair up `(`/`)`, `[`/`]` and `{`/`}` across the whole file.
///
/// `<`/`>` are deliberately absent: they are comparison operators as often as
/// they are template brackets, and guessing wrong here would corrupt the block
/// structure the rest of the parse depends on. The WGSL parser matches them
/// locally, where it knows a template is expected.
fn delimiters(
    toks: &[Token],
    source: &str,
) -> (Vec<Block>, Vec<Option<usize>>, Vec<SyntaxDiagnostic>) {
    let mut blocks = Vec::new();
    let mut matching = vec![None; toks.len()];
    let mut diagnostics = Vec::new();
    let mut stack: Vec<(usize, u8)> = Vec::new();

    for (i, token) in toks.iter().enumerate() {
        if token.kind != TokenKind::Punct || token.span.len() != 1 {
            continue;
        }
        let byte = source.as_bytes()[token.span.start as usize];
        match byte {
            b'(' | b'[' | b'{' => stack.push((i, byte)),
            b')' | b']' | b'}' => {
                let want = match byte {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                match stack.last() {
                    Some(&(open, opener)) if opener == want => {
                        stack.pop();
                        matching[open] = Some(i);
                        matching[i] = Some(open);
                        blocks.push(Block {
                            kind: match byte {
                                b')' => BlockKind::Paren,
                                b']' => BlockKind::Bracket,
                                _ => BlockKind::Brace,
                            },
                            span: ByteSpan::new(toks[open].span.start, token.span.end),
                        });
                    }
                    // A close with no matching open, or one that closes the
                    // wrong kind. Report it and carry on; unwinding the stack
                    // here would turn one typo into a cascade.
                    _ => diagnostics.push(SyntaxDiagnostic {
                        span: token.span,
                        message: format!("unmatched `{}`", byte as char),
                    }),
                }
            }
            _ => {}
        }
    }

    for (open, opener) in stack {
        diagnostics.push(SyntaxDiagnostic {
            span: toks[open].span,
            message: format!("unclosed `{}`", opener as char),
        });
    }

    (blocks, matching, diagnostics)
}

/// Every identifier occurrence in the file, tagged with whether it follows a
/// `.` and whether it is a declaration's own name.
fn references(toks: &[Token], symbols: &[Symbol], source: &str) -> Vec<crate::tree::Reference> {
    let declarations: Vec<ByteSpan> = symbols.iter().map(|s| s.name_span).collect();
    toks.iter()
        .enumerate()
        .filter(|(_, t)| t.kind == TokenKind::Ident)
        .map(|(i, t)| {
            // A `.` token is always the access operator: the lexer folds `.5`
            // into a single number token, so a lone `.` never starts a literal.
            let is_member = i > 0
                && toks[i - 1].kind == TokenKind::Punct
                && source.as_bytes().get(toks[i - 1].span.start as usize) == Some(&b'.');
            crate::tree::Reference {
                span: t.span,
                is_declaration: declarations.contains(&t.span),
                is_member,
            }
        })
        .collect()
}

/// A position in the significant-token stream.
pub(crate) struct Cursor<'a> {
    pub source: &'a str,
    /// Comments removed, so the parser never has to step over one.
    pub toks: Vec<Token>,
    /// For each bracket token, the index of its partner.
    pub matching: Vec<Option<usize>>,
    pub pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }

    pub fn get(&self, i: usize) -> Option<&Token> {
        self.toks.get(i)
    }

    pub fn kind(&self, i: usize) -> Option<TokenKind> {
        self.toks.get(i).map(|t| t.kind)
    }

    pub fn text(&self, i: usize) -> &'a str {
        match self.toks.get(i) {
            Some(t) => &self.source[t.span.start as usize..t.span.end as usize],
            None => "",
        }
    }

    pub fn span(&self, i: usize) -> ByteSpan {
        self.toks.get(i).map(|t| t.span).unwrap_or(ByteSpan::EMPTY)
    }

    /// Whether token `i` is exactly this text.
    pub fn is(&self, i: usize, text: &str) -> bool {
        self.text(i) == text
    }

    /// Whether token `i` is a name — an identifier, or a type name, which the
    /// lexer classifies separately but a declaration may still be introducing.
    pub fn is_name(&self, i: usize) -> bool {
        matches!(self.kind(i), Some(TokenKind::Ident | TokenKind::Type))
    }

    /// The index just past the group opening at `i`, or `i + 1` if `i` is not
    /// a matched opener.
    pub fn past_group(&self, i: usize) -> usize {
        match self.matching.get(i).copied().flatten() {
            Some(close) if close > i => close + 1,
            _ => i + 1,
        }
    }

    /// The closing index of the group opening at `i`.
    pub fn group_end(&self, i: usize) -> Option<usize> {
        self.matching.get(i).copied().flatten().filter(|&close| close > i)
    }

    /// Advance past the next `;`, or to the end of the enclosing group.
    ///
    /// The recovery path: whatever we failed to understand, a statement
    /// boundary is where understanding can resume.
    pub fn recover(&mut self) {
        while !self.at_end() {
            if self.is(self.pos, ";") {
                self.pos += 1;
                return;
            }
            if self.is(self.pos, "}") {
                return;
            }
            if self.is(self.pos, "{") {
                self.pos = self.past_group(self.pos);
                continue;
            }
            self.pos += 1;
        }
    }

    /// Scan a `<…>` template argument list starting at `i`, returning the
    /// index just past the closing `>`.
    ///
    /// Angle brackets are not paired by [`delimiters`], so this counts them
    /// locally and bails at any token that cannot appear inside a template —
    /// which is what keeps `a < b` from swallowing the rest of the file.
    pub fn past_template(&self, i: usize) -> usize {
        if !self.is(i, "<") {
            return i;
        }
        let mut depth = 0usize;
        let mut j = i;
        while j < self.toks.len() {
            match self.text(j) {
                "<" => depth += 1,
                // Punctuation lexes one byte at a time, so `>>` closing two
                // levels of `array<vec2<f32>>` arrives as two separate tokens.
                ">" => {
                    depth -= 1;
                    if depth == 0 {
                        return j + 1;
                    }
                }
                ";" | "{" | "}" | "(" | ")" => return i,
                _ => {}
            }
            j += 1;
        }
        i
    }

    /// The index of the `;` ending the statement that starts at `from`.
    ///
    /// Groups are stepped over whole, so the `;`s inside a `for` header do not
    /// end the statement containing it.
    pub fn statement_end(&self, from: usize) -> usize {
        self.statement_end_before(from, self.toks.len())
    }

    /// [`Cursor::statement_end`], bounded — for scanning inside a group whose
    /// closing brace must not be crossed.
    pub fn statement_end_before(&self, from: usize, limit: usize) -> usize {
        let limit = limit.min(self.toks.len());
        let mut i = from;
        while i < limit {
            match self.text(i) {
                ";" => return i,
                "{" | "(" | "[" => i = self.past_group(i),
                // An unterminated statement ends where its block does.
                "}" => return i,
                _ => i += 1,
            }
        }
        limit.saturating_sub(1).max(from)
    }

    /// Split `[from, to)` at the commas that are not nested inside a group.
    ///
    /// Empty chunks are dropped, so the trailing comma both languages allow in
    /// a member list does not produce one.
    pub fn comma_chunks(&self, from: usize, to: usize) -> Vec<(usize, usize)> {
        let to = to.min(self.toks.len());
        let mut chunks = Vec::new();
        let mut start = from;
        let mut i = from;
        while i < to {
            match self.text(i) {
                "(" | "[" | "{" => {
                    i = self.past_group(i);
                    continue;
                }
                "," => {
                    if i > start {
                        chunks.push((start, i));
                    }
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        if to > start {
            chunks.push((start, to));
        }
        chunks
    }
}

/// Accumulates symbols and keeps the parent/child links consistent.
pub(crate) struct Builder<'a> {
    pub source: &'a str,
    pub symbols: Vec<Symbol>,
    pub roots: Vec<usize>,
}

impl<'a> Builder<'a> {
    /// Record a symbol and return its index.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        parent: Option<usize>,
        name: &str,
        kind: SymbolKind,
        name_span: ByteSpan,
        full_span: ByteSpan,
        scope: ByteSpan,
        detail: String,
    ) -> usize {
        let index = self.symbols.len();
        self.symbols.push(Symbol {
            name: name.to_string(),
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

    /// The declaration as written, with runs of whitespace collapsed so it fits
    /// on the one line a hover or an outline row gives it.
    pub fn detail(&self, span: ByteSpan) -> String {
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

    /// Duplicate declarations at the same scope, which neither language allows.
    pub fn diagnostics(&self) -> Vec<SyntaxDiagnostic> {
        let mut diagnostics = Vec::new();
        for (i, symbol) in self.symbols.iter().enumerate() {
            // GLSL genuinely allows overloading, so only flag repeats of
            // things that cannot be overloaded.
            if matches!(symbol.kind, SymbolKind::Function | SymbolKind::EntryPoint) {
                continue;
            }
            if let Some(previous) = self.symbols[..i].iter().find(|other| {
                other.name == symbol.name
                    && other.parent == symbol.parent
                    && other.scope == symbol.scope
                    && !matches!(other.kind, SymbolKind::Function | SymbolKind::EntryPoint)
            }) {
                let _ = previous;
                diagnostics.push(SyntaxDiagnostic {
                    span: symbol.name_span,
                    message: format!("`{}` is already declared in this scope", symbol.name),
                });
            }
        }
        diagnostics
    }
}
