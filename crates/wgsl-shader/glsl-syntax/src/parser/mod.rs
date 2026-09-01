//! The recovering GLSL parser — GLSL 4.60 §9, plus the ES and legacy variances.
//!
//! Input is the preprocessor's live token stream; output is the flat CST of
//! [decision 0006](../../../../docs/rfc/012-glsl-analyzer/decisions/0006-flat-cst-arrays.md).
//! The shape and the rules are design/cst.md; what follows is how they are
//! implemented.
//!
//! The parser emits a **flat event list** — `Open(kind)`, `Token(id)`, `Close`
//! — which one build pass turns into the arena. Two things make that worth the
//! indirection:
//!
//! - **Recovery is "close what is open".** No tree surgery, no partially built
//!   node to unwind; a node closes wherever the tokens ran out.
//! - **A node can be opened in the past.** Precedence climbing only learns that
//!   `a` was the left operand of a `+` after it has parsed `a`, so
//!   [`Parser::open_before`] records an `Open` for an earlier point in the
//!   stream and [`Parser::finish`] merges those in with one sort. That keeps
//!   left associativity honest without an `O(n²)` insert per operator.
//!
//! Three rules hold for every path through this module:
//!
//! 1. **Every token is emitted exactly once, in order.** Whatever cannot be
//!    understood is emitted inside an [`NodeKind::Error`] node, never dropped.
//! 2. **Every loop makes progress.** Loops that call a sub-parser compare the
//!    cursor before and after, and force one token into an `Error` when it did
//!    not move. There is no input that spins.
//! 3. **Recursion is capped.** An arena cannot overflow the stack, but a
//!    recursive-descent parser can, and "never panics" has to include "never
//!    aborts". [`Parser::deeper`] is on every recursive cycle.

mod decl;
mod expr;
mod stmt;

use analyzer_core::spans::ByteSpan;

use crate::cst::{Event, NodeKind, SyntaxTree, TokenId, build};
use crate::diagnostics::{ParseCode, SyntaxDiagnostic};
use crate::lexer::{Punct, TokenKind};
use crate::preprocessor::{PpToken, Preprocessed};

/// How deep the parser will recurse before it calls the source pathological.
///
/// One level costs about a dozen stack frames (an expression walks the whole
/// precedence ladder), so this is deliberately far below what the stack could
/// take. Nothing a human writes comes near it; generated shaders sometimes do.
const MAX_DEPTH: u32 = 64;

/// How far a lookahead will scan for a matching bracket before giving up.
///
/// Only used by the declaration-or-expression test, which never needs to look
/// past an array subscript. The cap is what keeps an unclosed `[` from turning
/// a statement scan into a whole-file scan.
const LOOKAHEAD_LIMIT: usize = 512;

/// The words that may qualify a declaration.
///
/// Storage, interpolation, precision, memory and the odd ones out, in the order
/// GLSL 4.60 §4 lists them, plus the legacy `attribute`/`varying` that pre-1.30
/// and ES 1.00 sources use. Membership matters only in *leading* position: a
/// declarator named `sample` still parses, because the name of a thing is only
/// ever read after its type.
pub(crate) const QUALIFIERS: &[&str] = &[
    // storage
    "const", "in", "out", "inout", "attribute", "varying", "uniform", "buffer", "shared",
    "centroid", "sample", "patch", "subroutine",
    // interpolation
    "smooth", "flat", "noperspective",
    // precision
    "highp", "mediump", "lowp",
    // memory
    "coherent", "volatile", "restrict", "readonly", "writeonly", "devicecoherent",
    "queuefamilycoherent", "workgroupcoherent", "subgroupcoherent", "nonprivate",
    // the rest
    "invariant", "precise", "layout",
    // Extension storage qualifiers that appear throughout the corpus. Parsing
    // them costs nothing and keeps a ray-tracing or mesh shader from becoming
    // one long `Error` node; whether they are *allowed* is Phase 4's question.
    "rayPayloadNV", "rayPayloadEXT", "rayPayloadInNV", "rayPayloadInEXT", "hitAttributeNV",
    "hitAttributeEXT", "callableDataNV", "callableDataEXT", "callableDataInNV",
    "callableDataInEXT", "shaderRecordNV", "shaderRecordEXT", "perprimitiveNV",
    "perprimitiveEXT", "perviewNV", "taskNV", "taskPayloadSharedEXT", "perVertexNV",
    "perVertexEXT", "pervertexNV", "pervertexEXT", "nonuniformEXT", "nonuniformNV",
    "nontemporal",
];

