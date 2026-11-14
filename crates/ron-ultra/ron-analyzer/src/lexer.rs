//! RON token stream.
//!
//! Comments are tokens too — the parser attaches them to the AST rather
//! than throwing them away — and every token records how many newlines
//! sat in the whitespace before it. That count is all the parser needs to
//! tell a trailing comment from a leading one, to notice blank lines worth
//! keeping, and to see whether the author broke a container across lines.
//!
//! Lexical errors (an unterminated string, a bad escape, a malformed
//! number) are reported here; the token is still produced, so the parser
//! keeps going and the file still gets an outline.

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::spans::ByteSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Hash,
    Bang,
    /// A plain or raw (`r#name`) identifier. `true`, `false`, `inf`,
    /// `NaN`, `Some` and `None` are identifiers here; the parser gives
    /// them meaning.
    Ident,
    Integer,
    /// Includes a signed `inf`/`NaN` (`-inf`); the bare words are
    /// identifiers.
    Float,
    /// `"..."`, `r"..."`, `r#"..."#`.
    String,
    /// `b"..."`, `br"..."`, `br#"..."#`.
    ByteString,
    /// `'x'`, `b'x'`.
    Char,
    LineComment,
    BlockComment,
    /// Any character that starts no RON token.
    Unknown,
}

impl TokenKind {
    pub fn is_comment(self) -> bool {
        matches!(self, TokenKind::LineComment | TokenKind::BlockComment)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: ByteSpan,
    /// Newlines in the whitespace between the previous token (comments
    /// included) and this one.
    pub newlines_before: u32,
    /// The decoded text of a string literal, escapes resolved; `None` for
    /// every other token and for a string whose escapes were invalid.
    pub value: Option<String>,
}

pub struct Lexed {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn lex(source: &str) -> Lexed {
    let mut lx = Lexer { src: source, pos: 0, newlines: 0, tokens: Vec::new(), diagnostics: Vec::new() };
    lx.run();
    Lexed { tokens: lx.tokens, diagnostics: lx.diagnostics }
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    newlines: u32,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn run(&mut self) {
        while let Some(c) = self.peek() {
            let start = self.pos;
            match c {
                '\n' => {
                    self.newlines += 1;
                    self.pos += 1;
                }
                c if c.is_whitespace() || c == '\u{feff}' => self.pos += c.len_utf8(),
                '/' if self.peek_at(1) == Some('/') => self.line_comment(),
                '/' if self.peek_at(1) == Some('*') => self.block_comment(),
                '(' => self.single(TokenKind::LParen),
                ')' => self.single(TokenKind::RParen),
                '[' => self.single(TokenKind::LBracket),
                ']' => self.single(TokenKind::RBracket),
                '{' => self.single(TokenKind::LBrace),
                '}' => self.single(TokenKind::RBrace),
                ',' => self.single(TokenKind::Comma),
                ':' => self.single(TokenKind::Colon),
                '#' => self.single(TokenKind::Hash),
                '!' => self.single(TokenKind::Bang),
                '"' => {
                    self.pos += 1;
                    self.quoted_string(start, TokenKind::String);
                }
                '\'' => {
                    self.pos += 1;
                    self.char_literal(start);
                }
                'r' if self.raw_string_ahead(1) => {
                    self.pos += 1;
                    self.raw_string(start, TokenKind::String);
                }
                'b' if self.peek_at(1) == Some('"') => {
                    self.pos += 2;
                    self.quoted_string(start, TokenKind::ByteString);
                }
                'b' if self.peek_at(1) == Some('\'') => {
                    self.pos += 2;
                    self.char_literal(start);
                }
                'b' if self.peek_at(1) == Some('r') && self.raw_string_ahead(2) => {
                    self.pos += 2;
                    self.raw_string(start, TokenKind::ByteString);
                }
                'r' if self.peek_at(1) == Some('#') && self.peek_at(2).is_some_and(is_raw_ident_char) => {
                    self.pos += 2;
                    while self.peek().is_some_and(is_raw_ident_char) {
                        self.bump();
                    }
                    self.push(TokenKind::Ident, start, None);
                }
                c if c.is_ascii_digit() => self.number(start),
                '.' if self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) => self.number(start),
                '+' | '-' => self.signed(start),
                c if is_ident_start(c) => {
                    while self.peek().is_some_and(is_ident_continue) {
                        self.bump();
                    }
                    self.push(TokenKind::Ident, start, None);
                }
                _ => {
                    self.bump();
                    self.push(TokenKind::Unknown, start, None);
                }
            }
        }
    }

