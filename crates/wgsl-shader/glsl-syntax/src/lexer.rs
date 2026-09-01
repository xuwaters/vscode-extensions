//! The GLSL token layer — GLSL 4.60 §3.
//!
//! Nothing here can fail. An unterminated block comment runs to end of file, a
//! stray byte becomes [`TokenKind::Unknown`], and lexing continues. The token
//! stream is *lossless*: every byte of the source belongs to exactly one token
//! or one piece of trivia, so the preprocessor can rebuild inactive regions and
//! the parser (phase 3) can rebuild the file verbatim.
//!
//! Trivia is part of the stream rather than skipped, and whitespace is split
//! from newlines, because the preprocessor is line-oriented: a `#` only opens a
//! directive when nothing but whitespace precedes it on its logical line.
//!
//! Line continuation (`\` immediately followed by a newline) is handled *here*,
//! at the character level, exactly as glslang's scanner does — so
//!
//! ```text
//! #define FO\
//! O 1
//! ```
//!
//! defines `FOO`. A splice between two tokens becomes its own
//! [`TokenKind::LineContinuation`] trivia; a splice *inside* a token is covered
//! by that token's span and flagged with [`Token::spliced`], which is what makes
//! [`Token::text`] return a `Cow` rather than a plain `&str`.

use std::borrow::Cow;

use analyzer_core::spans::ByteSpan;

/// A punctuator or operator, lexed at max munch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Punct {
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Dot,
    Comma,
    Colon,
    Semi,
    Question,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Tilde,
    Bang,
    Amp,
    Caret,
    Pipe,
    Lt,
    Gt,
    Le,
    Ge,
    EqEq,
    Ne,
    Shl,
    Shr,
    AndAnd,
    XorXor,
    OrOr,
    PlusPlus,
    MinusMinus,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    ShlEq,
    ShrEq,
    AmpEq,
    CaretEq,
    PipeEq,
    /// Opens a directive line, and is the stringify operator in a macro body.
    Hash,
    /// The token-pasting operator. Desktop GLSL 130+; ES rejects it.
    HashHash,
}

impl Punct {
    /// The exact spelling. Round-trips: `Punct::ShlEq.as_str() == "<<="`.
    pub fn as_str(self) -> &'static str {
        match self {
            Punct::LParen => "(",
            Punct::RParen => ")",
            Punct::LBracket => "[",
            Punct::RBracket => "]",
            Punct::LBrace => "{",
            Punct::RBrace => "}",
            Punct::Dot => ".",
            Punct::Comma => ",",
            Punct::Colon => ":",
            Punct::Semi => ";",
            Punct::Question => "?",
            Punct::Plus => "+",
            Punct::Minus => "-",
            Punct::Star => "*",
            Punct::Slash => "/",
            Punct::Percent => "%",
            Punct::Tilde => "~",
            Punct::Bang => "!",
            Punct::Amp => "&",
            Punct::Caret => "^",
            Punct::Pipe => "|",
            Punct::Lt => "<",
            Punct::Gt => ">",
            Punct::Le => "<=",
            Punct::Ge => ">=",
            Punct::EqEq => "==",
            Punct::Ne => "!=",
            Punct::Shl => "<<",
            Punct::Shr => ">>",
            Punct::AndAnd => "&&",
            Punct::XorXor => "^^",
            Punct::OrOr => "||",
            Punct::PlusPlus => "++",
            Punct::MinusMinus => "--",
            Punct::Eq => "=",
            Punct::PlusEq => "+=",
            Punct::MinusEq => "-=",
            Punct::StarEq => "*=",
            Punct::SlashEq => "/=",
            Punct::PercentEq => "%=",
            Punct::ShlEq => "<<=",
            Punct::ShrEq => ">>=",
            Punct::AmpEq => "&=",
            Punct::CaretEq => "^=",
            Punct::PipeEq => "|=",
            Punct::Hash => "#",
            Punct::HashHash => "##",
        }
    }
}

/// What a token is, at preprocessing-token granularity.
///
/// Keywords are *not* separated from identifiers here. Which words are reserved
/// depends on the version the file declares, which is not known until `#version`
/// has been read, so classification belongs to the parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// A word: identifier, keyword or type name.
    Ident,
    /// An integer literal in any base, suffix included.
    Int,
    /// A floating-point literal, suffix included.
    Float,
    /// A `"…"` literal. Only ever legal after `#include` or `#line`, and the
    /// product of the stringify operator.
    Str,
    Punct(Punct),
    /// `//` to end of line, or a `/* … */` block. Trivia.
    Comment,
    /// A run of spaces, tabs and lone carriage returns. Trivia.
    Space,
    /// One line terminator, `\n` or `\r\n`. Trivia to everything but the
    /// preprocessor, which ends directives on it.
    Newline,
    /// A `\` immediately followed by a line terminator, sitting between two
    /// tokens. Trivia; it exists so that every byte is owned.
    LineContinuation,
    /// A byte that belongs to none of the above (`$`, `@`, a lone `\`).
    Unknown,
}

