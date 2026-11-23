//! Reads item declarations out of a Rust file.
//!
//! This is a skimmer, not a Rust parser: it recognizes `mod`, `use`,
//! `struct`, `enum` and `type` at item level and skips every other item by
//! bracket matching. It never fails — an item it cannot make sense of is
//! dropped and the next one read — so a file mid-edit still contributes
//! every declaration that is intact.

use super::lexer::{lex, Token, TokenKind};
use super::syntax::{
    Field, Fields, Item, ItemKind, PathSegment, RIdent, SerdeAttrs, TypeExpr, UseImport, Variant,
};
use crate::spans::ByteSpan;

pub fn parse_items(source: &str) -> Vec<Item> {
    let mut p = Parser { src: source, toks: lex(source), pos: 0 };
    p.items(false)
}

/// Parse a type written outside a Rust file, such as the `T` in
/// `#[serde(from = "T")]` or a schema annotation. `None` when the text is
/// not a type or has anything after it.
pub fn parse_type_text(text: &str) -> Option<TypeExpr> {
    let mut p = Parser { src: text, toks: lex(text), pos: 0 };
    if p.toks.is_empty() {
        return None;
    }
    let ty = p.ty();
    (p.pos == p.toks.len() && ty != TypeExpr::Other).then_some(ty)
}

/// Collapse a type's source text to a single line: `HashMap<String, u32>`.
pub fn normalize_type_text(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.replace("< ", "<").replace(" >", ">").replace(" ,", ",").replace("( ", "(").replace(" )", ")")
}

#[derive(Debug, Clone)]
enum Meta {
    Word(String),
    NameValue(String, String),
    List(String, Vec<Meta>),
}

#[derive(Default)]
struct Attrs {
    docs: Vec<String>,
    serde: SerdeAttrs,
    path: Option<String>,
}

impl Attrs {
    fn doc_text(&self) -> String {
        join_docs(&self.docs)
    }
}