    // ---- primitives ------------------------------------------------------

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn push(&mut self, kind: TokenKind, start: usize, value: Option<String>) {
        self.tokens.push(Token {
            kind,
            span: ByteSpan::from_usize(start, self.pos),
            newlines_before: self.newlines,
            value,
        });
        self.newlines = 0;
    }

    fn single(&mut self, kind: TokenKind) {
        let start = self.pos;
        self.pos += 1;
        self.push(kind, start, None);
    }

    fn error(&mut self, code: DiagnosticCode, message: impl Into<String>, start: usize, end: usize) {
        self.diagnostics.push(Diagnostic::error(code, message, ByteSpan::from_usize(start, end)));
    }

    // ---- comments --------------------------------------------------------

    fn line_comment(&mut self) {
        let start = self.pos;
        let len = self.src[start..].find('\n').unwrap_or(self.src.len() - start);
        // A CRLF file leaves the `\r` on the comment; it is whitespace.
        let text = self.src[start..start + len].trim_end_matches('\r');
        self.pos = start + text.len();
        self.push(TokenKind::LineComment, start, None);
    }

    /// RON block comments nest, like Rust's.
    fn block_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        let mut depth = 1u32;
        while depth > 0 {
            let rest = &self.src[self.pos..];
            if rest.starts_with("*/") {
                depth -= 1;
                self.pos += 2;
            } else if rest.starts_with("/*") {
                depth += 1;
                self.pos += 2;
            } else if self.bump().is_none() {
                self.error(
                    DiagnosticCode::UnterminatedComment,
                    "unterminated block comment",
                    start,
                    (start + 2).min(self.src.len()),
                );
                break;
            }
        }
        self.push(TokenKind::BlockComment, start, None);
    }

    // ---- strings and chars -----------------------------------------------

    /// After the opening `"` (and `b`, for a byte string).
    fn quoted_string(&mut self, start: usize, kind: TokenKind) {
        let mut value = String::new();
        let mut valid = true;
        loop {
            match self.bump() {
                None => {
                    self.error(DiagnosticCode::UnterminatedString, "unterminated string", start, self.pos);
                    break;
                }
                Some('"') => break,
                Some('\\') => match self.escape(kind == TokenKind::ByteString) {
                    Some(c) => value.push(c),
                    None => valid = false,
                },
                Some(c) => value.push(c),
            }
        }
        self.push(kind, start, valid.then_some(value));
    }

    /// Whether `r` (at offset `n - 1`) starts a raw string: `#`* then `"`.
    fn raw_string_ahead(&self, n: usize) -> bool {
        let mut i = n;
        while self.peek_at(i) == Some('#') {
            i += 1;
        }
        self.peek_at(i) == Some('"')
    }

    /// After the `r` (or `br`): hashes, `"`, body, `"`, the same hashes.
    fn raw_string(&mut self, start: usize, kind: TokenKind) {
        let mut hashes = 0;
        while self.eat('#') {
            hashes += 1;
        }
        self.eat('"');
        let body_start = self.pos;
        let closing = format!("\"{}", "#".repeat(hashes));
        match self.src[body_start..].find(&closing) {
            Some(i) => {
                self.pos = body_start + i + closing.len();
                let value = self.src[body_start..body_start + i].to_string();
                self.push(kind, start, Some(value));
            }
            None => {
                self.pos = self.src.len();
                self.error(DiagnosticCode::UnterminatedString, "unterminated raw string", start, self.pos);
                self.push(kind, start, None);
            }
        }
    }

