//! Recursive-descent parser for Cap'n Proto schema files.
//!
//! Recovery model: on an unexpected token the parser records a
//! [`ParseError`] and skips forward to the next `;` or matching `}` so the
//! outline can still be built for the rest of the file. This matches the
//! behaviour of the proto3 parser in the sibling crate.
//!
//! Scope note: this pass captures declaration structure (names, ordinals,
//! type references, nesting) — enough for diagnostics and document
//! outlines. Complex expressions (field defaults, annotation arguments) are
//! preserved as token-range spans rather than parsed into value nodes.

use crate::ast::*;
use crate::lexer::{lex, Token, TokenKind};
use crate::spans::ByteSpan;
use smol_str::SmolStr;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: ByteSpan,
}

pub struct ParseResult {
    pub file: File,
    pub errors: Vec<ParseError>,
}

pub fn parse(source: &str) -> ParseResult {
    let tokens = lex(source);
    let mut p = Parser::new(tokens);
    let file = p.parse_file();
    ParseResult { file, errors: p.errors }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<ParseError>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0, errors: Vec::new() }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn error(&mut self, span: ByteSpan, msg: impl Into<String>) {
        self.errors.push(ParseError { message: msg.into(), span });
    }

    fn expect(&mut self, want: &TokenKind, ctx: &str) -> Option<Token> {
        if std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(want) {
            Some(self.bump())
        } else {
            let span = self.peek().span;
            self.error(span, format!("expected {} in {}", describe(want), ctx));
            None
        }
    }

    fn skip_to_semi_or_brace(&mut self) {
        let mut depth = 0i32;
        while !self.at_eof() {
            match self.peek().kind {
                TokenKind::LBrace | TokenKind::LParen => { depth += 1; self.bump(); }
                TokenKind::RBrace | TokenKind::RParen => {
                    if depth == 0 { return; }
                    depth -= 1; self.bump();
                }
                TokenKind::Semi if depth == 0 => { self.bump(); return; }
                _ => { self.bump(); }
            }
        }
    }

    fn parse_file(&mut self) -> File {
        let start = self.peek().span.start;
        let file_id = self.parse_file_id();

        let mut decls = Vec::new();
        while !self.at_eof() {
            match self.parse_top_decl() {
                Some(d) => decls.push(d),
                None => {
                    let bad = self.peek().span;
                    self.error(bad, "expected top-level declaration");
                    self.skip_to_semi_or_brace();
                }
            }
        }

        let end = self
            .tokens
            .last()
            .map(|t| t.span.end)
            .unwrap_or(start);
        File { file_id, decls, span: ByteSpan::new(start, end) }
    }

    fn parse_file_id(&mut self) -> Option<FileId> {
        if !matches!(self.peek().kind, TokenKind::At) {
            return None;
        }
        let at = self.bump();
        let int = match &self.peek().kind {
            TokenKind::IntLit(_) => self.bump(),
            _ => {
                let span = at.span;
                self.error(span, "file id must be @0x…");
                return None;
            }
        };
        let text = if let TokenKind::IntLit(ref s) = int.kind { s.clone() } else { SmolStr::default() };
        self.expect(&TokenKind::Semi, "file id");
        let span = at.span.join(int.span);
        Some(FileId { value: text, span })
    }

    fn parse_top_decl(&mut self) -> Option<Decl> {
        match self.peek().kind {
            TokenKind::KwUsing => Some(Decl::Using(self.parse_using())),
            TokenKind::KwStruct => Some(Decl::Struct(self.parse_struct())),
            TokenKind::KwEnum => Some(Decl::Enum(self.parse_enum())),
            TokenKind::KwInterface => Some(Decl::Interface(self.parse_interface())),
            TokenKind::KwConst => Some(Decl::Const(self.parse_const())),
            TokenKind::KwAnnotation => Some(Decl::Annotation(self.parse_annotation_decl())),
            TokenKind::Dollar => Some(Decl::TopAnnotation(self.parse_top_annotation())),
            _ => None,
        }
    }

    fn parse_using(&mut self) -> Using {
        let start = self.peek().span;
        self.bump(); // 'using'

        // `using import "file".Tag;` — no explicit alias name. The last
        // component of the import target becomes the implicit alias.
        if matches!(self.peek().kind, TokenKind::KwImport) {
            let (import_path, import_target) = self.parse_import_expr();
            let name = import_target.last().cloned();
            let end = self.peek().span;
            self.expect(&TokenKind::Semi, "using");
            return Using { name, import_path, import_target, span: start.join(end) };
        }

        let mut name = None;
        if let TokenKind::Ident(_) = self.peek().kind {
            name = Some(self.parse_ident());
            // Optional `= ...`
            if matches!(self.peek().kind, TokenKind::Eq) {
                self.bump();
            }
        }

        let mut import_path = None;
        let mut import_target = Vec::new();
        if matches!(self.peek().kind, TokenKind::KwImport) {
            let (p, t) = self.parse_import_expr();
            import_path = p;
            import_target = t;
        } else {
            // Right-hand side is a type reference; we just skip to `;` keeping no data.
            while !self.at_eof() && !matches!(self.peek().kind, TokenKind::Semi) {
                self.bump();
            }
        }
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "using");
        Using { name, import_path, import_target, span: start.join(end) }
    }

    /// Parse `import "path"[.Ident[.Ident…]]` starting at the `import`
    /// keyword. Returns the string literal and the dotted path that follows.
    fn parse_import_expr(&mut self) -> (Option<StringLit>, Vec<Ident>) {
        self.bump(); // 'import'
        let mut import_path = None;
        if let TokenKind::StringLit(_) = self.peek().kind.clone() {
            let t = self.bump();
            let value = if let TokenKind::StringLit(s) = t.kind { s } else { String::new() };
            import_path = Some(StringLit { value, span: t.span });
        } else {
            let span = self.peek().span;
            self.error(span, "expected string literal after 'import'");
        }
        let mut target = Vec::new();
        while matches!(self.peek().kind, TokenKind::Dot) {
            self.bump();
            target.push(self.parse_type_ident());
        }
        (import_path, target)
    }

    fn parse_ident(&mut self) -> Ident {
        if let TokenKind::Ident(_) = self.peek().kind {
            let t = self.bump();
            let text = match t.kind {
                TokenKind::Ident(s) => s,
                _ => SmolStr::default(),
            };
            return Ident { text, span: t.span };
        }
        // Some type keywords are also valid as identifiers in certain
        // positions (e.g. `List` inside `using Something = List`). Accept them
        // transparently.
        if let Some(text) = type_keyword_text(&self.peek().kind) {
            let t = self.bump();
            return Ident { text: SmolStr::new(text), span: t.span };
        }
        let span = self.peek().span;
        self.error(span, "expected identifier");
        Ident { text: SmolStr::default(), span }
    }

    fn parse_annotations(&mut self) -> Vec<AnnotationApp> {
        let mut out = Vec::new();
        while matches!(self.peek().kind, TokenKind::Dollar) {
            out.push(self.parse_annotation_app());
        }
        out
    }

    fn parse_annotation_app(&mut self) -> AnnotationApp {
        let start = self.bump().span; // '$'
        let mut path = Vec::new();
        path.push(self.parse_ident());
        while matches!(self.peek().kind, TokenKind::Dot) {
            self.bump();
            path.push(self.parse_ident());
        }
        let args_span = if matches!(self.peek().kind, TokenKind::LParen) {
            Some(self.skip_parenthesised())
        } else {
            None
        };
        let end = path.last().map(|i| i.span).unwrap_or(start);
        let full = start.join(args_span.unwrap_or(end));
        AnnotationApp { path, args_span, span: full }
    }

    fn parse_top_annotation(&mut self) -> AnnotationApp {
        let app = self.parse_annotation_app();
        self.expect(&TokenKind::Semi, "top-level annotation");
        app
    }

    fn skip_parenthesised(&mut self) -> ByteSpan {
        let start = self.peek().span;
        let mut depth = 0i32;
        let mut last = start;
        while !self.at_eof() {
            let t = self.peek().clone();
            match t.kind {
                TokenKind::LParen => { depth += 1; last = t.span; self.bump(); }
                TokenKind::RParen => {
                    last = t.span;
                    self.bump();
                    depth -= 1;
                    if depth == 0 { return start.join(last); }
                }
                _ => { last = t.span; self.bump(); }
            }
        }
        start.join(last)
    }

    fn parse_type_params(&mut self) -> Vec<Ident> {
        let mut out = Vec::new();
        if !matches!(self.peek().kind, TokenKind::LParen) {
            return out;
        }
        self.bump(); // '('
        loop {
            match self.peek().kind {
                TokenKind::RParen => { self.bump(); break; }
                TokenKind::Eof => break,
                TokenKind::Ident(_) => {
                    out.push(self.parse_ident());
                    if matches!(self.peek().kind, TokenKind::Comma) { self.bump(); }
                }
                _ => { self.bump(); }
            }
        }
        out
    }

    fn parse_struct(&mut self) -> Struct {
        let start = self.bump().span; // 'struct'
        let name = self.parse_ident();
        let type_params = self.parse_type_params();
        let annotations = self.parse_annotations();
        let members = self.parse_struct_body();
        let end = self.prev_span();
        Struct { name, type_params, members, annotations, span: start.join(end) }
    }

    fn parse_struct_body(&mut self) -> Vec<StructMember> {
        let mut members = Vec::new();
        if self.expect(&TokenKind::LBrace, "struct body").is_none() {
            return members;
        }
        while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
            match self.parse_struct_member() {
                Some(m) => members.push(m),
                None => {
                    self.skip_to_semi_or_brace();
                }
            }
        }
        self.expect(&TokenKind::RBrace, "struct body");
        members
    }

    fn parse_struct_member(&mut self) -> Option<StructMember> {
        match self.peek().kind {
            TokenKind::KwStruct => Some(StructMember::Struct(self.parse_struct())),
            TokenKind::KwEnum => Some(StructMember::Enum(self.parse_enum())),
            TokenKind::KwInterface => Some(StructMember::Interface(self.parse_interface())),
            TokenKind::KwConst => Some(StructMember::Const(self.parse_const())),
            TokenKind::KwAnnotation => Some(StructMember::Annotation(self.parse_annotation_decl())),
            TokenKind::KwUsing => Some(StructMember::Using(self.parse_using())),
            TokenKind::KwUnion => {
                let ub = self.parse_anon_union();
                Some(StructMember::AnonUnion(ub))
            }
            TokenKind::Ident(_) => Some(StructMember::Field(self.parse_field())),
            _ => {
                let span = self.peek().span;
                self.error(span, "expected struct member");
                None
            }
        }
    }

    fn parse_anon_union(&mut self) -> UnionBlock {
        let start = self.bump().span; // 'union'
        self.expect(&TokenKind::LBrace, "union body");
        let mut members = Vec::new();
        while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
            if let TokenKind::Ident(_) = self.peek().kind {
                members.push(self.parse_field());
            } else {
                let span = self.peek().span;
                self.error(span, "expected field in union");
                self.skip_to_semi_or_brace();
            }
        }
        self.expect(&TokenKind::RBrace, "union body");
        let end = self.prev_span();
        UnionBlock { members, span: start.join(end) }
    }

    fn parse_field(&mut self) -> Field {
        let start = self.peek().span;
        let name = self.parse_ident();

        let ordinal = if matches!(self.peek().kind, TokenKind::At) {
            Some(self.parse_ordinal())
        } else {
            None
        };

        // After the (optional) ordinal we need to decide: slot, named union,
        // or named group.
        let body = if matches!(self.peek().kind, TokenKind::Colon) {
            self.bump(); // ':'
            match self.peek().kind {
                TokenKind::KwUnion => {
                    let ub = self.parse_anon_union(); // reuses union body parsing
                    FieldBody::NamedUnion(ub)
                }
                TokenKind::KwGroup => {
                    let gb = self.parse_group();
                    FieldBody::NamedGroup(gb)
                }
                _ => {
                    let ty = self.parse_type_ref();
                    let default_span = if matches!(self.peek().kind, TokenKind::Eq) {
                        Some(self.skip_expr_to_end())
                    } else {
                        None
                    };
                    FieldBody::Slot { ty, default_span }
                }
            }
        } else {
            let span = self.peek().span;
            self.error(span, "expected ':' in field");
            let dummy = TypeRef { import_path: None, path: Vec::new(), args: Vec::new(), span };
            FieldBody::Slot { ty: dummy, default_span: None }
        };

        let annotations = self.parse_annotations();

        let end = match body {
            FieldBody::NamedUnion(ref u) => u.span,
            FieldBody::NamedGroup(ref g) => g.span,
            _ => self.peek().span,
        };

        // Named unions/groups carry their own brace-terminated body; slot
        // fields still need the trailing `;`.
        if matches!(body, FieldBody::Slot { .. }) {
            self.expect(&TokenKind::Semi, "field");
        }

        Field { name, ordinal, body, annotations, span: start.join(end) }
    }

    fn parse_group(&mut self) -> GroupBlock {
        let start = self.bump().span; // 'group'
        let members = self.parse_struct_body();
        let end = self.prev_span();
        GroupBlock { members, span: start.join(end) }
    }

    fn parse_ordinal(&mut self) -> Ordinal {
        let at = self.bump(); // '@'
        if let TokenKind::IntLit(ref s) = self.peek().kind.clone() {
            let tok = self.bump();
            let value = parse_int(s).unwrap_or(u32::MAX);
            return Ordinal { value, span: at.span.join(tok.span) };
        }
        let span = at.span;
        self.error(span, "expected ordinal number after '@'");
        Ordinal { value: u32::MAX, span }
    }

    /// Consume `= <expr>` and return the span of the expression. Stops at
    /// `;` or a top-level `,` / `)` / `}` (top-level relative to the nesting
    /// we enter here).
    fn skip_expr_to_end(&mut self) -> ByteSpan {
        let start = self.bump().span; // '='
        let mut depth = 0i32;
        let mut last = start;
        while !self.at_eof() {
            let k = self.peek().kind.clone();
            match k {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => {
                    depth += 1;
                    last = self.peek().span; self.bump();
                }
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    if depth == 0 { break; }
                    depth -= 1;
                    last = self.peek().span; self.bump();
                }
                TokenKind::Semi if depth == 0 => break,
                TokenKind::Comma if depth == 0 => break,
                _ => { last = self.peek().span; self.bump(); }
            }
        }
        start.join(last)
    }

    fn parse_type_ref(&mut self) -> TypeRef {
        let start = self.peek().span;
        let mut import_path = None;
        let mut path = Vec::new();
        if matches!(self.peek().kind, TokenKind::KwImport) {
            let (p, target) = self.parse_import_expr();
            import_path = p;
            path = target;
        } else {
            path.push(self.parse_type_ident());
            while matches!(self.peek().kind, TokenKind::Dot) {
                self.bump();
                path.push(self.parse_type_ident());
            }
        }
        let mut args = Vec::new();
        if matches!(self.peek().kind, TokenKind::LParen) {
            self.bump();
            loop {
                match self.peek().kind {
                    TokenKind::RParen => { self.bump(); break; }
                    TokenKind::Eof => break,
                    _ => {
                        args.push(self.parse_type_ref());
                        if matches!(self.peek().kind, TokenKind::Comma) { self.bump(); }
                    }
                }
            }
        }
        let end = self.prev_span();
        TypeRef { import_path, path, args, span: start.join(end) }
    }

    fn parse_type_ident(&mut self) -> Ident {
        if let Some(text) = type_keyword_text(&self.peek().kind) {
            let t = self.bump();
            return Ident { text: SmolStr::new(text), span: t.span };
        }
        self.parse_ident()
    }

    fn parse_enum(&mut self) -> EnumDecl {
        let start = self.bump().span; // 'enum'
        let name = self.parse_ident();
        let annotations = self.parse_annotations();
        self.expect(&TokenKind::LBrace, "enum body");
        let mut enumerants = Vec::new();
        while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
            if let TokenKind::Ident(_) = self.peek().kind {
                enumerants.push(self.parse_enumerant());
            } else {
                let span = self.peek().span;
                self.error(span, "expected enumerant");
                self.skip_to_semi_or_brace();
            }
        }
        self.expect(&TokenKind::RBrace, "enum body");
        let end = self.prev_span();
        EnumDecl { name, enumerants, annotations, span: start.join(end) }
    }

    fn parse_enumerant(&mut self) -> Enumerant {
        let start = self.peek().span;
        let name = self.parse_ident();
        let ordinal = if matches!(self.peek().kind, TokenKind::At) {
            Some(self.parse_ordinal())
        } else {
            None
        };
        let annotations = self.parse_annotations();
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "enumerant");
        Enumerant { name, ordinal, annotations, span: start.join(end) }
    }

    fn parse_interface(&mut self) -> Interface {
        let start = self.bump().span; // 'interface'
        let name = self.parse_ident();
        let type_params = self.parse_type_params();
        let mut superclasses = Vec::new();
        if matches!(self.peek().kind, TokenKind::KwExtends) {
            self.bump();
            if matches!(self.peek().kind, TokenKind::LParen) {
                self.bump();
                loop {
                    match self.peek().kind {
                        TokenKind::RParen => { self.bump(); break; }
                        TokenKind::Eof => break,
                        _ => {
                            superclasses.push(self.parse_type_ref());
                            if matches!(self.peek().kind, TokenKind::Comma) { self.bump(); }
                        }
                    }
                }
            }
        }
        let annotations = self.parse_annotations();
        self.expect(&TokenKind::LBrace, "interface body");
        let mut methods = Vec::new();
        let mut nested = Vec::new();
        while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
            match self.peek().kind {
                TokenKind::KwStruct => nested.push(StructMember::Struct(self.parse_struct())),
                TokenKind::KwEnum => nested.push(StructMember::Enum(self.parse_enum())),
                TokenKind::KwInterface => nested.push(StructMember::Interface(self.parse_interface())),
                TokenKind::KwConst => nested.push(StructMember::Const(self.parse_const())),
                TokenKind::KwAnnotation => nested.push(StructMember::Annotation(self.parse_annotation_decl())),
                TokenKind::KwUsing => nested.push(StructMember::Using(self.parse_using())),
                TokenKind::Ident(_) => methods.push(self.parse_method()),
                _ => {
                    let span = self.peek().span;
                    self.error(span, "expected method or nested declaration");
                    self.skip_to_semi_or_brace();
                }
            }
        }
        self.expect(&TokenKind::RBrace, "interface body");
        let end = self.prev_span();
        Interface { name, type_params, superclasses, methods, nested, annotations, span: start.join(end) }
    }

    fn parse_method(&mut self) -> Method {
        let start = self.peek().span;
        let name = self.parse_ident();
        let ordinal = if matches!(self.peek().kind, TokenKind::At) {
            Some(self.parse_ordinal())
        } else {
            None
        };
        let (params, params_span) = if matches!(self.peek().kind, TokenKind::LParen) {
            let (list, span) = self.parse_method_param_list();
            (Some(list), Some(span))
        } else {
            (None, None)
        };
        let (results, results_span) = if matches!(self.peek().kind, TokenKind::Arrow) {
            self.bump();
            if matches!(self.peek().kind, TokenKind::LParen) {
                let (list, span) = self.parse_method_param_list();
                (Some(list), Some(span))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };
        let annotations = self.parse_annotations();
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "method");
        Method {
            name,
            ordinal,
            params,
            params_span,
            results,
            results_span,
            annotations,
            span: start.join(end),
        }
    }

    /// Parse `(name :Type [= default], …)` starting at the `(`. Returns the
    /// parsed list plus the whole-parens span.
    fn parse_method_param_list(&mut self) -> (Vec<MethodParam>, ByteSpan) {
        let lparen = self.bump().span; // '('
        let mut out = Vec::new();
        let mut last = lparen;
        loop {
            match self.peek().kind {
                TokenKind::RParen => {
                    last = self.peek().span;
                    self.bump();
                    break;
                }
                TokenKind::Eof => break,
                TokenKind::Comma => {
                    self.bump();
                    continue;
                }
                _ => {
                    if let Some(p) = self.parse_method_param() {
                        last = p.span;
                        out.push(p);
                    } else {
                        // Recovery: skip to next ',' or ')'.
                        while !self.at_eof()
                            && !matches!(self.peek().kind, TokenKind::Comma | TokenKind::RParen)
                        {
                            self.bump();
                        }
                    }
                }
            }
        }
        (out, lparen.join(last))
    }

    fn parse_method_param(&mut self) -> Option<MethodParam> {
        let start = self.peek().span;
        if !matches!(self.peek().kind, TokenKind::Ident(_)) {
            let span = self.peek().span;
            self.error(span, "expected parameter name");
            return None;
        }
        let name = self.parse_ident();
        if !matches!(self.peek().kind, TokenKind::Colon) {
            let span = self.peek().span;
            self.error(span, "expected ':' in parameter");
            return None;
        }
        self.bump(); // ':'
        let ty = self.parse_type_ref();
        let default_span = if matches!(self.peek().kind, TokenKind::Eq) {
            Some(self.skip_expr_to_end())
        } else {
            None
        };
        let annotations = self.parse_annotations();
        let end = self.prev_span();
        Some(MethodParam { name, ty, default_span, annotations, span: start.join(end) })
    }

    fn parse_const(&mut self) -> ConstDecl {
        let start = self.bump().span; // 'const'
        let name = self.parse_ident();
        self.expect(&TokenKind::Colon, "const");
        let ty = self.parse_type_ref();
        let value_span = if matches!(self.peek().kind, TokenKind::Eq) {
            Some(self.skip_expr_to_end())
        } else {
            None
        };
        let annotations = self.parse_annotations();
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "const");
        ConstDecl { name, ty, value_span, annotations, span: start.join(end) }
    }

    fn parse_annotation_decl(&mut self) -> AnnotationDecl {
        let start = self.bump().span; // 'annotation'
        let name = self.parse_ident();
        let targets_span = if matches!(self.peek().kind, TokenKind::LParen) {
            Some(self.skip_parenthesised())
        } else {
            None
        };
        let ty = if matches!(self.peek().kind, TokenKind::Colon) {
            self.bump();
            Some(self.parse_type_ref())
        } else {
            None
        };
        let annotations = self.parse_annotations();
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "annotation declaration");
        AnnotationDecl { name, targets_span, ty, annotations, span: start.join(end) }
    }

    fn prev_span(&self) -> ByteSpan {
        if self.pos == 0 {
            self.tokens[0].span
        } else {
            self.tokens[self.pos - 1].span
        }
    }
}

