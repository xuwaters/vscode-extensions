//! Textproto lexer — hand-written tokenizer for Protocol Buffers text format.
//!
//! Text format has a simpler surface than `.proto`: only `#` line comments,
//! message delimiters `{…}` or `<…>`, numeric literals (hex/oct/dec/float with
//! an optional `f`/`F` suffix), single- and double-quoted strings with the
//! standard C-style escape set, and bare identifiers. Comments are kept in a
//! separate `Vec` (rather than attached as trivia) because textproto uses
//! leading `#` comments to carry the `# proto-file:` / `# proto-message:`
//! schema hints that must be read from the file header.

use crate::spans::ByteSpan;
use smol_str::SmolStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Ident(SmolStr),
    IntLit(SmolStr),
    FloatLit(SmolStr),
    StringLit(String),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LAngle,
    RAngle,
    Comma,
    Semi,
    Colon,
    Dot,
    Minus,
    Plus,
    Slash,
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
pub struct Token {
    pub kind: TokenKind,
    pub span: ByteSpan,
}

impl Token {
    pub fn eof(offset: u32) -> Self {
        Token { kind: TokenKind::Eof, span: ByteSpan::new(offset, offset) }
    }
}

/// A `#` line comment retained for header-annotation extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub span: ByteSpan,
    pub text: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexResult {
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
}

pub fn lex(source: &str) -> LexResult {
    Lexer::new(source).run()
}

struct Lexer<'s> {
    src: &'s str,
    bytes: &'s [u8],
    pos: usize,
    out: Vec<Token>,
    comments: Vec<Comment>,
}

impl<'s> Lexer<'s> {
    fn new(src: &'s str) -> Self {
        Lexer { src, bytes: src.as_bytes(), pos: 0, out: Vec::new(), comments: Vec::new() }
    }

    fn run(mut self) -> LexResult {
        while self.pos < self.bytes.len() {
            self.skip_trivia();
            if self.pos >= self.bytes.len() {
                break;
            }
            self.lex_one();
        }
        self.out.push(Token::eof(self.pos as u32));
        LexResult { tokens: self.out, comments: self.comments }
    }

