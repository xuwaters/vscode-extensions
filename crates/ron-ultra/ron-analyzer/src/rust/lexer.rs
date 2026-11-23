//! A forgiving Rust lexer, just deep enough to read item declarations.
//!
//! Nothing here reports errors: a Rust file being typed is as likely to be
//! half-written as not, and the worst a bad token can do is hide one item
//! from the schema. Comments are dropped except doc comments, which become
//! [`TokenKind::Doc`] tokens so a field's documentation can reach a hover.

use crate::spans::ByteSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Includes keywords; a raw `r#name` keeps its `r#` in the source but
    /// not in [`Token::text`].
    Ident,
    /// `'a`, `'static`.
    Lifetime,
    /// `"..."`, `r#"..."#`, `b"..."`, `c"..."`; [`Token::text`] is the
    /// decoded content for plain and raw strings.
    Str,
    /// Integer and float literals, with any suffix.
    Number,
    /// `'x'`, `b'x'`.
    Char,
    /// `///` or `/** */` documentation; [`Token::text`] is its content.
    Doc,
    /// `//!` or `/*! */` documentation of the enclosing item.
    InnerDoc,
    /// `::`
    PathSep,
    /// `->`
    Arrow,
    /// `=>`
    FatArrow,
    /// Any other single character.
    Punct(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: ByteSpan,
    pub text: String,
}

impl Token {
    pub fn is_punct(&self, c: char) -> bool {
        self.kind == TokenKind::Punct(c)
    }

    pub fn is_ident(&self, name: &str) -> bool {
        self.kind == TokenKind::Ident && self.text == name
    }
}