fn describe(k: &TokenKind) -> &'static str {
    match k {
        TokenKind::LBrace => "'{'",
        TokenKind::RBrace => "'}'",
        TokenKind::LParen => "'('",
        TokenKind::RParen => "')'",
        TokenKind::Semi => "';'",
        TokenKind::Colon => "':'",
        TokenKind::Eq => "'='",
        TokenKind::At => "'@'",
        _ => "token",
    }
}

fn type_keyword_text(k: &TokenKind) -> Option<&'static str> {
    Some(match k {
        TokenKind::TyVoid => "Void",
        TokenKind::TyBool => "Bool",
        TokenKind::TyInt8 => "Int8",
        TokenKind::TyInt16 => "Int16",
        TokenKind::TyInt32 => "Int32",
        TokenKind::TyInt64 => "Int64",
        TokenKind::TyUInt8 => "UInt8",
        TokenKind::TyUInt16 => "UInt16",
        TokenKind::TyUInt32 => "UInt32",
        TokenKind::TyUInt64 => "UInt64",
        TokenKind::TyFloat32 => "Float32",
        TokenKind::TyFloat64 => "Float64",
        TokenKind::TyText => "Text",
        TokenKind::TyData => "Data",
        TokenKind::TyList => "List",
        TokenKind::TyAnyPointer => "AnyPointer",
        TokenKind::TyCapability => "Capability",
        _ => return None,
    })
}