    /// After the opening `'`. Lifetimes do not exist in RON, so a `'` is
    /// always a character literal.
    fn char_literal(&mut self, start: usize) {
        let mut count = 0;
        let mut valid = true;
        loop {
            match self.peek() {
                None | Some('\n') => {
                    self.error(DiagnosticCode::InvalidChar, "unterminated character literal", start, self.pos);
                    self.push(TokenKind::Char, start, None);
                    return;
                }
                Some('\'') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    valid &= self.escape(false).is_some();
                    count += 1;
                }
                Some(_) => {
                    self.bump();
                    count += 1;
                }
            }
        }
        if count != 1 && valid {
            let message = if count == 0 {
                "empty character literal"
            } else {
                "character literal holds more than one character; use a string"
            };
            self.error(DiagnosticCode::InvalidChar, message, start, self.pos);
        }
        self.push(TokenKind::Char, start, None);
    }

    /// After a `\`. Reports and returns `None` for an unknown or malformed
    /// escape.
    fn escape(&mut self, byte_string: bool) -> Option<char> {
        let start = self.pos - 1;
        let decoded = match self.bump() {
            Some('"') => Some('"'),
            Some('\'') => Some('\''),
            Some('\\') => Some('\\'),
            Some('/') => Some('/'),
            Some('b') => Some('\u{8}'),
            Some('f') => Some('\u{c}'),
            Some('n') => Some('\n'),
            Some('r') => Some('\r'),
            Some('t') => Some('\t'),
            Some('0') => Some('\0'),
            Some('x') => {
                let digits = self.take_hex(2);
                match u32::from_str_radix(&digits, 16) {
                    Ok(v) if digits.len() == 2 && (byte_string || v <= 0x7f) => char::from_u32(v),
                    _ => None,
                }
            }
            Some('u') => {
                let digits = if self.eat('{') {
                    let d = self.take_hex(6);
                    if self.eat('}') { d } else { String::new() }
                } else {
                    let d = self.take_hex(4);
                    if d.len() == 4 { d } else { String::new() }
                };
                u32::from_str_radix(&digits, 16).ok().and_then(char::from_u32)
            }
            _ => None,
        };
        if decoded.is_none() {
            let end = self.pos.max(start + 1).min(self.src.len());
            let text = &self.src[start..end];
            self.error(DiagnosticCode::InvalidEscape, format!("invalid escape `{text}`"), start, end);
        }
        decoded
    }

    fn take_hex(&mut self, max: usize) -> String {
        let mut out = String::new();
        while out.len() < max {
            match self.peek() {
                Some(c) if c.is_ascii_hexdigit() => {
                    out.push(c);
                    self.bump();
                }
                _ => break,
            }
        }
        out
    }

    // ---- numbers ---------------------------------------------------------

    /// A `+` or `-`: the sign of a number or of `inf`/`NaN`, or a stray.
    fn signed(&mut self, start: usize) {
        let next = self.peek_at(1);
        let after = self.peek_at(2);
        if next.is_some_and(|c| c.is_ascii_digit())
            || (next == Some('.') && after.is_some_and(|c| c.is_ascii_digit()))
        {
            self.pos += 1;
            self.number(start);
            return;
        }
        for word in ["inf", "NaN"] {
            let rest = &self.src[self.pos + 1..];
            if rest.starts_with(word) && !rest[word.len()..].starts_with(is_ident_continue) {
                self.pos += 1 + word.len();
                self.push(TokenKind::Float, start, None);
                return;
            }
        }
        self.bump();
        self.push(TokenKind::Unknown, start, None);
    }

    /// `start` may sit on a sign the caller already skipped.
    fn number(&mut self, start: usize) {
        let digits_start = self.pos;
        let rest = &self.src[digits_start..];
        let radix = if rest.starts_with("0x") {
            16
        } else if rest.starts_with("0o") {
            8
        } else if rest.starts_with("0b") {
            2
        } else {
            10
        };
        let kind = if radix == 10 { self.decimal() } else { self.radix_int(radix) };
        let text = self.src[start..self.pos].to_string();
        match kind {
            Some(kind) => self.push(kind, start, None),
            None => {
                self.error(DiagnosticCode::InvalidNumber, format!("invalid number `{text}`"), start, self.pos);
                self.push(TokenKind::Integer, start, None);
            }
        }
    }

    fn radix_int(&mut self, radix: u32) -> Option<TokenKind> {
        self.pos += 2;
        let digits_start = self.pos;
        while self.peek().is_some_and(|c| c.is_digit(radix) || c == '_') {
            self.bump();
        }
        let has_digit = self.src[digits_start..self.pos].chars().any(|c| c != '_');
        let suffix = self.take_word();
        (has_digit && (suffix.is_empty() || is_int_suffix(&suffix))).then_some(TokenKind::Integer)
    }

    fn decimal(&mut self) -> Option<TokenKind> {
        let mut float = false;
        self.take_digits();
        if self.peek() == Some('.') && self.peek_at(1) != Some('.') {
            self.bump();
            float = true;
            self.take_digits();
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let sign = matches!(self.peek_at(1), Some('+' | '-'));
            let digit_at = if sign { 2 } else { 1 };
            if self.peek_at(digit_at).is_some_and(|c| c.is_ascii_digit()) {
                self.pos += digit_at;
                self.take_digits();
                float = true;
            } else {
                // `1e`, `1e+`: swallow the stub so the error covers it.
                self.pos += digit_at;
                self.take_word();
                return None;
            }
        }
        let suffix = self.take_word();
        if suffix.is_empty() {
            Some(if float { TokenKind::Float } else { TokenKind::Integer })
        } else if suffix == "f32" || suffix == "f64" {
            Some(TokenKind::Float)
        } else if !float && is_int_suffix(&suffix) {
            Some(TokenKind::Integer)
        } else {
            None
        }
    }

    fn take_digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
            self.bump();
        }
    }

    /// Whatever identifier characters run on from here — a type suffix,
    /// or junk that makes the number invalid.
    fn take_word(&mut self) -> String {
        let start = self.pos;
        while self.peek().is_some_and(is_ident_continue) {
            self.bump();
        }
        self.src[start..self.pos].to_string()
    }
}

