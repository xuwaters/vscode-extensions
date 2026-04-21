//! Message, field, oneof, reserved, extensions, and map parsing.

use super::{Parser, TokenKindTag, MESSAGE_BODY_SYNC};
use crate::ast;
use crate::diagnostics::DiagnosticCode;
use crate::lexer::TokenKind;
use crate::spans::ByteSpan;

impl<'s> Parser<'s> {
    pub(crate) fn parse_message(&mut self) -> Option<ast::Message> {
        let leading = self.peek().leading.comments.clone();
        let start = self.bump().span.start; // `message`
        let name = self.expect_ident("message name")?;
        self.expect(&TokenKind::LBrace, "`{`");

        let mut m = ast::Message {
            name,
            fields: Vec::new(),
            oneofs: Vec::new(),
            nested_messages: Vec::new(),
            nested_enums: Vec::new(),
            reserved: Vec::new(),
            extensions: Vec::new(),
            options: Vec::new(),
            span: ByteSpan::new(start, start),
            leading_comments: leading,
        };

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            self.parse_message_body_item(&mut m);
        }
        let end_tok = self.expect(&TokenKind::RBrace, "`}`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(self.peek().span.start);
        m.span = ByteSpan::new(start, end);
        Some(m)
    }

    fn parse_message_body_item(&mut self, m: &mut ast::Message) {
        match self.peek_kind() {
            TokenKind::KwMessage => {
                if let Some(nested) = self.parse_message() {
                    m.nested_messages.push(nested);
                }
            }
            TokenKind::KwEnum => {
                if let Some(e) = self.parse_enum() {
                    m.nested_enums.push(e);
                }
            }
            TokenKind::KwOneof => {
                if let Some(o) = self.parse_oneof() {
                    m.oneofs.push(o);
                }
            }
            TokenKind::KwReserved => {
                if let Some(r) = self.parse_reserved() {
                    m.reserved.push(r);
                }
            }
            TokenKind::KwExtensions => {
                if let Some(e) = self.parse_extensions_decl() {
                    m.extensions.push(e);
                }
            }
            TokenKind::KwExtend => {
                // extend nested within a message — not a common proto3 shape.
                // Collect it into a floating extend under the file? We
                // currently skip it (Phase 1 focus on well-formed proto3).
                let _ = self.parse_extend();
            }
            TokenKind::KwOption => {
                if let Some(opt) = self.parse_option_decl() {
                    m.options.push(opt);
                }
            }
            TokenKind::Semi => { self.bump(); }
            _ => {
                if let Some(f) = self.parse_field_decl_in_message() {
                    m.fields.push(f);
                } else {
                    self.sync_to(MESSAGE_BODY_SYNC);
                    if matches!(self.peek_kind(), TokenKind::Semi) {
                        self.bump();
                    }
                }
            }
        }
    }

