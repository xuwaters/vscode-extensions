//! One lexer, two dialects.
//!
//! Nothing here can fail. An unterminated block comment runs to end of file, a
//! stray byte becomes [`TokenKind::Unknown`], and lexing continues — the point
//! of this layer is that it produces a usable token stream for *any* input,
//! including the half-typed line the editor asks about most often.
//!
//! Comments are kept in the stream rather than skipped. Folding ranges and
//! semantic tokens both need them, and the parser filters them out with
//! [`TokenKind::is_trivia`].

use analyzer_core::spans::ByteSpan;

use crate::Language;

/// What a token is, at the granularity semantic highlighting needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A word that is reserved by the language.
    Keyword,
    /// A word naming a builtin type (`vec4f`, `mat3`, `sampler2D`).
    Type,
    /// Any other word.
    Ident,
    /// An integer or floating-point literal, suffixes included.
    Number,
    /// A string literal. GLSL only, and only really valid after `#include`.
    Str,
    /// `//` to end of line, or a `/* … */` block.
    Comment,
    /// The `#` and directive name of a GLSL preprocessor line. The rest of the
    /// line is lexed normally, so `#define PI 3.14` yields
    /// `Preprocessor Ident Number` and the parser can see the macro name.
    Preprocessor,
    /// A WGSL `@attribute` name, `@` included.
    Attribute,
    /// An operator or delimiter.
    Punct,
    /// A byte that belongs to none of the above.
    Unknown,
}

impl TokenKind {
    /// Whether the parser should skip past this token.
    pub fn is_trivia(self) -> bool {
        matches!(self, TokenKind::Comment)
    }
}

/// A lexed token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: ByteSpan,
    /// Zero-based line the token starts on. Carried rather than recomputed
    /// because the GLSL preprocessor is line-sensitive and folding groups runs
    /// of adjacent comments.
    pub line: u32,
    /// Whether only whitespace precedes this token on its line. This is what
    /// makes a GLSL `#` a directive rather than an operator.
    pub at_line_start: bool,
}

