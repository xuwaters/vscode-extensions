//! Recursive-descent parser for proto3.
//!
//! The parser is designed for editor use: on any syntactic mismatch it emits
//! a diagnostic and resyncs to a set of follow tokens, keeping the rest of
//! the file parseable. Nodes that cannot be filled in are represented via
//! `Missing` / `TypeRef::Missing` so downstream passes can tolerate them.

mod enum_;
mod file;
mod message;
mod options;
mod service;

use crate::ast;
use crate::diagnostics::{DiagnosticCode, ProtoDiagnostic, Severity};
use crate::lexer::{Token, TokenKind};
use crate::spans::ByteSpan;
use smol_str::SmolStr;

/// Deepest brace/bracket/angle nesting the parser will descend into.
///
/// Every nested body (message, enum, oneof, service, rpc, extend) and every
/// nested value (option aggregate `{`, option list `[`, `map<`) takes one
/// level. Real schemas stay in the low single digits; 64 is far above that,
/// yet it bounds the AST so the recursive passes that walk it afterwards
/// (resolve, checks, symbols, formatting, serde, Drop) fit in the 1 MB WASM
/// stack. A debug build, whose frames are the fattest, overflows 1 MB
/// between 128 and 256 levels, so 64 keeps at least a 2x margin.
pub const MAX_NESTING_DEPTH: usize = 64;

pub struct Parser<'s> {
    source: &'s str,
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<ProtoDiagnostic>,
    /// Current nesting depth, bounded by [`MAX_NESTING_DEPTH`].
    depth: usize,
}

impl<'s> Parser<'s> {
    pub fn new(source: &'s str, tokens: Vec<Token>) -> Self {
        Parser { source, tokens, pos: 0, diagnostics: Vec::new(), depth: 0 }
    }

    pub fn into_diagnostics(self) -> Vec<ProtoDiagnostic> {
        self.diagnostics
    }

    pub(crate) fn source(&self) -> &str {
        self.source
    }

    pub(crate) fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    pub(crate) fn peek_at(&self, n: usize) -> &Token {
        &self.tokens[(self.pos + n).min(self.tokens.len() - 1)]
    }

    pub(crate) fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    pub(crate) fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if !matches!(t.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        // Surface any lex errors encountered along the way.
        if let TokenKind::LexError(ref err) = t.kind {
            use crate::lexer::LexErrorKind;
            let (code, msg) = match err {
                LexErrorKind::UnterminatedString => (DiagnosticCode::LexUnterminatedString, "Unterminated string literal".to_string()),
                LexErrorKind::UnterminatedBlockComment => (DiagnosticCode::LexUnterminatedComment, "Unterminated block comment".to_string()),
                LexErrorKind::InvalidEscape(c) => (DiagnosticCode::LexInvalidEscape, format!("Invalid escape sequence `\\{}`", c)),
                LexErrorKind::StrayChar(c) => (DiagnosticCode::ParseUnexpectedToken, format!("Stray character `{}`", c)),
                LexErrorKind::InvalidNumber(raw) => (DiagnosticCode::ParseUnexpectedToken, format!("Invalid number literal `{}`", raw)),
            };
            self.diagnostics.push(ProtoDiagnostic::new(code, Severity::Error, msg, t.span));
        }
        t
    }

