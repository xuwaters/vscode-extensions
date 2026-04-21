//! Proto3 lexer — hand-written, zero-allocation tokenizer.
//!
//! Emits a flat `Vec<Token>`. Trivia (whitespace + comments) is attached to
//! the following token as [`LeadingTrivia`] so the parser and downstream
//! hover consumers can recover doc-comments without a separate pass.

use crate::spans::ByteSpan;
use smol_str::SmolStr;

/// The kind of a lexed token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Identifiers and literals
    Ident(SmolStr),
    IntLit(SmolStr),
    FloatLit(SmolStr),
    StringLit(String),

    // Punctuation
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LAngle,
    RAngle,
    Semi,
    Comma,
    Dot,
    Eq,
    Minus,
    Plus,
    Colon,
    Slash,

    // Keywords
    KwSyntax,
    KwEdition,
    KwPackage,
    KwImport,
    KwPublic,
    KwWeak,
    KwOption,
    KwMessage,
    KwEnum,
    KwService,
    KwRpc,
    KwReturns,
    KwStream,
    KwOneof,
    KwMap,
    KwReserved,
    KwTo,
    KwMax,
    KwRepeated,
    KwOptional,
    KwRequired,
    KwGroup,
    KwExtensions,
    KwExtend,
    KwTrue,
    KwFalse,

    // Scalar type keywords (treated as identifiers by the grammar, but the
    // lexer tags them for convenience — the parser keeps its type checks
    // table-driven either way.)
    TyDouble,
    TyFloat,
    TyInt32,
    TyInt64,
    TyUint32,
    TyUint64,
    TySint32,
    TySint64,
    TyFixed32,
    TyFixed64,
    TySfixed32,
    TySfixed64,
    TyBool,
    TyString,
    TyBytes,

    /// Unterminated or malformed token — recorded so the parser can emit a
    /// diagnostic at the exact span without aborting the stream.
    LexError(LexErrorKind),

    /// End-of-file sentinel.
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexErrorKind {
    UnterminatedString,
    UnterminatedBlockComment,
    InvalidEscape(char),
    StrayChar(char),
    InvalidNumber(SmolStr),
}

/// A comment preceding a token, retained for hover / doc-comment extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub span: ByteSpan,
    pub text: SmolStr,
    pub kind: CommentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    Line,  // `// ...`
    Block, // `/* ... */`
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeadingTrivia {
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: ByteSpan,
    pub leading: LeadingTrivia,
}

impl Token {
    pub fn eof(offset: u32) -> Self {
        Token {
            kind: TokenKind::Eof,
            span: ByteSpan::new(offset, offset),
            leading: LeadingTrivia::default(),
        }
    }
}

/// Lex a full proto3 source string into a token stream. The stream is always
/// terminated by a `TokenKind::Eof` sentinel.
pub fn lex(source: &str) -> Vec<Token> {
    Lexer::new(source).run()
}

struct Lexer<'s> {
    src: &'s str,
    bytes: &'s [u8],
    pos: usize,
    pending_trivia: LeadingTrivia,
    out: Vec<Token>,
}

impl<'s> Lexer<'s> {
    fn new(src: &'s str) -> Self {
        Lexer {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            pending_trivia: LeadingTrivia::default(),
            out: Vec::new(),
        }
    }

    fn run(mut self) -> Vec<Token> {
        while self.pos < self.bytes.len() {
            self.skip_trivia();
            if self.pos >= self.bytes.len() {
                break;
            }
            self.lex_one();
        }
        self.out.push(Token::eof(self.pos as u32));
        self.out
    }

