//! Top-level parser entry: syntax/edition, package, imports, options, and
//! top-level message/enum/service/extend declarations.

use super::{Parser, TokenKindTag, TOP_LEVEL_SYNC};
use crate::ast;
use crate::diagnostics::{DiagnosticCode, Severity, ProtoDiagnostic};
use crate::lexer::TokenKind;
use crate::spans::ByteSpan;

impl<'s> Parser<'s> {
    pub fn parse_file(&mut self) -> ast::File {
        let start = self.peek().span.start;
        let leading_comments = self.peek().leading.comments.clone();

        let (syntax, syntax_span) = self.parse_syntax_or_edition();
        let mut package: Option<ast::Package> = None;
        let mut imports: Vec<ast::Import> = Vec::new();
        let mut options: Vec<ast::OptionDecl> = Vec::new();
        let mut items: Vec<ast::TopLevelItem> = Vec::new();

        while !matches!(self.peek_kind(), TokenKind::Eof) {
            match self.peek_kind() {
                TokenKind::KwPackage => {
                    let pkg = self.parse_package_decl();
                    if let Some(p) = pkg {
                        if package.is_some() {
                            self.diagnostics.push(ProtoDiagnostic::new(
                                DiagnosticCode::ParseUnexpectedToken,
                                Severity::Error,
                                "Duplicate `package` declaration".into(),
                                p.span,
                            ));
                        } else {
                            package = Some(p);
                        }
                    }
                }
                TokenKind::KwImport => {
                    if let Some(imp) = self.parse_import() {
                        imports.push(imp);
                    }
                }
                TokenKind::KwOption => {
                    if let Some(opt) = self.parse_option_decl() {
                        options.push(opt);
                    }
                }
                TokenKind::KwMessage => {
                    if let Some(m) = self.parse_message() {
                        items.push(ast::TopLevelItem::Message(m));
                    }
                }
                TokenKind::KwEnum => {
                    if let Some(e) = self.parse_enum() {
                        items.push(ast::TopLevelItem::Enum(e));
                    }
                }
                TokenKind::KwService => {
                    if let Some(s) = self.parse_service() {
                        items.push(ast::TopLevelItem::Service(s));
                    }
                }
                TokenKind::KwExtend => {
                    if let Some(e) = self.parse_extend() {
                        items.push(ast::TopLevelItem::Extend(e));
                    }
                }
                TokenKind::Semi => {
                    // Empty statement; harmless per the spec.
                    self.bump();
                }
                _ => {
                    self.unexpected("top-level declaration");
                    self.bump();
                    self.sync_to(TOP_LEVEL_SYNC);
                }
            }
        }

        let end = self.peek().span.end;
        ast::File {
            syntax,
            syntax_span,
            package,
            imports,
            options,
            items,
            span: ByteSpan::new(start, end),
            leading_comments,
        }
    }

    fn parse_syntax_or_edition(&mut self) -> (ast::Syntax, Option<ByteSpan>) {
        match self.peek_kind() {
            TokenKind::KwSyntax => {
                let start = self.bump().span.start;
                self.expect(&TokenKind::Eq, "`=`");
                let syntax = match &self.peek().kind {
                    TokenKind::StringLit(s) => {
                        let text = s.clone();
                        self.bump();
                        match text.as_str() {
                            "proto3" => ast::Syntax::Proto3,
                            "proto2" => ast::Syntax::Proto2,
                            _ => {
                                let sp = self.tokens[self.pos - 1].span;
                                self.error_at(
                                    DiagnosticCode::ParseUnexpectedToken,
                                    format!("Unknown syntax `\"{}\"` (expected \"proto2\" or \"proto3\")", text),
                                    sp,
                                );
                                ast::Syntax::Unspecified
                            }
                        }
                    }
                    _ => {
                        self.unexpected("syntax string");
                        ast::Syntax::Unspecified
                    }
                };
                let end_tok = self.expect(&TokenKind::Semi, "`;`");
                let end = end_tok.map(|t| t.span.end).unwrap_or(self.peek().span.start);
                (syntax, Some(ByteSpan::new(start, end)))
            }
            TokenKind::KwEdition => {
                let start = self.bump().span.start;
                self.expect(&TokenKind::Eq, "`=`");
                let syntax = match &self.peek().kind {
                    TokenKind::StringLit(s) => {
                        let edition = smol_str::SmolStr::new(s);
                        self.bump();
                        ast::Syntax::Edition(edition)
                    }
                    _ => {
                        self.unexpected("edition string");
                        ast::Syntax::Unspecified
                    }
                };
                let end_tok = self.expect(&TokenKind::Semi, "`;`");
                let end = end_tok.map(|t| t.span.end).unwrap_or(self.peek().span.start);
                (syntax, Some(ByteSpan::new(start, end)))
            }
            _ => (ast::Syntax::Unspecified, None),
        }
    }

    fn parse_package_decl(&mut self) -> Option<ast::Package> {
        let start = self.bump().span.start; // package
        let name = match self.parse_qualified_name() {
            Some(n) => n,
            None => {
                self.unexpected("package name");
                self.sync_to(&[TokenKindTag::Semi, TokenKindTag::Eof]);
                self.eat(&TokenKind::Semi);
                return None;
            }
        };
        let end_tok = self.expect(&TokenKind::Semi, "`;`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(name.span.end);
        Some(ast::Package { name, span: ByteSpan::new(start, end) })
    }

    fn parse_import(&mut self) -> Option<ast::Import> {
        let start = self.bump().span.start; // import
        let modifier = match self.peek_kind() {
            TokenKind::KwPublic => {
                self.bump();
                ast::ImportModifier::Public
            }
            TokenKind::KwWeak => {
                self.bump();
                ast::ImportModifier::Weak
            }
            _ => ast::ImportModifier::None,
        };
        let (path, path_span) = match &self.peek().kind {
            TokenKind::StringLit(s) => {
                let val = s.clone();
                let span = self.bump().span;
                (val, span)
            }
            _ => {
                self.unexpected("import path string");
                self.sync_to(&[TokenKindTag::Semi, TokenKindTag::Eof]);
                self.eat(&TokenKind::Semi);
                return None;
            }
        };
        let end_tok = self.expect(&TokenKind::Semi, "`;`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(path_span.end);
        Some(ast::Import {
            path,
            path_span,
            modifier,
            span: ByteSpan::new(start, end),
        })
    }

    pub(crate) fn parse_extend(&mut self) -> Option<ast::Extend> {
        let start = self.bump().span.start; // extend
        let ty = self.parse_qualified_name()?;
        self.expect(&TokenKind::LBrace, "`{`");
        let mut fields = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            if let Some(f) = self.parse_field_decl_in_message() {
                fields.push(f);
            } else {
                self.bump();
            }
        }
        let end_tok = self.expect(&TokenKind::RBrace, "`}`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(ty.span.end);
        Some(ast::Extend {
            ty,
            fields,
            span: ByteSpan::new(start, end),
        })
    }
}