    pub(crate) fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(kind)
    }

    pub(crate) fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, kind: &TokenKind, label: &str) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            let found = describe_kind(self.peek_kind());
            let span = self.peek().span;
            self.diagnostics.push(ProtoDiagnostic::new(
                DiagnosticCode::ParseExpected,
                Severity::Error,
                format!("Expected {}, found {}", label, found),
                span,
            ));
            None
        }
    }

    pub(crate) fn unexpected(&mut self, ctx: &str) {
        let span = self.peek().span;
        let found = describe_kind(self.peek_kind());
        self.diagnostics.push(ProtoDiagnostic::new(
            DiagnosticCode::ParseUnexpectedToken,
            Severity::Error,
            format!("Unexpected token {} (while parsing {})", found, ctx),
            span,
        ));
    }

    pub(crate) fn error_at(&mut self, code: DiagnosticCode, msg: impl Into<String>, span: ByteSpan) {
        self.diagnostics
            .push(ProtoDiagnostic::new(code, Severity::Error, msg.into(), span));
    }

    /// Advance tokens until we hit one of the follow-set tokens, or EOF.
    pub(crate) fn sync_to(&mut self, follow: &[TokenKindTag]) {
        while !matches!(self.peek_kind(), TokenKind::Eof) {
            if follow.iter().any(|tag| tag.matches(self.peek_kind())) {
                return;
            }
            self.bump();
        }
    }

    /// Run `f` one nesting level deeper. The construct starts at the current
    /// token and its body is delimited by `open` / `close`. Past
    /// [`MAX_NESTING_DEPTH`] the construct is reported, skipped as a whole,
    /// and `None` is returned without calling `f`.
    pub(crate) fn nested<T>(
        &mut self,
        open: TokenKind,
        close: TokenKind,
        f: impl FnOnce(&mut Self) -> T,
    ) -> Option<T> {
        if self.depth >= MAX_NESTING_DEPTH {
            let span = self.peek().span;
            self.error_at(
                DiagnosticCode::ParseUnexpectedToken,
                format!("nesting too deep (limit {})", MAX_NESTING_DEPTH),
                span,
            );
            self.skip_balanced(&open, &close);
            return None;
        }
        self.depth += 1;
        let out = f(self);
        self.depth -= 1;
        Some(out)
    }

    /// Span from `start` to the end of the last consumed token.
    pub(crate) fn span_since(&self, start: ByteSpan) -> ByteSpan {
        let end = self.tokens[self.pos.saturating_sub(1)].span.end;
        ByteSpan::new(start.start, end.max(start.end))
    }

    /// Iteratively skip a construct: tokens up to its first `open`, then
    /// through the matching `close`. Stops without consuming at a `close`
    /// that belongs to the enclosing construct, after a `;` met before any
    /// `open` (a body-less declaration), or at EOF.
    pub(crate) fn skip_balanced(&mut self, open: &TokenKind, close: &TokenKind) {
        let same = |a: &TokenKind, b: &TokenKind| std::mem::discriminant(a) == std::mem::discriminant(b);
        let mut depth = 0usize;
        loop {
            let k = self.peek_kind();
            if matches!(k, TokenKind::Eof) {
                return;
            }
            if same(k, open) {
                depth += 1;
            } else if same(k, close) {
                if depth == 0 {
                    return;
                }
                depth -= 1;
                if depth == 0 {
                    self.bump();
                    return;
                }
            } else if depth == 0 && matches!(k, TokenKind::Semi) {
                self.bump();
                return;
            }
            self.bump();
        }
    }

    pub(crate) fn parse_ident(&mut self) -> Option<ast::Ident> {
        match &self.peek().kind {
            TokenKind::Ident(_)
            // Allow keyword-ident collisions (common: `message Map { ... }`
            // is legal). We tolerate it by accepting any keyword-class token
            // in identifier position and extracting its source slice.
            | TokenKind::KwSyntax
            | TokenKind::KwEdition
            | TokenKind::KwPackage
            | TokenKind::KwImport
            | TokenKind::KwPublic
            | TokenKind::KwWeak
            | TokenKind::KwOption
            | TokenKind::KwReturns
            | TokenKind::KwStream
            | TokenKind::KwTo
            | TokenKind::KwMax
            | TokenKind::KwGroup => {
                let t = self.bump();
                let slice = &self.source[t.span.start as usize..t.span.end as usize];
                Some(ast::Ident { name: SmolStr::new(slice), span: t.span })
            }
            _ => None,
        }
    }

    /// Parse an identifier and report a diagnostic if not found.
    pub(crate) fn expect_ident(&mut self, label: &str) -> Option<ast::Ident> {
        match self.parse_ident() {
            Some(i) => Some(i),
            None => {
                let span = self.peek().span;
                let found = describe_kind(self.peek_kind());
                self.diagnostics.push(ProtoDiagnostic::new(
                    DiagnosticCode::ParseExpected,
                    Severity::Error,
                    format!("Expected {}, found {}", label, found),
                    span,
                ));
                None
            }
        }
    }

    pub(crate) fn parse_qualified_name(&mut self) -> Option<ast::QualifiedName> {
        let start_span = self.peek().span;
        let absolute = self.eat(&TokenKind::Dot);
        let first = match self.parse_ident() {
            Some(i) => i,
            None if absolute => {
                return Some(ast::QualifiedName {
                    absolute: true,
                    parts: Vec::new(),
                    span: start_span,
                });
            }
            None => return None,
        };
        let mut parts = vec![first];
        let mut end_span = parts.last().unwrap().span;
        while matches!(self.peek_kind(), TokenKind::Dot)
            && matches!(self.peek_at(1).kind,
                TokenKind::Ident(_)
                | TokenKind::KwSyntax | TokenKind::KwEdition | TokenKind::KwPackage
                | TokenKind::KwImport | TokenKind::KwPublic | TokenKind::KwWeak
                | TokenKind::KwOption | TokenKind::KwReturns | TokenKind::KwStream
                | TokenKind::KwTo | TokenKind::KwMax | TokenKind::KwGroup)
        {
            self.bump(); // dot
            let next = self.parse_ident()?;
            end_span = next.span;
            parts.push(next);
        }
        Some(ast::QualifiedName {
            absolute,
            parts,
            span: ByteSpan::new(start_span.start, end_span.end),
        })
    }

    pub(crate) fn parse_int_value(&mut self) -> Option<ast::IntValue> {
        let start = self.peek().span;
        let negative = self.eat(&TokenKind::Minus);
        let _ = self.eat(&TokenKind::Plus);
        match &self.peek().kind {
            TokenKind::IntLit(s) => {
                let raw = s.clone();
                let span = self.bump().span;
                Some(ast::IntValue {
                    raw,
                    negative,
                    span: ByteSpan::new(start.start, span.end),
                })
            }
            _ => {
                if negative {
                    self.diagnostics.push(ProtoDiagnostic::new(
                        DiagnosticCode::ParseExpected,
                        Severity::Error,
                        "Expected integer literal after `-`",
                        start,
                    ));
                }
                None
            }
        }
    }
}

