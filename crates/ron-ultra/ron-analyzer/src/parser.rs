//! Recursive-descent RON parser with local error recovery.
//!
//! The parser never gives up: a missing value becomes
//! [`ValueKind::Missing`], a missing comma is reported and the next entry
//! parsed anyway, a stray closing bracket that matches no open one is
//! skipped, and one that closes an outer container ends the inner one. So
//! a half-typed file still has an outline, folding and hovers.
//!
//! Comments are collected into `pending` as the parser looks past them and
//! handed out at the points where their owner is known — see [`crate::ast`]
//! for who owns what.

use crate::ast::{
    Attribute, Comment, CommentKind, Container, ContainerKind, Entry, EntryKey, File, Ident, Value,
    ValueKind,
};
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::extensions;
use crate::lexer::{lex, Token, TokenKind};
use crate::spans::ByteSpan;
use std::collections::HashSet;

pub fn parse(source: &str) -> (File, Vec<Diagnostic>) {
    let lexed = lex(source);
    let mut p = Parser {
        src: source,
        tokens: lexed.tokens,
        pos: 0,
        consumed: 0,
        last_end: 0,
        pending: Vec::new(),
        open: Vec::new(),
        diags: lexed.diagnostics,
    };
    let file = p.file();
    let mut diags = p.diags;
    diags.sort_by_key(|d| (d.span.start, d.span.end));
    (file, diags)
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    /// Index of the next unread token, comments included.
    pos: usize,
    /// Significant tokens consumed so far — the progress guard.
    consumed: usize,
    /// End of the last significant token consumed.
    last_end: u32,
    pending: Vec<Comment>,
    /// Closing brackets of the containers currently open, innermost last.
    open: Vec<TokenKind>,
    diags: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    // ---- token cursor ----------------------------------------------------

    /// The next significant token's kind; comments passed on the way are
    /// moved into `pending`.
    fn peek(&mut self) -> Option<TokenKind> {
        while let Some(tok) = self.tokens.get(self.pos) {
            if !tok.kind.is_comment() {
                return Some(tok.kind);
            }
            self.pending.push(Comment {
                span: tok.span,
                kind: if tok.kind == TokenKind::LineComment { CommentKind::Line } else { CommentKind::Block },
                own_line: tok.newlines_before > 0,
                blank_before: tok.newlines_before > 1,
            });
            self.pos += 1;
        }
        None
    }

    /// The token [`Self::peek`] just looked at. Only call after a `Some`.
    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    /// Kind of the `n`th significant token from here, without collecting
    /// comments.
    fn nth(&self, n: usize) -> Option<TokenKind> {
        self.tokens[self.pos..].iter().filter(|t| !t.kind.is_comment()).nth(n).map(|t| t.kind)
    }

    fn bump(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        self.pos += 1;
        self.consumed += 1;
        self.last_end = tok.span.end;
        tok
    }

    fn take_pending(&mut self) -> Vec<Comment> {
        std::mem::take(&mut self.pending)
    }

    /// The comments that continue the line of the last token consumed.
    fn take_same_line(&mut self) -> Vec<Comment> {
        self.peek();
        let n = self.pending.iter().take_while(|c| !c.own_line).count();
        self.pending.drain(..n).collect()
    }

    fn text(&self, span: ByteSpan) -> &'a str {
        &self.src[span.start as usize..span.end as usize]
    }

    fn eof(&self) -> ByteSpan {
        let len = self.src.len() as u32;
        ByteSpan::new(len, len)
    }

    fn error(&mut self, code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) {
        self.diags.push(Diagnostic::error(code, message, span));
    }

    fn syntax(&mut self, message: impl Into<String>, span: ByteSpan) {
        self.error(DiagnosticCode::SyntaxError, message, span);
    }

    /// "end of file" or "`x`", for "expected ..., found ..." messages.
    fn found(&self) -> String {
        match self.tokens.get(self.pos) {
            Some(t) => format!("`{}`", self.text(t.span)),
            None => "end of file".into(),
        }
    }

    // ---- file --------------------------------------------------------------

    fn file(&mut self) -> File {
        let mut file = File::default();
        while self.peek() == Some(TokenKind::Hash) {
            let leading = self.take_pending();
            let blank_before = self.current().newlines_before > 1;
            file.attributes.push(self.attribute(leading, blank_before));
        }
        file.leading = self.take_pending();
        if self.peek().is_none() {
            // Comments only (or nothing at all). RON wants a value, but an
            // empty file is what every new file looks like, so it is left
            // undiagnosed.
            return file;
        }
        file.value_blank_before = self.current().newlines_before > 1;
        let value = self.value();
        file.value_trailing = self.take_same_line();
        if self.peek().is_some() {
            let start = self.current().span.start;
            let message = format!("unexpected {} after the value; a RON file holds exactly one value", self.found());
            let end = self.tokens.iter().rev().find(|t| !t.kind.is_comment()).map_or(start, |t| t.span.end);
            self.error(DiagnosticCode::TrailingContent, message, ByteSpan::new(start, end));
            // Nothing past here is part of the document; drop it, comments
            // and all, so nothing downstream mistakes it for content.
            self.pos = self.tokens.len();
            self.pending.clear();
        }
        file.trailing = self.take_pending();
        file.value = Some(value);
        file
    }

    /// `#![name(...)]`. Walks raw tokens so comments inside the brackets
    /// stay part of the attribute's verbatim text.
    fn attribute(&mut self, leading: Vec<Comment>, blank_before: bool) -> Attribute {
        let hash = self.bump();
        let mut attr = Attribute {
            span: hash.span,
            leading,
            trailing: Vec::new(),
            blank_before,
            name: None,
            extensions: Vec::new(),
        };
        for (kind, what) in [(TokenKind::Bang, "`!` after `#`"), (TokenKind::LBracket, "`[` after `#!`")] {
            if self.peek() == Some(kind) && self.pending.is_empty() {
                attr.span = attr.span.join(self.bump().span);
            } else {
                let message = format!("expected {what}, found {}; attributes are written `#![enable(...)]`", self.found());
                self.syntax(message, attr.span);
                return attr;
            }
        }
        // Inside the brackets: depth 1 is `[...]`, depth 2 is `name(...)`.
        let mut depth = 1u32;
        while let Some(tok) = self.tokens.get(self.pos).cloned() {
            self.pos += 1;
            if tok.kind.is_comment() {
                continue;
            }
            self.consumed += 1;
            self.last_end = tok.span.end;
            attr.span = attr.span.join(tok.span);
            match tok.kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Ident if depth == 1 && attr.name.is_none() => attr.name = Some(ident(&tok, self.src)),
                TokenKind::Ident if depth == 2 && attr.name.as_ref().is_some_and(|n| n.name == "enable") => {
                    attr.extensions.push(ident(&tok, self.src));
                }
                _ => {}
            }
        }
        if depth > 0 {
            self.syntax("unclosed attribute: expected `]`", attr.span);
        }
        for ext in &attr.extensions {
            if extensions::find(&ext.name).is_none() {
                let known: Vec<&str> = extensions::EXTENSIONS.iter().map(|e| e.name).collect();
                let message = format!("unknown extension `{}`; RON has {}", ext.name, known.join(", "));
                self.diags.push(Diagnostic::error(DiagnosticCode::UnknownExtension, message, ext.span));
            }
        }
        attr.trailing = self.take_same_line();
        attr
    }

    // ---- values ------------------------------------------------------------

    fn value(&mut self) -> Value {
        let kind = self.peek();
        let prefix = self.take_pending();
        let Some(kind) = kind else {
            self.syntax("expected a value, found end of file", self.eof());
            return Value { span: self.eof(), kind: ValueKind::Missing, prefix };
        };
        let mut value = match kind {
            TokenKind::Integer => self.scalar(ValueKind::Integer),
            TokenKind::Float => self.scalar(ValueKind::Float),
            TokenKind::ByteString => self.scalar(ValueKind::ByteString),
            TokenKind::Char => self.scalar(ValueKind::Char),
            TokenKind::String => {
                let decoded = self.current().value.clone();
                self.scalar(ValueKind::String { decoded })
            }
            TokenKind::Ident => self.word(),
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => {
                let c = self.container();
                Value { span: container_span(&c, c.open.start, self.last_end), kind: ValueKind::Container(c), prefix: vec![] }
            }
            _ => {
                let tok = self.current().clone();
                self.syntax(format!("expected a value, found {}", self.found()), tok.span);
                // Leave separators and closers for the enclosing container;
                // anything else cannot start anything, so skip it.
                let structural = matches!(
                    kind,
                    TokenKind::Comma | TokenKind::Colon | TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace
                );
                let span = if structural {
                    ByteSpan::new(tok.span.start, tok.span.start)
                } else {
                    self.bump().span
                };
                Value { span, kind: ValueKind::Missing, prefix: vec![] }
            }
        };
        value.prefix = prefix;
        value
    }

    fn scalar(&mut self, kind: ValueKind) -> Value {
        let span = self.bump().span;
        Value { span, kind, prefix: vec![] }
    }

    /// An identifier in value position: a literal word, or a name that may
    /// own a parenthesized body.
    fn word(&mut self) -> Value {
        let tok = self.bump();
        let kind = match self.text(tok.span) {
            "true" => ValueKind::Bool(true),
            "false" => ValueKind::Bool(false),
            "inf" | "NaN" => ValueKind::Float,
            _ => {
                let name = ident(&tok, self.src);
                if self.peek() == Some(TokenKind::LParen) {
                    let body = self.container();
                    let span = container_span(&body, tok.span.start, self.last_end);
                    return Value { span, kind: ValueKind::Named { name, body: Some(body) }, prefix: vec![] };
                }
                ValueKind::Named { name, body: None }
            }
        };
        Value { span: tok.span, kind, prefix: vec![] }
    }

    fn container(&mut self) -> Container {
        let open_tok = self.bump();
        // Any newline before the first entry (or the closer), even one
        // after a comment that shares the bracket's line.
        let broken = {
            let rest = &self.tokens[self.pos..];
            let n = rest.iter().take_while(|t| t.kind.is_comment()).count();
            rest.iter().take(n + 1).any(|t| t.newlines_before > 0)
        };
        let (kind, closer) = match open_tok.kind {
            TokenKind::LBracket => (ContainerKind::List, TokenKind::RBracket),
            TokenKind::LBrace => (ContainerKind::Map, TokenKind::RBrace),
            _ => {
                // The first entry decides: `ident:` makes a struct.
                let is_struct = self.peek() == Some(TokenKind::Ident) && self.nth(1) == Some(TokenKind::Colon);
                (if is_struct { ContainerKind::Struct } else { ContainerKind::Tuple }, TokenKind::RParen)
            }
        };
        let mut c = Container { kind, open: open_tok.span, close: None, entries: Vec::new(), dangling: Vec::new(), broken };
        self.open.push(closer);
        loop {
            let Some(next) = self.peek() else {
                let message = format!("unclosed `{}`: expected `{}`", kind.open(), kind.close());
                self.syntax(message, c.open);
                break;
            };
            if next == closer {
                c.close = Some(self.bump().span);
                break;
            }
            if is_closer(next) {
                let span = self.current().span;
                if self.open.contains(&next) {
                    // It closes an outer container: this one ends here.
                    let message = format!("expected `{}`, found {}", kind.close(), self.found());
                    self.syntax(message, span);
                    break;
                }
                let message = format!("unexpected {}", self.found());
                self.syntax(message, span);
                self.bump();
                continue;
            }
            if next == TokenKind::Comma {
                let span = self.current().span;
                self.syntax("unexpected `,`: expected a value", span);
                self.bump();
                continue;
            }
            let before = self.consumed;
            let entry = self.entry(kind, closer);
            c.entries.push(entry);
            if self.consumed == before {
                // Nothing could be made of this token; it was reported.
                self.bump();
            }
        }
        self.open.pop();
        c.dangling = self.take_pending();
        self.check_duplicates(&c);
        c
    }

    fn entry(&mut self, kind: ContainerKind, closer: TokenKind) -> Entry {
        let leading = self.take_pending();
        let blank_before = self.current().newlines_before > 1;
        let key = match kind {
            ContainerKind::List => None,
            ContainerKind::Map => {
                let key = self.value();
                if self.peek() == Some(TokenKind::Colon) {
                    self.bump();
                } else {
                    let message = format!("expected `:` after the map key, found {}", self.found());
                    self.syntax(message, key.span);
                }
                Some(EntryKey::Map(Box::new(key)))
            }
            ContainerKind::Struct | ContainerKind::Tuple => {
                if self.peek() == Some(TokenKind::Ident) && self.nth(1) == Some(TokenKind::Colon) {
                    let tok = self.bump();
                    self.peek();
                    self.bump();
                    if kind == ContainerKind::Tuple {
                        self.error(
                            DiagnosticCode::MixedFields,
                            "named field among positional values; write every entry as `field: value` or none",
                            tok.span,
                        );
                    }
                    Some(EntryKey::Field(ident(&tok, self.src)))
                } else {
                    if kind == ContainerKind::Struct {
                        let span = self.current().span;
                        self.error(
                            DiagnosticCode::MixedFields,
                            "positional value among named fields; expected `field: value`",
                            span,
                        );
                    }
                    None
                }
            }
        };
        let value = self.value();
        let mut trailing = self.take_same_line();
        match self.peek() {
            Some(TokenKind::Comma) => {
                self.bump();
                trailing.extend(self.take_same_line());
            }
            Some(k) if k == closer || is_closer(k) => {}
            None => {}
            Some(_) => {
                let close = match closer {
                    TokenKind::RBracket => "]",
                    TokenKind::RBrace => "}",
                    _ => ")",
                };
                let message = format!("expected `,` or `{close}`, found {}", self.found());
                let span = self.current().span;
                self.syntax(message, span);
            }
        }
        Entry { key, value, leading, trailing, blank_before }
    }

    fn check_duplicates(&mut self, c: &Container) {
        let mut seen: HashSet<String> = HashSet::new();
        for entry in &c.entries {
            match &entry.key {
                Some(EntryKey::Field(id)) if c.kind == ContainerKind::Struct => {
                    if !seen.insert(id.name.clone()) {
                        let message = format!("duplicate field `{}`", id.name);
                        self.diags.push(Diagnostic::error(DiagnosticCode::DuplicateField, message, id.span));
                    }
                }
                Some(EntryKey::Map(key)) => {
                    let Some(canonical) = canonical_key(key, self.src) else { continue };
                    if !seen.insert(canonical) {
                        let message = format!("duplicate key `{}`; the last one wins", key.raw(self.src));
                        self.diags.push(Diagnostic::warning(DiagnosticCode::DuplicateKey, message, key.span));
                    }
                }
                _ => {}
            }
        }
    }
}

