//! Mojom lexer — hand-written, produces a flat `Vec<Token>` terminated by
//! [`TokenKind::Eof`]. Trivia (whitespace + `//` line and `/* */` block
//! comments) is attached to the following token as [`LeadingTrivia`] so the
//! parser and later hover/doc-comment consumers can reach it without a
//! second pass.
//!
//! Notable Mojom rules handled here:
//! - `@N` ordinals are emitted as an `At` token followed by an int literal.
//! - `=>` (method response arrow) is emitted as `Arrow`.
//! - `&` (interface request) and `?` (nullable) are single tokens.
//! - Dotted names (`foo.bar.Baz`) are lexed as `Ident Dot Ident …`; the
//!   parser stitches the path back together.
//! - Type-position words like `array`, `map`, `handle`, `associated`,
//!   `pending_remote`, `int32`, `string` are *not* keywords — they are
//!   ordinary identifiers recognised contextually by the parser. Only the
//!   structural declaration keywords get dedicated token kinds.

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
    Langle,
    Rangle,
    Semi,
    Comma,
    Dot,
    Eq,
    Amp,
    Question,
    At,
    Plus,
    Minus,
    Arrow, // =>

    // Structural declaration keywords.
    KwModule,
    KwImport,
    KwStruct,
    KwUnion,
    KwInterface,
    KwEnum,
    KwConst,

    // Value keywords.
    KwTrue,
    KwFalse,
    KwDefault,

    LexError(LexErrorKind),
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexErrorKind {
    UnterminatedString,
    UnterminatedBlockComment,
    StrayChar(char),
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
                Some(b'/') if self.peek(1) == Some(b'/') => self.lex_line_comment(),
                Some(b'/') if self.peek(1) == Some(b'*') => self.lex_block_comment(),
                _ => break,
            }
        }
    }

    fn lex_line_comment(&mut self) {
        let start = self.pos as u32;
        self.pos += 2; // consume '//'
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

    fn lex_block_comment(&mut self) {
        let start = self.pos as u32;
        self.pos += 2; // consume '/*'
        let mut terminated = false;
        while let Some(b) = self.peek(0) {
            if b == b'*' && self.peek(1) == Some(b'/') {
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
            b'<' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Langle); }
            b'>' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Rangle); }
            b';' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Semi); }
            b',' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Comma); }
            b'&' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Amp); }
            b'?' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Question); }
            b'@' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::At); }
            b'+' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Plus); }
            b'-' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Minus); }
            b'=' => {
                if self.peek(1) == Some(b'>') {
                    self.pos += 2;
                    self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Arrow);
                } else {
                    self.pos += 1;
                    self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Eq);
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

        if self.bytes[self.pos] == b'0' && matches!(self.peek(1), Some(b'x') | Some(b'X')) {
            self.pos += 2;
            while let Some(b) = self.peek(0) {
                if b.is_ascii_hexdigit() { self.pos += 1; } else { break; }
            }
        } else {
            if self.peek(0) == Some(b'.') {
                // Leading-dot float like `.5`.
                is_float = true;
                self.pos += 1;
                while let Some(b) = self.peek(0) {
                    if b.is_ascii_digit() { self.pos += 1; } else { break; }
                }
            } else {
                while let Some(b) = self.peek(0) {
                    if b.is_ascii_digit() { self.pos += 1; } else { break; }
                }
                if self.peek(0) == Some(b'.') && !matches!(self.peek(1), Some(b'.')) {
                    is_float = true;
                    self.pos += 1;
                    while let Some(b) = self.peek(0) {
                        if b.is_ascii_digit() { self.pos += 1; } else { break; }
                    }
                }
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
        let kind = if is_float { TokenKind::FloatLit(text) } else { TokenKind::IntLit(text) };
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
                        Some(b'b') => { value.push('\u{0008}'); self.pos += 1; }
                        Some(b'f') => { value.push('\u{000C}'); self.pos += 1; }
                        Some(b'/') => { value.push('/'); self.pos += 1; }
                        Some(b'"') => { value.push('"'); self.pos += 1; }
                        Some(b'\\') => { value.push('\\'); self.pos += 1; }
                        Some(other) => { value.push(other as char); self.pos += 1; }
                        None => break,
                    }
                }
                _ => {
                    value.push(b as char);
                    self.pos += 1;
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
        "module" => TokenKind::KwModule,
        "import" => TokenKind::KwImport,
        "struct" => TokenKind::KwStruct,
        "union" => TokenKind::KwUnion,
        "interface" => TokenKind::KwInterface,
        "enum" => TokenKind::KwEnum,
        "const" => TokenKind::KwConst,
        "true" => TokenKind::KwTrue,
        "false" => TokenKind::KwFalse,
        "default" => TokenKind::KwDefault,
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
    fn lex_module_and_struct() {
        let k = kinds("module foo.bar; struct S {};");
        assert!(matches!(k[0], TokenKind::KwModule));
        assert!(matches!(k[1], TokenKind::Ident(_)));
        assert!(matches!(k[2], TokenKind::Dot));
        assert!(matches!(k[3], TokenKind::Ident(_)));
        assert!(matches!(k[4], TokenKind::Semi));
        assert!(matches!(k[5], TokenKind::KwStruct));
    }

    #[test]
    fn lex_arrow_and_ordinal() {
        let k = kinds("Method@3() => ();");
        assert!(matches!(k[0], TokenKind::Ident(_)));
        assert!(matches!(k[1], TokenKind::At));
        assert!(matches!(k[2], TokenKind::IntLit(_)));
        assert!(k.iter().any(|t| matches!(t, TokenKind::Arrow)));
    }

    #[test]
    fn lex_line_and_block_comments_are_trivia() {
        let src = "// hi\n/* block */ interface X {};";
        let toks = lex(src);
        assert!(matches!(toks[0].kind, TokenKind::KwInterface));
        assert_eq!(toks[0].leading.comments.len(), 2);
    }

    #[test]
    fn lex_string_with_escape() {
        let k = kinds(r#""he\"llo""#);
        assert!(matches!(k[0], TokenKind::StringLit(ref s) if s == "he\"llo"));
    }

    #[test]
    fn lex_generic_brackets() {
        let k = kinds("array<int32>");
        assert!(matches!(k[0], TokenKind::Ident(_)));
        assert!(matches!(k[1], TokenKind::Langle));
        assert!(matches!(k[2], TokenKind::Ident(_)));
        assert!(matches!(k[3], TokenKind::Rangle));
    }
}
