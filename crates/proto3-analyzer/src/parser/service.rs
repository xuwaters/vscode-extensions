//! Service / RPC parsing.

use super::{Parser, TokenKindTag};
use crate::ast;
use crate::lexer::{Token, TokenKind};
use crate::spans::ByteSpan;

impl<'s> Parser<'s> {
    pub(crate) fn parse_service(&mut self) -> Option<ast::Service> {
        self.nested(TokenKind::LBrace, TokenKind::RBrace, Self::parse_service_decl).flatten()
    }

    fn parse_service_decl(&mut self) -> Option<ast::Service> {
        let leading = self.peek().leading.comments.clone();
        let start = self.bump().span.start; // service
        let name = self.expect_ident("service name")?;
        self.expect(&TokenKind::LBrace, "`{`");

        let mut methods = Vec::new();
        let mut options = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            match self.peek_kind() {
                TokenKind::KwOption => {
                    if let Some(o) = self.parse_option_decl() {
                        options.push(o);
                    }
                }
                TokenKind::KwRpc => {
                    if let Some(r) = self.parse_rpc() {
                        methods.push(r);
                    } else {
                        self.sync_to(&[TokenKindTag::Semi, TokenKindTag::RBrace, TokenKindTag::Eof]);
                        self.eat(&TokenKind::Semi);
                    }
                }
                TokenKind::Semi => { self.bump(); }
                _ => {
                    self.unexpected("rpc declaration");
                    self.bump();
                    self.sync_to(&[
                        TokenKindTag::KwRpc,
                        TokenKindTag::KwOption,
                        TokenKindTag::RBrace,
                        TokenKindTag::Eof,
                    ]);
                }
            }
        }
        let end_tok = self.expect(&TokenKind::RBrace, "`}`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(name.span.end);
        Some(ast::Service {
            name,
            methods,
            options,
            span: ByteSpan::new(start, end),
            leading_comments: leading,
        })
    }

    fn parse_rpc(&mut self) -> Option<ast::Rpc> {
        let leading = self.peek().leading.comments.clone();
        let start = self.bump().span.start; // rpc
        let name = self.expect_ident("rpc name")?;
        let input = self.parse_rpc_type()?;
        self.expect(&TokenKind::KwReturns, "`returns`");
        let output = self.parse_rpc_type()?;

        let mut options = Vec::new();
        let close = match self.peek_kind() {
            TokenKind::LBrace => self
                .nested(TokenKind::LBrace, TokenKind::RBrace, |p| p.parse_rpc_body(&mut options))
                .flatten(),
            _ => self.expect(&TokenKind::Semi, "`;`"),
        };
        let end = close.map(|t| t.span.end).unwrap_or(output.span.end);

        Some(ast::Rpc {
            name: name.clone(),
            input,
            output,
            options,
            span: ByteSpan::new(start, end),
            leading_comments: leading,
        })
    }

    /// Parse `{ option ...; }` after an rpc signature; returns the `}`.
    fn parse_rpc_body(&mut self, options: &mut Vec<ast::OptionDecl>) -> Option<Token> {
        self.bump(); // {
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            match self.peek_kind() {
                TokenKind::KwOption => {
                    if let Some(o) = self.parse_option_decl() {
                        options.push(o);
                    }
                }
                TokenKind::Semi => { self.bump(); }
                _ => {
                    self.bump();
                }
            }
        }
        self.expect(&TokenKind::RBrace, "`}`")
    }

    fn parse_rpc_type(&mut self) -> Option<ast::RpcType> {
        let start = self.peek().span.start;
        self.expect(&TokenKind::LParen, "`(`")?;
        let streaming = self.eat(&TokenKind::KwStream);
        let ty = self.parse_qualified_name()?;
        let end_tok = self.expect(&TokenKind::RParen, "`)`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(ty.span.end);
        Some(ast::RpcType {
            streaming,
            ty,
            span: ByteSpan::new(start, end),
        })
    }
}