/// Comparable text for a scalar map key — strings by decoded value, so
/// `"a"` and `r"a"` collide. Compound keys are not compared.
fn canonical_key(key: &Value, source: &str) -> Option<String> {
    match &key.kind {
        ValueKind::String { decoded: Some(s) } => Some(format!("s:{s}")),
        ValueKind::String { decoded: None } | ValueKind::Missing => None,
        ValueKind::Container(_) | ValueKind::Named { body: Some(_), .. } => None,
        _ => Some(format!("r:{}", key.raw(source))),
    }
}

fn container_span(c: &Container, start: u32, last_end: u32) -> ByteSpan {
    let end = c.close.map_or(last_end.max(c.open.end), |s| s.end);
    ByteSpan::new(start, end)
}

fn is_closer(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace)
}

fn ident(tok: &Token, source: &str) -> Ident {
    let text = &source[tok.span.start as usize..tok.span.end as usize];
    Ident { span: tok.span, name: text.strip_prefix("r#").unwrap_or(text).to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn ok(src: &str) -> File {
        let (file, diags) = parse(src);
        assert!(diags.is_empty(), "{src:?}: {diags:?}");
        file
    }

    fn codes(src: &str) -> Vec<DiagnosticCode> {
        parse(src).1.iter().map(|d| d.code).collect()
    }

    fn messages(src: &str) -> Vec<String> {
        parse(src).1.into_iter().map(|d| d.message).collect()
    }

    fn root(src: &str) -> Value {
        ok(src).value.expect("value")
    }

    #[test]
    fn scalars() {
        assert_eq!(root("true").kind, ValueKind::Bool(true));
        assert_eq!(root("false").kind, ValueKind::Bool(false));
        assert_eq!(root("42").kind, ValueKind::Integer);
        assert_eq!(root("-1.5e3").kind, ValueKind::Float);
        assert_eq!(root("inf").kind, ValueKind::Float);
        assert_eq!(root("'x'").kind, ValueKind::Char);
        assert_eq!(root("b\"x\"").kind, ValueKind::ByteString);
        assert_eq!(root("\"hi\"").kind, ValueKind::String { decoded: Some("hi".into()) });
    }

    #[test]
    fn named_values() {
        let v = root("Point(x: 1, y: 2)");
        let name = v.name().unwrap();
        assert_eq!(name.name, "Point");
        let body = v.container().unwrap();
        assert_eq!(body.kind, ContainerKind::Struct);
        assert_eq!(body.entries.len(), 2);
        assert_eq!(v.span, ByteSpan::new(0, 17));

        let v = root("Some(5)");
        assert_eq!(v.container().unwrap().kind, ContainerKind::Tuple);
        assert_eq!(v.describe(), "option `Some`");
        assert_eq!(root("None").describe(), "option `None`");
        assert_eq!(root("Red").describe(), "unit struct or enum variant `Red`");
        assert_eq!(root("r#type").name().unwrap().name, "type");
    }

    #[test]
    fn anonymous_containers() {
        assert_eq!(root("()").describe(), "unit");
        assert_eq!(root("(1, \"a\")").describe(), "tuple");
        assert_eq!(root("(a: 1)").describe(), "struct");
        assert_eq!(root("[1, 2, 3,]").container().unwrap().entries.len(), 3);
        let map = root("{\"a\": 1, (1, 2): [3], Key: 4}");
        assert_eq!(map.container().unwrap().entries.len(), 3);
    }

    #[test]
    fn nested_document() {
        let src = r#"
#![enable(implicit_some)]
GameConfig( // the root
    window_size: (800, 600),
    window_title: "PAC-MAN",
    fullscreen: false,

    mouse_sensitivity: 1.4,
    key_bindings: {
        "up": Up,
        "down": Down,
    },
    difficulty_options: (
        start_difficulty: Easy,
        adaptive: false,
    ),
)
"#;
        let file = ok(src);
        assert_eq!(file.attributes.len(), 1);
        assert_eq!(file.attributes[0].name.as_ref().unwrap().name, "enable");
        assert_eq!(file.attributes[0].extensions[0].name, "implicit_some");
        let body = file.value.as_ref().unwrap().container().unwrap();
        assert!(body.broken);
        assert_eq!(body.entries.len(), 6);
        assert!(body.entries[3].blank_before);
        assert!(!body.entries[2].blank_before);
        assert_eq!(file.value_trailing.len(), 0);
        // `// the root` sits after `(`, so it leads the first field.
        assert_eq!(body.entries[0].leading.len(), 1);
        assert!(!body.entries[0].leading[0].own_line);
    }

    #[test]
    fn comment_attachment() {
        let src = "(\n    // lead\n    a: 1, // trail\n    b: /* pre */ 2,\n    // dangling\n)";
        let file = ok(src);
        let c = file.value.as_ref().unwrap().container().unwrap();
        assert_eq!(c.entries[0].leading.len(), 1);
        assert_eq!(c.entries[0].trailing.len(), 1);
        assert_eq!(c.entries[1].value.prefix.len(), 1);
        assert_eq!(c.dangling.len(), 1);
    }

    #[test]
    fn root_comments() {
        let file = ok("// head\n\n5 // tail\n// foot\n");
        assert_eq!(file.leading.len(), 1);
        assert!(file.value_blank_before);
        assert!(!ok("// head\n5").value_blank_before);
        assert_eq!(file.value_trailing.len(), 1);
        assert_eq!(file.trailing.len(), 1);
    }

    #[test]
    fn empty_and_comment_only_files_are_fine() {
        assert!(ok("").value.is_none());
        assert_eq!(ok("// nothing yet\n").leading.len(), 1);
    }

    #[test]
    fn missing_comma() {
        assert_eq!(messages("[1 2]"), vec!["expected `,` or `]`, found `2`"]);
        let (file, _) = parse("[1 2]");
        assert_eq!(file.value.unwrap().container().unwrap().entries.len(), 2);
    }

    #[test]
    fn missing_value() {
        assert_eq!(messages("(a: , b: 1)"), vec!["expected a value, found `,`"]);
        assert_eq!(messages("[1,"), vec!["unclosed `[`: expected `]`"]);
        assert_eq!(messages("(a:"), vec!["unclosed `(`: expected `)`", "expected a value, found end of file"]);
    }

    #[test]
    fn mismatched_closers() {
        // `)` closes nothing open: skipped.
        assert_eq!(messages("[1, )]"), vec!["unexpected `)`"]);
        // `)` closes the outer struct: the list ends early.
        assert_eq!(messages("(a: [1)"), vec!["expected `]`, found `)`"]);
    }

    #[test]
    fn stray_tokens_never_hang() {
        for src in ["(: : :)", "[@ @]", "{:}", "(a: 1 b: 2)", "#", "#!", "#![", "{1 2: 3}", ":::"] {
            let _ = parse(src);
        }
    }

    #[test]
    fn trailing_content() {
        assert_eq!(codes("1 2"), vec![DiagnosticCode::TrailingContent]);
        assert_eq!(codes("(a: 1))"), vec![DiagnosticCode::TrailingContent]);
    }

    #[test]
    fn mixed_fields() {
        assert_eq!(codes("(a: 1, 2)"), vec![DiagnosticCode::MixedFields]);
        assert_eq!(codes("(1, a: 2)"), vec![DiagnosticCode::MixedFields]);
    }

    #[test]
    fn duplicates() {
        assert_eq!(codes("(a: 1, a: 2)"), vec![DiagnosticCode::DuplicateField]);
        assert_eq!(codes("{\"a\": 1, r\"a\": 2}"), vec![DiagnosticCode::DuplicateKey]);
        assert_eq!(codes("{1: 1, 1: 2}"), vec![DiagnosticCode::DuplicateKey]);
        assert!(codes("{1: 1, 2: 2}").is_empty());
        let (_, diags) = parse("{\"a\": 1, \"a\": 2}");
        assert_eq!(diags[0].severity, crate::diagnostics::Severity::Warning);
    }

    #[test]
    fn attributes() {
        let file = ok("#![enable(implicit_some, unwrap_newtypes)] // exts\n#![enable(explicit_struct_names)]\n5");
        assert_eq!(file.attributes.len(), 2);
        assert_eq!(file.attributes[0].extensions.len(), 2);
        assert_eq!(file.attributes[0].trailing.len(), 1);
        assert_eq!(codes("#![enable(implicit_none)] 5"), vec![DiagnosticCode::UnknownExtension]);
        assert_eq!(codes("#[enable(implicit_some)] 5"), vec![DiagnosticCode::SyntaxError, DiagnosticCode::TrailingContent]);
        assert_eq!(messages("#![enable(implicit_some)"), vec!["unclosed attribute: expected `]`"]);
    }

    #[test]
    fn lexer_errors_surface() {
        assert_eq!(codes("\"open"), vec![DiagnosticCode::UnterminatedString]);
        assert_eq!(codes("[0b2]"), vec![DiagnosticCode::InvalidNumber]);
    }

    #[test]
    fn diagnostics_are_sorted() {
        let (_, diags) = parse("(a: 1, a: 2, 3 4)");
        let starts: Vec<u32> = diags.iter().map(|d| d.span.start).collect();
        let mut sorted = starts.clone();
        sorted.sort();
        assert_eq!(starts, sorted);
    }
}
