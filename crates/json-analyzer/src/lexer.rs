//! Tolerant JSON5-superset lexer.
//!
//! The lexer always *reads* the full JSON5 syntax; the flavor only
//! decides what gets flagged. Strings are decoded here (escapes
//! resolved) because both duplicate-key detection and the JSONL table
//! need the decoded text, while the spans keep the raw bytes available
//! to the formatter.

use crate::ast::{CommentKind, QuoteKind};
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::flavor::Flavor;
use crate::spans::ByteSpan;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Comma,
    /// Decoded string. The quote kind distinguishes `"` from `'`.
    String { value: String, quote: QuoteKind },
    /// A numeric literal, raw text in the span. Covers the JSON5
    /// extensions (`0x1F`, `+1`, `.5`, `-Infinity`, `NaN`).
    Number,
    /// A bare word: `true`, `null`, `Infinity`, an unquoted key…
    Ident,
    Comment(CommentKind),
    /// A byte sequence no flavor understands; one diagnostic per token.
    Error,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: ByteSpan,
}

pub struct Lexer<'a> {
    source: &'a str,
    /// Byte offset added to every span, so a JSONL line can be lexed in
    /// isolation while reporting file-absolute positions.
    base: u32,
    pos: usize,
    flavor: Flavor,
    pub diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str, base: u32, flavor: Flavor) -> Self {
        Lexer { source, base, pos: 0, flavor, diagnostics: Vec::new() }
    }

    /// Lex everything, Eof included as the final token.
    pub fn tokens(mut self) -> (Vec<Token>, Vec<Diagnostic>) {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token();
            let done = token.kind == TokenKind::Eof;
            tokens.push(token);
            if done {
                break;
            }
        }
        (tokens, self.diagnostics)
    }

    fn span_from(&self, start: usize) -> ByteSpan {
        ByteSpan::new(self.base + start as u32, self.base + self.pos as u32)
    }

    fn peek_char(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn peek_char_at(&self, offset: usize) -> Option<char> {
        self.source[self.pos..].chars().nth(offset)
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn error(&mut self, code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) {
        self.diagnostics.push(Diagnostic::error(code, message, span));
    }

    fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        let start = self.pos;
        let Some(ch) = self.peek_char() else {
            return Token { kind: TokenKind::Eof, span: self.span_from(start) };
        };
        match ch {
            '{' => self.punct(TokenKind::LBrace),
            '}' => self.punct(TokenKind::RBrace),
            '[' => self.punct(TokenKind::LBracket),
            ']' => self.punct(TokenKind::RBracket),
            ':' => self.punct(TokenKind::Colon),
            ',' => self.punct(TokenKind::Comma),
            '"' | '\'' => self.string(ch),
            '/' => self.comment(),
            '-' | '+' | '.' | '0'..='9' => self.number(),
            c if is_ident_start(c) => self.ident(),
            _ => {
                self.bump();
                let span = self.span_from(start);
                self.error(
                    DiagnosticCode::SyntaxError,
                    format!("unexpected character `{ch}`"),
                    span,
                );
                Token { kind: TokenKind::Error, span }
            }
        }
    }

    fn punct(&mut self, kind: TokenKind) -> Token {
        let start = self.pos;
        self.bump();
        Token { kind, span: self.span_from(start) }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            // The JSON core four plus the JSON5 additions. Accepting the
            // extras everywhere keeps recovery calm; a pasted NBSP will
            // still surface as a syntax error in whatever it interrupts.
            let ws = matches!(ch, ' ' | '\t' | '\n' | '\r')
                || matches!(ch, '\u{000B}' | '\u{000C}' | '\u{00A0}' | '\u{FEFF}')
                || matches!(ch, '\u{2028}' | '\u{2029}')
                || (ch.is_whitespace() && !ch.is_ascii());
            if !ws {
                break;
            }
            self.bump();
        }
    }

    fn comment(&mut self) -> Token {
        let start = self.pos;
        self.bump(); // '/'
        let kind = match self.peek_char() {
            Some('/') => {
                while let Some(ch) = self.peek_char() {
                    if ch == '\n' {
                        break;
                    }
                    self.bump();
                }
                // Exclude a trailing '\r' so CRLF sources keep clean spans.
                let end = self.pos - usize::from(self.source[start..self.pos].ends_with('\r'));
                let span = ByteSpan::new(self.base + start as u32, self.base + end as u32);
                self.check_comment_allowed(span);
                return Token { kind: TokenKind::Comment(CommentKind::Line), span };
            }
            Some('*') => {
                self.bump();
                let mut closed = false;
                while let Some(ch) = self.bump() {
                    if ch == '*' && self.peek_char() == Some('/') {
                        self.bump();
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    let span = self.span_from(start);
                    self.error(
                        DiagnosticCode::UnterminatedComment,
                        "block comment is never closed",
                        span,
                    );
                }
                CommentKind::Block
            }
            _ => {
                self.bump();
                let span = self.span_from(start);
                self.error(DiagnosticCode::SyntaxError, "unexpected character `/`", span);
                return Token { kind: TokenKind::Error, span };
            }
        };
        let span = self.span_from(start);
        self.check_comment_allowed(span);
        Token { kind: TokenKind::Comment(kind), span }
    }

    fn check_comment_allowed(&mut self, span: ByteSpan) {
        if !self.flavor.allows_comments() {
            let noun = match self.flavor {
                Flavor::Json => "JSON",
                Flavor::Jsonl => "JSON Lines",
                _ => "this flavor",
            };
            self.error(
                DiagnosticCode::CommentNotAllowed,
                format!("comments are not allowed in {noun}"),
                span,
            );
        }
    }

    fn string(&mut self, quote_char: char) -> Token {
        let start = self.pos;
        self.bump(); // opening quote
        let quote = if quote_char == '"' { QuoteKind::Double } else { QuoteKind::Single };
        if quote == QuoteKind::Single && !self.flavor.allows_json5_syntax() {
            // Span patched to the full literal once it is lexed.
            let open = self.span_from(start);
            self.error(
                DiagnosticCode::SingleQuoteNotAllowed,
                "single-quoted strings are only allowed in JSON5",
                open,
            );
        }
        let mut value = String::new();
        let mut terminated = false;
        while let Some(ch) = self.peek_char() {
            match ch {
                c if c == quote_char => {
                    self.bump();
                    terminated = true;
                    break;
                }
                '\n' | '\r' => break,
                '\\' => self.escape(&mut value),
                c if (c as u32) < 0x20 => {
                    let esc_start = self.pos;
                    self.bump();
                    let span = self.span_from(esc_start);
                    self.error(
                        DiagnosticCode::ControlCharacterInString,
                        "control characters must be escaped inside strings",
                        span,
                    );
                    value.push(c);
                }
                c => {
                    self.bump();
                    value.push(c);
                }
            }
        }
        let span = self.span_from(start);
        if !terminated {
            self.error(DiagnosticCode::UnterminatedString, "string is never closed", span);
        }
        Token { kind: TokenKind::String { value, quote }, span }
    }

    /// Decodes one escape sequence, cursor on the backslash.
    fn escape(&mut self, out: &mut String) {
        let esc_start = self.pos;
        self.bump(); // '\'
        let Some(ch) = self.bump() else {
            let span = self.span_from(esc_start);
            self.error(DiagnosticCode::InvalidEscape, "lone `\\` at end of input", span);
            return;
        };
        let strict = !self.flavor.allows_json5_syntax();
        match ch {
            '"' | '\\' | '/' => out.push(ch),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000C}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => self.unicode_escape(esc_start, out),
            // JSON5 extensions from here down.
            '\'' => {
                out.push('\'');
                self.json5_escape_check(strict, esc_start, "\\'");
            }
            'v' => {
                out.push('\u{000B}');
                self.json5_escape_check(strict, esc_start, "\\v");
            }
            '0' => {
                out.push('\0');
                self.json5_escape_check(strict, esc_start, "\\0");
            }
            'x' => {
                let mut code = 0u32;
                let mut ok = true;
                for _ in 0..2 {
                    match self.peek_char().and_then(|c| c.to_digit(16)) {
                        Some(d) => {
                            code = code * 16 + d;
                            self.bump();
                        }
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                let span = self.span_from(esc_start);
                if !ok {
                    self.error(
                        DiagnosticCode::InvalidEscape,
                        "`\\x` expects two hex digits",
                        span,
                    );
                } else if strict {
                    self.error(
                        DiagnosticCode::InvalidEscape,
                        "`\\x` escapes are only allowed in JSON5",
                        span,
                    );
                }
                out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            }
            '\n' | '\r' | '\u{2028}' | '\u{2029}' => {
                // Line continuation: the escaped terminator vanishes.
                if ch == '\r' && self.peek_char() == Some('\n') {
                    self.bump();
                }
                self.json5_escape_check(strict, esc_start, "line continuations");
            }
            other => {
                out.push(other);
                if strict {
                    let span = self.span_from(esc_start);
                    self.error(
                        DiagnosticCode::InvalidEscape,
                        format!("`\\{other}` is not a valid escape"),
                        span,
                    );
                }
            }
        }
    }

    fn json5_escape_check(&mut self, strict: bool, esc_start: usize, what: &str) {
        if strict {
            let span = self.span_from(esc_start);
            self.error(
                DiagnosticCode::InvalidEscape,
                format!("{what} are only allowed in JSON5"),
                span,
            );
        }
    }

    /// `\u` already consumed. Handles surrogate pairs.
    fn unicode_escape(&mut self, esc_start: usize, out: &mut String) {
        let Some(first) = self.hex4(esc_start) else {
            return;
        };
        if (0xD800..0xDC00).contains(&first) {
            // High surrogate: a following `\uXXXX` low surrogate completes it.
            let saved = self.pos;
            if self.peek_char() == Some('\\') && self.peek_char_at(1) == Some('u') {
                self.bump();
                self.bump();
                if let Some(second) = self.hex4(esc_start) {
                    if (0xDC00..0xE000).contains(&second) {
                        let c = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                        out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                        return;
                    }
                }
                self.pos = saved;
            }
            out.push('\u{FFFD}');
            return;
        }
        out.push(char::from_u32(first).unwrap_or('\u{FFFD}'));
    }

    fn hex4(&mut self, esc_start: usize) -> Option<u32> {
        let mut code = 0u32;
        for _ in 0..4 {
            match self.peek_char().and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    code = code * 16 + d;
                    self.bump();
                }
                None => {
                    let span = self.span_from(esc_start);
                    self.error(
                        DiagnosticCode::InvalidEscape,
                        "`\\u` expects four hex digits",
                        span,
                    );
                    return None;
                }
            }
        }
        Some(code)
    }

    fn number(&mut self) -> Token {
        let start = self.pos;
        if matches!(self.peek_char(), Some('-') | Some('+')) {
            self.bump();
        }
        if self.peek_char().is_some_and(|c| c.is_ascii_alphabetic()) {
            // `-Infinity`, `+NaN`…
            while self.peek_char().is_some_and(is_ident_continue) {
                self.bump();
            }
        } else {
            let mut seen_exponent = false;
            while let Some(ch) = self.peek_char() {
                match ch {
                    '0'..='9' | '.' => {
                        self.bump();
                    }
                    'x' | 'X' => {
                        self.bump();
                    }
                    'a'..='f' | 'A'..='F' => {
                        // Hex digit — but `e`/`E` doubles as the exponent
                        // marker in decimal literals. Treat it as an
                        // exponent when a sign or digit follows and the
                        // literal has no `x` so far.
                        let is_hex = self.source[start..self.pos].contains(['x', 'X']);
                        if !is_hex && matches!(ch, 'e' | 'E') {
                            seen_exponent = true;
                        }
                        self.bump();
                    }
                    '+' | '-' => {
                        let prev = self.source[start..self.pos].chars().next_back();
                        if seen_exponent && matches!(prev, Some('e') | Some('E')) {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        }
        let span = self.span_from(start);
        let raw = &self.source[start..self.pos];
        match classify_number(raw) {
            NumberClass::Strict => {}
            NumberClass::Json5 => {
                if !self.flavor.allows_json5_syntax() {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::NonStandardNumber,
                        format!("`{raw}` is only a valid number in JSON5"),
                        span,
                    ));
                }
            }
            NumberClass::Invalid => {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidNumber,
                    format!("`{raw}` is not a valid number"),
                    span,
                ));
            }
        }
        Token { kind: TokenKind::Number, span }
    }

    fn ident(&mut self) -> Token {
        let start = self.pos;
        while self.peek_char().is_some_and(is_ident_continue) {
            self.bump();
        }
        Token { kind: TokenKind::Ident, span: self.span_from(start) }
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphanumeric()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumberClass {
    /// Valid RFC 8259 number.
    Strict,
    /// Valid only with the JSON5 extensions.
    Json5,
    /// Not a number under any flavor.
    Invalid,
}

fn classify_number(raw: &str) -> NumberClass {
    if is_strict_number(raw) {
        NumberClass::Strict
    } else if is_json5_number(raw) {
        NumberClass::Json5
    } else {
        NumberClass::Invalid
    }
}

fn is_strict_number(raw: &str) -> bool {
    let mut s = raw.strip_prefix('-').unwrap_or(raw);
    // Integer part: `0` alone, or a non-zero-led digit run.
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    if digits > 1 && s.starts_with('0') {
        return false;
    }
    s = &s[digits..];
    if let Some(rest) = s.strip_prefix('.') {
        let frac = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if frac == 0 {
            return false;
        }
        s = &rest[frac..];
    }
    if let Some(mut rest) = s.strip_prefix(['e', 'E']) {
        rest = rest.strip_prefix(['+', '-']).unwrap_or(rest);
        let exp = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if exp == 0 {
            return false;
        }
        s = &rest[exp..];
    }
    s.is_empty()
}

fn is_json5_number(raw: &str) -> bool {
    let s = raw.strip_prefix(['-', '+']).unwrap_or(raw);
    if s == "Infinity" || s == "NaN" {
        return true;
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    // Decimal with optional leading/trailing dot: `.5`, `5.`, `5.e2`.
    // JSON5 still forbids leading zeros (`012`).
    let mut rest = s;
    let int_digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if int_digits > 1 && rest.starts_with('0') {
        return false;
    }
    rest = &rest[int_digits..];
    let mut frac_digits = 0;
    if let Some(after_dot) = rest.strip_prefix('.') {
        frac_digits = after_dot.chars().take_while(|c| c.is_ascii_digit()).count();
        rest = &after_dot[frac_digits..];
    }
    if int_digits == 0 && frac_digits == 0 {
        return false;
    }
    if let Some(mut exp) = rest.strip_prefix(['e', 'E']) {
        exp = exp.strip_prefix(['+', '-']).unwrap_or(exp);
        let exp_digits = exp.chars().take_while(|c| c.is_ascii_digit()).count();
        if exp_digits == 0 {
            return false;
        }
        rest = &exp[exp_digits..];
    }
    rest.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn lex(src: &str, flavor: Flavor) -> (Vec<Token>, Vec<Diagnostic>) {
        Lexer::new(src, 0, flavor).tokens()
    }

    fn kinds(src: &str, flavor: Flavor) -> Vec<TokenKind> {
        lex(src, flavor).0.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn lexes_basic_punctuation_and_scalars() {
        let ks = kinds("{\"a\": [1, true, null]}", Flavor::Json);
        assert_eq!(
            ks,
            vec![
                TokenKind::LBrace,
                TokenKind::String { value: "a".into(), quote: QuoteKind::Double },
                TokenKind::Colon,
                TokenKind::LBracket,
                TokenKind::Number,
                TokenKind::Comma,
                TokenKind::Ident,
                TokenKind::Comma,
                TokenKind::Ident,
                TokenKind::RBracket,
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn decodes_escapes() {
        let (tokens, diags) = lex(r#""a\n\tA😀""#, Flavor::Json);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            tokens[0].kind,
            TokenKind::String { value: "a\n\tA😀".into(), quote: QuoteKind::Double }
        );
    }

    #[test]
    fn lone_surrogate_becomes_replacement_char() {
        let (tokens, _) = lex(r#""\uD800x""#, Flavor::Json);
        assert_eq!(
            tokens[0].kind,
            TokenKind::String { value: "\u{FFFD}x".into(), quote: QuoteKind::Double }
        );
    }

    #[test]
    fn unterminated_string_is_diagnosed() {
        let (_, diags) = lex("\"abc\n", Flavor::Json);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnterminatedString));
    }

    #[test]
    fn single_quotes_flagged_outside_json5() {
        let (_, diags) = lex("'a'", Flavor::Json);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::SingleQuoteNotAllowed));
        let (_, diags) = lex("'a'", Flavor::Json5);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn comments_flagged_by_flavor() {
        let (_, diags) = lex("// hi\n1", Flavor::Json);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::CommentNotAllowed));
        let (_, diags) = lex("// hi\n/* block */ 1", Flavor::Jsonc);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unterminated_block_comment() {
        let (_, diags) = lex("/* forever", Flavor::Jsonc);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnterminatedComment));
    }

    #[test]
    fn strict_numbers_pass_every_flavor() {
        for n in ["0", "-1", "12.5", "1e10", "1.5E-3", "0.0"] {
            let (_, diags) = lex(n, Flavor::Json);
            assert!(diags.is_empty(), "{n}: {diags:?}");
        }
    }

    #[test]
    fn json5_numbers_flagged_in_strict_json() {
        for n in ["0x1F", "+1", ".5", "5.", "Infinity", "-Infinity", "NaN"] {
            let (_, diags) = lex(n, Flavor::Json);
            let flagged = diags
                .iter()
                .any(|d| d.code == DiagnosticCode::NonStandardNumber);
            // Bare `Infinity`/`NaN` lex as idents; the parser flags those.
            let ident_words = n == "Infinity" || n == "NaN";
            assert_eq!(flagged, !ident_words, "{n}: {diags:?}");
            let (_, diags5) = lex(n, Flavor::Json5);
            assert!(diags5.is_empty(), "{n}: {diags5:?}");
        }
    }

    #[test]
    fn leading_zero_is_json5_only() {
        let (_, diags) = lex("012", Flavor::Json);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::InvalidNumber), "{diags:?}");
    }

    #[test]
    fn broken_number_is_invalid_everywhere() {
        let (_, diags) = lex("1.2.3", Flavor::Json5);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::InvalidNumber));
    }

    #[test]
    fn line_continuation_joins_string() {
        let (tokens, diags) = lex("'one \\\ntwo'", Flavor::Json5);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            tokens[0].kind,
            TokenKind::String { value: "one two".into(), quote: QuoteKind::Single }
        );
    }

    #[test]
    fn control_char_in_string_flagged() {
        let (_, diags) = lex("\"a\u{0001}b\"", Flavor::Json);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::ControlCharacterInString));
    }

    #[test]
    fn spans_respect_base_offset() {
        let (tokens, _) = Lexer::new("[1]", 100, Flavor::Json).tokens();
        assert_eq!(tokens[0].span, ByteSpan::new(100, 101));
        assert_eq!(tokens[1].span, ByteSpan::new(101, 102));
    }
}