/// Lex a whole source.
pub fn tokenize(source: &str, language: Language) -> Vec<Token> {
    Lexer::new(source, language).run()
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    /// Whether everything since the last newline has been whitespace.
    fresh_line: bool,
    language: Language,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str, language: Language) -> Self {
        Self { src, bytes: src.as_bytes(), pos: 0, line: 0, fresh_line: true, language }
    }

    fn run(mut self) -> Vec<Token> {
        // Shader sources run about six bytes to the token.
        let mut tokens = Vec::with_capacity(self.src.len() / 6 + 1);
        loop {
            self.skip_whitespace();
            if self.pos >= self.bytes.len() {
                return tokens;
            }
            let start = self.pos;
            let line = self.line;
            let at_line_start = self.fresh_line;
            let kind = self.next_kind();
            // Every branch must consume at least one byte or this loops forever.
            debug_assert!(self.pos > start, "lexer stalled at {start}");
            self.fresh_line = false;
            tokens.push(Token {
                kind,
                span: ByteSpan::from_usize(start, self.pos),
                line,
                at_line_start,
            });
        }
    }

    fn peek(&self, ahead: usize) -> u8 {
        self.bytes.get(self.pos + ahead).copied().unwrap_or(0)
    }

    fn bump(&mut self) {
        if self.peek(0) == b'\n' {
            self.line += 1;
            self.fresh_line = true;
        }
        // Stay on a char boundary: a multi-byte identifier or a comment with
        // an em-dash in it must not be split mid-scalar.
        self.pos += 1;
        while self.pos < self.bytes.len() && !self.src.is_char_boundary(self.pos) {
            self.pos += 1;
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len() {
            match self.peek(0) {
                b' ' | b'\t' | b'\r' => self.bump(),
                b'\n' => {
                    self.bump();
                    self.fresh_line = true;
                }
                // A backslash-newline splices two physical lines into one
                // logical line. GLSL's preprocessor relies on it; treating the
                // pair as whitespace is enough for everything here.
                b'\\' if matches!(self.peek(1), b'\n' | b'\r') => {
                    self.bump();
                    self.bump();
                }
                _ => return,
            }
        }
    }

    fn next_kind(&mut self) -> TokenKind {
        let c = self.peek(0);
        match c {
            b'/' if self.peek(1) == b'/' => self.line_comment(),
            b'/' if self.peek(1) == b'*' => self.block_comment(),
            b'#' if self.language == Language::Glsl && self.fresh_line => self.directive(),
            b'@' if self.language == Language::Wgsl => self.attribute(),
            b'"' if self.language == Language::Glsl => self.string(),
            b'0'..=b'9' => self.number(),
            // `.5` is a number; `.x` is a swizzle.
            b'.' if self.peek(1).is_ascii_digit() => self.number(),
            _ if is_ident_start(c) => self.word(),
            _ => {
                self.bump();
                if c.is_ascii_graphic() { TokenKind::Punct } else { TokenKind::Unknown }
            }
        }
    }

    fn line_comment(&mut self) -> TokenKind {
        while self.pos < self.bytes.len() && self.peek(0) != b'\n' {
            self.bump();
        }
        TokenKind::Comment
    }

    /// WGSL block comments nest; GLSL's do not. Both run to end of file when
    /// unterminated, which is the common state while one is being typed.
    fn block_comment(&mut self) -> TokenKind {
        self.bump();
        self.bump();
        let nests = self.language == Language::Wgsl;
        let mut depth = 1usize;
        while self.pos < self.bytes.len() {
            if self.peek(0) == b'*' && self.peek(1) == b'/' {
                self.bump();
                self.bump();
                depth -= 1;
                if depth == 0 {
                    break;
                }
            } else if nests && self.peek(0) == b'/' && self.peek(1) == b'*' {
                self.bump();
                self.bump();
                depth += 1;
            } else {
                self.bump();
            }
        }
        TokenKind::Comment
    }

    /// `#` plus the directive name. The rest of the line lexes normally.
    fn directive(&mut self) -> TokenKind {
        self.bump();
        // `#  version` is legal: whitespace may follow the hash.
        while matches!(self.peek(0), b' ' | b'\t') {
            self.bump();
        }
        while is_ident_continue(self.peek(0)) {
            self.bump();
        }
        TokenKind::Preprocessor
    }

    fn attribute(&mut self) -> TokenKind {
        self.bump();
        while is_ident_continue(self.peek(0)) {
            self.bump();
        }
        TokenKind::Attribute
    }

    fn string(&mut self) -> TokenKind {
        self.bump();
        while self.pos < self.bytes.len() {
            match self.peek(0) {
                b'"' => {
                    self.bump();
                    break;
                }
                // Don't run past the end of the line: an unterminated string
                // should not swallow the rest of the file.
                b'\n' => break,
                b'\\' => {
                    self.bump();
                    self.bump();
                }
                _ => self.bump(),
            }
        }
        TokenKind::Str
    }

    /// Numbers are lexed loosely on purpose: every WGSL and GLSL literal form
    /// is accepted, and so are some that are not literals at all. Nothing
    /// downstream evaluates them.
    fn number(&mut self) -> TokenKind {
        if self.peek(0) == b'0' && matches!(self.peek(1), b'x' | b'X') {
            self.bump();
            self.bump();
            while self.peek(0).is_ascii_hexdigit()
                || self.peek(0) == b'.'
                || matches!(self.peek(0), b'p' | b'P')
                || (matches!(self.peek(0), b'+' | b'-')
                    && matches!(self.bytes.get(self.pos - 1), Some(b'p' | b'P')))
            {
                self.bump();
            }
        } else {
            while self.peek(0).is_ascii_digit()
                || self.peek(0) == b'.'
                || matches!(self.peek(0), b'e' | b'E')
                || (matches!(self.peek(0), b'+' | b'-')
                    && matches!(self.bytes.get(self.pos - 1), Some(b'e' | b'E')))
            {
                self.bump();
            }
        }
        // Suffixes: `f`, `h`, `u`, `i`, `lf`, `LF`, `U`, `ul`.
        while is_ident_continue(self.peek(0)) {
            self.bump();
        }
        TokenKind::Number
    }

    fn word(&mut self) -> TokenKind {
        let start = self.pos;
        while is_ident_continue(self.peek(0)) {
            self.bump();
        }
        let text = &self.src[start..self.pos];
        crate::builtins::classify_word(text, self.language)
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

fn is_ident_continue(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}
