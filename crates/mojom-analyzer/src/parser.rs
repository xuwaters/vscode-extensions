//! Recursive-descent parser for Mojom files.
//!
//! Recovery model: on an unexpected token the parser records a
//! [`ParseError`] and skips forward to the next `;` or matching `}` so the
//! outline can still be built for the rest of the file. This matches the
//! behaviour of the sibling capnp / proto3 parsers.
//!
//! Scope note: this pass captures declaration structure (names, ordinals,
//! type references, nesting) — enough for diagnostics, document outlines and
//! name resolution. Value expressions (const/default/enum-value initialisers)
//! are preserved as token-range spans rather than parsed into value nodes.

use crate::ast::*;
use crate::lexer::{lex, Token, TokenKind};
use crate::spans::ByteSpan;
use smol_str::SmolStr;

/// Deepest `array<…>` / `map<…, …>` nesting the parser descends into. The
/// type grammar is the parser's only recursion (declarations, attributes and
/// values are walked by iterative skip loops, and `TypeRef` is flat), so this
/// bounds the whole pipeline's stack use. Real files nest 2–3 deep. On a 1 MB
/// stack, the unguarded parser overflowed at ~450 levels in debug and ~3000
/// in release, so 64 keeps a 7x margin even in debug.
pub const MAX_NESTING_DEPTH: u32 = 64;

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
    /// Current generic-type nesting; see [`MAX_NESTING_DEPTH`].
    depth: u32,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0, errors: Vec::new(), depth: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
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

    fn prev_span(&self) -> ByteSpan {
        if self.pos == 0 {
            self.tokens[0].span
        } else {
            self.tokens[self.pos - 1].span
        }
    }

    fn skip_to_semi_or_brace(&mut self) {
        let mut depth = 0i32;
        while !self.at_eof() {
            match self.peek().kind {
                TokenKind::LBrace | TokenKind::LParen => { depth += 1; self.bump(); }
                TokenKind::RBrace | TokenKind::RParen => {
                    if depth == 0 { return; }
                    depth -= 1;
                    self.bump();
                }
                TokenKind::Semi if depth == 0 => { self.bump(); return; }
                _ => { self.bump(); }
            }
        }
    }

    /// Skip a leading `[ … ]` attribute section (or several). Attributes are
    /// not modelled in the AST; we only need to step over them.
    fn skip_attributes(&mut self) {
        while matches!(self.peek().kind, TokenKind::LBracket) {
            let mut depth = 0i32;
            while !self.at_eof() {
                match self.peek().kind {
                    TokenKind::LBracket => { depth += 1; self.bump(); }
                    TokenKind::RBracket => {
                        depth -= 1;
                        self.bump();
                        if depth == 0 { break; }
                    }
                    _ => { self.bump(); }
                }
            }
        }
    }

    fn parse_file(&mut self) -> File {
        let start = self.peek().span.start;
        let mut module = None;
        let mut imports = Vec::new();
        let mut decls = Vec::new();

        while !self.at_eof() {
            let before = self.pos;
            self.skip_attributes();
            match self.peek().kind {
                TokenKind::KwModule => {
                    let m = self.parse_module();
                    if module.is_none() {
                        module = Some(m);
                    }
                }
                TokenKind::KwImport => imports.push(self.parse_import()),
                TokenKind::KwStruct => decls.push(Decl::Struct(self.parse_struct())),
                TokenKind::KwUnion => decls.push(Decl::Union(self.parse_union())),
                TokenKind::KwInterface => decls.push(Decl::Interface(self.parse_interface())),
                TokenKind::KwEnum => decls.push(Decl::Enum(self.parse_enum())),
                TokenKind::KwConst => decls.push(Decl::Const(self.parse_const())),
                TokenKind::Eof => break,
                _ => {
                    let bad = self.peek().span;
                    self.error(bad, "expected top-level declaration");
                    self.skip_to_semi_or_brace();
                }
            }
            // Forward-progress guard.
            if self.pos == before {
                self.bump();
            }
        }

        let end = self.tokens.last().map(|t| t.span.end).unwrap_or(start);
        File { module, imports, decls, span: ByteSpan::new(start, end) }
    }

    fn parse_module(&mut self) -> Module {
        let start = self.bump().span; // 'module'
        let (name, name_span) = self.parse_dotted_name();
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "module declaration");
        Module { name, name_span, span: start.join(end) }
    }

    fn parse_import(&mut self) -> Import {
        let start = self.bump().span; // 'import'
        let path = self.parse_string_lit("import");
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "import declaration");
        Import { path, span: start.join(end) }
    }

    fn parse_string_lit(&mut self, ctx: &str) -> StringLit {
        if let TokenKind::StringLit(_) = self.peek().kind {
            let t = self.bump();
            let value = if let TokenKind::StringLit(s) = t.kind { s } else { String::new() };
            StringLit { value, span: t.span }
        } else {
            let span = self.peek().span;
            self.error(span, format!("expected string literal in {}", ctx));
            StringLit { value: String::new(), span }
        }
    }

    fn parse_dotted_name(&mut self) -> (SmolStr, ByteSpan) {
        let start = self.peek().span;
        let mut text = String::new();
        if let TokenKind::Ident(s) = self.peek().kind.clone() {
            text.push_str(&s);
            self.bump();
        } else {
            let span = self.peek().span;
            self.error(span, "expected name");
            return (SmolStr::default(), span);
        }
        while matches!(self.peek().kind, TokenKind::Dot) {
            self.bump();
            text.push('.');
            if let TokenKind::Ident(s) = self.peek().kind.clone() {
                text.push_str(&s);
                self.bump();
            } else {
                break;
            }
        }
        (SmolStr::new(text), start.join(self.prev_span()))
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
        let span = self.peek().span;
        self.error(span, "expected identifier");
        Ident { text: SmolStr::default(), span }
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

    /// Consume `= <expr>` and return the span of the whole `= …` run. Stops
    /// at a `;`, or a top-level `,` / `)` / `}` relative to the nesting we
    /// enter here.
    fn skip_value(&mut self) -> ByteSpan {
        let start = self.bump().span; // '='
        let mut depth = 0i32;
        let mut last = start;
        while !self.at_eof() {
            match self.peek().kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace | TokenKind::Langle => {
                    depth += 1;
                    last = self.peek().span;
                    self.bump();
                }
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace | TokenKind::Rangle => {
                    if depth == 0 { break; }
                    depth -= 1;
                    last = self.peek().span;
                    self.bump();
                }
                TokenKind::Semi if depth == 0 => break,
                TokenKind::Comma if depth == 0 => break,
                _ => { last = self.peek().span; self.bump(); }
            }
        }
        start.join(last)
    }

    // ── Declarations ──────────────────────────────────────────────────

    fn parse_struct(&mut self) -> Struct {
        let start = self.bump().span; // 'struct'
        let name = self.parse_ident();
        let mut members = Vec::new();
        if matches!(self.peek().kind, TokenKind::LBrace) {
            self.bump();
            while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
                let before = self.pos;
                self.skip_attributes();
                match self.parse_struct_member() {
                    Some(m) => members.push(m),
                    None => self.skip_to_semi_or_brace(),
                }
                if self.pos == before {
                    self.bump();
                }
            }
            self.expect(&TokenKind::RBrace, "struct body");
        }
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "struct declaration");
        Struct { name, members, span: start.join(end) }
    }

    fn parse_struct_member(&mut self) -> Option<StructMember> {
        match self.peek().kind {
            TokenKind::KwConst => Some(StructMember::Const(self.parse_const())),
            TokenKind::KwEnum => Some(StructMember::Enum(self.parse_enum())),
            TokenKind::Ident(_) => Some(StructMember::Field(self.parse_field())),
            _ => {
                let span = self.peek().span;
                self.error(span, "expected struct field, const or enum");
                None
            }
        }
    }

    fn parse_field(&mut self) -> Field {
        let start = self.peek().span;
        let ty = self.parse_type();
        let name = self.parse_ident();
        let ordinal = if matches!(self.peek().kind, TokenKind::At) {
            Some(self.parse_ordinal())
        } else {
            None
        };
        let default_span = if matches!(self.peek().kind, TokenKind::Eq) {
            Some(self.skip_value())
        } else {
            None
        };
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "field");
        Field { ty, name, ordinal, default_span, span: start.join(end) }
    }

    fn parse_union(&mut self) -> Union {
        let start = self.bump().span; // 'union'
        let name = self.parse_ident();
        let mut fields = Vec::new();
        if self.expect(&TokenKind::LBrace, "union body").is_some() {
            while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
                let before = self.pos;
                self.skip_attributes();
                if matches!(self.peek().kind, TokenKind::Ident(_)) {
                    fields.push(self.parse_field());
                } else {
                    let span = self.peek().span;
                    self.error(span, "expected union field");
                    self.skip_to_semi_or_brace();
                }
                if self.pos == before {
                    self.bump();
                }
            }
            self.expect(&TokenKind::RBrace, "union body");
        }
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "union declaration");
        Union { name, fields, span: start.join(end) }
    }

    fn parse_interface(&mut self) -> Interface {
        let start = self.bump().span; // 'interface'
        let name = self.parse_ident();
        let mut members = Vec::new();
        if self.expect(&TokenKind::LBrace, "interface body").is_some() {
            while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
                let before = self.pos;
                self.skip_attributes();
                match self.peek().kind {
                    TokenKind::KwConst => members.push(InterfaceMember::Const(self.parse_const())),
                    TokenKind::KwEnum => members.push(InterfaceMember::Enum(self.parse_enum())),
                    TokenKind::Ident(_) => members.push(InterfaceMember::Method(self.parse_method())),
                    _ => {
                        let span = self.peek().span;
                        self.error(span, "expected method, const or enum");
                        self.skip_to_semi_or_brace();
                    }
                }
                if self.pos == before {
                    self.bump();
                }
            }
            self.expect(&TokenKind::RBrace, "interface body");
        }
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "interface declaration");
        Interface { name, members, span: start.join(end) }
    }

    fn parse_method(&mut self) -> Method {
        let start = self.peek().span;
        let name = self.parse_ident();
        let ordinal = if matches!(self.peek().kind, TokenKind::At) {
            Some(self.parse_ordinal())
        } else {
            None
        };
        let (params, params_span) = self.parse_param_list();
        let (response, response_span) = if matches!(self.peek().kind, TokenKind::Arrow) {
            self.bump();
            let (list, span) = self.parse_param_list();
            (Some(list), Some(span))
        } else {
            (None, None)
        };
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "method");
        Method { name, ordinal, params, params_span, response, response_span, span: start.join(end) }
    }

    /// Parse `( param (, param)* )` starting at the `(`. Returns the parsed
    /// list plus the whole-parens span. Recovers at `)`, `;`, `{`/`}` so a
    /// malformed param list cannot eat the enclosing declaration.
    fn parse_param_list(&mut self) -> (Vec<Param>, ByteSpan) {
        let start = self.peek().span;
        if self.expect(&TokenKind::LParen, "parameter list").is_none() {
            return (Vec::new(), start);
        }
        let mut out = Vec::new();
        let mut last = start;
        loop {
            match self.peek().kind {
                TokenKind::RParen => {
                    last = self.peek().span;
                    self.bump();
                    break;
                }
                TokenKind::Eof | TokenKind::LBrace | TokenKind::RBrace | TokenKind::Semi => break,
                TokenKind::Comma => { self.bump(); }
                _ => {
                    let before = self.pos;
                    self.skip_attributes();
                    if matches!(self.peek().kind, TokenKind::Ident(_)) {
                        let p = self.parse_param();
                        last = p.span;
                        out.push(p);
                    } else {
                        let span = self.peek().span;
                        self.error(span, "expected parameter");
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
                    if self.pos == before {
                        self.bump();
                    }
                }
            }
        }
        (out, start.join(last))
    }

    fn parse_param(&mut self) -> Param {
        let start = self.peek().span;
        let ty = self.parse_type();
        let name = self.parse_ident();
        let ordinal = if matches!(self.peek().kind, TokenKind::At) {
            Some(self.parse_ordinal())
        } else {
            None
        };
        Param { ty, name, ordinal, span: start.join(self.prev_span()) }
    }

    fn parse_enum(&mut self) -> EnumDecl {
        let start = self.bump().span; // 'enum'
        let name = self.parse_ident();
        let mut values = Vec::new();
        let mut bodyless = true;
        if matches!(self.peek().kind, TokenKind::LBrace) {
            bodyless = false;
            self.bump();
            while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
                let before = self.pos;
                self.skip_attributes();
                if matches!(self.peek().kind, TokenKind::Ident(_)) {
                    values.push(self.parse_enum_value());
                    if matches!(self.peek().kind, TokenKind::Comma) {
                        self.bump();
                    }
                } else {
                    let span = self.peek().span;
                    self.error(span, "expected enum value");
                    self.bump();
                }
                if self.pos == before {
                    self.bump();
                }
            }
            self.expect(&TokenKind::RBrace, "enum body");
        }
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "enum declaration");
        EnumDecl { name, values, bodyless, span: start.join(end) }
    }

    fn parse_enum_value(&mut self) -> EnumValue {
        let start = self.peek().span;
        let name = self.parse_ident();
        let value_span = if matches!(self.peek().kind, TokenKind::Eq) {
            Some(self.skip_value())
        } else {
            None
        };
        EnumValue { name, value_span, span: start.join(self.prev_span()) }
    }

    fn parse_const(&mut self) -> ConstDecl {
        let start = self.bump().span; // 'const'
        let ty = self.parse_type();
        let name = self.parse_ident();
        let value_span = if matches!(self.peek().kind, TokenKind::Eq) {
            Some(self.skip_value())
        } else {
            self.error(self.peek().span, "expected '=' in const declaration");
            None
        };
        let end = self.peek().span;
        self.expect(&TokenKind::Semi, "const declaration");
        ConstDecl { ty, name, value_span, span: start.join(end) }
    }

    // ── Types ────────────────────────────────────────────────────────

    fn parse_type(&mut self) -> TypeRef {
        let start = self.peek().span;
        let mut label = String::new();
        let mut refs = Vec::new();
        self.parse_type_name(&mut label, &mut refs);
        if matches!(self.peek().kind, TokenKind::Question) {
            self.bump();
            label.push('?');
        }
        TypeRef { label, refs, span: start.join(self.prev_span()) }
    }

    fn parse_type_name(&mut self, label: &mut String, refs: &mut Vec<NamedRef>) {
        let head = match self.peek_kind() {
            TokenKind::Ident(s) => s.clone(),
            _ => {
                let span = self.peek().span;
                self.error(span, "expected type");
                label.push_str("<error>");
                return;
            }
        };
        match head.as_str() {
            "array" => {
                let head_span = self.bump().span;
                label.push_str("array");
                if matches!(self.peek().kind, TokenKind::Langle) {
                    if !self.enter_nested(head_span, label) {
                        return;
                    }
                    self.bump();
                    label.push('<');
                    let inner = self.parse_type();
                    label.push_str(&inner.label);
                    refs.extend(inner.refs);
                    if matches!(self.peek().kind, TokenKind::Comma) {
                        self.bump();
                        label.push_str(", ");
                        if let TokenKind::IntLit(s) = self.peek().kind.clone() {
                            label.push_str(&s);
                            self.bump();
                        }
                    }
                    self.expect(&TokenKind::Rangle, "array type");
                    label.push('>');
                    self.depth -= 1;
                }
            }
            "map" => {
                let head_span = self.bump().span;
                label.push_str("map");
                if matches!(self.peek().kind, TokenKind::Langle) {
                    if !self.enter_nested(head_span, label) {
                        return;
                    }
                    self.bump();
                    label.push('<');
                    self.parse_named_or_builtin(label, refs);
                    if matches!(self.peek().kind, TokenKind::Comma) {
                        self.bump();
                        label.push_str(", ");
                    }
                    let val = self.parse_type();
                    label.push_str(&val.label);
                    refs.extend(val.refs);
                    self.expect(&TokenKind::Rangle, "map type");
                    label.push('>');
                    self.depth -= 1;
                }
            }
            "handle" => {
                self.bump();
                label.push_str("handle");
                if matches!(self.peek().kind, TokenKind::Langle) {
                    self.bump();
                    label.push('<');
                    if let TokenKind::Ident(s) = self.peek().kind.clone() {
                        label.push_str(&s);
                        self.bump();
                    }
                    self.expect(&TokenKind::Rangle, "handle type");
                    label.push('>');
                }
            }
            "pending_remote"
            | "pending_receiver"
            | "pending_associated_remote"
            | "pending_associated_receiver" => {
                self.bump();
                label.push_str(head.as_str());
                if matches!(self.peek().kind, TokenKind::Langle) {
                    self.bump();
                    label.push('<');
                    self.parse_named_or_builtin(label, refs);
                    self.expect(&TokenKind::Rangle, "type parameter");
                    label.push('>');
                }
            }
            "associated" => {
                self.bump();
                label.push_str("associated ");
                self.parse_named_or_builtin(label, refs);
                if matches!(self.peek().kind, TokenKind::Amp) {
                    self.bump();
                    label.push('&');
                }
            }
            _ => {
                self.parse_named_or_builtin(label, refs);
                if matches!(self.peek().kind, TokenKind::Amp) {
                    self.bump();
                    label.push('&');
                }
            }
        }
    }

    /// Step into the `<` of a recursive generic type whose head token is
    /// `head`. Past [`MAX_NESTING_DEPTH`] this reports the construct at
    /// `head`, skips its `<…>` without recursing, and returns `false`;
    /// otherwise it bumps the depth, which the caller drops after its `>`.
    fn enter_nested(&mut self, head: ByteSpan, label: &mut String) -> bool {
        if self.depth >= MAX_NESTING_DEPTH {
            self.error(head, format!("nesting too deep (limit {})", MAX_NESTING_DEPTH));
            self.skip_angles();
            label.push_str("<…>");
            return false;
        }
        self.depth += 1;
        true
    }

    /// Skip a `<…>` run starting at its `<` by counting angle brackets, in a
    /// loop rather than by recursion. Stops, without consuming, at a token
    /// that cannot occur inside a type, so an unclosed run cannot swallow
    /// the enclosing declaration.
    fn skip_angles(&mut self) {
        let mut depth = 0i32;
        while !self.at_eof() {
            match self.peek().kind {
                TokenKind::Langle => { depth += 1; self.bump(); }
                TokenKind::Rangle => {
                    depth -= 1;
                    self.bump();
                    if depth <= 0 { return; }
                }
                TokenKind::Semi
                | TokenKind::LBrace
                | TokenKind::RBrace
                | TokenKind::LParen
                | TokenKind::RParen => return,
                _ => { self.bump(); }
            }
        }
    }

    /// Parse a dotted name in type position. Records it as a [`NamedRef`]
    /// unless it is a single builtin scalar name (`bool`, `int32`, `string`,
    /// …), which carries no user-defined reference.
    fn parse_named_or_builtin(&mut self, label: &mut String, refs: &mut Vec<NamedRef>) {
        if !matches!(self.peek().kind, TokenKind::Ident(_)) {
            let span = self.peek().span;
            self.error(span, "expected type name");
            return;
        }
        let start = self.peek().span;
        let mut path = vec![self.parse_ident()];
        while matches!(self.peek().kind, TokenKind::Dot) {
            self.bump();
            path.push(self.parse_ident());
        }
        let dotted = path.iter().map(|i| i.text.as_str()).collect::<Vec<_>>().join(".");
        label.push_str(&dotted);
        let span = start.join(self.prev_span());
        if !(path.len() == 1 && is_builtin_scalar(&path[0].text)) {
            refs.push(NamedRef { path, span });
        }
    }
}

