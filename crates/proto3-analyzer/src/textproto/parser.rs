//! Recursive-descent parser for Protobuf text format.
//!
//! The parser is intentionally lenient: it always produces a [`File`] and
//! collects any errors into a side-channel [`Vec<ProtoDiagnostic>`]. Error
//! recovery skips to the next `,`, `;`, `}`, `>`, `]`, or EOF.

use super::ast::*;
use super::lexer::{LexErrorKind, LexResult, Token, TokenKind};
use crate::diagnostics::{DiagnosticCode, ProtoDiagnostic, Severity};
use crate::spans::ByteSpan;
use smol_str::SmolStr;

pub struct Parser<'s> {
    _src: std::marker::PhantomData<&'s str>,
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<ProtoDiagnostic>,
}

pub struct ParseOutcome {
    pub file: File,
    pub diagnostics: Vec<ProtoDiagnostic>,
}

pub fn parse_file(source: &str, lex: LexResult) -> ParseOutcome {
    let mut p = Parser::new(lex.tokens);
    let _ = source;
    for t in &p.tokens {
        if let TokenKind::LexError(e) = &t.kind {
            p.diagnostics.push(lex_error_to_diag(e, t.span));
        }
    }
    let fields = p.parse_fields_until(&[TokenKind::Eof]);
    let span = ByteSpan::new(0, source.len() as u32);
    ParseOutcome {
        file: File { header: HeaderHints::default(), fields, span },
        diagnostics: p.diagnostics,
    }
}

