//! Cap'n Proto lexer — hand-written, produces a flat `Vec<Token>` terminated
//! by [`TokenKind::Eof`]. Trivia (whitespace + `#`-comments) is attached to
//! the following token as [`LeadingTrivia`] so the parser and later
//! hover/doc-comment consumers can reach it without a second pass.
//!
//! Notable Cap'n Proto rules handled here:
//! - Comments use `#` to end-of-line (no block comments in the language).
//! - `@N` ordinals are emitted as a separate `At` token followed by an int.
//! - `$Annotation` uses `Dollar` as a prefix.
//! - `->` (method-return arrow) is emitted as `Arrow`.
//! - Data literals like `0x"62 61 72"` are not special-cased: the lexer
//!   just emits `IntLit("0x")` then a `StringLit` — the parser stitches them
//!   together. This keeps the lexer simple and the tokens round-trippable.

use crate::spans::ByteSpan;
use smol_str::SmolStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
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
    Semi,
    Comma,
    Dot,
    Eq,
    Minus,
    Plus,
    Colon,
    At,
    Dollar,
    Star,
    Arrow, // ->

    // Keywords — structural
    KwStruct,
    KwEnum,
    KwInterface,
    KwUnion,
    KwGroup,
    KwConst,
    KwAnnotation,
    KwUsing,
    KwImport,
    KwExtends,
    KwStream,

    // Value keywords
    KwTrue,
    KwFalse,
    KwVoid,
    KwInf,
    KwNan,

    // Scalar type names (tagged for convenience — parser treats them as
    // recognisable named types but does not require them).
    TyVoid,
    TyBool,
    TyInt8,
    TyInt16,
    TyInt32,
    TyInt64,
    TyUInt8,
    TyUInt16,
    TyUInt32,
    TyUInt64,
    TyFloat32,
    TyFloat64,
    TyText,
    TyData,
    TyList,
    TyAnyPointer,
    TyCapability,

    LexError(LexErrorKind),
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexErrorKind {
    UnterminatedString,
    InvalidEscape(char),
    StrayChar(char),
    InvalidNumber(SmolStr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub span: ByteSpan,
    pub text: SmolStr,
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

    fn skip_trivia(&mut self) {
        loop {
            match self.peek(0) {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => {
                    self.pos += 1;
                }
                Some(b'#') => self.lex_line_comment(),
                _ => break,
            }
        }
    }

    fn lex_line_comment(&mut self) {
        let start = self.pos as u32;
        self.pos += 1; // consume '#'
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
        });
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
            b';' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Semi); }
            b',' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Comma); }
            b'=' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Eq); }
            b'+' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Plus); }
            b':' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Colon); }
            b'@' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::At); }
            b'$' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Dollar); }
            b'*' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Star); }
            b'-' => {
                if self.peek(1) == Some(b'>') {
                    self.pos += 2;
                    self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Arrow);
                } else {
                    self.pos += 1;
                    self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Minus);
                }
            }
            b'.' => self.lex_dot_or_number(),
            b'"' => self.lex_string(),
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

        if self.bytes[self.pos] == b'0' && matches!(self.peek(1), Some(b'x') | Some(b'X')) {
            is_hex = true;
            self.pos += 2;
            while let Some(b) = self.peek(0) {
                if b.is_ascii_hexdigit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        } else {
            while let Some(b) = self.peek(0) {
                if b.is_ascii_digit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.peek(0) == Some(b'.')
                && matches!(self.peek(1), Some(b'0'..=b'9'))
            {
                is_float = true;
                self.pos += 1;
                while let Some(b) = self.peek(0) {
                    if b.is_ascii_digit() { self.pos += 1; } else { break; }
                }
            } else if self.peek(0) == Some(b'.') && !matches!(self.peek(1), Some(b'.')) {
                // trailing dot like `1.` — treat as float
                is_float = true;
                self.pos += 1;
            }
            if matches!(self.peek(0), Some(b'e') | Some(b'E')) {
                is_float = true;
                self.pos += 1;
                if matches!(self.peek(0), Some(b'+') | Some(b'-')) {
                    self.pos += 1;
                }
                while let Some(b) = self.peek(0) {
                    if b.is_ascii_digit() { self.pos += 1; } else { break; }
                }
            }
        }

        let text = SmolStr::new(&self.src[start..self.pos]);
        let span = ByteSpan::from_usize(start, self.pos);
        let kind = if is_float {
            TokenKind::FloatLit(text)
        } else if is_hex {
            TokenKind::IntLit(text)
        } else {
            TokenKind::IntLit(text)
        };
        self.emit(span, kind);
    }

    fn lex_string(&mut self) {
        let start = self.pos;
        self.pos += 1; // consume opening '"'
        let mut value = String::new();
        let mut terminated = false;
        while let Some(b) = self.peek(0) {
            match b {
                b'"' => {
                    self.pos += 1;
                    terminated = true;
                    break;
                }
                b'\n' => break, // unterminated string at EOL
                b'\\' => {
                    self.pos += 1;
                    match self.peek(0) {
                        Some(b'n') => { value.push('\n'); self.pos += 1; }
                        Some(b't') => { value.push('\t'); self.pos += 1; }
                        Some(b'r') => { value.push('\r'); self.pos += 1; }
                        Some(b'"') => { value.push('"'); self.pos += 1; }
                        Some(b'\\') => { value.push('\\'); self.pos += 1; }
                        Some(b'0') => { value.push('\0'); self.pos += 1; }
                        Some(_) => {
                            // A whole char, not a byte: `\é` must not leave
                            // `pos` or the span inside the é.
                            let esc_start = self.pos - 1;
                            let ch = self.bump_char();
                            let esc_span = ByteSpan::from_usize(esc_start, self.pos);
                            // record escape error token but still keep the char raw
                            self.out.push(Token {
                                kind: TokenKind::LexError(LexErrorKind::InvalidEscape(ch)),
                                span: esc_span,
                                leading: LeadingTrivia::default(),
                            });
                            value.push(ch);
                        }
                        None => break,
                    }
                }
                _ => {
                    let ch = self.bump_char();
                    value.push(ch);
                }
            }
        }
        let end = self.pos;
        let span = ByteSpan::from_usize(start, end);
        if !terminated {
            self.emit(span, TokenKind::LexError(LexErrorKind::UnterminatedString));
        } else {
            self.emit(span, TokenKind::StringLit(value));
        }
    }

    fn lex_ident(&mut self) {
        let start = self.pos;
        self.pos += 1;
        while let Some(b) = self.peek(0) {
            if is_ident_continue(b) { self.pos += 1; } else { break; }
        }
        let text = &self.src[start..self.pos];
        let span = ByteSpan::from_usize(start, self.pos);
        let kind = keyword_or_ident(text);
        self.emit(span, kind);
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn keyword_or_ident(text: &str) -> TokenKind {
    match text {
        // Structural keywords
        "struct" => TokenKind::KwStruct,
        "enum" => TokenKind::KwEnum,
        "interface" => TokenKind::KwInterface,
        "union" => TokenKind::KwUnion,
        "group" => TokenKind::KwGroup,
        "const" => TokenKind::KwConst,
        "annotation" => TokenKind::KwAnnotation,
        "using" => TokenKind::KwUsing,
        "import" => TokenKind::KwImport,
        "extends" => TokenKind::KwExtends,
        "stream" => TokenKind::KwStream,
        // Value keywords
        "true" => TokenKind::KwTrue,
        "false" => TokenKind::KwFalse,
        "void" => TokenKind::KwVoid,
        "inf" => TokenKind::KwInf,
        "nan" => TokenKind::KwNan,
        // Type names
        "Void" => TokenKind::TyVoid,
        "Bool" => TokenKind::TyBool,
        "Int8" => TokenKind::TyInt8,
        "Int16" => TokenKind::TyInt16,
        "Int32" => TokenKind::TyInt32,
        "Int64" => TokenKind::TyInt64,
        "UInt8" => TokenKind::TyUInt8,
        "UInt16" => TokenKind::TyUInt16,
        "UInt32" => TokenKind::TyUInt32,
        "UInt64" => TokenKind::TyUInt64,
        "Float32" => TokenKind::TyFloat32,
        "Float64" => TokenKind::TyFloat64,
        "Text" => TokenKind::TyText,
        "Data" => TokenKind::TyData,
        "List" => TokenKind::TyList,
        "AnyPointer" => TokenKind::TyAnyPointer,
        "Capability" => TokenKind::TyCapability,
        _ => TokenKind::Ident(SmolStr::new(text)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<TokenKind> {
        lex(s).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn invalid_escape_of_multibyte_char() {
        let src = r#""\é""#;
        let toks = lex(src);
        let esc = toks
            .iter()
            .find(|t| matches!(t.kind, TokenKind::LexError(LexErrorKind::InvalidEscape('é'))))
            .unwrap();
        assert_eq!((esc.span.start, esc.span.end), (1, 4));
    }

    #[test]
    fn string_keeps_multibyte_chars() {
        let k = kinds(r#""café""#);
        assert!(matches!(k[0], TokenKind::StringLit(ref s) if s == "café"), "{:?}", k[0]);
    }

    #[test]
    fn lex_file_id_and_struct() {
        let src = "@0x9eb32e19f86ee174;\nstruct Foo { id @0 :UInt32; }";
        let k = kinds(src);
        assert!(matches!(k[0], TokenKind::At));
        assert!(matches!(k[1], TokenKind::IntLit(_)));
        assert!(matches!(k[2], TokenKind::Semi));
        assert!(matches!(k[3], TokenKind::KwStruct));
        assert!(matches!(k[4], TokenKind::Ident(_)));
    }

    #[test]
    fn lex_hash_comment_is_trivia() {
        let src = "# hi\nstruct X {}";
        let toks = lex(src);
        assert!(matches!(toks[0].kind, TokenKind::KwStruct));
        assert_eq!(toks[0].leading.comments.len(), 1);
    }

    #[test]
    fn lex_arrow() {
        let k = kinds("a -> b");
        assert!(matches!(k[1], TokenKind::Arrow));
    }

    #[test]
    fn lex_stream_keyword() {
        // `stream` is a contextual keyword used after `->`; the lexer always
        // emits KwStream — the parser is responsible for treating it as an
        // identifier when it appears in non-stream-position contexts.
        let k = kinds("stream");
        assert!(matches!(k[0], TokenKind::KwStream));
    }

    #[test]
    fn lex_string_literal() {
        let k = kinds("\"hello\\n\"");
        assert!(matches!(k[0], TokenKind::StringLit(ref s) if s == "hello\n"));
    }
}