impl TokenKind {
    /// Whether the parser should skip past this token.
    ///
    /// `Newline` counts: by the time the parser runs, the preprocessor has
    /// already consumed every line-sensitive construct.
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            TokenKind::Comment
                | TokenKind::Space
                | TokenKind::Newline
                | TokenKind::LineContinuation
        )
    }

    /// Whether this is a literal the `#if` evaluator or the parser can read a
    /// value out of.
    pub fn is_literal(self) -> bool {
        matches!(self, TokenKind::Int | TokenKind::Float | TokenKind::Str)
    }
}

/// A lexed token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// The full physical extent, line continuations included.
    pub span: ByteSpan,
    /// Zero-based *physical* line the token starts on.
    pub line: u32,
    /// Whether only whitespace precedes this token on its logical line. This is
    /// what makes a `#` a directive rather than an operator.
    pub at_line_start: bool,
    /// Whether a line continuation was spliced out of the middle of this token,
    /// so its text is not a contiguous slice of the source.
    pub spliced: bool,
}

impl Token {
    /// The token's spelling, with any spliced line continuations removed.
    ///
    /// Borrowed in every case but the spliced one, which is rare enough that
    /// paying an allocation for it is the right trade.
    pub fn text<'a>(&self, source: &'a str) -> Cow<'a, str> {
        let raw = &source[self.span.start as usize..self.span.end as usize];
        if !self.spliced {
            return Cow::Borrowed(raw);
        }
        let mut out = String::with_capacity(raw.len());
        let bytes = raw.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                if let Some(skip) = splice_len(bytes, i) {
                    i += skip;
                    continue;
                }
            }
            let start = i;
            i += 1;
            while i < bytes.len() && !raw.is_char_boundary(i) {
                i += 1;
            }
            out.push_str(&raw[start..i]);
        }
        Cow::Owned(out)
    }
}

/// The length of the line continuation starting at `at`, if there is one.
#[inline]
fn splice_len(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'\\') {
        return None;
    }
    match bytes.get(at + 1) {
        Some(b'\n') => Some(2),
        Some(b'\r') if bytes.get(at + 2) == Some(&b'\n') => Some(3),
        Some(b'\r') => Some(2),
        _ => None,
    }
}