/// Whether `a` sorts before `b`, in a `const` context. `str`'s own `Ord` is
/// not `const`, and [`QUALIFIER_ORDER`] is built at compile time.
const fn before(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let shortest = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0;
    while i < shortest {
        if a[i] != b[i] {
            return a[i] < b[i];
        }
        i += 1;
    }
    a.len() < b.len()
}

/// [`QUALIFIERS`] in alphabetical order, as indices.
///
/// The table itself stays grouped by what each word *is*, because that is how
/// it is checked against §4; the search reads it through this. Every statement
/// in a file asks [`is_qualifier`] at least once, and scanning sixty spellings
/// per question put `memcmp` among the pipeline's largest costs (RFC 012
/// P5-10).
pub(crate) const QUALIFIER_ORDER: [u8; QUALIFIERS.len()] = {
    let mut order = [0u8; QUALIFIERS.len()];
    let mut i = 0;
    while i < order.len() {
        order[i] = i as u8;
        i += 1;
    }
    let mut i = 1;
    while i < order.len() {
        let mut j = i;
        while j > 0 && before(QUALIFIERS[order[j] as usize], QUALIFIERS[order[j - 1] as usize]) {
            let swap = order[j];
            order[j] = order[j - 1];
            order[j - 1] = swap;
            j -= 1;
        }
        i += 1;
    }
    order
};

/// Where each first byte's run begins in [`QUALIFIER_ORDER`]: the qualifiers
/// starting with byte `b` are `QUALIFIER_FIRST[b] .. QUALIFIER_FIRST[b + 1]`.
const QUALIFIER_FIRST: [u8; 257] = {
    let mut index = [0u8; 257];
    let mut i = 0;
    while i < QUALIFIER_ORDER.len() {
        let byte = QUALIFIERS[QUALIFIER_ORDER[i] as usize].as_bytes()[0];
        index[byte as usize + 1] += 1;
        i += 1;
    }
    let mut byte = 1;
    while byte < 257 {
        index[byte] += index[byte - 1];
        byte += 1;
    }
    index
};

/// Whether `word` may lead a declaration's qualifier sequence.
///
/// First byte, then a binary search inside that byte's run. A word starting
/// with a letter no qualifier does — which is most of them — is rejected by one
/// array read.
pub(crate) fn is_qualifier(word: &str) -> bool {
    let Some(&byte) = word.as_bytes().first() else {
        return false;
    };
    let mut low = QUALIFIER_FIRST[byte as usize] as usize;
    let mut high = QUALIFIER_FIRST[byte as usize + 1] as usize;
    while low < high {
        let mid = (low + high) / 2;
        match QUALIFIERS[QUALIFIER_ORDER[mid] as usize].cmp(word) {
            std::cmp::Ordering::Less => low = mid + 1,
            std::cmp::Ordering::Greater => high = mid,
            std::cmp::Ordering::Equal => return true,
        }
    }
    false
}

/// Parse a preprocessed source into a lossless CST.
///
/// Never fails and never panics. What it could not read is in the tree as
/// [`NodeKind::Error`] nodes, and in [`SyntaxTree::diagnostics`] as
/// `GLSL0100`-range diagnostics.
pub fn parse(pp: &Preprocessed) -> SyntaxTree {
    let mut p = Parser::new(pp);
    p.open(NodeKind::SourceFile);
    while !p.at_end() {
        let before = p.pos;
        decl::external_declaration(&mut p);
        if p.pos == before {
            p.skip_one_into_error("this is not a declaration");
        }
    }
    p.close();
    p.finish()
}

/// The parser's whole state.
pub(crate) struct Parser<'a> {
    pp: &'a Preprocessed,
    /// The next token to read.
    pub(crate) pos: usize,
    events: Vec<Event>,
    /// `Open`s for a point in the past: `(event index, kind)`, in the order
    /// they were recorded. [`Parser::finish`] merges them in.
    inserts: Vec<(u32, NodeKind)>,
    diagnostics: Vec<SyntaxDiagnostic>,
    depth: u32,
}

impl<'a> Parser<'a> {
    fn new(pp: &'a Preprocessed) -> Self {
        Parser {
            pp,
            pos: 0,
            events: Vec::with_capacity(pp.tokens.len() * 2 + 8),
            inserts: Vec::new(),
            diagnostics: Vec::new(),
            depth: 0,
        }
    }

    // -- reading the stream ------------------------------------------------