impl<'s> Parser<'s> {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { _src: std::marker::PhantomData, tokens, pos: 0, diagnostics: Vec::new() }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_at(&self, n: usize) -> &Token {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i]
    }

    fn bump(&mut self) -> Token {
        let tok = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if !matches!(tok.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        tok
    }

    fn eat_kind(&mut self, kind: &TokenKind) -> Option<Token> {
        if std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn error(&mut self, code: DiagnosticCode, msg: impl Into<String>, span: ByteSpan) {
        self.diagnostics.push(ProtoDiagnostic::new(code, Severity::Error, msg.into(), span));
    }

    fn parse_fields_until(&mut self, stops: &[TokenKind]) -> Vec<Field> {
        let mut out = Vec::new();
        loop {
            // Consume stray separators between fields; they are optional per spec.
            while matches!(self.peek().kind, TokenKind::Comma | TokenKind::Semi) {
                self.bump();
            }
            if stop_here(&self.peek().kind, stops) {
                break;
            }
            if matches!(self.peek().kind, TokenKind::Eof) {
                break;
            }
            match self.parse_field() {
                Some(f) => out.push(f),
                None => {
                    // Recovery: advance past any sync token.
                    self.recover_to_field_boundary(stops);
                }
            }
        }
        out
    }

    fn recover_to_field_boundary(&mut self, stops: &[TokenKind]) {
        loop {
            let k = &self.peek().kind.clone();
            if matches!(k, TokenKind::Eof) || stop_here(k, stops) {
                return;
            }
            if matches!(k, TokenKind::Comma | TokenKind::Semi) {
                self.bump();
                return;
            }
            self.bump();
        }
    }

    fn parse_field(&mut self) -> Option<Field> {
        let start_tok = self.peek().clone();
        let name = self.parse_field_name()?;
        let name_end_span = name.span();

        let has_colon = matches!(self.peek().kind, TokenKind::Colon);
        if has_colon {
            self.bump();
        }

        let value = match &self.peek().kind {
            TokenKind::LBrace | TokenKind::LAngle => self.parse_message_value()?,
            TokenKind::LBracket => self.parse_list_value()?,
            _ => {
                if !has_colon {
                    self.error(
                        DiagnosticCode::TextprotoParseError,
                        format!(
                            "Expected ':' between field '{}' and its value",
                            name.display_name()
                        ),
                        name_end_span,
                    );
                }
                self.parse_scalar_value()?
            }
        };

        // Consume optional trailing ',' or ';'.
        let trailing_end = match &self.peek().kind {
            TokenKind::Comma | TokenKind::Semi => {
                let t = self.bump();
                t.span.end
            }
            _ => value.span().end,
        };

        let span = ByteSpan::new(start_tok.span.start, trailing_end);
        Some(Field { name, has_colon, value, span })
    }

    fn parse_field_name(&mut self) -> Option<FieldName> {
        match &self.peek().kind {
            TokenKind::Ident(_) => {
                let tok = self.bump();
                if let TokenKind::Ident(s) = tok.kind {
                    Some(FieldName::Ident(Ident { name: s, span: tok.span }))
                } else {
                    unreachable!()
                }
            }
            TokenKind::LBracket => self.parse_bracketed_field_name(),
            _ => {
                let span = self.peek().span;
                self.error(
                    DiagnosticCode::TextprotoParseError,
                    "Expected a field name",
                    span,
                );
                None
            }
        }
    }

    fn parse_bracketed_field_name(&mut self) -> Option<FieldName> {
        let lbracket = self.bump();
        let qn = self.parse_qualified_name()?;
        // If the next token is `/`, it's an Any type URL: `[host/pkg.Type]`.
        if matches!(self.peek().kind, TokenKind::Slash) {
            self.bump();
            let type_name = self.parse_qualified_name()?;
            let rbracket = match self.eat_kind(&TokenKind::RBracket) {
                Some(t) => t,
                None => {
                    self.error(
                        DiagnosticCode::TextprotoParseError,
                        "Expected ']' closing Any type URL",
                        self.peek().span,
                    );
                    return None;
                }
            };
            let full_span = ByteSpan::new(lbracket.span.start, rbracket.span.end);
            let url = AnyTypeUrl {
                host: qn.to_display(),
                host_span: qn.span,
                type_name,
                span: full_span,
            };
            return Some(FieldName::Any { url, span: full_span });
        }
        let rbracket = match self.eat_kind(&TokenKind::RBracket) {
            Some(t) => t,
            None => {
                self.error(
                    DiagnosticCode::TextprotoParseError,
                    "Expected ']' closing extension field name",
                    self.peek().span,
                );
                return None;
            }
        };
        let full_span = ByteSpan::new(lbracket.span.start, rbracket.span.end);
        Some(FieldName::Extension { name: qn, span: full_span })
    }

    fn parse_qualified_name(&mut self) -> Option<QualifiedName> {
        let first = match &self.peek().kind {
            TokenKind::Ident(_) => self.bump(),
            _ => {
                self.error(
                    DiagnosticCode::TextprotoParseError,
                    "Expected an identifier",
                    self.peek().span,
                );
                return None;
            }
        };
        let name = match first.kind {
            TokenKind::Ident(s) => Ident { name: s, span: first.span },
            _ => unreachable!(),
        };
        let mut parts = vec![name];
        let mut end = first.span.end;
        while matches!(self.peek().kind, TokenKind::Dot) {
            // Peek one more to make sure an identifier follows the dot.
            if !matches!(self.peek_at(1).kind, TokenKind::Ident(_)) {
                break;
            }
            self.bump();
            let ident_tok = self.bump();
            match ident_tok.kind {
                TokenKind::Ident(s) => {
                    end = ident_tok.span.end;
                    parts.push(Ident { name: s, span: ident_tok.span });
                }
                _ => unreachable!(),
            }
        }
        let span = ByteSpan::new(parts[0].span.start, end);
        Some(QualifiedName { parts, span })
    }

    fn parse_message_value(&mut self) -> Option<Value> {
        let opener = self.bump();
        let (is_angle, close_kind) = match opener.kind {
            TokenKind::LBrace => (false, TokenKind::RBrace),
            TokenKind::LAngle => (true, TokenKind::RAngle),
            _ => unreachable!(),
        };
        let fields = self.parse_fields_until(&[close_kind.clone(), TokenKind::Eof]);
        let closer = match self.eat_kind(&close_kind) {
            Some(t) => t,
            None => {
                self.error(
                    DiagnosticCode::TextprotoParseError,
                    if is_angle { "Expected '>' closing message" } else { "Expected '}' closing message" },
                    self.peek().span,
                );
                return Some(Value::Message { fields, is_angle, span: opener.span });
            }
        };
        let span = ByteSpan::new(opener.span.start, closer.span.end);
        Some(Value::Message { fields, is_angle, span })
    }

    fn parse_list_value(&mut self) -> Option<Value> {
        let lbracket = self.bump();
        let mut elements = Vec::new();
        // Handle empty list.
        if matches!(self.peek().kind, TokenKind::RBracket) {
            let rbracket = self.bump();
            return Some(Value::List {
                elements,
                span: ByteSpan::new(lbracket.span.start, rbracket.span.end),
            });
        }
        loop {
            let v = match &self.peek().kind {
                TokenKind::LBrace | TokenKind::LAngle => self.parse_message_value(),
                _ => self.parse_scalar_value(),
            };
            if let Some(v) = v {
                elements.push(v);
            } else {
                // Recovery: skip to next `,` or `]` or EOF.
                while !matches!(
                    self.peek().kind,
                    TokenKind::Comma | TokenKind::RBracket | TokenKind::Eof
                ) {
                    self.bump();
                }
            }
            match &self.peek().kind {
                TokenKind::Comma => {
                    self.bump();
                }
                TokenKind::RBracket => break,
                TokenKind::Eof => break,
                _ => break,
            }
        }
        let rbracket = match self.eat_kind(&TokenKind::RBracket) {
            Some(t) => t,
            None => {
                self.error(
                    DiagnosticCode::TextprotoParseError,
                    "Expected ']' closing list value",
                    self.peek().span,
                );
                return Some(Value::List {
                    elements,
                    span: ByteSpan::new(lbracket.span.start, self.peek().span.end),
                });
            }
        };
        Some(Value::List {
            elements,
            span: ByteSpan::new(lbracket.span.start, rbracket.span.end),
        })
    }

    fn parse_scalar_value(&mut self) -> Option<Value> {
        match &self.peek().kind {
            TokenKind::StringLit(_) => Some(self.parse_string_chain()),
            TokenKind::IntLit(_) => {
                let tok = self.bump();
                match tok.kind {
                    TokenKind::IntLit(raw) => Some(Value::Integer { raw, negative: false, span: tok.span }),
                    _ => unreachable!(),
                }
            }
            TokenKind::FloatLit(_) => {
                let tok = self.bump();
                match tok.kind {
                    TokenKind::FloatLit(raw) => Some(Value::Float { raw, negative: false, span: tok.span }),
                    _ => unreachable!(),
                }
            }
            TokenKind::Minus => {
                let minus = self.bump();
                match &self.peek().kind {
                    TokenKind::IntLit(_) => {
                        let tok = self.bump();
                        let raw = match tok.kind {
                            TokenKind::IntLit(r) => r,
                            _ => unreachable!(),
                        };
                        Some(Value::Integer {
                            raw,
                            negative: true,
                            span: ByteSpan::new(minus.span.start, tok.span.end),
                        })
                    }
                    TokenKind::FloatLit(_) => {
                        let tok = self.bump();
                        let raw = match tok.kind {
                            TokenKind::FloatLit(r) => r,
                            _ => unreachable!(),
                        };
                        Some(Value::Float {
                            raw,
                            negative: true,
                            span: ByteSpan::new(minus.span.start, tok.span.end),
                        })
                    }
                    TokenKind::Ident(_) => {
                        let tok = self.bump();
                        let ident = match tok.kind {
                            TokenKind::Ident(s) => Ident { name: s, span: tok.span },
                            _ => unreachable!(),
                        };
                        Some(Value::SignedIdent {
                            ident,
                            span: ByteSpan::new(minus.span.start, tok.span.end),
                        })
                    }
                    _ => {
                        self.error(
                            DiagnosticCode::TextprotoParseError,
                            "Expected a number or identifier after '-'",
                            minus.span,
                        );
                        Some(Value::Missing(minus.span))
                    }
                }
            }
            TokenKind::Plus => {
                let plus = self.bump();
                match &self.peek().kind {
                    TokenKind::IntLit(_) => {
                        let tok = self.bump();
                        let raw = match tok.kind {
                            TokenKind::IntLit(r) => r,
                            _ => unreachable!(),
                        };
                        Some(Value::Integer {
                            raw,
                            negative: false,
                            span: ByteSpan::new(plus.span.start, tok.span.end),
                        })
                    }
                    TokenKind::FloatLit(_) => {
                        let tok = self.bump();
                        let raw = match tok.kind {
                            TokenKind::FloatLit(r) => r,
                            _ => unreachable!(),
                        };
                        Some(Value::Float {
                            raw,
                            negative: false,
                            span: ByteSpan::new(plus.span.start, tok.span.end),
                        })
                    }
                    _ => {
                        self.error(
                            DiagnosticCode::TextprotoParseError,
                            "Expected a number after '+'",
                            plus.span,
                        );
                        Some(Value::Missing(plus.span))
                    }
                }
            }
            TokenKind::Ident(_) => {
                let tok = self.bump();
                match tok.kind {
                    TokenKind::Ident(s) => Some(Value::Ident(Ident { name: s, span: tok.span })),
                    _ => unreachable!(),
                }
            }
            _ => {
                let span = self.peek().span;
                self.error(
                    DiagnosticCode::TextprotoParseError,
                    format!("Unexpected token — expected a value"),
                    span,
                );
                None
            }
        }
    }

    fn parse_string_chain(&mut self) -> Value {
        // Concatenate adjacent string literals per spec.
        let first = self.bump();
        let start = first.span.start;
        let mut end = first.span.end;
        let mut buf = match first.kind {
            TokenKind::StringLit(s) => s,
            _ => unreachable!(),
        };
        while matches!(self.peek().kind, TokenKind::StringLit(_)) {
            let tok = self.bump();
            end = tok.span.end;
            if let TokenKind::StringLit(s) = tok.kind {
                buf.push_str(&s);
            }
        }
        Value::String { value: buf, span: ByteSpan::new(start, end) }
    }
}

fn stop_here(k: &TokenKind, stops: &[TokenKind]) -> bool {
    stops.iter().any(|s| std::mem::discriminant(s) == std::mem::discriminant(k))
}

fn lex_error_to_diag(e: &LexErrorKind, span: ByteSpan) -> ProtoDiagnostic {
    match e {
        LexErrorKind::UnterminatedString => ProtoDiagnostic::new(
            DiagnosticCode::LexUnterminatedString,
            Severity::Error,
            "Unterminated string literal",
            span,
        ),
        LexErrorKind::InvalidEscape(ch) => ProtoDiagnostic::new(
            DiagnosticCode::LexInvalidEscape,
            Severity::Error,
            format!("Invalid escape sequence `\\{}`", ch),
            span,
        ),
        LexErrorKind::StrayChar(ch) => ProtoDiagnostic::new(
            DiagnosticCode::ParseUnexpectedToken,
            Severity::Error,
            format!("Unexpected character `{}`", ch),
            span,
        ),
        LexErrorKind::InvalidNumber(raw) => ProtoDiagnostic::new(
            DiagnosticCode::ParseUnexpectedToken,
            Severity::Error,
            format!("Invalid numeric literal `{}`", raw),
            span,
        ),
    }
}

// `SmolStr` is referenced by other textproto modules via the AST; suppress
// the "unused import" lint when building this file standalone.
#[allow(dead_code)]
fn _smolstr_touch(_: SmolStr) {}

#[cfg(test)]
mod tests {
    use super::super::lexer::lex;
    use super::super::parse::parse;
    use super::*;
    use crate::vfs::FileUri;

    fn parse_src(src: &str) -> (File, Vec<ProtoDiagnostic>) {
        let uri = FileUri::new("mem://test.textproto");
        let pt = parse(uri, src.to_string());
        (pt.ast, pt.diagnostics)
    }

    #[test]
    fn parses_simple_fields() {
        let (file, diags) = parse_src("name: \"Alice\"\nage: 30\n");
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
        assert_eq!(file.fields.len(), 2);
        assert!(matches!(&file.fields[0].name, FieldName::Ident(i) if i.name == "name"));
        assert!(matches!(&file.fields[0].value, Value::String { value, .. } if value == "Alice"));
        assert!(matches!(&file.fields[1].value, Value::Integer { .. }));
    }

    #[test]
    fn parses_message_with_braces_and_angles() {
        let (file, diags) = parse_src("pet { kind: DOG } pet < legs: 4 >");
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
        assert_eq!(file.fields.len(), 2);
        match &file.fields[0].value {
            Value::Message { is_angle: false, fields, .. } => assert_eq!(fields.len(), 1),
            other => panic!("{:?}", other),
        }
        match &file.fields[1].value {
            Value::Message { is_angle: true, .. } => {}
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn parses_lists_and_separators() {
        let (file, diags) = parse_src("xs: [1, 2, 3]; ys: [\"a\", \"b\"],");
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
        assert_eq!(file.fields.len(), 2);
        match &file.fields[0].value {
            Value::List { elements, .. } => assert_eq!(elements.len(), 3),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn parses_extension_and_any_names() {
        let (file, diags) = parse_src(
            "[com.example.ext]: 1\n[type.googleapis.com/pkg.Foo] { x: 1 }\n",
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
        assert!(matches!(&file.fields[0].name, FieldName::Extension { .. }));
        assert!(matches!(&file.fields[1].name, FieldName::Any { .. }));
    }

    #[test]
    fn concatenates_adjacent_strings() {
        let (file, diags) = parse_src("greeting: \"hi \" 'there'");
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);
        match &file.fields[0].value {
            Value::String { value, .. } => assert_eq!(value, "hi there"),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn recovers_from_missing_value() {
        let (_file, diags) = parse_src("name: \nage: 30");
        // Should flag the missing value but still parse `age: 30`.
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::TextprotoParseError));
    }

    #[test]
    fn header_is_extracted_through_parse() {
        let uri = FileUri::new("mem://x.textproto");
        let pt = super::super::parse::parse(
            uri,
            "# proto-file: foo.proto\n# proto-message: pkg.Msg\nname: \"x\"".into(),
        );
        assert_eq!(pt.ast.header.proto_file.as_ref().unwrap().value, "foo.proto");
        assert_eq!(pt.ast.header.proto_message.as_ref().unwrap().value, "pkg.Msg");
    }
}