/// Lex a whole source. Never fails, never panics, never loses a byte.
pub fn tokenize(source: &str) -> Vec<Token> {
    Lexer::new(source).run()
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    /// Whether everything since the last newline has been whitespace.
    fresh_line: bool,
    /// Set by [`Lexer::bump`] when it steps over a splice; read and reset once
    /// per token.
    spliced: bool,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Lexer { src, bytes: src.as_bytes(), pos: 0, line: 0, fresh_line: true, spliced: false }
    }

    fn run(mut self) -> Vec<Token> {
        // Shader sources run about four bytes to the token once trivia counts.
        let mut tokens = Vec::with_capacity(self.src.len() / 4 + 1);
        while self.pos < self.bytes.len() {
            let start = self.pos;
            let line = self.line;
            let at_line_start = self.fresh_line;
            self.spliced = false;
            let kind = self.next_kind();
            // Every branch must consume at least one byte or this loops forever.
            debug_assert!(self.pos > start, "lexer stalled at {start}");
            match kind {
                TokenKind::Newline => self.fresh_line = true,
                // Whitespace and splices leave a `#` still at the start of its
                // logical line.
                TokenKind::Space | TokenKind::LineContinuation => {}
                _ => self.fresh_line = false,
            }
            tokens.push(Token {
                kind,
                span: ByteSpan::from_usize(start, self.pos),
                line,
                at_line_start,
                spliced: self.spliced,
            });
        }
        tokens
    }

    /// The physical offset of the logical character at or after `from`, i.e.
    /// `from` with any line continuations skipped.
    #[inline]
    fn logical(&self, from: usize) -> usize {
        // A splice always starts with a `\`, and a `\` is rare, so the test
        // that answers "nothing to skip" must be one byte compare.
        if self.bytes.get(from) != Some(&b'\\') {
            return from;
        }
        let mut p = from;
        while let Some(skip) = splice_len(self.bytes, p) {
            p += skip;
        }
        p
    }

    /// The `ahead`-th logical byte from the cursor, or 0 at end of input.
    #[inline]
    fn peek(&self, ahead: usize) -> u8 {
        let mut p = self.logical(self.pos);
        for _ in 0..ahead {
            if p >= self.bytes.len() {
                return 0;
            }
            p = self.logical(p + 1);
        }
        self.bytes.get(p).copied().unwrap_or(0)
    }

    /// Consume every logical byte at the cursor that `accept` takes.
    ///
    /// The point is the inner loop: a run of ordinary bytes — the letters of an
    /// identifier, the digits of a number, a stretch of indentation — is
    /// consumed without going through [`Lexer::peek`] and [`Lexer::bump`] once
    /// per character. Only a `\` drops out to the general path, and only that
    /// path can move `line` or set `spliced`, because none of the bytes the
    /// callers accept is a line terminator.
    fn take_while(&mut self, accept: impl Fn(u8) -> bool) {
        loop {
            while self.pos < self.bytes.len() {
                let byte = self.bytes[self.pos];
                if byte == b'\\' || !accept(byte) {
                    break;
                }
                self.pos += 1;
            }
            // Either the run ended, or a `\` is in the way — and a `\` is only
            // in the way when it opens a splice the run continues past.
            if !accept(self.peek(0)) {
                return;
            }
            self.bump();
        }
    }

    /// Consume one logical character, stepping invisibly over any splices in
    /// front of it and staying on a UTF-8 boundary.
    #[inline]
    fn bump(&mut self) {
        while let Some(skip) = splice_len(self.bytes, self.pos) {
            self.pos += skip;
            self.line += 1;
            self.spliced = true;
        }
        if self.pos >= self.bytes.len() {
            return;
        }
        if self.bytes[self.pos] == b'\n' {
            self.line += 1;
        }
        self.pos += 1;
        while self.pos < self.bytes.len() && !self.src.is_char_boundary(self.pos) {
            self.pos += 1;
        }
    }

    fn next_kind(&mut self) -> TokenKind {
        // Splices are only their own token between two tokens; inside one,
        // `bump` swallows them.
        if let Some(skip) = splice_len(self.bytes, self.pos) {
            self.pos += skip;
            self.line += 1;
            return TokenKind::LineContinuation;
        }
        let c = self.peek(0);
        match c {
            b'\n' => {
                self.bump();
                TokenKind::Newline
            }
            b'\r' if self.peek(1) == b'\n' => {
                self.bump();
                self.bump();
                TokenKind::Newline
            }
            b' ' | b'\t' | b'\r' | 0x0b | 0x0c => self.space(),
            b'/' if self.peek(1) == b'/' => self.line_comment(),
            b'/' if self.peek(1) == b'*' => self.block_comment(),
            b'"' => self.string(),
            b'0'..=b'9' => self.number(),
            // `.5` is a number; `.x` is a swizzle.
            b'.' if self.peek(1).is_ascii_digit() => self.number(),
            _ if is_ident_start(c) => self.word(),
            _ => match self.punct() {
                Some(p) => TokenKind::Punct(p),
                None => {
                    self.bump();
                    TokenKind::Unknown
                }
            },
        }
    }

    fn space(&mut self) -> TokenKind {
        // A lone `\r` is whitespace; a `\r\n` is a line terminator, and only
        // the second byte tells them apart — which is why it is not part of the
        // run [`Lexer::take_while`] consumes.
        loop {
            self.take_while(|c| matches!(c, b' ' | b'\t' | 0x0b | 0x0c));
            if self.peek(0) == b'\r' && self.peek(1) != b'\n' {
                self.bump();
                continue;
            }
            return TokenKind::Space;
        }
    }

    /// A `//` comment runs to the newline — but a splice at the end of the line
    /// continues it onto the next, because `peek` sees past the splice.
    fn line_comment(&mut self) -> TokenKind {
        while self.pos < self.bytes.len() && !matches!(self.peek(0), b'\n' | b'\r') {
            self.bump();
        }
        TokenKind::Comment
    }

    /// GLSL block comments do not nest, and run to end of file when
    /// unterminated — the common state while one is being typed.
    fn block_comment(&mut self) -> TokenKind {
        self.bump();
        self.bump();
        while self.pos < self.bytes.len() {
            if self.peek(0) == b'*' && self.peek(1) == b'/' {
                self.bump();
                self.bump();
                break;
            }
            self.bump();
        }
        TokenKind::Comment
    }

    fn string(&mut self) -> TokenKind {
        self.bump();
        while self.pos < self.bytes.len() {
            match self.peek(0) {
                b'"' => {
                    self.bump();
                    break;
                }
                // An unterminated string must not swallow the rest of the file.
                b'\n' | b'\r' => break,
                b'\\' => {
                    self.bump();
                    self.bump();
                }
                _ => self.bump(),
            }
        }
        TokenKind::Str
    }

    /// Numbers are lexed loosely and classified afterwards. `0x1p3` and `1e`
    /// are accepted here and rejected later; nothing at this layer evaluates a
    /// literal, and the `#if` evaluator parses integers itself.
    fn number(&mut self) -> TokenKind {
        let mut float = false;
        if self.peek(0) == b'0' && matches!(self.peek(1), b'x' | b'X') {
            self.bump();
            self.bump();
            self.take_while(|c| c.is_ascii_hexdigit());
        } else {
            self.take_while(|c| c.is_ascii_digit());
            if self.peek(0) == b'.' {
                float = true;
                self.bump();
                self.take_while(|c| c.is_ascii_digit());
            }
            if matches!(self.peek(0), b'e' | b'E') {
                let exponent = self.peek(1).is_ascii_digit()
                    || (matches!(self.peek(1), b'+' | b'-') && self.peek(2).is_ascii_digit());
                if exponent {
                    float = true;
                    self.bump();
                    self.bump();
                    self.take_while(|c| c.is_ascii_digit());
                }
            }
        }
        // Suffixes: `u U` for uint, `f F` for float, `lf LF` for double,
        // `l L ul UL` for the int64 extension. Anything word-shaped is taken so
        // that `1foo` is one bad token rather than a number and a name.
        let suffix_start = self.pos;
        self.take_while(is_ident_continue);
        let suffix = &self.src[suffix_start.min(self.src.len())..self.pos];
        if suffix.eq_ignore_ascii_case("f") || suffix.eq_ignore_ascii_case("lf") {
            float = true;
        }
        if float { TokenKind::Float } else { TokenKind::Int }
    }

    fn word(&mut self) -> TokenKind {
        self.take_while(is_ident_continue);
        TokenKind::Ident
    }

    /// The punctuator at the cursor, at max munch.
    ///
    /// Dispatched on the first byte rather than matched against a table of
    /// spellings: every punctuator's family is decided by one byte, and only
    /// the eleven families with a longer member look any further. The table
    /// this replaced compared up to 47 spellings per punctuator, which made
    /// `memcmp` the largest single cost in the pipeline (RFC 012 P5-10).
    ///
    /// How far to advance comes from [`Punct::as_str`], so the scanner and the
    /// spelling cannot drift apart.
    fn punct(&mut self) -> Option<Punct> {
        let punct = match self.peek(0) {
            b'(' => Punct::LParen,
            b')' => Punct::RParen,
            b'[' => Punct::LBracket,
            b']' => Punct::RBracket,
            b'{' => Punct::LBrace,
            b'}' => Punct::RBrace,
            b'.' => Punct::Dot,
            b',' => Punct::Comma,
            b':' => Punct::Colon,
            b';' => Punct::Semi,
            b'?' => Punct::Question,
            b'~' => Punct::Tilde,
            b'+' => match self.peek(1) {
                b'+' => Punct::PlusPlus,
                b'=' => Punct::PlusEq,
                _ => Punct::Plus,
            },
            b'-' => match self.peek(1) {
                b'-' => Punct::MinusMinus,
                b'=' => Punct::MinusEq,
                _ => Punct::Minus,
            },
            b'*' => match self.peek(1) {
                b'=' => Punct::StarEq,
                _ => Punct::Star,
            },
            b'/' => match self.peek(1) {
                b'=' => Punct::SlashEq,
                _ => Punct::Slash,
            },
            b'%' => match self.peek(1) {
                b'=' => Punct::PercentEq,
                _ => Punct::Percent,
            },
            b'!' => match self.peek(1) {
                b'=' => Punct::Ne,
                _ => Punct::Bang,
            },
            b'=' => match self.peek(1) {
                b'=' => Punct::EqEq,
                _ => Punct::Eq,
            },
            b'&' => match self.peek(1) {
                b'&' => Punct::AndAnd,
                b'=' => Punct::AmpEq,
                _ => Punct::Amp,
            },
            b'^' => match self.peek(1) {
                b'^' => Punct::XorXor,
                b'=' => Punct::CaretEq,
                _ => Punct::Caret,
            },
            b'|' => match self.peek(1) {
                b'|' => Punct::OrOr,
                b'=' => Punct::PipeEq,
                _ => Punct::Pipe,
            },
            b'<' => match (self.peek(1), self.peek(2)) {
                (b'<', b'=') => Punct::ShlEq,
                (b'<', _) => Punct::Shl,
                (b'=', _) => Punct::Le,
                _ => Punct::Lt,
            },
            b'>' => match (self.peek(1), self.peek(2)) {
                (b'>', b'=') => Punct::ShrEq,
                (b'>', _) => Punct::Shr,
                (b'=', _) => Punct::Ge,
                _ => Punct::Gt,
            },
            b'#' => match self.peek(1) {
                b'#' => Punct::HashHash,
                _ => Punct::Hash,
            },
            _ => return None,
        };
        for _ in 0..punct.as_str().len() {
            self.bump();
        }
        Some(punct)
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_continue(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}
