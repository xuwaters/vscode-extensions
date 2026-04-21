//! Option parsing — including full custom-option names with extensions
//! (`option (foo.bar).baz = ...`) and nested message-literal values.

use super::{Parser, TokenKindTag};
use crate::ast;
use crate::lexer::TokenKind;
use crate::spans::ByteSpan;
use smol_str::SmolStr;

impl<'s> Parser<'s> {
    pub(crate) fn parse_option_decl(&mut self) -> Option<ast::OptionDecl> {
        let start = self.bump().span.start; // option
        let name = self.parse_option_name()?;
        self.expect(&TokenKind::Eq, "`=`");
        let value = self.parse_option_value();
        let end_tok = self.expect(&TokenKind::Semi, "`;`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(value.span().end);
        Some(ast::OptionDecl {
            name,
            value,
            span: ByteSpan::new(start, end),
        })
    }

    pub(crate) fn parse_option_name(&mut self) -> Option<ast::OptionName> {
        let start_span = self.peek().span;
        let mut parts = Vec::new();
        loop {
            let part_start = self.peek().span.start;
            let is_extension = self.eat(&TokenKind::LParen);
            let name = self.parse_qualified_name()?;
            if is_extension {
                self.expect(&TokenKind::RParen, "`)`");
            }
            let end = self.tokens[self.pos.saturating_sub(1)].span.end;
            parts.push(ast::OptionNamePart {
                is_extension,
                name,
                span: ByteSpan::new(part_start, end),
            });
            if !matches!(self.peek_kind(), TokenKind::Dot) {
                break;
            }
            // consume the separator dot
            self.bump();
        }
        let end = parts.last().map(|p| p.span.end).unwrap_or(start_span.end);
        Some(ast::OptionName {
            parts,
            span: ByteSpan::new(start_span.start, end),
        })
    }

    pub(crate) fn parse_option_value(&mut self) -> ast::OptionValue {
        match self.peek_kind().clone() {
            TokenKind::StringLit(s) => {
                let sp = self.bump().span;
                ast::OptionValue::String(s, sp)
            }
            TokenKind::IntLit(_) | TokenKind::Minus | TokenKind::Plus => {
                if let Some(v) = self.parse_int_value() {
                    return ast::OptionValue::Int(v);
                }
                ast::OptionValue::Missing(self.peek().span)
            }
            TokenKind::FloatLit(s) => {
                let sp = self.bump().span;
                ast::OptionValue::Float(s, sp)
            }
            TokenKind::KwTrue => {
                let sp = self.bump().span;
                ast::OptionValue::Bool(true, sp)
            }
            TokenKind::KwFalse => {
                let sp = self.bump().span;
                ast::OptionValue::Bool(false, sp)
            }
            TokenKind::LBrace => self.parse_message_literal(),
            TokenKind::LBracket => self.parse_list_literal(),
            TokenKind::Ident(s) => {
                let sp = self.bump().span;
                ast::OptionValue::Ident(ast::Ident { name: s, span: sp })
            }
            _ => {
                let sp = self.peek().span;
                self.unexpected("option value");
                ast::OptionValue::Missing(sp)
            }
        }
    }

    fn parse_message_literal(&mut self) -> ast::OptionValue {
        let start = self.bump().span.start; // {
        let mut fields = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            let field_start = self.peek().span.start;
            let name = match self.parse_ident() {
                Some(i) => i,
                None => {
                    self.sync_to(&[TokenKindTag::RBrace, TokenKindTag::Eof]);
                    break;
                }
            };
            // proto text-format allows `name: value` or `name { ... }`
            let value = match self.peek_kind() {
                TokenKind::Colon => {
                    self.bump();
                    self.parse_option_value()
                }
                TokenKind::LBrace | TokenKind::LBracket => self.parse_option_value(),
                _ => {
                    let sp = self.peek().span;
                    self.unexpected("`:` or message body");
                    ast::OptionValue::Missing(sp)
                }
            };
            let end = value.span().end;
            fields.push(ast::MessageLiteralField {
                name,
                value,
                span: ByteSpan::new(field_start, end),
            });
            // Field separator is optional: `,` or `;` or bare.
            if self.eat(&TokenKind::Comma) || self.eat(&TokenKind::Semi) {
                continue;
            }
        }
        let end_tok = self.expect(&TokenKind::RBrace, "`}`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(self.peek().span.start);
        ast::OptionValue::Message(fields, ByteSpan::new(start, end))
    }

    fn parse_list_literal(&mut self) -> ast::OptionValue {
        let start = self.bump().span.start; // [
        let mut values = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBracket | TokenKind::Eof) {
            values.push(self.parse_option_value());
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let end_tok = self.expect(&TokenKind::RBracket, "`]`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(self.peek().span.start);
        ast::OptionValue::List(values, ByteSpan::new(start, end))
    }
}

/// Convenience: convert a scalar keyword token into a `SmolStr` used by
/// some error messages.
#[allow(dead_code)]
fn token_as_name(k: &TokenKind) -> Option<SmolStr> {
    if let TokenKind::Ident(s) = k {
        Some(s.clone())
    } else {
        None
    }
}
