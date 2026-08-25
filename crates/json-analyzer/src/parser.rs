//! Recursive-descent parser over the tolerant lexer.
//!
//! Recovery philosophy: never give up on the file. A missing value
//! becomes a `Missing` node, junk is skipped to the next separator, and
//! every deviation is a diagnostic — so folding, symbols, and the JSONL
//! table keep working while the user is mid-edit.

use crate::ast::{
    Array, Ast, Comment, Element, Key, LineRecord, Member, Object, QuoteKind, Root, StringLit,
    Value, ValueKind,
};
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::flavor::Flavor;
use crate::lexer::{Lexer, Token, TokenKind};
use crate::spans::ByteSpan;
use std::collections::HashMap;

pub fn parse_document(source: &str, flavor: Flavor) -> (Ast, Vec<Diagnostic>) {
    if flavor == Flavor::Jsonl {
        let (records, diagnostics) = parse_lines(source);
        (Ast::Lines(records), diagnostics)
    } else {
        let (root, diagnostics) = parse_single(source, flavor);
        (Ast::Single(root), diagnostics)
    }
}

pub fn parse_single(source: &str, flavor: Flavor) -> (Root, Vec<Diagnostic>) {
    let (tokens, mut diagnostics) = Lexer::new(source, 0, flavor).tokens();
    let mut parser = Parser { source, flavor, tokens, index: 0, diagnostics: Vec::new() };
    let root = parser.parse_root();
    diagnostics.append(&mut parser.diagnostics);
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
    (root, diagnostics)
}

pub fn parse_lines(source: &str) -> (Vec<LineRecord>, Vec<Diagnostic>) {
    let mut records = Vec::new();
    let mut diagnostics = Vec::new();
    let mut offset = 0usize;
    for (line_idx, line) in source.split('\n').enumerate() {
        let start = offset;
        offset += line.len() + 1;
        if line.trim().is_empty() {
            continue;
        }
        let (tokens, mut lex_diags) = Lexer::new(line, start as u32, Flavor::Jsonl).tokens();
        diagnostics.append(&mut lex_diags);
        let mut parser =
            Parser { source, flavor: Flavor::Jsonl, tokens, index: 0, diagnostics: Vec::new() };
        parser.collect_comments();
        let value = parser.parse_value();
        parser.collect_comments();
        if !matches!(parser.peek().kind, TokenKind::Eof) {
            let extra = parser.peek().span;
            let end = parser.last_span().end.max(extra.end);
            parser.diagnostics.push(Diagnostic::error(
                DiagnosticCode::MultipleTopLevelValues,
                "a JSON Lines record is a single value on one line",
                ByteSpan::new(extra.start, end),
            ));
        }
        diagnostics.append(&mut parser.diagnostics);
        if !matches!(value.kind, ValueKind::Missing) {
            records.push(LineRecord { line: line_idx as u32, value });
        }
    }
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
    (records, diagnostics)
}

