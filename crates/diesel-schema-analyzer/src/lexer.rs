//! A tiny token stream for diesel schema files.
//!
//! We do not aim to be a full Rust lexer — only enough to find the three
//! macro invocations we care about and skip over comments, strings, and
//! attributes that might otherwise confuse delimiter matching.
//!
//! The token kinds we emit:
//!
//! * `Ident` — a bare identifier (`channel_members`, `Nullable`, `diesel`).
//! * `Punct` — a single significant punctuation character. We also emit
//!   the two-character `->` and `::` as their own kinds to simplify the
//!   parser.
//! * `Other` — anything we don't classify (literals, raw strings, etc).
//!   Carries no semantic value beyond its span.

use crate::spans::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    /// `::` — exactly two colons in a row, treated as one token.
    ColonColon,
    /// `->` — arrow.
    Arrow,
    /// `!`
    Bang,
    /// `.`
    Dot,
    /// `,`
    Comma,
    /// `;`
    Semicolon,
    /// `(` or `)`
    LParen,
    RParen,
    /// `{` or `}`
    LBrace,
    RBrace,
    /// `[` or `]`
    LBracket,
    RBracket,
    /// `<` or `>`
    LAngle,
    RAngle,
    /// `:` (single)
    Colon,
    /// `#` (attribute introducer in Rust)
    Hash,
    /// Anything else we don't model.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: ByteSpan,
}

pub fn tokenize(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // Whitespace.
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            i += 1;
            continue;
        }
        // Line comment.
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Block comment (supports nesting, like Rust).
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i + 1 < bytes.len() && depth > 0 {
                if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if depth > 0 {
                i = bytes.len();
            }
            continue;
        }
        // String / char / raw string literals — opaque single Other tokens.
        if c == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push(Token { kind: TokenKind::Other, span: ByteSpan::from_usize(start, i) });
            continue;
        }
        if c == b'\'' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'\'' {
                    i += 1;
                    break;
                }
                if bytes[i] == b'\n' {
                    break;
                }
                i += 1;
            }
            out.push(Token { kind: TokenKind::Other, span: ByteSpan::from_usize(start, i) });
            continue;
        }
        // Raw string `r"..."` / `r#"..."#`.
        if c == b'r'
            && i + 1 < bytes.len()
            && (bytes[i + 1] == b'"' || bytes[i + 1] == b'#')
        {
            let start = i;
            i += 1;
            let mut hashes = 0usize;
            while i < bytes.len() && bytes[i] == b'#' {
                hashes += 1;
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'"' {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'"' {
                        let mut h = 0;
                        let mut j = i + 1;
                        while h < hashes && j < bytes.len() && bytes[j] == b'#' {
                            h += 1;
                            j += 1;
                        }
                        if h == hashes {
                            i = j;
                            break;
                        }
                    }
                    i += 1;
                }
            }
            out.push(Token { kind: TokenKind::Other, span: ByteSpan::from_usize(start, i) });
            continue;
        }
        // Identifier / keyword.
        if is_ident_start(c) {
            let start = i;
            i += 1;
            while i < bytes.len() && is_ident_cont(bytes[i]) {
                i += 1;
            }
            let name = std::str::from_utf8(&bytes[start..i]).unwrap_or("").to_string();
            out.push(Token { kind: TokenKind::Ident(name), span: ByteSpan::from_usize(start, i) });
            continue;
        }
        // Multi-char punctuation.
        if c == b':' && i + 1 < bytes.len() && bytes[i + 1] == b':' {
            out.push(Token { kind: TokenKind::ColonColon, span: ByteSpan::from_usize(i, i + 2) });
            i += 2;
            continue;
        }
        if c == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            out.push(Token { kind: TokenKind::Arrow, span: ByteSpan::from_usize(i, i + 2) });
            i += 2;
            continue;
        }
        // Single-char punctuation.
        let kind = match c {
            b'!' => Some(TokenKind::Bang),
            b'.' => Some(TokenKind::Dot),
            b',' => Some(TokenKind::Comma),
            b';' => Some(TokenKind::Semicolon),
            b'(' => Some(TokenKind::LParen),
            b')' => Some(TokenKind::RParen),
            b'{' => Some(TokenKind::LBrace),
            b'}' => Some(TokenKind::RBrace),
            b'[' => Some(TokenKind::LBracket),
            b']' => Some(TokenKind::RBracket),
            b'<' => Some(TokenKind::LAngle),
            b'>' => Some(TokenKind::RAngle),
            b':' => Some(TokenKind::Colon),
            b'#' => Some(TokenKind::Hash),
            _ => None,
        };
        if let Some(k) = kind {
            out.push(Token { kind: k, span: ByteSpan::from_usize(i, i + 1) });
            i += 1;
            continue;
        }
        // Number / unknown punctuation — opaque. One whole char, so a
        // non-ASCII one (`中`, a BOM) never leaves a span inside it.
        let start = i;
        i += source[start..].chars().next().map_or(1, char::len_utf8);
        out.push(Token { kind: TokenKind::Other, span: ByteSpan::from_usize(start, i) });
    }
    out
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_macro_call() {
        let toks = tokenize("diesel::table!");
        assert_eq!(toks.len(), 4);
        assert!(matches!(toks[0].kind, TokenKind::Ident(ref s) if s == "diesel"));
        assert_eq!(toks[1].kind, TokenKind::ColonColon);
        assert!(matches!(toks[2].kind, TokenKind::Ident(ref s) if s == "table"));
        assert_eq!(toks[3].kind, TokenKind::Bang);
    }

    #[test]
    fn unknown_multibyte_char_is_one_token() {
        let src = "diesel::table! { 中 }";
        let toks = tokenize(src);
        let other = toks.iter().find(|t| t.kind == TokenKind::Other).unwrap();
        assert_eq!(&src[other.span.start as usize..other.span.end as usize], "中");
        assert!(toks.iter().all(|t| src.is_char_boundary(t.span.start as usize)
            && src.is_char_boundary(t.span.end as usize)));
    }

    #[test]
    fn skips_line_comments() {
        let toks = tokenize("// hi\nfoo");
        assert_eq!(toks.len(), 1);
        assert!(matches!(toks[0].kind, TokenKind::Ident(ref s) if s == "foo"));
    }

    #[test]
    fn skips_block_comments() {
        let toks = tokenize("/* a /* b */ c */ foo");
        assert_eq!(toks.len(), 1);
        assert!(matches!(toks[0].kind, TokenKind::Ident(ref s) if s == "foo"));
    }

    #[test]
    fn arrow_is_single_token() {
        let toks = tokenize("a -> b");
        assert_eq!(toks[1].kind, TokenKind::Arrow);
    }
}