fn is_builtin_scalar(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "int8" | "uint8"
            | "int16" | "uint16"
            | "int32" | "uint32"
            | "int64" | "uint64"
            | "float" | "double"
            | "string"
    )
}

fn describe(k: &TokenKind) -> &'static str {
    match k {
        TokenKind::LBrace => "'{'",
        TokenKind::RBrace => "'}'",
        TokenKind::LParen => "'('",
        TokenKind::RParen => "')'",
        TokenKind::Langle => "'<'",
        TokenKind::Rangle => "'>'",
        TokenKind::Semi => "';'",
        TokenKind::Comma => "','",
        TokenKind::Eq => "'='",
        TokenKind::At => "'@'",
        TokenKind::StringLit(_) => "string literal",
        TokenKind::Ident(_) => "identifier",
        _ => "token",
    }
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
    fn parses_module_imports_and_decls() {
        let src = r#"
            module test.mod;
            import "a.mojom";
            import "b.mojom";

            const string kName = "value";
            enum E { kOne, kTwo = 2, kThree };
            struct S { int32 id; string? name; };
            union U { string s; int32 n; };
            interface I {
                Method(int32 a, string b) => (bool ok);
            };
        "#;
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        assert_eq!(r.file.module.as_ref().unwrap().name.as_str(), "test.mod");
        assert_eq!(r.file.imports.len(), 2);
        assert_eq!(r.file.decls.len(), 5);
    }

    #[test]
    fn parses_method_with_ordinal_and_response() {
        let src = "interface I { Foo@3(int32 a) => (MyStruct out); };";
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        let iface = match &r.file.decls[0] {
            Decl::Interface(i) => i,
            _ => panic!("expected interface"),
        };
        let m = match &iface.members[0] {
            InterfaceMember::Method(m) => m,
            _ => panic!("expected method"),
        };
        assert_eq!(m.name.text.as_str(), "Foo");
        assert_eq!(m.ordinal.as_ref().unwrap().value, 3);
        assert_eq!(m.params.len(), 1);
        let resp = m.response.as_ref().unwrap();
        assert_eq!(resp.len(), 1);
        assert_eq!(resp[0].name.text.as_str(), "out");
    }

    #[test]
    fn collects_type_refs_in_generics() {
        let src = "struct S { array<Foo> xs; map<string, Bar> m; pending_remote<Baz> r; };";
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        let s = match &r.file.decls[0] {
            Decl::Struct(s) => s,
            _ => panic!("expected struct"),
        };
        let names: Vec<&str> = s
            .members
            .iter()
            .filter_map(|m| match m {
                StructMember::Field(f) => Some(f),
                _ => None,
            })
            .flat_map(|f| f.ty.refs.iter().map(|r| r.path[0].text.as_str()))
            .collect();
        assert_eq!(names, vec!["Foo", "Bar", "Baz"]);
    }

    #[test]
    fn parses_bodyless_struct_and_enum() {
        let src = "[Native] struct S; [Native] enum E;";
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        assert_eq!(r.file.decls.len(), 2);
        match &r.file.decls[1] {
            Decl::Enum(e) => assert!(e.bodyless),
            _ => panic!("expected enum"),
        }
    }

    #[test]
    fn parses_interface_request_and_associated() {
        let src = "struct S { Foo& req; associated Bar assoc; associated Baz& areq; };";
        let r = parse(src);
        assert!(r.errors.is_empty(), "errors: {:?}", r.errors);
        let s = match &r.file.decls[0] {
            Decl::Struct(s) => s,
            _ => panic!("expected struct"),
        };
        assert_eq!(s.members.len(), 3);
    }

    #[test]
    fn recovers_on_bad_member() {
        let src = "struct S { ??? int32 id; };";
        let r = parse(src);
        assert!(!r.errors.is_empty());
        assert_eq!(r.file.decls.len(), 1);
    }

    #[test]
    fn terminates_on_half_typed_forms() {
        let snippets = [
            "module",
            "module foo",
            "import",
            "import \"",
            "struct",
            "struct S {",
            "struct S { int32",
            "struct S { int32 x",
            "interface I {",
            "interface I { Foo(",
            "interface I { Foo() =>",
            "interface I { Foo() => (",
            "enum E {",
            "enum E { a,",
            "const int32 x =",
            "union U {",
            "struct S { array<",
            "struct S { map<string,",
            "struct S { pending_remote<",
            "}",
            "struct S { } }",
        ];
        for s in snippets {
            let _ = parse(s); // must not hang
        }
    }

    // ── Nesting-depth limit ──────────────────────────────────────────
    //
    // Each case runs everything the WASM host drives after `update_file`,
    // on a 1 MB thread like the WASM stack. Debug frames are bigger than
    // release / WASM ones, so passing here is the conservative check.

    const LIMIT: usize = MAX_NESTING_DEPTH as usize;

    struct Pipeline {
        /// `MOJOM0001` parse-error diagnostics as (message, line, col).
        parse_errors: Vec<(String, u64, u64)>,
        /// Every document-symbol name, children included.
        symbol_names: Vec<String>,
    }

    fn run_pipeline(src: String) -> Pipeline {
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(move || {
                let a = crate::wasm_api::Analyzer::new();
                let uri = "file:///deep.mojom";
                a.update_file(uri, &src);
                let diags = a.diagnostics(uri);
                let symbols = a.document_symbols(uri);
                let _ = a.folding_ranges(uri);
                for col in [0, 20, 400] {
                    let _ = a.hover(uri, 0, col);
                    let _ = a.definition(uri, 0, col);
                    let _ = a.completion(uri, 0, col);
                }
                let _ = a.workspace_symbols("");
                a.remove_file(uri);
                drop(a);
                (diags, symbols)
            })
            .expect("spawn pipeline thread")
            .join()
            .map(|(diags, symbols)| {
                let diags: Vec<serde_json::Value> = serde_json::from_str(&diags).unwrap();
                let parse_errors = diags
                    .iter()
                    .filter(|d| d["code"] == "MOJOM0001")
                    .map(|d| {
                        let msg = d["message"].as_str().unwrap().to_string();
                        (msg, d["start"]["line"].as_u64().unwrap(), d["start"]["col"].as_u64().unwrap())
                    })
                    .collect();
                let mut symbol_names = Vec::new();
                let mut stack: Vec<serde_json::Value> = serde_json::from_str(&symbols).unwrap();
                while let Some(s) = stack.pop() {
                    symbol_names.push(s["name"].as_str().unwrap().to_string());
                    stack.extend(s["children"].as_array().cloned().unwrap_or_default());
                }
                Pipeline { parse_errors, symbol_names }
            })
            .expect("pipeline panicked")
    }

    /// A recursive type shape: `open(i)` opens level `i`, `inner` sits at
    /// the bottom, `close` ends each level.
    struct Shape {
        name: &'static str,
        open: fn(usize) -> &'static str,
        inner: &'static str,
        close: &'static str,
    }

    impl Shape {
        fn nested(&self, n: usize) -> String {
            let mut s: String = (0..n).map(self.open).collect();
            s.push_str(self.inner);
            s.push_str(&self.close.repeat(n));
            s
        }

        /// Byte length of the first `n` openers.
        fn open_len(&self, n: usize) -> usize {
            (0..n).map(|i| (self.open)(i).len()).sum()
        }
    }

    const SHAPES: &[Shape] = &[
        Shape { name: "array", open: |_| "array<", inner: "int32", close: ">" },
        Shape { name: "fixed array", open: |_| "array<", inner: "int32", close: ", 4>" },
        Shape { name: "map value", open: |_| "map<string, ", inner: "Foo", close: ">" },
        Shape { name: "nullable", open: |_| "array<", inner: "int32?", close: ">?" },
        Shape {
            name: "array/map mix",
            open: |i| if i % 2 == 0 { "array<" } else { "map<int32, " },
            inner: "pending_remote<Foo>",
            close: ">?",
        },
    ];

    /// Declaration contexts a type can sit in. `{T}` marks the deep type;
    /// everything it contains is on line 0. The names must still come out
    /// as symbols, proving parsing carried on past the deep construct.
    const CONTEXTS: &[(&str, &str, &[&str])] = &[
        (
            "struct field",
            "struct S { {T} deep; int32 kept; };\nstruct After { int32 tail; };",
            &["S", "deep", "kept", "After", "tail"],
        ),
        (
            "union field",
            "union U { {T} deep; int32 kept; };\nstruct After { int32 tail; };",
            &["U", "deep", "kept", "After", "tail"],
        ),
        (
            "method params and response",
            "interface I { M({T} deep, int32 kept) => ({T} r); Kept(); };\nstruct After { int32 tail; };",
            &["I", "M", "Kept", "After", "tail"],
        ),
        (
            "const type",
            "const {T} deep = 1;\nconst int32 kept = 2;\nstruct After { int32 tail; };",
            &["deep", "kept", "After", "tail"],
        ),
    ];

    const FOO: &str = "\nstruct Foo {};";

    #[test]
    fn nested_types_are_capped_at_the_limit() {
        for shape in SHAPES {
            for (ctx, template, names) in CONTEXTS {
                for depth in [LIMIT, LIMIT + 1, 100_000] {
                    let case = format!("{} in {} at depth {}", shape.name, ctx, depth);
                    let src = template.replace("{T}", &shape.nested(depth)) + FOO;
                    let first_col = template.find("{T}").unwrap() + shape.open_len(LIMIT);
                    let occurrences = template.matches("{T}").count();
                    let r = run_pipeline(src);

                    let msg = format!("nesting too deep (limit {})", MAX_NESTING_DEPTH);
                    let too_deep: Vec<_> = r.parse_errors.iter().filter(|e| e.0 == msg).collect();
                    let others: Vec<_> = r.parse_errors.iter().filter(|e| e.0 != msg).collect();
                    assert!(others.is_empty(), "{case}: unexpected errors {others:?}");
                    if depth <= LIMIT {
                        assert!(too_deep.is_empty(), "{case}: {too_deep:?}");
                    } else {
                        assert_eq!(too_deep.len(), occurrences, "{case}: {too_deep:?}");
                        // Reported at the opening token of level LIMIT + 1.
                        assert_eq!((too_deep[0].1, too_deep[0].2), (0, first_col as u64), "{case}");
                    }
                    for name in *names {
                        assert!(r.symbol_names.iter().any(|n| n == name), "{case}: {name} missing");
                    }
                }
            }
        }
    }

    /// Nesting that the parser walks with iterative skip loops rather than
    /// recursion: it must survive 100 000 levels and recover afterwards.
    #[test]
    fn deep_non_type_nesting_is_skipped_iteratively() {
        const N: usize = 100_000;
        let decl_kw = |i: usize| ["struct", "union", "interface", "enum"][i % 4];
        let cases: Vec<(&str, String)> = vec![
            (
                "nested declarations in a struct",
                format!(
                    "struct S {{ {}{} int32 kept; }};",
                    (0..N).map(|i| format!("{} N{} {{ ", decl_kw(i), i)).collect::<String>(),
                    "}; ".repeat(N),
                ),
            ),
            (
                "nested interfaces",
                format!("interface S {{ {}{} Kept(); }};", "interface J { ".repeat(N), "}; ".repeat(N)),
            ),
            (
                "attribute lists",
                format!("{}A{} struct S {{ int32 kept; }};", "[".repeat(N), "]".repeat(N)),
            ),
            (
                "attribute arguments",
                format!("[A{}1{}] struct S {{ int32 kept; }};", "(".repeat(N), ")".repeat(N)),
            ),
            (
                "const value",
                format!("const int32 S = {}1{};\nconst int32 kept = 2;", "(".repeat(N), ")".repeat(N)),
            ),
            (
                "field default value",
                format!("struct S {{ array<int32> v = {}{}; int32 kept; }};", "{".repeat(N), "}".repeat(N)),
            ),
            (
                "handle chain",
                format!("struct S {{ {}x{} h; int32 kept; }};", "handle<".repeat(N), ">".repeat(N)),
            ),
            (
                "pending_remote chain",
                format!("struct S {{ {}Foo{} r; int32 kept; }};", "pending_remote<".repeat(N), ">".repeat(N)),
            ),
            (
                "unclosed angles",
                format!("struct S {{ {}int32 x; int32 kept; }};", "array<".repeat(N)),
            ),
        ];
        for (case, src) in cases {
            let r = run_pipeline(src + "\nstruct After { int32 tail; };" + FOO);
            for name in ["S", "After", "tail"] {
                assert!(r.symbol_names.iter().any(|n| n == name), "{case}: {name} missing");
            }
            let kept = if case == "nested interfaces" { "Kept" } else { "kept" };
            assert!(r.symbol_names.iter().any(|n| n == kept), "{case}: {kept} missing");
        }
    }
}
