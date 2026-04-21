//! Enum declaration parsing.

use super::{Parser, TokenKindTag};
use crate::ast;
use crate::lexer::TokenKind;
use crate::spans::ByteSpan;

impl<'s> Parser<'s> {
    pub(crate) fn parse_enum(&mut self) -> Option<ast::EnumDecl> {
        let leading = self.peek().leading.comments.clone();
        let start = self.bump().span.start; // enum
        let name = self.expect_ident("enum name")?;
        self.expect(&TokenKind::LBrace, "`{`");

        let mut values = Vec::new();
        let mut reserved = Vec::new();
        let mut options = Vec::new();

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            match self.peek_kind() {
                TokenKind::KwOption => {
                    if let Some(o) = self.parse_option_decl() {
                        options.push(o);
                    }
                }
                TokenKind::KwReserved => {
                    // Reuse the message-reserved parser by peeking and calling
                    // `parse_reserved` is not accessible here; a duplicate is
                    // cleaner but for v1 we skip: delegate using the same
                    // helper approach.
                    if let Some(r) = self.parse_enum_reserved() {
                        reserved.push(r);
                    }
                }
                TokenKind::Semi => { self.bump(); }
                _ => {
                    if let Some(v) = self.parse_enum_value() {
                        values.push(v);
                    } else {
                        self.sync_to(&[TokenKindTag::Semi, TokenKindTag::RBrace, TokenKindTag::Eof]);
                        self.eat(&TokenKind::Semi);
                    }
                }
            }
        }
        let end_tok = self.expect(&TokenKind::RBrace, "`}`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(name.span.end);
        Some(ast::EnumDecl {
            name,
            values,
            reserved,
            options,
            span: ByteSpan::new(start, end),
            leading_comments: leading,
        })
    }

    fn parse_enum_value(&mut self) -> Option<ast::EnumValue> {
        let leading = self.peek().leading.comments.clone();
        let name = self.expect_ident("enum value name")?;
        self.expect(&TokenKind::Eq, "`=`");
        let number = self.parse_int_value()?;
        let options = self.parse_enum_value_options();
        let end_tok = self.expect(&TokenKind::Semi, "`;`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(number.span.end);
        Some(ast::EnumValue {
            name: name.clone(),
            number,
            options,
            span: ByteSpan::new(name.span.start, end),
            leading_comments: leading,
        })
    }

    fn parse_enum_value_options(&mut self) -> Vec<ast::OptionDecl> {
        if !matches!(self.peek_kind(), TokenKind::LBracket) {
            return Vec::new();
        }
        self.bump();
        let mut out = Vec::new();
        loop {
            if matches!(self.peek_kind(), TokenKind::RBracket | TokenKind::Eof) {
                break;
            }
            let start = self.peek().span.start;
            let name = match self.parse_option_name() {
                Some(n) => n,
                None => break,
            };
            self.expect(&TokenKind::Eq, "`=`");
            let value = self.parse_option_value();
            let end = value.span().end;
            out.push(ast::OptionDecl {
                name,
                value,
                span: ByteSpan::new(start, end),
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBracket, "`]`");
        out
    }

    fn parse_enum_reserved(&mut self) -> Option<ast::Reserved> {
        let start = self.bump().span.start; // reserved
        let mut items = Vec::new();
        loop {
            match &self.peek().kind {
                TokenKind::StringLit(s) => {
                    let text = s.clone();
                    let span = self.bump().span;
                    items.push(ast::ReservedItem::Name(text, span));
                }
                TokenKind::IntLit(_) | TokenKind::Minus => {
                    let from = match self.parse_int_value() {
                        Some(v) => v,
                        None => break,
                    };
                    if matches!(self.peek_kind(), TokenKind::KwTo) {
                        self.bump();
                        let to = if matches!(self.peek_kind(), TokenKind::KwMax) {
                            let sp = self.bump().span;
                            ast::ReservedRangeEnd::Max(sp)
                        } else {
                            match self.parse_int_value() {
                                Some(v) => ast::ReservedRangeEnd::Value(v),
                                None => break,
                            }
                        };
                        let end = match &to {
                            ast::ReservedRangeEnd::Value(v) => v.span.end,
                            ast::ReservedRangeEnd::Max(s) => s.end,
                        };
                        items.push(ast::ReservedItem::Range {
                            from: from.clone(),
                            to,
                            span: ByteSpan::new(from.span.start, end),
                        });
                    } else {
                        items.push(ast::ReservedItem::Number(from));
                    }
                }
                _ => {
                    self.unexpected("reserved item");
                    break;
                }
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let end_tok = self.expect(&TokenKind::Semi, "`;`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(self.peek().span.start);
        Some(ast::Reserved { items, span: ByteSpan::new(start, end) })
    }
}
