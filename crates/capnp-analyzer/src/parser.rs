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

/// Deepest allowed nesting of declarations (struct / group / union / enum /
/// interface bodies) and type-argument lists, counted together. Real schemas
/// stay in single digits; capnp's own `schema.capnp` nests about 4 deep.
/// Every later pass (resolve, symbols, diagnostics, even `Drop`) walks the
/// AST recursively, so this bounds their stack use as well as the parser's.
/// Unguarded, the full pipeline in a debug build (whose frames are larger
/// than the release WASM ones) overflowed a 1 MB stack at about 209 nested
/// groups, the costliest construct, so 64 leaves over 3x headroom there and
/// more in WASM. The `deep_nesting_*` tests check this on a 1 MB stack.
pub const MAX_NESTING: usize = 64;

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
    /// Current nesting of guarded constructs; see [`MAX_NESTING`].
    depth: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0, errors: Vec::new(), depth: 0 }
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

    /// Enter one level of a nested construct whose opening token is at
    /// `open`. Past [`MAX_NESTING`] this reports the error and returns
    /// `false`; the caller must then skip the construct without recursing
    /// (and not call [`leave_nesting`](Self::leave_nesting)).
    fn enter_nesting(&mut self, open: ByteSpan) -> bool {
        if self.depth >= MAX_NESTING {
            self.error(open, format!("nesting too deep (limit {})", MAX_NESTING));
            return false;
        }
        self.depth += 1;
        true
    }

    fn leave_nesting(&mut self) {
        self.depth -= 1;
    }

    /// Skip the rest of a too-deep declaration: everything up to and
    /// including its `{ … }` body, or up to a `;` when it has no body.
    /// Stops before a close bracket that belongs to the enclosing
    /// construct. Iterative, so input depth can't exhaust the stack.
    fn skip_block(&mut self) {
        let mut depth = 0usize;
        while !self.at_eof() {
            match self.peek().kind {
                TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => { depth += 1; self.bump(); }
                TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                    if depth == 0 { return; }
                    depth -= 1;
                    let closed_body = matches!(self.bump().kind, TokenKind::RBrace);
                    if depth == 0 && closed_body { return; }
                }
                TokenKind::Semi if depth == 0 => { self.bump(); return; }
                _ => { self.bump(); }
            }
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
            let before = self.pos;
            match self.parse_top_decl() {
                Some(d) => decls.push(d),
                None => {
                    let bad = self.peek().span;
                    self.error(bad, "expected top-level declaration");
                    self.skip_to_semi_or_brace();
                }
            }
            // Forward-progress guard: a stray `}` (or any token a sub-parser
            // decided not to consume) must not pin the top-level loop.
            if self.pos == before {
                self.bump();
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
            return Using {
                name,
                import_path,
                import_target,
                target: Vec::new(),
                span: start.join(end),
            };
        }

        let mut name = None;
        if matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::KwStream) {
            name = Some(self.parse_ident());
            // Optional `= ...`
            if matches!(self.peek().kind, TokenKind::Eq) {
                self.bump();
            }
        }

        let mut import_path = None;
        let mut import_target = Vec::new();
        let mut target = Vec::new();
        if matches!(self.peek().kind, TokenKind::KwImport) {
            let (p, t) = self.parse_import_expr();
            import_path = p;
            import_target = t;
        } else {
            // Right-hand side is a type reference. Keep the dotted path so
            // the resolver can expand the alias; skip anything after it
            // (e.g. generic args in `using M = List(Int16);`) up to `;`.
            if type_keyword_text(&self.peek().kind).is_some()
                || matches!(self.peek().kind, TokenKind::Ident(_))
            {
                target.push(self.parse_type_ident());
                while matches!(self.peek().kind, TokenKind::Dot) {
                    self.bump();
                    target.push(self.parse_type_ident());
                }
            }
            while !self.at_eof() && !matches!(self.peek().kind, TokenKind::Semi) {
                self.bump();
            }
        }
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "using");
        Using { name, import_path, import_target, target, span: start.join(end) }
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
                TokenKind::Ident(_) | TokenKind::KwStream => {
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
        if !self.enter_nesting(start) {
            // Too deep: keep the name (so the symbol still exists) and skip
            // the rest of the declaration.
            self.skip_block();
            let span = start.join(self.prev_span());
            return Struct { name, type_params: Vec::new(), members: Vec::new(), annotations: Vec::new(), span };
        }
        let type_params = self.parse_type_params();
        let annotations = self.parse_annotations();
        let members = self.parse_struct_body();
        self.leave_nesting();
        let end = self.prev_span();
        Struct { name, type_params, members, annotations, span: start.join(end) }
    }

    fn parse_struct_body(&mut self) -> Vec<StructMember> {
        let mut members = Vec::new();
        if self.expect(&TokenKind::LBrace, "struct body").is_none() {
            return members;
        }
        while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
            let before = self.pos;
            match self.parse_struct_member() {
                Some(m) => members.push(m),
                None => {
                    self.skip_to_semi_or_brace();
                }
            }
            if self.pos == before {
                self.bump();
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
            TokenKind::Ident(_) | TokenKind::KwStream => {
                Some(StructMember::Field(self.parse_field()))
            }
            _ => {
                let span = self.peek().span;
                self.error(span, "expected struct member");
                None
            }
        }
    }

    fn parse_anon_union(&mut self) -> UnionBlock {
        let start = self.bump().span; // 'union'
        if !self.enter_nesting(start) {
            self.skip_block();
            return UnionBlock { members: Vec::new(), span: start.join(self.prev_span()) };
        }
        self.expect(&TokenKind::LBrace, "union body");
        let mut members = Vec::new();
        while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
            let before = self.pos;
            if matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::KwStream) {
                members.push(self.parse_field());
            } else {
                let span = self.peek().span;
                self.error(span, "expected field in union");
                self.skip_to_semi_or_brace();
            }
            if self.pos == before {
                self.bump();
            }
        }
        self.expect(&TokenKind::RBrace, "union body");
        self.leave_nesting();
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
        if !self.enter_nesting(start) {
            self.skip_block();
            return GroupBlock { members: Vec::new(), span: start.join(self.prev_span()) };
        }
        let members = self.parse_struct_body();
        self.leave_nesting();
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
            if !self.enter_nesting(self.peek().span) {
                self.skip_parenthesised();
                let end = self.prev_span();
                return TypeRef { import_path, path, args, span: start.join(end) };
            }
            self.bump();
            loop {
                match self.peek().kind {
                    TokenKind::RParen => { self.bump(); break; }
                    TokenKind::Eof => break,
                    _ => {
                        let before = self.pos;
                        args.push(self.parse_type_ref());
                        if matches!(self.peek().kind, TokenKind::Comma) { self.bump(); }
                        if self.pos == before {
                            self.bump();
                        }
                    }
                }
            }
            self.leave_nesting();
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
        if !self.enter_nesting(start) {
            self.skip_block();
            let span = start.join(self.prev_span());
            return EnumDecl { name, enumerants: Vec::new(), annotations: Vec::new(), span };
        }
        let annotations = self.parse_annotations();
        self.expect(&TokenKind::LBrace, "enum body");
        let mut enumerants = Vec::new();
        while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
            let before = self.pos;
            if matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::KwStream) {
                enumerants.push(self.parse_enumerant());
            } else {
                let span = self.peek().span;
                self.error(span, "expected enumerant");
                self.skip_to_semi_or_brace();
            }
            if self.pos == before {
                self.bump();
            }
        }
        self.expect(&TokenKind::RBrace, "enum body");
        self.leave_nesting();
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
        if !self.enter_nesting(start) {
            self.skip_block();
            let span = start.join(self.prev_span());
            return Interface {
                name,
                type_params: Vec::new(),
                superclasses: Vec::new(),
                methods: Vec::new(),
                nested: Vec::new(),
                annotations: Vec::new(),
                span,
            };
        }
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
                            let before = self.pos;
                            superclasses.push(self.parse_type_ref());
                            if matches!(self.peek().kind, TokenKind::Comma) { self.bump(); }
                            if self.pos == before {
                                self.bump();
                            }
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
            let before = self.pos;
            match self.peek().kind {
                TokenKind::KwStruct => nested.push(StructMember::Struct(self.parse_struct())),
                TokenKind::KwEnum => nested.push(StructMember::Enum(self.parse_enum())),
                TokenKind::KwInterface => nested.push(StructMember::Interface(self.parse_interface())),
                TokenKind::KwConst => nested.push(StructMember::Const(self.parse_const())),
                TokenKind::KwAnnotation => nested.push(StructMember::Annotation(self.parse_annotation_decl())),
                TokenKind::KwUsing => nested.push(StructMember::Using(self.parse_using())),
                TokenKind::Ident(_) | TokenKind::KwStream => {
                    methods.push(self.parse_method())
                }
                _ => {
                    let span = self.peek().span;
                    self.error(span, "expected method or nested declaration");
                    self.skip_to_semi_or_brace();
                }
            }
            if self.pos == before {
                self.bump();
            }
        }
        self.expect(&TokenKind::RBrace, "interface body");
        self.leave_nesting();
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
        let mut streaming = false;
        let (results, results_span) = if matches!(self.peek().kind, TokenKind::Arrow) {
            self.bump();
            if matches!(self.peek().kind, TokenKind::LParen) {
                let (list, span) = self.parse_method_param_list();
                (Some(list), Some(span))
            } else if matches!(self.peek().kind, TokenKind::KwStream) {
                self.bump();
                streaming = true;
                (None, None)
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
            streaming,
            annotations,
            span: start.join(end),
        }
    }

    /// Parse `(name :Type [= default], …)` starting at the `(`. Returns the
    /// parsed list plus the whole-parens span. Recovers at the first `)`,
    /// `{`/`}` (we've escaped the enclosing body), or `;`, so a malformed
    /// param list can't eat tokens that belong to the enclosing declaration.
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
                TokenKind::Eof
                | TokenKind::LBrace
                | TokenKind::RBrace
                | TokenKind::Semi => break,
                TokenKind::Comma => {
                    self.bump();
                    continue;
                }
                _ => {
                    let before = self.pos;
                    if let Some(p) = self.parse_method_param() {
                        last = p.span;
                        out.push(p);
                    } else {
                        // Recovery: skip to the next boundary without
                        // crossing the enclosing body.
                        while !self.at_eof()
                            && !matches!(
                                self.peek().kind,
                                TokenKind::Comma
                                    | TokenKind::RParen
                                    | TokenKind::LBrace
                                    | TokenKind::RBrace
                                    | TokenKind::Semi
                            )
                        {
                            self.bump();
                        }
                    }
                    // Guarantee forward progress even if a sub-parser failed
                    // to advance (e.g. empty type ref at EOF).
                    if self.pos == before {
                        self.bump();
                    }
                }
            }
        }
        (out, lparen.join(last))
    }

    fn parse_method_param(&mut self) -> Option<MethodParam> {
        let start = self.peek().span;
        if !matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::KwStream) {
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
        // `stream` is a contextual keyword: it only acts as a keyword in
        // `-> stream;` position. Anywhere else (field/method/param names)
        // it must be usable as a plain identifier.
        TokenKind::KwStream => "stream",
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
    fn terminates_on_user_scenario() {
        let src = r#"@0xa1;
using import "file-b.capnp".WorkerInfo;
interface Admin {
    info @1 () -> (info: WorkerInfo);
}
"#;
        let _ = parse(src); // must not hang
    }

    #[test]
    fn terminates_on_half_typed_forms() {
        // Each of these fragments has been observed or is plausible while
        // the user is mid-edit. None should make the parser spin.
        let snippets = [
            "@0x1; using import",
            "@0x1; using import \"",
            "@0x1; using import \"x.capnp\"",
            "@0x1; using import \"x.capnp\".",
            "@0x1; using import \"x.capnp\".Foo",
            "@0x1; struct S { f @0 :import; }",
            "@0x1; struct S { f @0 :import \"x.capnp\"; }",
            "@0x1; struct S { f @0 :import \"x.capnp\".; }",
            "@0x1; interface I { foo @0 (",
            "@0x1; interface I { foo @0 () ->",
            "@0x1; interface I { foo @0 () -> (",
            "@0x1; interface I { foo @0 () -> (info :",
            "@0x1; interface I { foo @0 () -> (info :Foo",
            "@0x1; interface I { foo @0 () -> Text; }",
            "@0x1; interface I { foo @0 (a :Text, b :Int32 = 5) -> (r :Text); }",
            "@0x1; interface I { foo @0 (a :import \"x\".T) -> (r :import \"y\".U); }",
            "@0x1; }",
            "@0x1; struct S { }  }",
            "@0x1; interface I { foo @0 () -> (info :\n}",
            "@0x1; interface I { foo @0 () -> (info :WorkerInfo\n}",
            // Stray `)` inside a body — `skip_to_semi_or_brace` returns
            // without advancing at `)` at depth 0, so each of these used to
            // pin the enclosing loop forever.
            "@0x1; struct S { f @0 :Text; ) }",
            "@0x1; interface I { ) }",
            "@0x1; union U { ) }",
            "@0x1; enum E { ) }",
            "@0x1; struct S { f @0 :List(@0); }",
            "@0x1; interface I extends (@) { foo @0 () -> (); }",
        ];
        for s in snippets {
            let _ = parse(s);
        }
    }

    #[test]
    fn stream_usable_as_field_name() {
        // `stream` is a contextual keyword: it should still parse cleanly as
        // an ordinary identifier when it shows up as a field/param name.
        let src = "@0x1; struct S { stream @0 :Text; }";
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
    }

    #[test]
    fn parses_streaming_method() {
        let src = r#"@0x1;
interface Sink {
    write @0 (chunk :Data) -> stream;
    other @1 () -> (n :UInt32);
}
"#;
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        let iface = match &r.file.decls[0] {
            Decl::Interface(i) => i,
            _ => panic!("expected interface"),
        };
        assert_eq!(iface.methods.len(), 2);
        let write = &iface.methods[0];
        assert_eq!(write.name.text.as_str(), "write");
        assert!(write.streaming, "write should be streaming");
        assert!(write.results.is_none());
        assert!(write.results_span.is_none());
        let other = &iface.methods[1];
        assert!(!other.streaming);
        assert!(other.results.is_some());
    }

    #[test]
    fn parses_streaming_method_with_annotation() {
        let src = r#"@0x1;
interface Sink {
    write @0 (chunk :Data) -> stream $foo;
}
"#;
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        let iface = match &r.file.decls[0] {
            Decl::Interface(i) => i,
            _ => panic!("expected interface"),
        };
        let m = &iface.methods[0];
        assert!(m.streaming);
        assert_eq!(m.annotations.len(), 1);
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

    // ---- nesting-depth limit ------------------------------------------
    //
    // Each case nests one construct on line 1 (line 0 is the file id) and
    // puts a top-level `After` struct on line 2. The whole WASM pipeline
    // then runs on a 1 MB stack, the size the WASM build gets. Tests run in
    // debug, whose frames are bigger than release / WASM ones, so passing
    // here is the conservative check.

    const DEEP: usize = 100_000;

    struct NestCase {
        /// Builds line 1 holding `levels` guarded constructs in total.
        build: fn(usize) -> String,
        /// Text of each guarded opening token on line 1, plus the offset of
        /// the token within that text; used to find the one past the limit.
        markers: &'static [(&'static str, usize)],
        /// Each declaration level holds one member named `x` after its
        /// nested child. Counting them proves parsing carried on inside the
        /// enclosing body after the too-deep construct was skipped.
        has_x: bool,
    }

    fn nest(prefix: &str, open: &str, core: &str, close: &str, suffix: &str, n: usize) -> String {
        let mut s = String::with_capacity(prefix.len() + core.len() + suffix.len() + n * (open.len() + close.len()));
        s.push_str(prefix);
        for _ in 0..n { s.push_str(open); }
        s.push_str(core);
        for _ in 0..n { s.push_str(close); }
        s.push_str(suffix);
        s
    }

    /// Run every WASM entry point over `line` on a 1 MB stack: update
    /// (lex, parse, per-file checks, import graph), diagnostics, symbols,
    /// folding, workspace symbols, and the cursor queries (each of which
    /// rebuilds the workspace index), then drop it all. Returns the
    /// diagnostics and document-symbol JSON.
    fn run_pipeline(line: String) -> (String, String) {
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(move || {
                use crate::wasm_api::Analyzer;
                let uri = "file:///deep.capnp";
                let src = format!("@0x1;\n{}\nstruct After {{ tail :Text; }}\n", line);
                let a = Analyzer::new();
                a.update_file(uri, &src);
                let diags = a.diagnostics(uri);
                let symbols = a.document_symbols(uri);
                a.folding_ranges(uri);
                a.workspace_symbols("");
                let len = line.len() as u32;
                for (l, c) in [(1, 0), (1, len / 2), (1, len.saturating_sub(2)), (2, 8)] {
                    a.hover(uri, l, c);
                    a.definition(uri, l, c);
                    a.completion(uri, l, c);
                }
                (diags, symbols)
            })
            .unwrap()
            .join()
            .expect("pipeline panicked on a 1 MB stack")
    }

    fn check_nesting(case: &NestCase, levels: usize) {
        let line = (case.build)(levels);
        let expected_col = (levels > MAX_NESTING).then(|| {
            let mut opens: Vec<usize> = case
                .markers
                .iter()
                .flat_map(|&(m, k)| line.match_indices(m).map(move |(i, _)| i + k))
                .collect();
            opens.sort_unstable();
            opens[MAX_NESTING]
        });
        let (diags, symbols) = run_pipeline(line);

        let diags: Vec<serde_json::Value> = serde_json::from_str(&diags).unwrap();
        let parse_errors: Vec<&serde_json::Value> =
            diags.iter().filter(|d| d["code"] == "CAPNP0001").collect();
        match expected_col {
            None => assert!(parse_errors.is_empty(), "levels {}: {:?}", levels, parse_errors),
            Some(col) => {
                // Exactly one error: the skip consumed the whole construct
                // and nothing around it.
                assert_eq!(parse_errors.len(), 1, "levels {}: {:?}", levels, parse_errors);
                let e = parse_errors[0];
                assert_eq!(e["message"], format!("nesting too deep (limit {})", MAX_NESTING));
                assert_eq!(e["start"]["line"], 1);
                assert_eq!(e["start"]["col"], col as u64, "levels {}", levels);
            }
        }
        assert!(symbols.contains(r#""name":"After""#), "levels {}: After missing", levels);
        assert!(symbols.contains(r#""name":"tail""#), "levels {}: After.tail missing", levels);
        if case.has_x {
            let xs = symbols.matches(r#""name":"x""#).count();
            assert_eq!(xs, levels.min(MAX_NESTING), "levels {}", levels);
        }
    }

    fn check_nesting_case(case: &NestCase) {
        for levels in [MAX_NESTING, MAX_NESTING + 1, DEEP] {
            check_nesting(case, levels);
        }
    }

    #[test]
    fn deep_nesting_struct() {
        check_nesting_case(&NestCase {
            build: |n| nest("", "struct S { ", "", "x :Text; } ", "", n),
            markers: &[("struct", 0)],
            has_x: true,
        });
    }

    #[test]
    fn deep_nesting_group() {
        check_nesting_case(&NestCase {
            build: |n| nest("struct T { ", "g :group { ", "", "x :Text; } ", "x :Text; } ", n - 1),
            markers: &[("struct", 0), ("group", 0)],
            has_x: true,
        });
    }

    #[test]
    fn deep_nesting_union() {
        check_nesting_case(&NestCase {
            build: |n| nest("struct T { ", "u :union { ", "", "x :Text; } ", "x :Text; } ", n - 1),
            markers: &[("struct", 0), ("union", 0)],
            has_x: true,
        });
    }

    #[test]
    fn deep_nesting_enum() {
        // Enums can't nest in each other, so the enum is the innermost
        // level under a stack of structs.
        check_nesting_case(&NestCase {
            build: |n| nest("", "struct S { ", "enum E { x; } ", "x :Text; } ", "", n - 1),
            markers: &[("struct", 0), ("enum", 0)],
            has_x: true,
        });
    }

    #[test]
    fn deep_nesting_interface() {
        check_nesting_case(&NestCase {
            build: |n| nest("", "interface I { ", "", "x (); } ", "", n),
            markers: &[("interface", 0)],
            has_x: true,
        });
    }

    #[test]
    fn deep_nesting_list_type() {
        check_nesting_case(&NestCase {
            build: |n| nest("const c :", "List(", "Int32", ")", "; ", n),
            markers: &[("(", 0)],
            has_x: false,
        });
    }

    #[test]
    fn deep_nesting_generic_type_args() {
        check_nesting_case(&NestCase {
            build: |n| nest("const c :", "Map(Text, ", "Text", ")", "; ", n),
            markers: &[("(", 0)],
            has_x: false,
        });
    }

    #[test]
    fn deep_nesting_counts_structs_and_types_together() {
        // One counter covers every construct: a type nested inside a struct
        // gets only the levels the struct left over.
        check_nesting_case(&NestCase {
            build: |n| {
                let field = nest("f :", "List(", "Text", ")", "; ", n / 2);
                nest("", "struct S { ", &field, "x :Text; } ", "", n - n / 2)
            },
            markers: &[("struct", 0), ("(", 0)],
            has_x: false,
        });
    }

    #[test]
    fn deep_nesting_superclass_type() {
        check_nesting_case(&NestCase {
            build: |n| nest("interface I extends (", "List(", "Text", ")", ") { } ", n - 1),
            markers: &[("interface", 0), ("List(", 4)],
            has_x: false,
        });
    }

    #[test]
    fn deep_nesting_method_param_type() {
        check_nesting_case(&NestCase {
            build: |n| nest("interface I { m (a :", "List(", "Text", ")", ") -> (); } ", n - 1),
            markers: &[("interface", 0), ("List(", 4)],
            has_x: false,
        });
    }

    #[test]
    fn deep_values_are_skipped_iteratively() {
        // Values, annotation arguments and `using` right-hand sides are kept
        // as token spans and skipped by iterative depth-counting loops, so
        // they have no limit: deep ones are neither an error nor a crash.
        let builds: [fn(usize) -> String; 6] = [
            |n| nest("const c :Int32 = ", "(", "1", ")", "; ", n),
            |n| nest("const c :List(Int32) = ", "[", "1", "]", "; ", n),
            |n| nest("struct T { f :T = ", "(a = ", "1", ")", "; } ", n),
            |n| nest("interface I { m (a :Int32 = ", "(", "1", ")", ") -> (); } ", n),
            |n| nest("struct T $ann", "(", "1", ")", " { } ", n),
            |n| nest("using M = ", "List(", "Text", ")", "; ", n),
        ];
        for build in builds {
            for n in [MAX_NESTING + 1, DEEP] {
                let (diags, symbols) = run_pipeline(build(n));
                assert!(!diags.contains("CAPNP0001"), "depth {}: {}", n, diags);
                assert!(symbols.contains(r#""name":"After""#), "depth {}", n);
            }
        }
    }
}