    pub(crate) fn at_end(&self) -> bool {
        self.pos >= self.pp.tokens.len()
    }

    /// The token `ahead` places from the cursor.
    pub(crate) fn nth(&self, ahead: usize) -> Option<&'a PpToken> {
        self.pp.tokens.get(self.pos + ahead)
    }

    pub(crate) fn nth_text(&self, ahead: usize) -> &'a str {
        self.nth(ahead).map_or("", |t| t.text.as_str())
    }

    pub(crate) fn nth_kind(&self, ahead: usize) -> Option<TokenKind> {
        self.nth(ahead).map(|t| t.kind)
    }

    /// Whether the token `ahead` places away is this punctuator.
    pub(crate) fn nth_is(&self, ahead: usize, punct: Punct) -> bool {
        self.nth_kind(ahead) == Some(TokenKind::Punct(punct))
    }

    /// Whether the token `ahead` places away is a word.
    pub(crate) fn nth_is_ident(&self, ahead: usize) -> bool {
        self.nth_kind(ahead) == Some(TokenKind::Ident)
    }

    /// Whether the token `ahead` places away is exactly this word.
    pub(crate) fn nth_is_word(&self, ahead: usize, word: &str) -> bool {
        self.nth_is_ident(ahead) && self.nth_text(ahead) == word
    }

    pub(crate) fn at(&self, punct: Punct) -> bool {
        self.nth_is(0, punct)
    }

    pub(crate) fn at_ident(&self) -> bool {
        self.nth_is_ident(0)
    }

    pub(crate) fn at_word(&self, word: &str) -> bool {
        self.nth_is_word(0, word)
    }

    /// The current punctuator, if the cursor is on one.
    pub(crate) fn punct(&self) -> Option<Punct> {
        match self.nth_kind(0) {
            Some(TokenKind::Punct(punct)) => Some(punct),
            _ => None,
        }
    }

    /// The span a diagnostic about "here" should carry: the current token, or
    /// the end of the last one when the stream has run out.
    pub(crate) fn here(&self) -> ByteSpan {
        match self.nth(0) {
            Some(token) => token.span,
            None => self
                .pp
                .tokens
                .last()
                .map(|t| ByteSpan::new(t.span.end, t.span.end))
                .unwrap_or(ByteSpan::EMPTY),
        }
    }

    /// The index just past the group opening at `ahead`, capped so an unclosed
    /// bracket cannot turn a lookahead into a whole-file scan.
    pub(crate) fn past_group(&self, ahead: usize) -> usize {
        let Some(TokenKind::Punct(open)) = self.nth_kind(ahead) else {
            return ahead + 1;
        };
        let close = match open {
            Punct::LParen => Punct::RParen,
            Punct::LBracket => Punct::RBracket,
            Punct::LBrace => Punct::RBrace,
            _ => return ahead + 1,
        };
        let mut depth = 0usize;
        let mut i = ahead;
        while i < ahead + LOOKAHEAD_LIMIT {
            match self.nth_kind(i) {
                Some(TokenKind::Punct(p)) if p == open => depth += 1,
                Some(TokenKind::Punct(p)) if p == close => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                None => break,
                _ => {}
            }
            i += 1;
        }
        ahead + 1
    }

    // -- writing events ----------------------------------------------------

    pub(crate) fn open(&mut self, kind: NodeKind) {
        self.events.push(Event::Open(kind));
    }

    /// A point in the event stream that a node may later be opened before.
    pub(crate) fn checkpoint(&self) -> u32 {
        self.events.len() as u32
    }

    /// Open a node as if it had been opened at `checkpoint`. Pairs with the
    /// usual [`Parser::close`].
    pub(crate) fn open_before(&mut self, checkpoint: u32, kind: NodeKind) {
        self.inserts.push((checkpoint, kind));
    }

    pub(crate) fn close(&mut self) {
        self.events.push(Event::Close);
    }

    /// Consume the current token into the node being built.
    pub(crate) fn bump(&mut self) {
        if self.at_end() {
            return;
        }
        self.events.push(Event::Token(TokenId(self.pos as u32)));
        self.pos += 1;
    }

    /// Consume the current token if it is this punctuator.
    pub(crate) fn eat(&mut self, punct: Punct) -> bool {
        if self.at(punct) {
            self.bump();
            return true;
        }
        false
    }

    /// Consume the current token if it is this word.
    pub(crate) fn eat_word(&mut self, word: &str) -> bool {
        if self.at_word(word) {
            self.bump();
            return true;
        }
        false
    }

    /// Wrap the current token in a one-token node — a `Name`, a `LiteralExpr`.
    pub(crate) fn bump_as(&mut self, kind: NodeKind) {
        self.open(kind);
        self.bump();
        self.close();
    }

    // -- diagnostics and recovery ------------------------------------------

    pub(crate) fn error(&mut self, code: ParseCode, message: impl Into<String>) {
        let span = self.here();
        self.error_at(code, message, span);
    }

    pub(crate) fn error_at(
        &mut self,
        code: ParseCode,
        message: impl Into<String>,
        span: ByteSpan,
    ) {
        self.diagnostics.push(SyntaxDiagnostic::error(code, message, span));
    }

    pub(crate) fn warn_at(
        &mut self,
        code: ParseCode,
        message: impl Into<String>,
        span: ByteSpan,
    ) {
        self.diagnostics.push(SyntaxDiagnostic::warning(code, message, span));
    }

    /// Require a punctuator, reporting where it should have been.
    pub(crate) fn expect(&mut self, punct: Punct) -> bool {
        if self.eat(punct) {
            return true;
        }
        if self.at_end() {
            // A file that simply stops is a file being typed, not a broken one.
            let span = self.here();
            self.warn_at(
                ParseCode::UnexpectedEndOfFile,
                format!("the file ends where a '{}' was expected", punct.as_str()),
                span,
            );
        } else {
            self.error(ParseCode::ExpectedToken, format!("expected '{}'", punct.as_str()));
        }
        false
    }

    /// Require a closing bracket, treating "the file just ends" as the
    /// half-typed state it usually is rather than as an error.
    pub(crate) fn expect_closing(&mut self, close: Punct, opened_at: ByteSpan) -> bool {
        if self.eat(close) {
            return true;
        }
        if self.at_end() {
            self.warn_at(
                ParseCode::UnclosedDelimiter,
                format!("this is never closed by a '{}'", close.as_str()),
                opened_at,
            );
        } else {
            self.error(ParseCode::ExpectedToken, format!("expected '{}'", close.as_str()));
        }
        false
    }

    /// Require the `;` that ends a declaration or a statement, recovering to
    /// the next boundary when it is not there.
    pub(crate) fn expect_semi(&mut self) {
        if self.eat(Punct::Semi) {
            return;
        }
        if self.at_end() {
            let span = self.here();
            self.warn_at(
                ParseCode::UnexpectedEndOfFile,
                "the file ends before this is finished",
                span,
            );
            return;
        }
        self.error(ParseCode::ExpectedToken, "expected ';'");
        self.recover_into_error();
    }

    /// Whether the cursor is on a token that ends a construct: a place where
    /// understanding can resume.
    pub(crate) fn at_boundary(&self) -> bool {
        self.at_end() || self.at(Punct::Semi) || self.at(Punct::RBrace)
    }

    /// Whether the cursor is on something that could begin a declaration or a
    /// statement.
    ///
    /// This is what stops recovery from eating the file: after a missing `;`,
    /// the very next word is far more likely to be the declaration the user is
    /// about to finish than more of the one that broke.
    pub(crate) fn at_construct_start(&self) -> bool {
        match self.nth_kind(0) {
            Some(TokenKind::Ident | TokenKind::Int | TokenKind::Float | TokenKind::Str) => true,
            Some(TokenKind::Punct(punct)) => matches!(
                punct,
                Punct::LBrace
                    | Punct::LParen
                    | Punct::LBracket
                    | Punct::Plus
                    | Punct::Minus
                    | Punct::Bang
                    | Punct::Tilde
                    | Punct::PlusPlus
                    | Punct::MinusMinus
            ),
            _ => false,
        }
    }

    /// Skip everything that cannot begin the next construct, stopping at a `;`
    /// (consumed), a `}` (left for the enclosing block), or the first token
    /// that could start something. Returns whether anything was skipped.
    pub(crate) fn skip_junk(&mut self) -> bool {
        if self.eat(Punct::Semi) {
            return true;
        }
        if self.at_end() || self.at(Punct::RBrace) || self.at_construct_start() {
            return false;
        }
        while !self.at_end() {
            if self.eat(Punct::Semi) {
                break;
            }
            if self.at(Punct::RBrace) || self.at_construct_start() {
                break;
            }
            self.bump();
        }
        true
    }

    /// [`Parser::skip_junk`], with whatever it skipped wrapped in an `Error`
    /// node so the tree still holds those tokens.
    pub(crate) fn recover_into_error(&mut self) {
        if self.eat(Punct::Semi) {
            return;
        }
        if self.at_end() || self.at(Punct::RBrace) || self.at_construct_start() {
            return;
        }
        self.open(NodeKind::Error);
        self.skip_junk();
        self.close();
    }

    /// The heavier recovery: skip a whole line the parser could not read.
    ///
    /// Stops at a `;` (consumed), a `}` (left in place), or the first point
    /// that *looks like a declaration* — a word followed by a word, which is
    /// the one thing that cannot happen inside a GLSL expression. Groups are
    /// stepped over whole.
    ///
    /// The two recoveries answer different questions.
    /// [`Parser::recover_into_error`] handles "you forgot a `;`", where the
    /// next word is almost certainly the next declaration and eating it would
    /// be a disaster. This one handles "I have no idea what this line is",
    /// where reporting once and moving on beats reporting on every token of it.
    pub(crate) fn recover_statement(&mut self) {
        if self.eat(Punct::Semi) {
            return;
        }
        if self.at_end() || self.at(Punct::RBrace) || decl::looks_like_declaration(self) {
            return;
        }
        self.open(NodeKind::Error);
        let mut groups = 0u32;
        while !self.at_end() {
            match self.punct() {
                Some(Punct::LParen | Punct::LBracket | Punct::LBrace) => {
                    groups += 1;
                    self.bump();
                    continue;
                }
                Some(Punct::RParen | Punct::RBracket) if groups > 0 => {
                    groups -= 1;
                    self.bump();
                    continue;
                }
                Some(Punct::RBrace) if groups > 0 => {
                    groups -= 1;
                    self.bump();
                    continue;
                }
                Some(Punct::RBrace) => break,
                Some(Punct::Semi) if groups == 0 => {
                    self.bump();
                    break;
                }
                _ => {}
            }
            if groups == 0 && decl::looks_like_declaration(self) {
                break;
            }
            self.bump();
        }
        self.close();
    }

    /// The last resort of every loop: one token into an `Error` node, so the
    /// cursor is guaranteed to move.
    pub(crate) fn skip_one_into_error(&mut self, message: &str) {
        if self.at_end() {
            return;
        }
        let span = self.here();
        let text = self.nth_text(0).to_string();
        self.open(NodeKind::Error);
        self.bump();
        self.close();
        self.error_at(
            ParseCode::UnexpectedToken,
            format!("unexpected '{text}': {message}"),
            span,
        );
    }

    /// Run `f` one level deeper, or give up on the construct when the source
    /// nests deeper than this parser recurses.
    ///
    /// Giving up consumes nothing: every caller either bumps a token of its own
    /// or is a loop with a no-progress guard, so the parse still terminates.
    pub(crate) fn deeper(&mut self, f: impl FnOnce(&mut Self)) {
        if self.depth >= MAX_DEPTH {
            self.open(NodeKind::Error);
            self.close();
            let span = self.here();
            // One report per parse is enough; a runaway nests every level.
            if !self.diagnostics.iter().any(|d| d.code == ParseCode::NestingLimit) {
                self.error_at(
                    ParseCode::NestingLimit,
                    format!("this nests more than {MAX_DEPTH} levels deep; the rest is not parsed"),
                    span,
                );
            }
            return;
        }
        self.depth += 1;
        f(self);
        self.depth -= 1;
    }

    // -- finishing ---------------------------------------------------------

    /// Merge the deferred `Open`s into the event list and build the tree.
    ///
    /// At one index the *last* recorded open is the outermost: precedence
    /// climbing records the inner node first (`a * b`) and the node that wraps
    /// it second (`a * b + c`).
    fn finish(self) -> SyntaxTree {
        let Parser { pp, events, inserts, diagnostics, .. } = self;
        let mut order: Vec<usize> = (0..inserts.len()).collect();
        order.sort_by(|&a, &b| inserts[a].0.cmp(&inserts[b].0).then(b.cmp(&a)));
        let mut merged: Vec<Event> = Vec::with_capacity(events.len() + inserts.len());
        let mut next = 0usize;
        for index in 0..=events.len() {
            while next < order.len() && inserts[order[next]].0 as usize == index {
                merged.push(Event::Open(inserts[order[next]].1));
                next += 1;
            }
            if let Some(event) = events.get(index) {
                merged.push(*event);
            }
        }
        build(&merged, pp, pp.tokens.len() as u32, diagnostics)
    }
}