pub(crate) fn describe_kind(k: &TokenKind) -> String {
    match k {
        TokenKind::Ident(s) => format!("identifier `{}`", s),
        TokenKind::IntLit(s) => format!("integer `{}`", s),
        TokenKind::FloatLit(s) => format!("float `{}`", s),
        TokenKind::StringLit(_) => "string literal".into(),
        TokenKind::LBrace => "`{`".into(),
        TokenKind::RBrace => "`}`".into(),
        TokenKind::LParen => "`(`".into(),
        TokenKind::RParen => "`)`".into(),
        TokenKind::LBracket => "`[`".into(),
        TokenKind::RBracket => "`]`".into(),
        TokenKind::LAngle => "`<`".into(),
        TokenKind::RAngle => "`>`".into(),
        TokenKind::Semi => "`;`".into(),
        TokenKind::Comma => "`,`".into(),
        TokenKind::Dot => "`.`".into(),
        TokenKind::Eq => "`=`".into(),
        TokenKind::Minus => "`-`".into(),
        TokenKind::Plus => "`+`".into(),
        TokenKind::Colon => "`:`".into(),
        TokenKind::Slash => "`/`".into(),
        TokenKind::Eof => "end of file".into(),
        TokenKind::LexError(_) => "invalid token".into(),
        other => format!("`{}`", keyword_or_type_text(other).unwrap_or("<kw>")),
    }
}