struct Parser<'a> {
    source: &'a str,
    flavor: Flavor,
    tokens: Vec<Token>,
    index: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Token {
        &self.tokens[self.index.min(self.tokens.len() - 1)]
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.index.min(self.tokens.len() - 1)].clone();
        if self.index < self.tokens.len() - 1 {
            self.index += 1;
        }
        token
    }

    /// Span of the token *behind* the cursor (or an empty span at 0).
    fn last_span(&self) -> ByteSpan {
        if self.index == 0 {
            ByteSpan::EMPTY
        } else {
            self.tokens[self.index - 1].span
        }
    }

    fn raw(&self, span: ByteSpan) -> &'a str {
        &self.source[span.start as usize..span.end as usize]
    }

    fn error(&mut self, code: DiagnosticCode, message: impl Into<String>, span: ByteSpan) {
        self.diagnostics.push(Diagnostic::error(code, message, span));
    }

    /// True when no line break separates the two offsets.
    fn same_line(&self, from: u32, to: u32) -> bool {
        let (from, to) = (from.min(to), from.max(to));
        !self.source[from as usize..to as usize].contains('\n')
    }

    fn collect_comments(&mut self) -> Vec<Comment> {
        let mut comments = Vec::new();
        while let TokenKind::Comment(kind) = self.peek().kind {
            let span = self.peek().span;
            comments.push(Comment { span, kind });
            self.bump();
        }
        comments
    }

    fn parse_root(&mut self) -> Root {
        let leading = self.collect_comments();
        if matches!(self.peek().kind, TokenKind::Eof) {
            return Root { leading, value: None, trailing: Vec::new() };
        }
        let value = self.parse_value();
        let trailing = self.collect_comments();
        if !matches!(self.peek().kind, TokenKind::Eof) {
            let start = self.peek().span;
            let mut end = start;
            while !matches!(self.peek().kind, TokenKind::Eof) {
                end = self.bump().span;
            }
            self.error(
                DiagnosticCode::MultipleTopLevelValues,
                "a document holds a single top-level value",
                ByteSpan::new(start.start, end.end),
            );
        }
        Root { leading, value: Some(value), trailing }
    }

    fn parse_value(&mut self) -> Value {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::LBrace => self.parse_object(),
            TokenKind::LBracket => self.parse_array(),
            TokenKind::String { value, quote } => {
                self.bump();
                Value { span: token.span, kind: ValueKind::String(StringLit { value, quote }) }
            }
            TokenKind::Number => {
                self.bump();
                Value { span: token.span, kind: ValueKind::Number }
            }
            TokenKind::Ident => {
                self.bump();
                let raw = self.raw(token.span);
                let kind = match raw {
                    "true" => ValueKind::Bool(true),
                    "false" => ValueKind::Bool(false),
                    "null" => ValueKind::Null,
                    "Infinity" | "NaN" => {
                        if !self.flavor.allows_json5_syntax() {
                            self.error(
                                DiagnosticCode::NonStandardNumber,
                                format!("`{raw}` is only a valid number in JSON5"),
                                token.span,
                            );
                        }
                        ValueKind::Number
                    }
                    other => {
                        self.error(
                            DiagnosticCode::SyntaxError,
                            format!("unexpected `{other}`"),
                            token.span,
                        );
                        ValueKind::Missing
                    }
                };
                Value { span: token.span, kind }
            }
            TokenKind::Error => {
                // The lexer already complained.
                self.bump();
                Value { span: token.span, kind: ValueKind::Missing }
            }
            TokenKind::Comment(_) => {
                // Callers collect comments first; a straggler ends up as
                // a hole rather than a crash.
                self.bump();
                Value { span: token.span, kind: ValueKind::Missing }
            }
            TokenKind::RBrace
            | TokenKind::RBracket
            | TokenKind::Colon
            | TokenKind::Comma
            | TokenKind::Eof => {
                self.error(
                    DiagnosticCode::SyntaxError,
                    "expected a value",
                    ByteSpan::new(token.span.start, token.span.start),
                );
                Value {
                    span: ByteSpan::new(token.span.start, token.span.start),
                    kind: ValueKind::Missing,
                }
            }
        }
    }

    fn flag_trailing_comma(&mut self, comma: ByteSpan) {
        if !self.flavor.allows_trailing_commas() {
            let noun = match self.flavor {
                Flavor::Json => "JSON",
                Flavor::Jsonl => "JSON Lines",
                _ => "this flavor",
            };
            self.error(
                DiagnosticCode::TrailingCommaNotAllowed,
                format!("trailing commas are not allowed in {noun}"),
                comma,
            );
        }
    }

    /// Consume tokens up to (not including) the next separator that can
    /// resynchronize an object/array body.
    fn skip_to_separator(&mut self, closer: &TokenKind) {
        loop {
            let kind = &self.peek().kind;
            if kind == closer || matches!(kind, TokenKind::Comma | TokenKind::Eof) {
                break;
            }
            // Skip a whole nested container, not just its opener.
            match self.peek().kind {
                TokenKind::LBrace => {
                    self.parse_object();
                }
                TokenKind::LBracket => {
                    self.parse_array();
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn parse_object(&mut self) -> Value {
        let open = self.bump().span; // '{'
        let mut members: Vec<Member> = Vec::new();
        let dangling;
        let mut last_comma: Option<ByteSpan> = None;
        let mut carry: Vec<Comment> = Vec::new();
        let close = loop {
            let progress = self.index;
            let mut pending = std::mem::take(&mut carry);
            pending.extend(self.collect_comments());
            match self.peek().kind {
                TokenKind::RBrace => {
                    if let Some(comma) = last_comma {
                        self.flag_trailing_comma(comma);
                    }
                    dangling = pending;
                    break self.bump().span;
                }
                TokenKind::Eof => {
                    self.error(DiagnosticCode::SyntaxError, "object is never closed", open);
                    dangling = pending;
                    break self.last_span();
                }
                TokenKind::Comma => {
                    let span = self.bump().span;
                    self.error(DiagnosticCode::SyntaxError, "unexpected `,`", span);
                    last_comma = Some(span);
                    continue;
                }
                _ => {}
            }
            let Some(key) = self.parse_key() else {
                continue;
            };
            last_comma = None;
            pending.extend(self.collect_comments());
            if matches!(self.peek().kind, TokenKind::Colon) {
                self.bump();
            } else {
                let at = self.peek().span;
                self.error(
                    DiagnosticCode::SyntaxError,
                    format!("expected `:` after key `{}`", key.name),
                    ByteSpan::new(key.span.end, at.start.max(key.span.end)),
                );
            }
            pending.extend(self.collect_comments());
            let value = self.parse_value();
            let mut member = Member { key, value, leading: pending, trailing: None };
            self.attach_trailing(&mut member.trailing, member.value.span.end);
            // Comments on the lines before the separator belong to the
            // *next* entry (or become dangling at the close).
            carry = self.collect_comments();
            match self.peek().kind {
                TokenKind::Comma => {
                    let comma = self.bump().span;
                    last_comma = Some(comma);
                    if carry.is_empty() {
                        self.attach_trailing(&mut member.trailing, comma.end);
                    }
                }
                TokenKind::RBrace | TokenKind::Eof => {}
                _ => {
                    let at = self.peek().span;
                    self.error(DiagnosticCode::SyntaxError, "expected `,` or `}`", at);
                    self.skip_to_separator(&TokenKind::RBrace);
                }
            }
            members.push(member);
            if self.index == progress {
                // Nothing consumed this round: force progress.
                self.bump();
            }
        };
        self.check_duplicate_keys(&members);
        Value {
            span: ByteSpan::new(open.start, close.end.max(open.end)),
            kind: ValueKind::Object(Object { members, dangling }),
        }
    }

    fn parse_key(&mut self) -> Option<Key> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::String { value, quote } => {
                self.bump();
                Some(Key { span: token.span, name: value, quote })
            }
            TokenKind::Ident => {
                self.bump();
                if !self.flavor.allows_json5_syntax() {
                    self.error(
                        DiagnosticCode::UnquotedKeyNotAllowed,
                        "object keys must be double-quoted outside JSON5",
                        token.span,
                    );
                }
                Some(Key {
                    span: token.span,
                    name: self.raw(token.span).to_string(),
                    quote: QuoteKind::Bare,
                })
            }
            TokenKind::Number => {
                self.bump();
                self.error(
                    DiagnosticCode::SyntaxError,
                    "expected a property name",
                    token.span,
                );
                Some(Key {
                    span: token.span,
                    name: self.raw(token.span).to_string(),
                    quote: QuoteKind::Bare,
                })
            }
            _ => {
                self.error(
                    DiagnosticCode::SyntaxError,
                    "expected a property name",
                    token.span,
                );
                self.skip_to_separator(&TokenKind::RBrace);
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.bump();
                }
                None
            }
        }
    }

    /// Attach a same-line comment following `after` as the entry's
    /// trailing comment, unless one is already set.
    fn attach_trailing(&mut self, slot: &mut Option<Comment>, after: u32) {
        if slot.is_some() {
            return;
        }
        if let TokenKind::Comment(kind) = self.peek().kind {
            let span = self.peek().span;
            if self.same_line(after, span.start) {
                *slot = Some(Comment { span, kind });
                self.bump();
            }
        }
    }

    fn check_duplicate_keys(&mut self, members: &[Member]) {
        let mut seen: HashMap<&str, ()> = HashMap::with_capacity(members.len());
        for member in members {
            if seen.insert(member.key.name.as_str(), ()).is_some() {
                self.diagnostics.push(Diagnostic::warning(
                    DiagnosticCode::DuplicateKey,
                    format!("duplicate key `{}`", member.key.name),
                    member.key.span,
                ));
            }
        }
    }

    fn parse_array(&mut self) -> Value {
        let open = self.bump().span; // '['
        let mut elements: Vec<Element> = Vec::new();
        let dangling;
        let mut last_comma: Option<ByteSpan> = None;
        let mut carry: Vec<Comment> = Vec::new();
        let close = loop {
            let progress = self.index;
            let mut pending = std::mem::take(&mut carry);
            pending.extend(self.collect_comments());
            match self.peek().kind {
                TokenKind::RBracket => {
                    if let Some(comma) = last_comma {
                        self.flag_trailing_comma(comma);
                    }
                    dangling = pending;
                    break self.bump().span;
                }
                TokenKind::Eof => {
                    self.error(DiagnosticCode::SyntaxError, "array is never closed", open);
                    dangling = pending;
                    break self.last_span();
                }
                TokenKind::Comma => {
                    let span = self.bump().span;
                    self.error(DiagnosticCode::SyntaxError, "unexpected `,`", span);
                    last_comma = Some(span);
                    continue;
                }
                _ => {}
            }
            last_comma = None;
            let value = self.parse_value();
            let recovered = matches!(value.kind, ValueKind::Missing);
            let mut element = Element { value, leading: pending, trailing: None };
            self.attach_trailing(&mut element.trailing, element.value.span.end);
            carry = self.collect_comments();
            match self.peek().kind {
                TokenKind::Comma => {
                    let comma = self.bump().span;
                    last_comma = Some(comma);
                    if carry.is_empty() {
                        self.attach_trailing(&mut element.trailing, comma.end);
                    }
                }
                TokenKind::RBracket | TokenKind::Eof => {}
                _ => {
                    let at = self.peek().span;
                    self.error(DiagnosticCode::SyntaxError, "expected `,` or `]`", at);
                    self.skip_to_separator(&TokenKind::RBracket);
                }
            }
            if !recovered {
                elements.push(element);
            }
            if self.index == progress {
                self.bump();
            }
        };
        Value {
            span: ByteSpan::new(open.start, close.end.max(open.end)),
            kind: ValueKind::Array(Array { elements, dangling }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn single(src: &str, flavor: Flavor) -> (Root, Vec<Diagnostic>) {
        parse_single(src, flavor)
    }

    fn ok_value(src: &str, flavor: Flavor) -> Value {
        let (root, diags) = single(src, flavor);
        assert!(diags.is_empty(), "unexpected diagnostics for {src:?}: {diags:?}");
        root.value.expect("a value")
    }

    fn object(value: &Value) -> &Object {
        match &value.kind {
            ValueKind::Object(o) => o,
            other => panic!("expected object, got {other:?}"),
        }
    }

    fn array(value: &Value) -> &Array {
        match &value.kind {
            ValueKind::Array(a) => a,
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_structure() {
        let v = ok_value(r#"{"a": [1, {"b": true}], "c": null}"#, Flavor::Json);
        let o = object(&v);
        assert_eq!(o.members.len(), 2);
        assert_eq!(o.members[0].key.name, "a");
        let inner = array(&o.members[0].value);
        assert_eq!(inner.elements.len(), 2);
        assert_eq!(o.members[1].value.kind, ValueKind::Null);
    }

    #[test]
    fn root_value_span_covers_whole_container() {
        let src = r#"{"a": 1}"#;
        let v = ok_value(src, Flavor::Json);
        assert_eq!(v.span, ByteSpan::new(0, src.len() as u32));
    }

    #[test]
    fn empty_file_has_no_value_and_no_errors() {
        let (root, diags) = single("  \n ", Flavor::Json);
        assert!(root.value.is_none());
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn comments_attach_leading_and_trailing() {
        let src = "{\n  // about a\n  \"a\": 1, // trailing\n  \"b\": 2\n}";
        let v = ok_value(src, Flavor::Jsonc);
        let o = object(&v);
        assert_eq!(o.members[0].leading.len(), 1);
        assert!(o.members[0].trailing.is_some());
        assert!(o.members[1].leading.is_empty());
        assert!(o.members[1].trailing.is_none());
    }

    #[test]
    fn comment_before_close_is_dangling() {
        let src = "{\n  \"a\": 1\n  // last words\n}";
        let v = ok_value(src, Flavor::Jsonc);
        let o = object(&v);
        assert!(o.members[0].trailing.is_none());
        assert_eq!(o.dangling.len(), 1);
    }

    #[test]
    fn trailing_comma_flagged_in_strict_json_only() {
        let (_, diags) = single(r#"{"a": 1,}"#, Flavor::Json);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::TrailingCommaNotAllowed));
        let (_, diags) = single(r#"{"a": 1,}"#, Flavor::Jsonc);
        assert!(diags.is_empty(), "{diags:?}");
        let (_, diags) = single("[1, 2,]", Flavor::Json);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::TrailingCommaNotAllowed));
    }

    #[test]
    fn unquoted_keys_flagged_outside_json5() {
        let (_, diags) = single("{key: 1}", Flavor::Jsonc);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnquotedKeyNotAllowed));
        let (root, diags) = single("{key: 1}", Flavor::Json5);
        assert!(diags.is_empty(), "{diags:?}");
        let o = object(root.value.as_ref().unwrap());
        assert_eq!(o.members[0].key.name, "key");
        assert_eq!(o.members[0].key.quote, QuoteKind::Bare);
    }

    #[test]
    fn duplicate_keys_warn() {
        let (_, diags) = single(r#"{"a": 1, "a": 2}"#, Flavor::Json);
        let dup: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::DuplicateKey)
            .collect();
        assert_eq!(dup.len(), 1);
        assert_eq!(dup[0].span, ByteSpan::new(9, 12));
    }

    #[test]
    fn multiple_top_level_values_flagged() {
        let (_, diags) = single("{} []", Flavor::Json);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::MultipleTopLevelValues));
    }

    #[test]
    fn missing_value_recovers() {
        let (root, diags) = single(r#"{"a": , "b": 2}"#, Flavor::Json);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::SyntaxError));
        let o = object(root.value.as_ref().unwrap());
        assert_eq!(o.members.len(), 2);
        assert_eq!(o.members[0].value.kind, ValueKind::Missing);
        assert_eq!(o.members[1].key.name, "b");
    }

    #[test]
    fn missing_colon_recovers() {
        let (root, diags) = single(r#"{"a" 1, "b": 2}"#, Flavor::Json);
        assert!(!diags.is_empty());
        let o = object(root.value.as_ref().unwrap());
        assert_eq!(o.members.len(), 2);
    }

    #[test]
    fn unclosed_object_recovers() {
        let (root, diags) = single("{\"a\": 1", Flavor::Json);
        assert!(diags.iter().any(|d| {
            d.code == DiagnosticCode::SyntaxError && d.message.contains("never closed")
        }));
        let o = object(root.value.as_ref().unwrap());
        assert_eq!(o.members.len(), 1);
    }

    #[test]
    fn infinity_is_number_with_flavor_gate() {
        let (root, diags) = single("[Infinity, NaN]", Flavor::Json5);
        assert!(diags.is_empty(), "{diags:?}");
        let a = array(root.value.as_ref().unwrap());
        assert_eq!(a.elements[0].value.kind, ValueKind::Number);
        let (_, diags) = single("[Infinity]", Flavor::Json);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::NonStandardNumber));
    }

    #[test]
    fn never_loops_on_garbage() {
        for src in ["{]", "[}", "{:}", "[:,]", "{\"a\" \"b\" \"c\"}", "}{", "]][[", "{,,,}"] {
            for flavor in [Flavor::Json, Flavor::Jsonc, Flavor::Json5] {
                let _ = parse_single(src, flavor);
            }
        }
    }

    #[test]
    fn jsonl_parses_records_with_absolute_lines_and_spans() {
        let src = "{\"a\": 1}\n\n{\"a\": 2}\n";
        let (records, diags) = parse_lines(src);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].line, 0);
        assert_eq!(records[1].line, 2);
        assert_eq!(records[1].value.span, ByteSpan::new(10, 18));
    }

    #[test]
    fn jsonl_flags_two_values_on_a_line() {
        let (_, diags) = parse_lines("1 2\n");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::MultipleTopLevelValues));
    }

    #[test]
    fn jsonl_flags_comments_and_bad_lines_but_keeps_good_ones() {
        let src = "{\"a\": 1}\n// nope\nnot json\n{\"b\": 2}\n";
        let (records, diags) = parse_lines(src);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::CommentNotAllowed));
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::SyntaxError));
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].line, 3);
    }

    #[test]
    fn json5_kitchen_sink_parses_clean() {
        let src = r#"{
  // comment
  unquoted: 'single',
  hex: 0xDEADbeef,
  half: .5,
  to: Infinity,
  trailing: [1, 2,],
}"#;
        let (root, diags) = single(src, Flavor::Json5);
        assert!(diags.is_empty(), "{diags:?}");
        let o = object(root.value.as_ref().unwrap());
        assert_eq!(o.members.len(), 5);
    }
}