    pub(crate) fn parse_field_decl_in_message(&mut self) -> Option<ast::FieldDecl> {
        let leading = self.peek().leading.comments.clone();
        let start_span = self.peek().span;

        // Optional label
        let label = match self.peek_kind() {
            TokenKind::KwRepeated => { self.bump(); ast::FieldLabel::Repeated }
            TokenKind::KwOptional => { self.bump(); ast::FieldLabel::Optional }
            TokenKind::KwRequired => {
                let sp = self.bump().span;
                self.error_at(
                    DiagnosticCode::Proto3RequiredForbidden,
                    "`required` is not permitted in proto3".to_string(),
                    sp,
                );
                ast::FieldLabel::Required
            }
            _ => ast::FieldLabel::None,
        };

        let ty = self.parse_type_ref()?;
        let name = self.expect_ident("field name")?;
        self.expect(&TokenKind::Eq, "`=`");
        let number = match self.parse_int_value() {
            Some(v) => v,
            None => {
                self.unexpected("field number");
                self.sync_to(&[TokenKindTag::Semi, TokenKindTag::RBrace, TokenKindTag::Eof]);
                self.eat(&TokenKind::Semi);
                return None;
            }
        };
        let options = self.parse_field_options();
        let end_tok = self.expect(&TokenKind::Semi, "`;`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(number.span.end);
        Some(ast::FieldDecl {
            label,
            ty,
            name,
            number,
            options,
            span: ByteSpan::new(start_span.start, end),
            leading_comments: leading,
        })
    }

    fn parse_field_options(&mut self) -> Vec<ast::OptionDecl> {
        if !matches!(self.peek_kind(), TokenKind::LBracket) {
            return Vec::new();
        }
        self.bump(); // [
        let mut out = Vec::new();
        loop {
            if matches!(self.peek_kind(), TokenKind::RBracket | TokenKind::Eof) {
                break;
            }
            let start_span = self.peek().span;
            let name = match self.parse_option_name() {
                Some(n) => n,
                None => {
                    self.sync_to(&[TokenKindTag::Semi, TokenKindTag::RBrace, TokenKindTag::Eof]);
                    break;
                }
            };
            self.expect(&TokenKind::Eq, "`=`");
            let value = self.parse_option_value();
            let end = value.span().end;
            out.push(ast::OptionDecl {
                name,
                value,
                span: ByteSpan::new(start_span.start, end),
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBracket, "`]`");
        out
    }

    fn parse_type_ref(&mut self) -> Option<ast::TypeRef> {
        use ast::ScalarType as S;
        match self.peek_kind() {
            TokenKind::TyDouble => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Double, sp)) }
            TokenKind::TyFloat => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Float, sp)) }
            TokenKind::TyInt32 => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Int32, sp)) }
            TokenKind::TyInt64 => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Int64, sp)) }
            TokenKind::TyUint32 => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Uint32, sp)) }
            TokenKind::TyUint64 => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Uint64, sp)) }
            TokenKind::TySint32 => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Sint32, sp)) }
            TokenKind::TySint64 => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Sint64, sp)) }
            TokenKind::TyFixed32 => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Fixed32, sp)) }
            TokenKind::TyFixed64 => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Fixed64, sp)) }
            TokenKind::TySfixed32 => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Sfixed32, sp)) }
            TokenKind::TySfixed64 => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Sfixed64, sp)) }
            TokenKind::TyBool => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Bool, sp)) }
            TokenKind::TyString => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::String, sp)) }
            TokenKind::TyBytes => { let sp = self.bump().span; Some(ast::TypeRef::Scalar(S::Bytes, sp)) }
            TokenKind::KwMap => Some(self.parse_map_type()),
            TokenKind::Dot | TokenKind::Ident(_) => {
                let q = self.parse_qualified_name()?;
                Some(ast::TypeRef::Named(q))
            }
            _ => {
                let sp = self.peek().span;
                self.unexpected("field type");
                Some(ast::TypeRef::Missing(sp))
            }
        }
    }

    fn parse_map_type(&mut self) -> ast::TypeRef {
        let start = self.bump().span.start; // map
        self.expect(&TokenKind::LAngle, "`<`");
        let key = self
            .parse_type_ref()
            .unwrap_or(ast::TypeRef::Missing(self.peek().span));
        self.expect(&TokenKind::Comma, "`,`");
        let value = self
            .parse_type_ref()
            .unwrap_or(ast::TypeRef::Missing(self.peek().span));
        let end_tok = self.expect(&TokenKind::RAngle, "`>`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(value.span().end);
        ast::TypeRef::Map(Box::new(ast::MapType {
            key,
            value,
            span: ByteSpan::new(start, end),
        }))
    }

    fn parse_oneof(&mut self) -> Option<ast::Oneof> {
        let start = self.bump().span.start; // oneof
        let name = self.expect_ident("oneof name")?;
        self.expect(&TokenKind::LBrace, "`{`");
        let mut fields = Vec::new();
        let mut options = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            match self.peek_kind() {
                TokenKind::KwOption => {
                    if let Some(o) = self.parse_option_decl() {
                        options.push(o);
                    }
                }
                TokenKind::KwRepeated | TokenKind::KwOptional | TokenKind::KwRequired => {
                    // Labels are illegal in oneof; error and continue to parse
                    // so recovery is smooth.
                    let sp = self.peek().span;
                    self.error_at(
                        DiagnosticCode::OneofInvalidField,
                        format!("Oneof `{}` cannot contain labeled field", name.name),
                        sp,
                    );
                    let _ = self.parse_field_decl_in_message();
                }
                TokenKind::Semi => { self.bump(); }
                _ => {
                    if let Some(f) = self.parse_field_decl_in_message() {
                        fields.push(f);
                    } else {
                        self.sync_to(&[TokenKindTag::Semi, TokenKindTag::RBrace, TokenKindTag::Eof]);
                        self.eat(&TokenKind::Semi);
                    }
                }
            }
        }
        let end_tok = self.expect(&TokenKind::RBrace, "`}`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(name.span.end);
        Some(ast::Oneof { name, fields, options, span: ByteSpan::new(start, end) })
    }

    fn parse_reserved(&mut self) -> Option<ast::Reserved> {
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

    fn parse_extensions_decl(&mut self) -> Option<ast::ExtensionsDecl> {
        let start = self.bump().span.start;
        let mut ranges = Vec::new();
        loop {
            let from = self.parse_int_value()?;
            let to = if matches!(self.peek_kind(), TokenKind::KwTo) {
                self.bump();
                if matches!(self.peek_kind(), TokenKind::KwMax) {
                    let sp = self.bump().span;
                    ast::ReservedRangeEnd::Max(sp)
                } else {
                    ast::ReservedRangeEnd::Value(self.parse_int_value()?)
                }
            } else {
                ast::ReservedRangeEnd::Value(from.clone())
            };
            let end_span = match &to {
                ast::ReservedRangeEnd::Value(v) => v.span.end,
                ast::ReservedRangeEnd::Max(s) => s.end,
            };
            ranges.push(ast::ExtensionRange {
                from: from.clone(),
                to,
                span: ByteSpan::new(from.span.start, end_span),
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let end_tok = self.expect(&TokenKind::Semi, "`;`");
        let end = end_tok.map(|t| t.span.end).unwrap_or(self.peek().span.start);
        Some(ast::ExtensionsDecl {
            ranges,
            options: Vec::new(),
            span: ByteSpan::new(start, end),
        })
    }
}