fn parse_int(s: &str) -> Option<u32> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_addressbook() {
        let src = r#"
@0x9eb32e19f86ee174;

using Cxx = import "/capnp/c++.capnp";
$Cxx.namespace("addressbook");

struct Person {
  id @0 :UInt32;
  name @1 :Text;
  phones @2 :List(PhoneNumber);

  struct PhoneNumber {
    number @0 :Text;
  }

  employment :union {
    unemployed @3 :Void;
    employer @4 :Text;
  }
}

enum Color { red @0; green @1; }
"#;
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        assert!(r.file.file_id.is_some());
        assert_eq!(r.file.decls.len(), 4); // using, $annotation, struct, enum
    }

    #[test]
    fn parses_interface() {
        let src = r#"
@0x1;
interface Greeter {
  hello @0 (name :Text) -> (greeting :Text);
}
"#;
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        let iface = match &r.file.decls[0] {
            Decl::Interface(i) => i,
            _ => panic!("expected interface"),
        };
        assert_eq!(iface.methods.len(), 1);
        assert_eq!(iface.methods[0].name.text.as_str(), "hello");
    }

    #[test]
    fn recovers_on_bad_member() {
        let src = "@0x1; struct S { ??? id @0 :UInt32; }";
        let r = parse(src);
        // Parser should still surface the struct even though it flagged an error.
        assert!(!r.errors.is_empty());
        assert_eq!(r.file.decls.len(), 1);
    }

    #[test]
    fn parses_inline_import_in_type_ref() {
        let src = r#"@0x1; struct S { f @0 :import "other.capnp".Foo; }"#;
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        let s = match &r.file.decls[0] {
            Decl::Struct(s) => s,
            _ => panic!("expected struct"),
        };
        let field = match &s.members[0] {
            StructMember::Field(f) => f,
            _ => panic!("expected field"),
        };
        let ty = match &field.body {
            FieldBody::Slot { ty, .. } => ty,
            _ => panic!("expected slot"),
        };
        assert_eq!(ty.import_path.as_ref().map(|p| p.value.as_str()), Some("other.capnp"));
        assert_eq!(ty.path.len(), 1);
        assert_eq!(ty.path[0].text.as_str(), "Foo");
    }

    #[test]
    fn parses_using_with_import_type_rhs() {
        let src = r#"@0x1; using Foo = import "other.capnp".Foo.Bar;"#;
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        let u = match &r.file.decls[0] {
            Decl::Using(u) => u,
            _ => panic!("expected using"),
        };
        assert_eq!(u.name.as_ref().map(|i| i.text.as_str()), Some("Foo"));
        assert_eq!(u.import_path.as_ref().map(|p| p.value.as_str()), Some("other.capnp"));
        let target: Vec<&str> = u.import_target.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(target, vec!["Foo", "Bar"]);
    }

    #[test]
    fn parses_using_import_shorthand() {
        let src = r#"@0x1; using import "types.capnp".Tag;"#;
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        let u = match &r.file.decls[0] {
            Decl::Using(u) => u,
            _ => panic!("expected using"),
        };
        // No explicit alias; last path component becomes the implicit name.
        assert_eq!(u.name.as_ref().map(|i| i.text.as_str()), Some("Tag"));
        assert_eq!(u.import_path.as_ref().map(|p| p.value.as_str()), Some("types.capnp"));
        let target: Vec<&str> = u.import_target.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(target, vec!["Tag"]);
    }
}