fn keyword_or_type_text(k: &TokenKind) -> Option<&'static str> {
    Some(match k {
        TokenKind::KwSyntax => "syntax",
        TokenKind::KwEdition => "edition",
        TokenKind::KwPackage => "package",
        TokenKind::KwImport => "import",
        TokenKind::KwPublic => "public",
        TokenKind::KwWeak => "weak",
        TokenKind::KwOption => "option",
        TokenKind::KwMessage => "message",
        TokenKind::KwEnum => "enum",
        TokenKind::KwService => "service",
        TokenKind::KwRpc => "rpc",
        TokenKind::KwReturns => "returns",
        TokenKind::KwStream => "stream",
        TokenKind::KwOneof => "oneof",
        TokenKind::KwMap => "map",
        TokenKind::KwReserved => "reserved",
        TokenKind::KwTo => "to",
        TokenKind::KwMax => "max",
        TokenKind::KwRepeated => "repeated",
        TokenKind::KwOptional => "optional",
        TokenKind::KwRequired => "required",
        TokenKind::KwGroup => "group",
        TokenKind::KwExtensions => "extensions",
        TokenKind::KwExtend => "extend",
        TokenKind::KwTrue => "true",
        TokenKind::KwFalse => "false",
        TokenKind::TyDouble => "double",
        TokenKind::TyFloat => "float",
        TokenKind::TyInt32 => "int32",
        TokenKind::TyInt64 => "int64",
        TokenKind::TyUint32 => "uint32",
        TokenKind::TyUint64 => "uint64",
        TokenKind::TySint32 => "sint32",
        TokenKind::TySint64 => "sint64",
        TokenKind::TyFixed32 => "fixed32",
        TokenKind::TyFixed64 => "fixed64",
        TokenKind::TySfixed32 => "sfixed32",
        TokenKind::TySfixed64 => "sfixed64",
        TokenKind::TyBool => "bool",
        TokenKind::TyString => "string",
        TokenKind::TyBytes => "bytes",
        _ => return None,
    })
}

/// A coarse discriminant for `TokenKind` used by `sync_to`. We can't use
/// `std::mem::discriminant` directly in a `&[...]` literal because the
/// payload-bearing variants need to be constructed.
#[derive(Debug, Clone, Copy)]
pub enum TokenKindTag {
    Semi,
    RBrace,
    KwMessage,
    KwEnum,
    KwService,
    KwRpc,
    KwImport,
    KwOption,
    KwPackage,
    KwOneof,
    KwReserved,
    KwExtensions,
    KwExtend,
    KwSyntax,
    KwEdition,
    Eof,
}

impl TokenKindTag {
    pub fn matches(self, k: &TokenKind) -> bool {
        matches!(
            (self, k),
            (TokenKindTag::Semi, TokenKind::Semi)
                | (TokenKindTag::RBrace, TokenKind::RBrace)
                | (TokenKindTag::KwMessage, TokenKind::KwMessage)
                | (TokenKindTag::KwEnum, TokenKind::KwEnum)
                | (TokenKindTag::KwService, TokenKind::KwService)
                | (TokenKindTag::KwRpc, TokenKind::KwRpc)
                | (TokenKindTag::KwImport, TokenKind::KwImport)
                | (TokenKindTag::KwOption, TokenKind::KwOption)
                | (TokenKindTag::KwPackage, TokenKind::KwPackage)
                | (TokenKindTag::KwOneof, TokenKind::KwOneof)
                | (TokenKindTag::KwReserved, TokenKind::KwReserved)
                | (TokenKindTag::KwExtensions, TokenKind::KwExtensions)
                | (TokenKindTag::KwExtend, TokenKind::KwExtend)
                | (TokenKindTag::KwSyntax, TokenKind::KwSyntax)
                | (TokenKindTag::KwEdition, TokenKind::KwEdition)
                | (TokenKindTag::Eof, TokenKind::Eof)
        )
    }
}

pub(crate) const TOP_LEVEL_SYNC: &[TokenKindTag] = &[
    TokenKindTag::KwMessage,
    TokenKindTag::KwEnum,
    TokenKindTag::KwService,
    TokenKindTag::KwImport,
    TokenKindTag::KwOption,
    TokenKindTag::KwPackage,
    TokenKindTag::KwSyntax,
    TokenKindTag::KwEdition,
    TokenKindTag::KwExtend,
    TokenKindTag::Semi,
    TokenKindTag::Eof,
];

pub(crate) const MESSAGE_BODY_SYNC: &[TokenKindTag] = &[
    TokenKindTag::Semi,
    TokenKindTag::RBrace,
    TokenKindTag::KwMessage,
    TokenKindTag::KwEnum,
    TokenKindTag::KwOneof,
    TokenKindTag::KwReserved,
    TokenKindTag::KwExtensions,
    TokenKindTag::KwOption,
    TokenKindTag::Eof,
];