fn join_docs(lines: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for chunk in lines {
        for line in chunk.lines() {
            let line = line.strip_prefix(' ').unwrap_or(line);
            let trimmed = line.trim_start();
            // `/** ... */` blocks conventionally prefix lines with ` * `.
            let line = trimmed.strip_prefix("* ").or_else(|| (trimmed == "*").then_some("")).unwrap_or(line);
            out.push(line.trim_end().to_string());
        }
    }
    while out.first().is_some_and(|l| l.is_empty()) {
        out.remove(0);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    pos: usize,
}

impl<'a> Parser<'a> {
    // ---- cursor ----------------------------------------------------------

    fn tok(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn nth(&self, n: usize) -> Option<&Token> {
        self.toks.get(self.pos + n)
    }

    fn at_punct(&self, c: char) -> bool {
        self.tok().is_some_and(|t| t.is_punct(c))
    }

    fn at_ident(&self, name: &str) -> bool {
        self.tok().is_some_and(|t| t.is_ident(name))
    }

    fn at_kind(&self, kind: TokenKind) -> bool {
        self.tok().is_some_and(|t| t.kind == kind)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat_punct(&mut self, c: char) -> bool {
        if self.at_punct(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Option<RIdent> {
        let t = self.tok()?;
        if t.kind != TokenKind::Ident {
            return None;
        }
        let id = RIdent { name: t.text.clone(), span: t.span };
        self.pos += 1;
        Some(id)
    }

    fn last_end(&self) -> u32 {
        self.pos.checked_sub(1).and_then(|i| self.toks.get(i)).map_or(0, |t| t.span.end)
    }

    /// Skip one balanced `(..)`, `[..]` or `{..}` group starting at the
    /// current opener; a single token otherwise.
    fn skip_group(&mut self) {
        let Some(open) = self.bump() else { return };
        let close = match open.kind {
            TokenKind::Punct('(') => ')',
            TokenKind::Punct('[') => ']',
            TokenKind::Punct('{') => '}',
            _ => return,
        };
        let mut stack = vec![close];
        while let Some(t) = self.bump() {
            match t.kind {
                TokenKind::Punct('(') => stack.push(')'),
                TokenKind::Punct('[') => stack.push(']'),
                TokenKind::Punct('{') => stack.push('}'),
                TokenKind::Punct(c @ (')' | ']' | '}')) => {
                    while let Some(top) = stack.pop() {
                        if top == c {
                            break;
                        }
                    }
                    if stack.is_empty() {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// The tokens inside the group at the cursor, consuming it.
    fn group_tokens(&mut self) -> Vec<Token> {
        let start = self.pos + 1;
        self.skip_group();
        let end = self.pos.saturating_sub(1).max(start);
        self.toks[start.min(end)..end].to_vec()
    }

    // ---- items -----------------------------------------------------------

    fn items(&mut self, in_block: bool) -> Vec<Item> {
        let mut items = Vec::new();
        loop {
            // Inner attributes and inner docs say nothing the schema reads.
            while self.at_kind(TokenKind::InnerDoc)
                || (self.at_punct('#') && self.nth(1).is_some_and(|t| t.is_punct('!')))
            {
                if self.at_kind(TokenKind::InnerDoc) {
                    self.pos += 1;
                } else {
                    self.pos += 2;
                    self.skip_group();
                }
            }
            match self.tok() {
                None => break,
                Some(t) if t.is_punct('}') => {
                    if in_block {
                        break;
                    }
                    self.pos += 1;
                    continue;
                }
                _ => {}
            }
            let before = self.pos;
            if let Some(item) = self.item() {
                items.push(item);
            }
            if self.pos == before {
                self.pos += 1;
            }
        }
        items
    }

    fn outer_attrs(&mut self) -> Attrs {
        let mut attrs = Attrs::default();
        loop {
            if self.at_kind(TokenKind::Doc) {
                let t = self.bump().unwrap();
                attrs.docs.push(t.text);
            } else if self.at_punct('#') && self.nth(1).is_some_and(|t| t.is_punct('[')) {
                self.pos += 1;
                let inner = self.group_tokens();
                apply_attr(&inner, &mut attrs);
            } else {
                break;
            }
        }
        attrs
    }

    fn visibility(&mut self) {
        if self.at_ident("pub") {
            self.pos += 1;
            if self.at_punct('(') {
                self.skip_group();
            }
        } else if self.at_ident("crate") && !self.nth(1).is_some_and(|t| t.kind == TokenKind::PathSep) {
            // `crate struct S` (old `crate` visibility).
            self.pos += 1;
        }
    }

    fn item(&mut self) -> Option<Item> {
        let attrs = self.outer_attrs();
        self.visibility();
        let start = self.tok()?.span.start;
        let keyword = self.tok().filter(|t| t.kind == TokenKind::Ident).map(|t| t.text.clone());
        let kind_name = match keyword.as_deref() {
            Some(k @ ("mod" | "use" | "struct" | "union" | "enum" | "type")) => k.to_string(),
            _ => {
                self.skip_item();
                return None;
            }
        };
        self.pos += 1;
        let (name, kind) = match kind_name.as_str() {
            "mod" => {
                let name = self.ident()?;
                if self.eat_punct(';') {
                    (Some(name), ItemKind::Mod { body: None })
                } else if self.eat_punct('{') {
                    let body = self.items(true);
                    self.eat_punct('}');
                    (Some(name), ItemKind::Mod { body: Some(body) })
                } else {
                    self.skip_item();
                    return None;
                }
            }
            "use" => {
                let mut out = Vec::new();
                let global = self.at_kind(TokenKind::PathSep);
                if global {
                    self.pos += 1;
                }
                self.use_tree(Vec::new(), global, &mut out);
                self.skip_item();
                (None, ItemKind::Use(out))
            }
            "struct" | "union" => {
                let name = self.ident()?;
                let generics = self.generics();
                self.where_clause();
                let fields = if self.at_punct('{') {
                    Fields::Named(self.named_fields())
                } else if self.at_punct('(') {
                    let f = self.tuple_fields();
                    self.where_clause();
                    self.eat_punct(';');
                    Fields::Tuple(f)
                } else {
                    self.eat_punct(';');
                    Fields::Unit
                };
                (Some(name), ItemKind::Struct { generics, fields })
            }
            "enum" => {
                let name = self.ident()?;
                let generics = self.generics();
                self.where_clause();
                if !self.at_punct('{') {
                    self.skip_item();
                    return None;
                }
                let variants = self.variants();
                (Some(name), ItemKind::Enum { generics, variants })
            }
            _ => {
                // `type`
                let name = self.ident()?;
                let generics = self.generics();
                if self.eat_punct(':') {
                    self.skip_until(&[';', '=']);
                }
                self.where_clause();
                if !self.eat_punct('=') {
                    self.skip_item();
                    return None;
                }
                let ty = self.ty();
                self.skip_item();
                (Some(name), ItemKind::TypeAlias { generics, ty })
            }
        };
        Some(Item {
            name,
            kind,
            serde: attrs.serde.clone(),
            docs: attrs.doc_text(),
            path_attr: attrs.path.clone(),
            span: ByteSpan::new(start, self.last_end().max(start)),
        })
    }

    /// Skip the rest of an item: up to a `;` or past a `{..}` block at depth
    /// zero, stopping before an unmatched `}` that closes the enclosing
    /// module.
    fn skip_item(&mut self) {
        while let Some(t) = self.tok() {
            match t.kind {
                TokenKind::Punct(';') => {
                    self.pos += 1;
                    return;
                }
                TokenKind::Punct('{') => {
                    self.skip_group();
                    self.eat_punct(';');
                    return;
                }
                TokenKind::Punct('(' | '[') => self.skip_group(),
                TokenKind::Punct('}') => return,
                _ => self.pos += 1,
            }
        }
    }

    /// Skip to (not past) one of `stops` at bracket depth zero.
    fn skip_until(&mut self, stops: &[char]) {
        let mut angle = 0i32;
        while let Some(t) = self.tok() {
            match t.kind {
                TokenKind::Punct(c) if angle <= 0 && stops.contains(&c) => return,
                TokenKind::Punct('<') => angle += 1,
                TokenKind::Punct('>') => angle -= 1,
                TokenKind::Punct('(' | '[' | '{') => {
                    if stops.contains(&'{') && t.is_punct('{') {
                        return;
                    }
                    self.skip_group();
                    continue;
                }
                TokenKind::Punct(')' | ']' | '}') => return,
                _ => {}
            }
            self.pos += 1;
        }
    }

    fn where_clause(&mut self) {
        if self.at_ident("where") {
            self.pos += 1;
            self.skip_until(&['{', ';', '=']);
        }
    }

    /// `<'a, T: Bound, const N: usize, U = Default>` → the type parameter
    /// names, `["T", "U"]`.
    fn generics(&mut self) -> Vec<String> {
        let mut names = Vec::new();
        if !self.at_punct('<') {
            return names;
        }
        self.pos += 1;
        let mut depth = 1;
        let mut expect_param = true;
        let mut skip_next = false;
        while let Some(t) = self.tok() {
            match t.kind {
                TokenKind::Punct('<') => depth += 1,
                TokenKind::Punct('>') => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += 1;
                        break;
                    }
                }
                TokenKind::Punct('(' | '[' | '{') => {
                    self.skip_group();
                    continue;
                }
                TokenKind::Punct(',') if depth == 1 => {
                    expect_param = true;
                    self.pos += 1;
                    continue;
                }
                TokenKind::Ident if depth == 1 && expect_param => {
                    if t.text == "const" {
                        skip_next = true;
                    } else if skip_next {
                        skip_next = false;
                        expect_param = false;
                    } else {
                        names.push(t.text.clone());
                        expect_param = false;
                    }
                }
                TokenKind::Punct(')' | ']' | '}' | ';') => break,
                _ => {
                    if depth == 1 {
                        expect_param = false;
                    }
                }
            }
            self.pos += 1;
        }
        names
    }

    fn named_fields(&mut self) -> Vec<Field> {
        self.pos += 1; // `{`
        let mut fields = Vec::new();
        loop {
            let attrs = self.outer_attrs();
            self.visibility();
            if self.eat_punct('}') || self.tok().is_none() {
                break;
            }
            let start = self.pos;
            if let Some(name) = self.ident() {
                if self.eat_punct(':') && !self.at_punct(',') && !self.at_punct('}') {
                    let ty_start = self.tok().map_or(0, |t| t.span.start);
                    let ty = self.ty();
                    let ty_text = normalize_type_text(&self.src[ty_start as usize..self.last_end().max(ty_start) as usize]);
                    let span = ByteSpan::new(name.span.start, self.last_end().max(name.span.end));
                    fields.push(Field { name: Some(name), ty, ty_text, serde: attrs.serde.clone(), docs: attrs.doc_text(), span });
                }
            }
            self.skip_until(&[',', '}']);
            self.eat_punct(',');
            if self.pos == start {
                self.pos += 1;
            }
        }
        fields
    }

    fn tuple_fields(&mut self) -> Vec<Field> {
        let inner = self.group_tokens();
        let mut sub = Parser { src: self.src, toks: inner, pos: 0 };
        let mut fields = Vec::new();
        while sub.tok().is_some() {
            let attrs = sub.outer_attrs();
            sub.visibility();
            let Some(first) = sub.tok() else { break };
            let start = sub.pos;
            let ty_start = first.span.start;
            let ty = sub.ty();
            let end = sub.last_end().max(ty_start);
            let ty_text = normalize_type_text(&self.src[ty_start as usize..end as usize]);
            fields.push(Field {
                name: None,
                ty,
                ty_text,
                serde: attrs.serde.clone(),
                docs: attrs.doc_text(),
                span: ByteSpan::new(ty_start, end),
            });
            sub.skip_until(&[',']);
            sub.eat_punct(',');
            if sub.pos == start {
                sub.pos += 1;
            }
        }
        fields
    }

    fn variants(&mut self) -> Vec<Variant> {
        self.pos += 1; // `{`
        let mut variants = Vec::new();
        loop {
            let attrs = self.outer_attrs();
            if self.eat_punct('}') || self.tok().is_none() {
                break;
            }
            let start = self.pos;
            if let Some(name) = self.ident() {
                let fields = if self.at_punct('{') {
                    Fields::Named(self.named_fields())
                } else if self.at_punct('(') {
                    Fields::Tuple(self.tuple_fields())
                } else {
                    Fields::Unit
                };
                let span = ByteSpan::new(name.span.start, self.last_end().max(name.span.end));
                variants.push(Variant { name, fields, serde: attrs.serde.clone(), docs: attrs.doc_text(), span });
            }
            // Past an explicit discriminant `= 3`.
            self.skip_until(&[',', '}']);
            self.eat_punct(',');
            if self.pos == start {
                self.pos += 1;
            }
        }
        variants
    }

    fn use_tree(&mut self, mut prefix: Vec<String>, global: bool, out: &mut Vec<UseImport>) {
        loop {
            let Some(t) = self.tok().cloned() else { return };
            match t.kind {
                TokenKind::PathSep => self.pos += 1,
                TokenKind::Punct('*') => {
                    self.pos += 1;
                    out.push(UseImport { global, path: prefix, binding: None, glob: true });
                    return;
                }
                TokenKind::Punct('{') => {
                    self.pos += 1;
                    loop {
                        if self.eat_punct('}') || self.tok().is_none() {
                            return;
                        }
                        let before = self.pos;
                        self.use_tree(prefix.clone(), global, out);
                        if !self.eat_punct(',') && !self.at_punct('}') {
                            // Unrecognized: give up on this group.
                            self.skip_until(&['}', ';']);
                            self.eat_punct('}');
                            return;
                        }
                        if self.pos == before {
                            self.pos += 1;
                        }
                    }
                }
                TokenKind::Ident => {
                    self.pos += 1;
                    if self.at_kind(TokenKind::PathSep) {
                        prefix.push(t.text);
                        self.pos += 1;
                        continue;
                    }
                    let (path, default_binding) = if t.text == "self" {
                        let last = prefix.last().cloned();
                        (prefix, last)
                    } else {
                        let name = t.text.clone();
                        prefix.push(t.text);
                        (prefix, Some(name))
                    };
                    let binding = if self.at_ident("as") {
                        self.pos += 1;
                        self.ident().map(|i| i.name).filter(|n| n != "_")
                    } else {
                        default_binding
                    };
                    out.push(UseImport { global, path, binding, glob: false });
                    return;
                }
                _ => return,
            }
        }
    }

    // ---- types -----------------------------------------------------------

    fn ty(&mut self) -> TypeExpr {
        let Some(t) = self.tok().cloned() else { return TypeExpr::Other };
        match t.kind {
            TokenKind::Punct('&') => {
                self.pos += 1;
                if self.at_kind(TokenKind::Lifetime) {
                    self.pos += 1;
                }
                if self.at_ident("mut") {
                    self.pos += 1;
                }
                TypeExpr::Ref(Box::new(self.ty()))
            }
            TokenKind::Punct('*') => {
                self.pos += 1;
                if self.at_ident("const") || self.at_ident("mut") {
                    self.pos += 1;
                }
                TypeExpr::Ref(Box::new(self.ty()))
            }
            TokenKind::Punct('(') => {
                self.pos += 1;
                let mut elems = Vec::new();
                let mut trailing_comma = false;
                while !self.at_punct(')') && self.tok().is_some() {
                    let before = self.pos;
                    elems.push(self.ty());
                    trailing_comma = self.eat_punct(',');
                    if !trailing_comma && !self.at_punct(')') {
                        self.skip_until(&[')']);
                    }
                    if self.pos == before {
                        self.pos += 1;
                    }
                }
                self.eat_punct(')');
                if elems.len() == 1 && !trailing_comma {
                    elems.pop().unwrap()
                } else {
                    TypeExpr::Tuple(elems)
                }
            }
            TokenKind::Punct('[') => {
                self.pos += 1;
                let elem = self.ty();
                if self.eat_punct(';') {
                    let len = match (self.tok(), self.nth(1)) {
                        (Some(n), Some(close)) if n.kind == TokenKind::Number && close.is_punct(']') => {
                            parse_int_literal(&n.text)
                        }
                        _ => None,
                    };
                    self.skip_until(&[']']);
                    self.eat_punct(']');
                    TypeExpr::Array(Box::new(elem), len)
                } else {
                    self.skip_until(&[']']);
                    self.eat_punct(']');
                    TypeExpr::Slice(Box::new(elem))
                }
            }
            TokenKind::Punct('!') => {
                self.pos += 1;
                TypeExpr::Other
            }
            TokenKind::Punct('<') => self.skip_type(),
            TokenKind::Ident if matches!(t.text.as_str(), "dyn" | "impl" | "fn" | "unsafe" | "extern" | "for") => {
                self.skip_type()
            }
            TokenKind::Ident if t.text == "_" => {
                self.pos += 1;
                TypeExpr::Other
            }
            TokenKind::Ident | TokenKind::PathSep => self.path_type(),
            _ => TypeExpr::Other,
        }
    }

    /// Skip a type we do not model, up to where the enclosing list resumes.
    fn skip_type(&mut self) -> TypeExpr {
        let mut angle = 0i32;
        while let Some(t) = self.tok() {
            match t.kind {
                TokenKind::Punct('<') => angle += 1,
                TokenKind::Punct('>') => {
                    if angle == 0 {
                        break;
                    }
                    angle -= 1;
                }
                TokenKind::Punct(',' | ';' | '=' | '{') if angle == 0 => break,
                TokenKind::Punct(')' | ']' | '}') => break,
                TokenKind::Punct('(' | '[') => {
                    self.skip_group();
                    continue;
                }
                _ => {}
            }
            self.pos += 1;
        }
        TypeExpr::Other
    }

    fn path_type(&mut self) -> TypeExpr {
        let global = self.at_kind(TokenKind::PathSep);
        if global {
            self.pos += 1;
        }
        let mut segments = Vec::new();
        while let Some(name) = self.ident() {
            let mut seg = PathSegment { name: name.name, args: Vec::new() };
            // Turbofish `::<` or plain `<`.
            if self.at_kind(TokenKind::PathSep) && self.nth(1).is_some_and(|t| t.is_punct('<')) {
                self.pos += 1;
            }
            if self.at_punct('<') {
                seg.args = self.generic_args();
            } else if self.at_punct('(') {
                // `Fn(A) -> B` sugar.
                self.skip_group();
                if self.at_kind(TokenKind::Arrow) {
                    self.pos += 1;
                    self.ty();
                }
                return TypeExpr::Other;
            }
            segments.push(seg);
            if self.at_kind(TokenKind::PathSep) && self.nth(1).is_some_and(|t| t.kind == TokenKind::Ident) {
                self.pos += 1;
                continue;
            }
            break;
        }
        if segments.is_empty() {
            return TypeExpr::Other;
        }
        TypeExpr::Path { global, segments }
    }

    fn generic_args(&mut self) -> Vec<TypeExpr> {
        self.pos += 1; // `<`
        let mut args = Vec::new();
        while let Some(t) = self.tok().cloned() {
            if t.is_punct('>') {
                self.pos += 1;
                break;
            }
            let before = self.pos;
            match t.kind {
                TokenKind::Lifetime => self.pos += 1,
                TokenKind::Ident
                    if self.nth(1).is_some_and(|n| n.is_punct('=') || n.is_punct(':'))
                        && !self.nth(2).is_some_and(|n| n.is_punct('=')) =>
                {
                    // An associated type binding or bound, `Item = T`.
                    self.pos += 2;
                    self.ty();
                }
                TokenKind::Number | TokenKind::Str | TokenKind::Char | TokenKind::Punct('{' | '-') => {
                    self.skip_type();
                }
                _ => args.push(self.ty()),
            }
            if !self.eat_punct(',') && !self.at_punct('>') {
                self.skip_type();
                if !self.eat_punct(',') && !self.at_punct('>') {
                    break;
                }
            }
            if self.pos == before {
                self.pos += 1;
            }
        }
        args
    }
}

// ---- attributes ------------------------------------------------------------

fn apply_attr(tokens: &[Token], attrs: &mut Attrs) {
    let mut p = Parser { src: "", toks: tokens.to_vec(), pos: 0 };
    let metas = p.metas();
    for meta in metas {
        apply_meta(meta, attrs);
    }
}

fn apply_meta(meta: Meta, attrs: &mut Attrs) {
    match meta {
        Meta::NameValue(name, value) if name == "doc" => attrs.docs.push(value),
        Meta::NameValue(name, value) if name == "path" => attrs.path = Some(value),
        Meta::List(name, inner) if name == "serde" => {
            for m in inner {
                apply_serde(m, &mut attrs.serde);
            }
        }
        // `cfg_attr(predicate, attr, attr...)`: assume the predicate holds,
        // since `cfg_attr(feature = "serde", serde(...))` is how optional
        // serde support is spelled.
        Meta::List(name, inner) if name == "cfg_attr" => {
            for m in inner.into_iter().skip(1) {
                apply_meta(m, attrs);
            }
        }
        _ => {}
    }
}

fn apply_serde(meta: Meta, s: &mut SerdeAttrs) {
    let deserialize_side = |inner: &[Meta]| {
        inner.iter().find_map(|m| match m {
            Meta::NameValue(k, v) if k == "deserialize" => Some(v.clone()),
            _ => None,
        })
    };
    match meta {
        Meta::Word(w) => match w.as_str() {
            "default" => s.default = true,
            "skip" | "skip_deserializing" => s.skip = true,
            "flatten" => s.flatten = true,
            "transparent" => s.transparent = true,
            "deny_unknown_fields" => s.deny_unknown_fields = true,
            "untagged" => s.untagged = true,
            "other" => s.other = true,
            _ => {}
        },
        Meta::NameValue(k, v) => match k.as_str() {
            "rename" => s.rename = Some(v),
            "alias" => s.aliases.push(v),
            "rename_all" => s.rename_all = Some(v),
            "rename_all_fields" => s.rename_all_fields = Some(v),
            "default" => s.default = true,
            "tag" => s.tag = Some(v),
            "content" => s.content = Some(v),
            "from" | "try_from" => s.from = Some(v),
            "with" | "deserialize_with" => s.custom = true,
            _ => {}
        },
        Meta::List(k, inner) => match k.as_str() {
            "rename" => s.rename = deserialize_side(&inner).or(s.rename.take()),
            "rename_all" => s.rename_all = deserialize_side(&inner).or(s.rename_all.take()),
            "rename_all_fields" => s.rename_all_fields = deserialize_side(&inner).or(s.rename_all_fields.take()),
            _ => {}
        },
    }
}

impl Parser<'_> {
    /// A comma-separated list of `word`, `name = literal` and `name(...)`.
    fn metas(&mut self) -> Vec<Meta> {
        let mut out = Vec::new();
        while self.tok().is_some() {
            let before = self.pos;
            // A path like `serde::rename`; only the last segment matters.
            let mut name = None;
            while let Some(t) = self.tok() {
                match t.kind {
                    TokenKind::Ident => {
                        name = Some(t.text.clone());
                        self.pos += 1;
                    }
                    TokenKind::PathSep => self.pos += 1,
                    _ => break,
                }
            }
            if let Some(name) = name {
                if self.eat_punct('=') {
                    let value = match self.bump() {
                        Some(t) => t.text,
                        None => String::new(),
                    };
                    out.push(Meta::NameValue(name, value));
                } else if self.at_punct('(') {
                    let inner = self.group_tokens();
                    let mut sub = Parser { src: "", toks: inner, pos: 0 };
                    out.push(Meta::List(name, sub.metas()));
                } else {
                    out.push(Meta::Word(name));
                }
            }
            // Past whatever is left of this entry.
            while let Some(t) = self.tok() {
                if t.is_punct(',') {
                    self.pos += 1;
                    break;
                }
                if matches!(t.kind, TokenKind::Punct('(' | '[' | '{')) {
                    self.skip_group();
                } else {
                    self.pos += 1;
                }
            }
            if self.pos == before {
                self.pos += 1;
            }
        }
        out
    }
}

/// An integer literal's value: decimal, `0x`, `0o` or `0b`, with
/// underscores and an optional type suffix.
pub fn parse_int_literal(text: &str) -> Option<u64> {
    let digits: String = text.chars().filter(|&c| c != '_').collect();
    let (radix, body) = if let Some(rest) = digits.strip_prefix("0x") {
        (16, rest)
    } else if let Some(rest) = digits.strip_prefix("0o") {
        (8, rest)
    } else if let Some(rest) = digits.strip_prefix("0b") {
        (2, rest)
    } else {
        (10, digits.as_str())
    };
    let end = body.find(|c: char| !c.is_digit(radix)).unwrap_or(body.len());
    u64::from_str_radix(&body[..end], radix).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn path(names: &[&str]) -> TypeExpr {
        TypeExpr::Path {
            global: false,
            segments: names.iter().map(|n| PathSegment { name: n.to_string(), args: vec![] }).collect(),
        }
    }

    fn generic(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
        TypeExpr::Path { global: false, segments: vec![PathSegment { name: name.into(), args }] }
    }

    #[test]
    fn structs_with_fields_docs_and_serde() {
        let items = parse_items(
            r#"
            use serde::Deserialize;

            /// The root config.
            #[derive(Debug, Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            pub struct GameConfig<'a, T: Clone> where T: Default {
                /// Window size in pixels.
                pub window_size: (u32, u32),
                #[serde(default, rename(deserialize = "title"))]
                pub(crate) window_title: Cow<'a, str>,
                keys: HashMap<String, Vec<T>>,
                grid: [[u8; 4]; 0x10],
            }

            fn not_an_item_we_read() -> Vec<u8> { vec![] }
            impl GameConfig { fn x(&self) {} }
            "#,
        );
        assert_eq!(items.len(), 2);
        let item = &items[1];
        assert_eq!(item.name(), "GameConfig");
        assert_eq!(item.docs, "The root config.");
        assert_eq!(item.serde.rename_all.as_deref(), Some("camelCase"));
        assert!(item.serde.deny_unknown_fields);
        let ItemKind::Struct { generics, fields: Fields::Named(fields) } = &item.kind else { panic!() };
        assert_eq!(generics, &vec!["T".to_string()]);
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].docs, "Window size in pixels.");
        assert_eq!(fields[0].ty, TypeExpr::Tuple(vec![path(&["u32"]), path(&["u32"])]));
        assert_eq!(fields[1].serde.rename.as_deref(), Some("title"));
        assert!(fields[1].serde.default);
        assert_eq!(fields[1].ty, generic("Cow", vec![path(&["str"])]));
        assert_eq!(fields[2].ty_text, "HashMap<String, Vec<T>>");
        assert_eq!(
            fields[3].ty,
            TypeExpr::Array(Box::new(TypeExpr::Array(Box::new(path(&["u8"])), Some(4))), Some(16))
        );
    }

    #[test]
    fn enums_tuples_and_aliases() {
        let items = parse_items(
            r#"
            #[cfg_attr(feature = "serde", serde(tag = "type"))]
            enum Shape {
                #[serde(rename = "dot")]
                Point,
                Circle(f32),
                Rect { w: f32, h: f32 } = 3,
                #[serde(other)]
                Unknown,
            }
            pub struct Meters(pub f64);
            struct Unit;
            type Grid<T> = Vec<Vec<T>>;
            "#,
        );
        let ItemKind::Enum { variants, .. } = &items[0].kind else { panic!() };
        assert_eq!(items[0].serde.tag.as_deref(), Some("type"));
        assert_eq!(variants.iter().map(|v| v.name.name.as_str()).collect::<Vec<_>>(), ["Point", "Circle", "Rect", "Unknown"]);
        assert_eq!(variants[0].serde.rename.as_deref(), Some("dot"));
        assert!(matches!(&variants[1].fields, Fields::Tuple(f) if f.len() == 1));
        assert!(matches!(&variants[2].fields, Fields::Named(f) if f.len() == 2));
        assert!(variants[3].serde.other);
        assert!(matches!(&items[1].kind, ItemKind::Struct { fields: Fields::Tuple(f), .. } if f[0].ty == path(&["f64"])));
        assert!(matches!(&items[2].kind, ItemKind::Struct { fields: Fields::Unit, .. }));
        let ItemKind::TypeAlias { generics, ty } = &items[3].kind else { panic!() };
        assert_eq!(generics, &vec!["T".to_string()]);
        assert_eq!(ty, &generic("Vec", vec![generic("Vec", vec![path(&["T"])])]));
    }

    #[test]
    fn modules_and_use_trees() {
        let items = parse_items(
            r#"
            #[path = "other.rs"] mod config;
            pub mod inline { pub struct A; }
            use crate::{config::{self, Window as W}, *};
            use ::std::collections::HashMap;
            use foo::bar as _;
            "#,
        );
        assert_eq!(items[0].path_attr.as_deref(), Some("other.rs"));
        assert!(matches!(&items[0].kind, ItemKind::Mod { body: None }));
        assert!(matches!(&items[1].kind, ItemKind::Mod { body: Some(b) } if b.len() == 1));
        let ItemKind::Use(imports) = &items[2].kind else { panic!() };
        let simple: Vec<(String, Option<String>, bool)> =
            imports.iter().map(|i| (i.path.join("::"), i.binding.clone(), i.glob)).collect();
        assert_eq!(
            simple,
            vec![
                ("crate::config".into(), Some("config".into()), false),
                ("crate::config::Window".into(), Some("W".into()), false),
                ("crate".into(), None, true),
            ]
        );
        let ItemKind::Use(imports) = &items[3].kind else { panic!() };
        assert!(imports[0].global);
        let ItemKind::Use(imports) = &items[4].kind else { panic!() };
        assert_eq!(imports[0].binding, None);
    }

    #[test]
    fn half_written_files_keep_their_intact_items() {
        let items = parse_items("struct A { x: u32, y: }\nstruct B { z: bool }\nfn broken( {\n");
        let names: Vec<&str> = items.iter().map(|i| i.name()).collect();
        assert_eq!(names, ["A", "B"]);
        let ItemKind::Struct { fields, .. } = &items[0].kind else { panic!() };
        assert_eq!(fields.list().len(), 1);
    }

    #[test]
    fn type_text() {
        assert_eq!(parse_type_text("crate::a::B<u8>").map(|_| ()), Some(()));
        assert_eq!(parse_type_text("crate::a::"), None);
        assert_eq!(parse_type_text(""), None);
        assert_eq!(parse_int_literal("1_000u32"), Some(1000));
        assert_eq!(parse_int_literal("0xFF"), Some(255));
    }
}