pub fn lex(source: &str) -> Vec<Token> {
    let mut lx = Lexer { src: source, bytes: source.as_bytes(), pos: 0, out: Vec::new() };
    lx.run();
    lx.out
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    out: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn peek(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.pos + n).copied()
    }

    fn push(&mut self, kind: TokenKind, start: usize, text: String) {
        self.out.push(Token { kind, span: ByteSpan::from_usize(start, self.pos), text });
    }

    fn run(&mut self) {
        while let Some(b) = self.peek(0) {
            let start = self.pos;
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                b'/' if self.peek(1) == Some(b'/') => self.line_comment(),
                b'/' if self.peek(1) == Some(b'*') => self.block_comment(),
                b'"' => {
                    let text = self.quoted(b'"');
                    self.push(TokenKind::Str, start, text);
                }
                b'\'' => self.quote_or_lifetime(),
                b'0'..=b'9' => self.number(),
                b':' if self.peek(1) == Some(b':') => {
                    self.pos += 2;
                    self.push(TokenKind::PathSep, start, "::".into());
                }
                b'-' if self.peek(1) == Some(b'>') => {
                    self.pos += 2;
                    self.push(TokenKind::Arrow, start, "->".into());
                }
                b'=' if self.peek(1) == Some(b'>') => {
                    self.pos += 2;
                    self.push(TokenKind::FatArrow, start, "=>".into());
                }
                _ if self.prefixed_literal() => {}
                _ => {
                    let c = self.src[self.pos..].chars().next().unwrap_or('\0');
                    if c == '_' || c.is_alphabetic() {
                        self.ident();
                    } else {
                        self.pos += c.len_utf8().max(1);
                        self.push(TokenKind::Punct(c), start, c.to_string());
                    }
                }
            }
        }
    }

    fn line_comment(&mut self) {
        let start = self.pos;
        let end = self.src[start..].find('\n').map_or(self.src.len(), |i| start + i);
        let body = &self.src[start + 2..end];
        self.pos = end;
        if let Some(doc) = body.strip_prefix('/').filter(|d| !d.starts_with('/')) {
            self.push(TokenKind::Doc, start, doc.trim_end_matches('\r').to_string());
        } else if let Some(doc) = body.strip_prefix('!') {
            self.push(TokenKind::InnerDoc, start, doc.trim_end_matches('\r').to_string());
        }
    }

    fn block_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        let mut depth = 1;
        while depth > 0 && self.pos < self.bytes.len() {
            if self.bytes[self.pos..].starts_with(b"/*") {
                depth += 1;
                self.pos += 2;
            } else if self.bytes[self.pos..].starts_with(b"*/") {
                depth -= 1;
                self.pos += 2;
            } else {
                self.pos += 1;
            }
        }
        let text = &self.src[start..self.pos];
        let inner = text.strip_prefix("/*").unwrap_or(text);
        let inner = inner.strip_suffix("*/").unwrap_or(inner);
        if let Some(doc) = inner.strip_prefix('*').filter(|d| !d.starts_with('*') && !d.is_empty()) {
            self.push(TokenKind::Doc, start, doc.to_string());
        } else if let Some(doc) = inner.strip_prefix('!') {
            self.push(TokenKind::InnerDoc, start, doc.to_string());
        }
    }

    /// `b"..."`, `r"..."`, `r#"..."#`, `br#"..."#`, `c"..."`, `b'x'` and
    /// raw identifiers `r#name`. Returns false when the current character
    /// starts none of them.
    fn prefixed_literal(&mut self) -> bool {
        let start = self.pos;
        let rest = &self.bytes[self.pos..];
        let (prefix_len, raw) = match rest {
            [b'b' | b'c', b'r', b'"' | b'#', ..] => (2, true),
            [b'r', b'"' | b'#', ..] => (1, true),
            [b'b' | b'c', b'"', ..] => (1, false),
            [b'b', b'\'', ..] => {
                self.pos += 1;
                let text = self.quoted(b'\'');
                self.push(TokenKind::Char, start, text);
                return true;
            }
            _ => return false,
        };
        if raw {
            let hashes = rest[prefix_len..].iter().take_while(|&&b| b == b'#').count();
            if rest.get(prefix_len + hashes) != Some(&b'"') {
                // `r#ident`, a raw identifier.
                if prefix_len == 1 && hashes == 1 {
                    self.pos += 2;
                    let ident_start = self.pos;
                    self.eat_ident_chars();
                    let name = self.src[ident_start..self.pos].to_string();
                    self.push(TokenKind::Ident, start, name);
                    return true;
                }
                return false;
            }
            self.pos += prefix_len + hashes + 1;
            let content_start = self.pos;
            let closer: Vec<u8> = std::iter::once(b'"').chain(std::iter::repeat_n(b'#', hashes)).collect();
            let content_end = match self.bytes[self.pos..].windows(closer.len()).position(|w| w == closer.as_slice()) {
                Some(i) => {
                    self.pos += i + closer.len();
                    content_start + i
                }
                None => {
                    self.pos = self.bytes.len();
                    self.bytes.len()
                }
            };
            let text = self.src[content_start..content_end].to_string();
            self.push(TokenKind::Str, start, text);
        } else {
            self.pos += prefix_len;
            let text = self.quoted(b'"');
            self.push(TokenKind::Str, start, text);
        }
        true
    }

    /// A `"`- or `'`-delimited literal starting at the current quote;
    /// returns its decoded content (simple escapes only).
    fn quoted(&mut self, quote: u8) -> String {
        self.pos += 1;
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let c = self.src[self.pos..].chars().next().unwrap_or('\0');
            self.pos += c.len_utf8().max(1);
            match c {
                '\\' => {
                    let e = self.src[self.pos..].chars().next().unwrap_or('\0');
                    self.pos += e.len_utf8().max(1);
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '0' => out.push('\0'),
                        '\n' => {
                            while matches!(self.peek(0), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                                self.pos += 1;
                            }
                        }
                        'u' if self.peek(0) == Some(b'{') => {
                            let end = self.src[self.pos..].find('}').map_or(self.src.len(), |i| self.pos + i);
                            let hex = &self.src[self.pos + 1..end];
                            if let Some(ch) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                                out.push(ch);
                            }
                            self.pos = (end + 1).min(self.src.len());
                        }
                        other => out.push(other),
                    }
                }
                c if c as u32 == quote as u32 => break,
                c => out.push(c),
            }
        }
        out
    }

    fn quote_or_lifetime(&mut self) {
        let start = self.pos;
        let mut chars = self.src[self.pos + 1..].chars();
        let first = chars.next();
        let second = chars.next();
        let is_char = matches!((first, second), (Some('\\'), _) | (Some(_), Some('\'')));
        if is_char {
            let text = self.quoted(b'\'');
            self.push(TokenKind::Char, start, text);
        } else {
            self.pos += 1;
            let name_start = self.pos;
            self.eat_ident_chars();
            let text = self.src[name_start..self.pos].to_string();
            self.push(TokenKind::Lifetime, start, text);
        }
    }

    fn number(&mut self) {
        let start = self.pos;
        loop {
            match self.peek(0) {
                Some(b) if b.is_ascii_alphanumeric() || b == b'_' => self.pos += 1,
                // A decimal point, but not a range `0..` or a method `1.max`.
                Some(b'.') if self.peek(1).is_some_and(|b| b.is_ascii_digit()) => self.pos += 1,
                _ => break,
            }
        }
        let text = self.src[start..self.pos].to_string();
        self.push(TokenKind::Number, start, text);
    }

    fn eat_ident_chars(&mut self) {
        while let Some(c) = self.src[self.pos..].chars().next() {
            if c == '_' || c.is_alphanumeric() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn ident(&mut self) {
        let start = self.pos;
        self.eat_ident_chars();
        let text = self.src[start..self.pos].to_string();
        self.push(TokenKind::Ident, start, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(TokenKind, String)> {
        lex(src).into_iter().map(|t| (t.kind, t.text)).collect()
    }

    #[test]
    fn paths_and_generics() {
        use TokenKind::*;
        assert_eq!(
            kinds("std::Vec<'a, T>"),
            vec![
                (Ident, "std".into()),
                (PathSep, "::".into()),
                (Ident, "Vec".into()),
                (Punct('<'), "<".into()),
                (Lifetime, "a".into()),
                (Punct(','), ",".into()),
                (Ident, "T".into()),
                (Punct('>'), ">".into()),
            ]
        );
    }

    #[test]
    fn docs_survive_and_comments_do_not() {
        use TokenKind::*;
        assert_eq!(
            kinds("/// field doc\n// plain\n//// not doc\n/** block */ /*! inner */ x"),
            vec![
                (Doc, " field doc".into()),
                (Doc, " block ".into()),
                (InnerDoc, " inner ".into()),
                (Ident, "x".into()),
            ]
        );
    }

    #[test]
    fn literals() {
        use TokenKind::*;
        assert_eq!(
            kinds(r####"r#"a"b"# "x\"y" 'c' '\n' b'z' 1.5f32 0..2 r#type"####),
            vec![
                (Str, "a\"b".into()),
                (Str, "x\"y".into()),
                (Char, "c".into()),
                (Char, "\n".into()),
                (Char, "z".into()),
                (Number, "1.5f32".into()),
                (Number, "0".into()),
                (Punct('.'), ".".into()),
                (Punct('.'), ".".into()),
                (Number, "2".into()),
                (Ident, "type".into()),
            ]
        );
    }

    #[test]
    fn nested_block_comments_and_unterminated_input() {
        assert_eq!(kinds("/* a /* b */ c */ x").len(), 1);
        assert_eq!(kinds("\"open").len(), 1);
        assert_eq!(kinds("/* open").len(), 0);
    }
}