fn is_int_suffix(s: &str) -> bool {
    matches!(
        s,
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
    )
}

pub fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

pub fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// Raw identifiers (`r#...`) may also hold `.`, `+` and `-`.
pub fn is_raw_ident_char(c: char) -> bool {
    is_ident_continue(c) || matches!(c, '.' | '+' | '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn kinds(src: &str) -> Vec<(TokenKind, &str)> {
        let lexed = lex(src);
        assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
        lexed
            .tokens
            .iter()
            .map(|t| (t.kind, &src[t.span.start as usize..t.span.end as usize]))
            .collect()
    }

    fn codes(src: &str) -> Vec<DiagnosticCode> {
        lex(src).diagnostics.iter().map(|d| d.code).collect()
    }

    #[test]
    fn punctuation_and_identifiers() {
        use TokenKind::*;
        assert_eq!(
            kinds("Foo(a: [1], b: {}) #! r#type"),
            vec![
                (Ident, "Foo"),
                (LParen, "("),
                (Ident, "a"),
                (Colon, ":"),
                (LBracket, "["),
                (Integer, "1"),
                (RBracket, "]"),
                (Comma, ","),
                (Ident, "b"),
                (Colon, ":"),
                (LBrace, "{"),
                (RBrace, "}"),
                (RParen, ")"),
                (Hash, "#"),
                (Bang, "!"),
                (Ident, "r#type"),
            ]
        );
    }

    #[test]
    fn raw_identifiers_allow_dots_and_dashes() {
        assert_eq!(kinds("r#foo.bar-baz"), vec![(TokenKind::Ident, "r#foo.bar-baz")]);
    }

    #[test]
    fn numbers() {
        use TokenKind::*;
        for (src, kind) in [
            ("42", Integer),
            ("-7", Integer),
            ("+7", Integer),
            ("1_000_000", Integer),
            ("0xFF_ff", Integer),
            ("0o777", Integer),
            ("0b1010", Integer),
            ("255u8", Integer),
            ("0xffu8", Integer),
            ("-3i128", Integer),
            ("1.5", Float),
            ("1.", Float),
            ("1.e5", Float),
            ("1.f32", Float),
            (".5", Float),
            ("-.5", Float),
            ("1e10", Float),
            ("1.5E-3", Float),
            ("2f32", Float),
            ("1.0f64", Float),
            ("-inf", Float),
            ("+NaN", Float),
        ] {
            assert_eq!(kinds(src), vec![(kind, src)], "{src}");
        }
    }

    #[test]
    fn invalid_numbers() {
        for src in ["0x", "0b102", "1e", "1.5u8", "12abc", "0o8"] {
            assert_eq!(codes(src), vec![DiagnosticCode::InvalidNumber], "{src}");
        }
    }

    #[test]
    fn bare_inf_and_nan_are_identifiers() {
        assert_eq!(kinds("inf"), vec![(TokenKind::Ident, "inf")]);
        assert_eq!(kinds("NaN"), vec![(TokenKind::Ident, "NaN")]);
    }

    #[test]
    fn strings_decode_escapes() {
        let lexed = lex(r#""a\n\"b\" \u{1F980} é \x41""#);
        assert!(lexed.diagnostics.is_empty());
        assert_eq!(lexed.tokens[0].value.as_deref(), Some("a\n\"b\" 🦀 é A"));
    }

    #[test]
    fn strings_may_span_lines() {
        let lexed = lex("\"one\ntwo\"");
        assert_eq!(lexed.tokens.len(), 1);
        assert_eq!(lexed.tokens[0].value.as_deref(), Some("one\ntwo"));
    }

    #[test]
    fn raw_and_byte_strings() {
        use TokenKind::*;
        assert_eq!(kinds(r#"r"a\b""#), vec![(String, r#"r"a\b""#)]);
        assert_eq!(kinds(r###"r#"say "hi""#"###), vec![(String, r###"r#"say "hi""#"###)]);
        assert_eq!(kinds(r#"b"bytes\xff""#), vec![(ByteString, r#"b"bytes\xff""#)]);
        assert_eq!(kinds(r##"br#"x"#"##), vec![(ByteString, r##"br#"x"#"##)]);
        let lexed = lex(r###"r#"say "hi""#"###);
        assert_eq!(lexed.tokens[0].value.as_deref(), Some(r#"say "hi""#));
    }

    #[test]
    fn non_ascii_hex_escape_only_in_byte_strings() {
        assert_eq!(codes(r#""\xff""#), vec![DiagnosticCode::InvalidEscape]);
        assert!(codes(r#"b"\xff""#).is_empty());
    }

    #[test]
    fn string_errors() {
        assert_eq!(codes("\"open"), vec![DiagnosticCode::UnterminatedString]);
        assert_eq!(codes("r#\"open\""), vec![DiagnosticCode::UnterminatedString]);
        assert_eq!(codes(r#""\q""#), vec![DiagnosticCode::InvalidEscape]);
        assert_eq!(codes(r#""\u{110000}""#), vec![DiagnosticCode::InvalidEscape]);
    }

    #[test]
    fn chars() {
        assert_eq!(kinds("'a'"), vec![(TokenKind::Char, "'a'")]);
        assert_eq!(kinds(r"'\n'"), vec![(TokenKind::Char, r"'\n'")]);
        assert_eq!(kinds("'🦀'"), vec![(TokenKind::Char, "'🦀'")]);
        assert_eq!(kinds("b'x'"), vec![(TokenKind::Char, "b'x'")]);
        assert_eq!(codes("'ab'"), vec![DiagnosticCode::InvalidChar]);
        assert_eq!(codes("''"), vec![DiagnosticCode::InvalidChar]);
        assert_eq!(codes("'a\n"), vec![DiagnosticCode::InvalidChar]);
    }

    #[test]
    fn comments_nest_and_record_newlines() {
        let src = "1 // tail\n\n/* a /* b */ c */ 2";
        let lexed = lex(src);
        assert!(lexed.diagnostics.is_empty());
        let shape: Vec<(TokenKind, u32)> = lexed.tokens.iter().map(|t| (t.kind, t.newlines_before)).collect();
        assert_eq!(
            shape,
            vec![
                (TokenKind::Integer, 0),
                (TokenKind::LineComment, 0),
                (TokenKind::BlockComment, 2),
                (TokenKind::Integer, 0),
            ]
        );
    }

    #[test]
    fn line_comment_excludes_carriage_return() {
        let src = "// note\r\n1";
        let lexed = lex(src);
        let t = &lexed.tokens[0];
        assert_eq!(&src[t.span.start as usize..t.span.end as usize], "// note");
    }

    #[test]
    fn unterminated_block_comment() {
        assert_eq!(codes("/* /* */"), vec![DiagnosticCode::UnterminatedComment]);
    }

    #[test]
    fn stray_characters_are_unknown_tokens() {
        let lexed = lex("@ - ;");
        let kinds: Vec<_> = lexed.tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Unknown; 3]);
    }
}