    fn peek(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.pos + n).copied()
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek(0) {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => self.pos += 1,
                Some(b'#') => self.lex_line_comment(),
                _ => break,
            }
        }
    }

    fn lex_line_comment(&mut self) {
        let start = self.pos as u32;
        self.pos += 1;
        while let Some(b) = self.peek(0) {
            if b == b'\n' {
                break;
            }
            self.pos += 1;
        }
        let end = self.pos as u32;
        let text = SmolStr::new(&self.src[start as usize..end as usize]);
        self.comments.push(Comment { span: ByteSpan::new(start, end), text });
    }

    fn emit(&mut self, span: ByteSpan, kind: TokenKind) {
        self.out.push(Token { kind, span });
    }

    fn bump_char(&mut self) -> char {
        let rest = &self.src[self.pos..];
        let ch = rest.chars().next().unwrap_or('\u{FFFD}');
        self.pos += ch.len_utf8();
        ch
    }

    fn lex_one(&mut self) {
        let start = self.pos;
        let b = self.bytes[self.pos];
        match b {
            b'{' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::LBrace); }
            b'}' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::RBrace); }
            b'[' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::LBracket); }
            b']' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::RBracket); }
            b'<' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::LAngle); }
            b'>' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::RAngle); }
            b',' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Comma); }
            b';' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Semi); }
            b':' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Colon); }
            b'-' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Minus); }
            b'+' => { self.pos += 1; self.emit(ByteSpan::from_usize(start, self.pos), TokenKind::Plus); }
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
        self.pos += 1;
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
                    Some(b'?') => { buf.push('?'); self.pos += 1; }
                    Some(b'a') => { buf.push('\x07'); self.pos += 1; }
                    Some(b'b') => { buf.push('\x08'); self.pos += 1; }
                    Some(b'f') => { buf.push('\x0c'); self.pos += 1; }
                    Some(b'v') => { buf.push('\x0b'); self.pos += 1; }
                    Some(b'0'..=b'7') => {
                        let mut v: u32 = 0;
                        let mut digits = 0;
                        while digits < 3 && matches!(self.peek(0), Some(b'0'..=b'7')) {
                            let d = (self.bytes[self.pos] as char).to_digit(8).unwrap();
                            v = v * 8 + d;
                            self.pos += 1;
                            digits += 1;
                        }
                        buf.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                    }
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
                    Some(b'u') => {
                        self.pos += 1;
                        let (v, d) = self.read_hex_digits(4);
                        if d == 4 {
                            buf.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                        } else {
                            error.get_or_insert(LexErrorKind::InvalidEscape('u'));
                        }
                    }
                    Some(b'U') => {
                        self.pos += 1;
                        let (v, d) = self.read_hex_digits(8);
                        if d == 8 {
                            buf.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                        } else {
                            error.get_or_insert(LexErrorKind::InvalidEscape('U'));
                        }
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

    fn read_hex_digits(&mut self, max: usize) -> (u32, usize) {
        let mut v: u32 = 0;
        let mut d = 0;
        while d < max && matches!(self.peek(0), Some(b'0'..=b'9') | Some(b'a'..=b'f') | Some(b'A'..=b'F')) {
            let digit = (self.bytes[self.pos] as char).to_digit(16).unwrap();
            v = v * 16 + digit;
            self.pos += 1;
            d += 1;
        }
        (v, d)
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
        self.emit(span, TokenKind::Ident(SmolStr::new(text)));
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    is_ident_start(b) || b.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex(src).tokens.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn empty() {
        assert_eq!(kinds(""), vec![TokenKind::Eof]);
    }

    #[test]
    fn hash_comments_are_trivia_and_collected() {
        let res = lex("# one\nfoo: 1\n# two");
        assert_eq!(res.comments.len(), 2);
        assert_eq!(res.comments[0].text.as_str(), "# one");
        assert_eq!(res.comments[1].text.as_str(), "# two");
        // First real token after trivia is `foo`.
        assert!(matches!(res.tokens[0].kind, TokenKind::Ident(ref s) if s == "foo"));
    }

    #[test]
    fn angle_brackets_and_braces() {
        let k = kinds("pet < kind: DOG >");
        assert!(matches!(k[0], TokenKind::Ident(ref s) if s == "pet"));
        assert!(matches!(k[1], TokenKind::LAngle));
        assert!(matches!(k[2], TokenKind::Ident(ref s) if s == "kind"));
        assert!(matches!(k[3], TokenKind::Colon));
        assert!(matches!(k[4], TokenKind::Ident(ref s) if s == "DOG"));
        assert!(matches!(k[5], TokenKind::RAngle));
    }

    #[test]
    fn numbers() {
        let k = kinds("1 0xFF 0755 3.14 1.5e2 2f 0.5F");
        assert!(matches!(k[0], TokenKind::IntLit(_)));
        assert!(matches!(k[1], TokenKind::IntLit(_)));
        assert!(matches!(k[2], TokenKind::IntLit(_)));
        assert!(matches!(k[3], TokenKind::FloatLit(_)));
        assert!(matches!(k[4], TokenKind::FloatLit(_)));
        assert!(matches!(k[5], TokenKind::FloatLit(_)));
        assert!(matches!(k[6], TokenKind::FloatLit(_)));
    }

    #[test]
    fn strings_with_escapes() {
        let k = kinds(r#""hello\n\x41""#);
        match &k[0] {
            TokenKind::StringLit(s) => assert_eq!(s, "hello\nA"),
            t => panic!("unexpected {:?}", t),
        }
    }

    #[test]
    fn any_type_url_tokenizes_with_slash() {
        let k = kinds("[type.googleapis.com/pkg.Foo]");
        assert!(matches!(k[0], TokenKind::LBracket));
        assert!(matches!(k[1], TokenKind::Ident(ref s) if s == "type"));
        assert!(matches!(k[2], TokenKind::Dot));
        assert!(matches!(k[3], TokenKind::Ident(ref s) if s == "googleapis"));
        assert!(matches!(k[4], TokenKind::Dot));
        assert!(matches!(k[5], TokenKind::Ident(ref s) if s == "com"));
        assert!(matches!(k[6], TokenKind::Slash));
        assert!(matches!(k[7], TokenKind::Ident(ref s) if s == "pkg"));
        assert!(matches!(k[8], TokenKind::Dot));
        assert!(matches!(k[9], TokenKind::Ident(ref s) if s == "Foo"));
        assert!(matches!(k[10], TokenKind::RBracket));
    }
}