    fn peek(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.pos + n).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.bytes.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek(0) {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => {
                    self.pos += 1;
                }
                Some(b'/') if self.peek(1) == Some(b'/') => self.lex_line_comment(),
                Some(b'/') if self.peek(1) == Some(b'*') => self.lex_block_comment(),
                _ => break,
            }
        }
    }

    fn lex_line_comment(&mut self) {
        let start = self.pos as u32;
        self.pos += 2;
        while let Some(b) = self.peek(0) {
            if b == b'\n' {
                break;
            }
            self.pos += 1;
        }
        let end = self.pos as u32;
        let text = SmolStr::new(&self.src[start as usize..end as usize]);
        self.pending_trivia.comments.push(Comment {
            span: ByteSpan::new(start, end),
            text,
            kind: CommentKind::Line,
        });
    }

    fn lex_block_comment(&mut self) {
        let start = self.pos as u32;
        self.pos += 2;
        let mut terminated = false;
        while self.pos < self.bytes.len() {
            if self.bytes[self.pos] == b'*' && self.peek(1) == Some(b'/') {
                self.pos += 2;
                terminated = true;
                break;
            }
            self.pos += 1;
        }
        let end = self.pos as u32;
        let text = SmolStr::new(&self.src[start as usize..end as usize]);
        self.pending_trivia.comments.push(Comment {
            span: ByteSpan::new(start, end),
            text,
            kind: CommentKind::Block,
        });
        if !terminated {
            self.emit(ByteSpan::new(start, end), TokenKind::LexError(LexErrorKind::UnterminatedBlockComment));
        }
    }

    fn emit(&mut self, span: ByteSpan, kind: TokenKind) {
        let leading = std::mem::take(&mut self.pending_trivia);
        self.out.push(Token { kind, span, leading });
    }

    fn lex_one(&mut self) {
        let start = self.pos;
        let b = self.bytes[self.pos];
        match b {
            b'{' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::LBrace); }
            b'}' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::RBrace); }
            b'(' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::LParen); }
            b')' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::RParen); }
            b'[' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::LBracket); }
            b']' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::RBracket); }
            b'<' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::LAngle); }
            b'>' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::RAngle); }
            b';' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Semi); }
            b',' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Comma); }
            b'=' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Eq); }
            b'-' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Minus); }
            b'+' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Plus); }
            b':' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Colon); }
            b'/' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Slash); }
            b'.' => self.lex_dot_or_number(),
            b'"' | b'\'' => self.lex_string(b),
            b'0'..=b'9' => self.lex_number(),
            b if is_ident_start(b) => self.lex_ident(),
            _ => {
                let ch = self.bump_char();
                self.emit(
                    ByteSpan::from_usize(start, self.pos),
                    TokenKind::LexError(LexErrorKind::StrayChar(ch)),
                );
            }
        }
    }

    fn bump_char(&mut self) -> char {
        let rest = &self.src[self.pos..];
        let ch = rest.chars().next().unwrap_or('\u{FFFD}');
        self.pos += ch.len_utf8();
        ch
    }

    fn lex_dot_or_number(&mut self) {
        if matches!(self.peek(1), Some(b'0'..=b'9')) {
            self.lex_number();
        } else {
            let start = self.pos;
            self.pos += 1;
            self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Dot);
        }
    }

    fn lex_number(&mut self) {
        let start = self.pos;
        let mut is_float = false;
        let mut is_hex = false;
        let mut is_oct = false;

        if self.bytes[self.pos] == b'.' {
            is_float = true;
            self.pos += 1;
            while matches!(self.peek(0), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        } else if self.bytes[self.pos] == b'0' && matches!(self.peek(1), Some(b'x') | Some(b'X')) {
            is_hex = true;
            self.pos += 2;
            while matches!(self.peek(0), Some(b'0'..=b'9') | Some(b'a'..=b'f') | Some(b'A'..=b'F')) {
                self.pos += 1;
            }
        } else {
            if self.bytes[self.pos] == b'0' {
                is_oct = true;
            }
            while matches!(self.peek(0), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            if self.peek(0) == Some(b'.') {
                is_float = true;
                is_oct = false;
                self.pos += 1;
                while matches!(self.peek(0), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
        }

        if matches!(self.peek(0), Some(b'e') | Some(b'E')) {
            is_float = true;
            is_oct = false;
            self.pos += 1;
            if matches!(self.peek(0), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(0), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }

        if matches!(self.peek(0), Some(b'f') | Some(b'F')) {
            is_float = true;
            self.pos += 1;
        }

        let text = SmolStr::new(&self.src[start..self.pos]);
        let span = ByteSpan::from_usize(start, self.pos);
        let kind = if is_float {
            TokenKind::FloatLit(text)
        } else if is_hex {
            if text.len() <= 2 {
                TokenKind::LexError(LexErrorKind::InvalidNumber(text))
            } else {
                TokenKind::IntLit(text)
            }
        } else if is_oct {
            // 0 is also a valid integer; it's only invalid if octal digits include 8/9.
            if text.chars().skip(1).any(|c| c == '8' || c == '9') {
                TokenKind::LexError(LexErrorKind::InvalidNumber(text))
            } else {
                TokenKind::IntLit(text)
            }
        } else {
            TokenKind::IntLit(text)
        };
        self.emit(span, kind);
    }

    fn lex_string(&mut self, quote: u8) {
        let start = self.pos;
        self.pos += 1; // opening quote
        let mut buf = String::new();
        let mut error: Option<LexErrorKind> = None;
        let mut terminated = false;
        while let Some(b) = self.peek(0) {
            if b == quote {
                self.pos += 1;
                terminated = true;
                break;
            }
            if b == b'\n' {
                // unterminated at newline
                break;
            }
            if b == b'\\' {
                self.pos += 1;
                match self.peek(0) {
                    Some(b'n') => { buf.push('\n'); self.pos += 1; }
                    Some(b't') => { buf.push('\t'); self.pos += 1; }
                    Some(b'r') => { buf.push('\r'); self.pos += 1; }
                    Some(b'\\') => { buf.push('\\'); self.pos += 1; }
                    Some(b'\'') => { buf.push('\''); self.pos += 1; }
                    Some(b'"') => { buf.push('"'); self.pos += 1; }
                    Some(b'0') => { buf.push('\0'); self.pos += 1; }
                    Some(b'a') => { buf.push('\x07'); self.pos += 1; }
                    Some(b'b') => { buf.push('\x08'); self.pos += 1; }
                    Some(b'f') => { buf.push('\x0c'); self.pos += 1; }
                    Some(b'v') => { buf.push('\x0b'); self.pos += 1; }
                    Some(b'x') | Some(b'X') => {
                        self.pos += 1;
                        let mut v: u32 = 0;
                        let mut digits = 0;
                        while digits < 2 && matches!(self.peek(0), Some(b'0'..=b'9') | Some(b'a'..=b'f') | Some(b'A'..=b'F')) {
                            let d = (self.bytes[self.pos] as char).to_digit(16).unwrap();
                            v = v * 16 + d;
                            self.pos += 1;
                            digits += 1;
                        }
                        buf.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                    }
                    Some(c) => {
                        let ch = c as char;
                        error.get_or_insert(LexErrorKind::InvalidEscape(ch));
                        buf.push(ch);
                        self.pos += 1;
                    }
                    None => break,
                }
            } else {
                let ch = self.bump_char();
                buf.push(ch);
            }
        }
        let end = self.pos;
        let span = ByteSpan::from_usize(start, end);
        if !terminated {
            self.emit(span, TokenKind::LexError(LexErrorKind::UnterminatedString));
        } else if let Some(e) = error {
            self.emit(span, TokenKind::LexError(e));
        } else {
            self.emit(span, TokenKind::StringLit(buf));
        }
    }

    fn lex_ident(&mut self) {
        let start = self.pos;
        while let Some(b) = self.peek(0) {
            if is_ident_continue(b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        let span = ByteSpan::from_usize(start, self.pos);
        let kind = match text {
            "syntax" => TokenKind::KwSyntax,
            "edition" => TokenKind::KwEdition,
            "package" => TokenKind::KwPackage,
            "import" => TokenKind::KwImport,
            "public" => TokenKind::KwPublic,
            "weak" => TokenKind::KwWeak,
            "option" => TokenKind::KwOption,
            "message" => TokenKind::KwMessage,
            "enum" => TokenKind::KwEnum,
            "service" => TokenKind::KwService,
            "rpc" => TokenKind::KwRpc,
            "returns" => TokenKind::KwReturns,
            "stream" => TokenKind::KwStream,
            "oneof" => TokenKind::KwOneof,
            "map" => TokenKind::KwMap,
            "reserved" => TokenKind::KwReserved,
            "to" => TokenKind::KwTo,
            "max" => TokenKind::KwMax,
            "repeated" => TokenKind::KwRepeated,
            "optional" => TokenKind::KwOptional,
            "required" => TokenKind::KwRequired,
            "group" => TokenKind::KwGroup,
            "extensions" => TokenKind::KwExtensions,
            "extend" => TokenKind::KwExtend,
            "true" => TokenKind::KwTrue,
            "false" => TokenKind::KwFalse,
            "double" => TokenKind::TyDouble,
            "float" => TokenKind::TyFloat,
            "int32" => TokenKind::TyInt32,
            "int64" => TokenKind::TyInt64,
            "uint32" => TokenKind::TyUint32,
            "uint64" => TokenKind::TyUint64,
            "sint32" => TokenKind::TySint32,
            "sint64" => TokenKind::TySint64,
            "fixed32" => TokenKind::TyFixed32,
            "fixed64" => TokenKind::TyFixed64,
            "sfixed32" => TokenKind::TySfixed32,
            "sfixed64" => TokenKind::TySfixed64,
            "bool" => TokenKind::TyBool,
            "string" => TokenKind::TyString,
            "bytes" => TokenKind::TyBytes,
            _ => TokenKind::Ident(SmolStr::new(text)),
        };
        self.emit(span, kind);
    }
}

fn is_ident_start(b: u8) -> bool {
    (b.is_ascii_alphabetic()) || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    is_ident_start(b) || b.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex(src).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn empty() {
        assert_eq!(kinds(""), vec![TokenKind::Eof]);
    }

    #[test]
    fn keywords_and_idents() {
        let k = kinds("message Foo { int32 a = 1; }");
        assert!(matches!(k[0], TokenKind::KwMessage));
        assert!(matches!(k[1], TokenKind::Ident(ref s) if s == "Foo"));
        assert!(matches!(k[2], TokenKind::LBrace));
        assert!(matches!(k[3], TokenKind::TyInt32));
        assert!(matches!(k[4], TokenKind::Ident(ref s) if s == "a"));
        assert!(matches!(k[5], TokenKind::Eq));
        assert!(matches!(k[6], TokenKind::IntLit(ref s) if s == "1"));
        assert!(matches!(k[7], TokenKind::Semi));
        assert!(matches!(k[8], TokenKind::RBrace));
    }

    #[test]
    fn string_literal_escapes() {
        let k = kinds(r#""hello\nworld""#);
        match &k[0] {
            TokenKind::StringLit(s) => assert_eq!(s, "hello\nworld"),
            t => panic!("unexpected {:?}", t),
        }
    }

    #[test]
    fn unterminated_string() {
        let k = kinds("\"oops\n");
        assert!(matches!(k[0], TokenKind::LexError(LexErrorKind::UnterminatedString)));
    }

    #[test]
    fn numeric_literals() {
        let k = kinds("3.14 42 0xff 1e10");
        assert!(matches!(k[0], TokenKind::FloatLit(_)));
        assert!(matches!(k[1], TokenKind::IntLit(_)));
        assert!(matches!(k[2], TokenKind::IntLit(_)));
        assert!(matches!(k[3], TokenKind::FloatLit(_)));
    }

    #[test]
    fn comments_attach_to_next_token() {
        let tokens = lex("// hi\n// ho\nmessage X {}");
        assert!(matches!(tokens[0].kind, TokenKind::KwMessage));
        assert_eq!(tokens[0].leading.comments.len(), 2);
    }
}
