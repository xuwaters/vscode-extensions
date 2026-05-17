//! Token-driven parser for diesel `schema.rs` files.
//!
//! Strategy: scan the token stream looking for a `diesel::table!`,
//! `diesel::joinable!`, or `diesel::allow_tables_to_appear_in_same_query!`
//! head pattern. When one is found, parse its delimited body; otherwise,
//! skip the current token and keep scanning. This means the parser is
//! robust against any surrounding Rust code we don't model — we simply
//! don't look at it.

use crate::ast::{AllowGroup, Column, Ident, Joinable, SchemaFile, Table, TypeExpr};
use crate::diagnostics::{DiagnosticCode, SchemaDiagnostic};
use crate::lexer::{Token, TokenKind, tokenize};
use crate::spans::ByteSpan;

pub struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<SchemaDiagnostic>,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str) -> Self {
        let tokens = tokenize(source);
        Parser { source, tokens, pos: 0, diagnostics: Vec::new() }
    }

    pub fn into_diagnostics(self) -> Vec<SchemaDiagnostic> {
        self.diagnostics
    }

    pub fn parse_file(&mut self) -> SchemaFile {
        let mut file = SchemaFile::default();
        while self.pos < self.tokens.len() {
            if let Some(kind) = self.match_macro_head() {
                match kind {
                    MacroKind::Table => {
                        if let Some(t) = self.parse_table_body() {
                            file.tables.push(t);
                        }
                    }
                    MacroKind::Joinable => {
                        if let Some(j) = self.parse_joinable_body() {
                            file.joinables.push(j);
                        }
                    }
                    MacroKind::AllowTablesTogether => {
                        if let Some(g) = self.parse_allow_body() {
                            file.allow_groups.push(g);
                        }
                    }
                }
            } else {
                self.pos += 1;
            }
        }
        file
    }

    /// If the cursor is at `diesel::<name>!` (or just `<name>!`), consume
    /// the head and return the macro kind. Otherwise leave the cursor
    /// alone and return None.
    fn match_macro_head(&mut self) -> Option<MacroKind> {
        let start = self.pos;
        // Optional `diesel::` prefix.
        let mut i = start;
        if let Some(Token { kind: TokenKind::Ident(name), .. }) = self.tokens.get(i) {
            if name == "diesel" {
                if matches!(self.tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::ColonColon)) {
                    i += 2;
                }
            }
        }
        let name_tok = self.tokens.get(i)?;
        let TokenKind::Ident(name) = &name_tok.kind else { return None };
        let kind = MacroKind::from_name(name)?;
        let bang = self.tokens.get(i + 1)?;
        if bang.kind != TokenKind::Bang {
            return None;
        }
        self.pos = i + 2;
        Some(kind)
    }

    fn parse_table_body(&mut self) -> Option<Table> {
        let head_start = self.macro_head_start();
        // Skip any attributes before the opening `(`/`{`.
        self.skip_attributes_inside_macro();
        let open = self.peek_kind()?;
        let (close_kind, body_close_span);
        let body_open_pos = self.pos;
        match open {
            TokenKind::LBrace => {
                close_kind = TokenKind::RBrace;
            }
            TokenKind::LParen => {
                close_kind = TokenKind::RParen;
            }
            _ => {
                // Not a recognizable body — bail out.
                self.diagnostics.push(SchemaDiagnostic::warning(
                    DiagnosticCode::MalformedTable,
                    "`diesel::table!` is not followed by `{` or `(`",
                    self.current_span(),
                ));
                return None;
            }
        }
        // Consume the opening delimiter.
        let open_span = self.tokens[self.pos].span;
        self.pos += 1;

        // Skip any attributes / `use` items inside the macro.
        self.skip_use_and_attrs();

        // Optional `schema.table_name` then `(pk1, pk2, ...)` then `{ cols }`.
        let (schema, name) = self.parse_table_name()?;

        let primary_keys = if self.peek_kind() == Some(&TokenKind::LParen) {
            self.parse_paren_ident_list()?
        } else {
            Vec::new()
        };

        // Now expect the column-body braces.
        let body_brace_open = match self.peek_kind() {
            Some(TokenKind::LBrace) => self.tokens[self.pos].span,
            _ => {
                self.diagnostics.push(SchemaDiagnostic::warning(
                    DiagnosticCode::MalformedTable,
                    format!("table `{}` is missing its column body `{{ ... }}`", name.name),
                    name.span,
                ));
                // Skip past the macro outer delimiter to keep parsing.
                self.skip_until_matched(close_kind);
                return None;
            }
        };
        self.pos += 1;
        let columns = self.parse_column_list();
        // Expect closing `}` of column body.
        let body_brace_close = match self.peek_kind() {
            Some(TokenKind::RBrace) => {
                let s = self.tokens[self.pos].span;
                self.pos += 1;
                s
            }
            _ => {
                self.diagnostics.push(SchemaDiagnostic::warning(
                    DiagnosticCode::MalformedTable,
                    format!("table `{}` column body is missing its closing `}}`", name.name),
                    body_brace_open,
                ));
                ByteSpan::new(body_brace_open.end, body_brace_open.end)
            }
        };
        // Consume outer macro close.
        body_close_span = match self.skip_until_matched(close_kind.clone()) {
            Some(s) => s,
            None => body_brace_close,
        };
        let _ = body_open_pos;
        let total_span = ByteSpan::new(head_start, body_close_span.end);
        let body_span = ByteSpan::new(open_span.start, body_close_span.end);
        Some(Table { span: total_span, schema, name, primary_keys, columns, body_span })
    }

    fn parse_table_name(&mut self) -> Option<(Option<Ident>, Ident)> {
        let first = self.expect_ident("table name")?;
        if self.peek_kind() == Some(&TokenKind::Dot) {
            self.pos += 1;
            let second = self.expect_ident("table name after `.`")?;
            Some((Some(first), second))
        } else {
            Some((None, first))
        }
    }

    fn parse_paren_ident_list(&mut self) -> Option<Vec<Ident>> {
        // Caller has already verified we're on `(`.
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::RParen) => {
                    self.pos += 1;
                    return Some(out);
                }
                Some(TokenKind::Comma) => {
                    self.pos += 1;
                }
                Some(TokenKind::Ident(_)) => {
                    let ident = self.expect_ident("identifier in `( ... )`")?;
                    out.push(ident);
                }
                None => return Some(out),
                _ => {
                    self.pos += 1;
                }
            }
        }
    }

    fn parse_column_list(&mut self) -> Vec<Column> {
        let mut out = Vec::new();
        loop {
            // Skip attributes (e.g. `#[sql_name = "..."]`).
            self.skip_attributes_inside_macro();
            match self.peek_kind() {
                Some(TokenKind::RBrace) | None => return out,
                Some(TokenKind::Comma) => {
                    self.pos += 1;
                    continue;
                }
                _ => {}
            }
            let Some(col) = self.parse_one_column() else {
                // Skip to next comma / closing brace.
                self.skip_to_column_boundary();
                continue;
            };
            out.push(col);
        }
    }

    fn parse_one_column(&mut self) -> Option<Column> {
        let name = self.expect_ident_silent()?;
        if self.peek_kind() != Some(&TokenKind::Arrow) {
            return None;
        }
        self.pos += 1;
        let tpe = self.parse_type_expr()?;
        // Optional trailing comma.
        if self.peek_kind() == Some(&TokenKind::Comma) {
            self.pos += 1;
        }
        Some(Column { name, sql_type: tpe })
    }

    /// Parse a type expression `Ident(<TypeExpr (, TypeExpr)*>)?`.
    /// Returns `None` if the cursor is not on an identifier.
    fn parse_type_expr(&mut self) -> Option<TypeExpr> {
        // Walk a possibly-qualified path: `sql_types::Text`.
        let outer_tok = self.peek()?.clone();
        let TokenKind::Ident(_) = outer_tok.kind else { return None };
        let start = outer_tok.span.start;
        // Track the final ident name (after any `::` segments).
        let mut last_ident = ident_text(&outer_tok)?.to_string();
        self.pos += 1;
        while self.peek_kind() == Some(&TokenKind::ColonColon) {
            self.pos += 1;
            let nxt = self.peek()?.clone();
            let TokenKind::Ident(_) = nxt.kind else { break };
            last_ident = ident_text(&nxt)?.to_string();
            self.pos += 1;
        }
        let mut end = self.tokens[self.pos - 1].span.end;
        // Optional generic argument list.
        if self.peek_kind() == Some(&TokenKind::LAngle) {
            self.pos += 1;
            let mut depth = 1usize;
            while depth > 0 && self.pos < self.tokens.len() {
                match self.peek_kind() {
                    Some(TokenKind::LAngle) => depth += 1,
                    Some(TokenKind::RAngle) => depth -= 1,
                    _ => {}
                }
                end = self.tokens[self.pos].span.end;
                self.pos += 1;
                if depth == 0 {
                    break;
                }
            }
        }
        let raw = &self.source[start as usize..end as usize];
        let display: String = collapse_ws(raw);
        let nullable = last_ident == "Nullable";
        let array = last_ident == "Array";
        Some(TypeExpr {
            span: ByteSpan::new(start, end),
            display,
            outer: last_ident,
            nullable,
            array,
        })
    }

    fn parse_joinable_body(&mut self) -> Option<Joinable> {
        let head_start = self.macro_head_start();
        self.skip_attributes_inside_macro();
        // Expect `(`.
        if self.peek_kind() != Some(&TokenKind::LParen) {
            return None;
        }
        self.pos += 1;

        let child = self.expect_ident_silent()?;
        if self.peek_kind() != Some(&TokenKind::Arrow) {
            return None;
        }
        self.pos += 1;
        let parent = self.expect_ident_silent()?;
        if self.peek_kind() != Some(&TokenKind::LParen) {
            return None;
        }
        self.pos += 1;
        let fk_column = self.expect_ident_silent()?;
        // Skip until matching `)` of the inner paren.
        if self.peek_kind() == Some(&TokenKind::RParen) {
            self.pos += 1;
        } else {
            self.skip_until_matched(TokenKind::RParen);
        }
        // Outer `)`.
        let outer_end = self.skip_until_matched(TokenKind::RParen)
            .map(|s| s.end)
            .unwrap_or(fk_column.span.end);
        // Optional trailing `;`.
        if self.peek_kind() == Some(&TokenKind::Semicolon) {
            self.pos += 1;
        }
        Some(Joinable {
            span: ByteSpan::new(head_start, outer_end),
            child,
            parent,
            fk_column,
        })
    }

    fn parse_allow_body(&mut self) -> Option<AllowGroup> {
        let head_start = self.macro_head_start();
        self.skip_attributes_inside_macro();
        if self.peek_kind() != Some(&TokenKind::LParen) {
            return None;
        }
        self.pos += 1;
        let mut tables = Vec::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::RParen) | None => break,
                Some(TokenKind::Comma) => {
                    self.pos += 1;
                }
                Some(TokenKind::Ident(_)) => {
                    if let Some(id) = self.expect_ident_silent() {
                        tables.push(id);
                    }
                }
                _ => {
                    self.pos += 1;
                }
            }
        }
        let end = match self.peek_kind() {
            Some(TokenKind::RParen) => {
                let s = self.tokens[self.pos].span.end;
                self.pos += 1;
                s
            }
            _ => tables.last().map(|t| t.span.end).unwrap_or(head_start),
        };
        if self.peek_kind() == Some(&TokenKind::Semicolon) {
            self.pos += 1;
        }
        Some(AllowGroup { span: ByteSpan::new(head_start, end), tables })
    }

    // ---- helpers ----

    fn macro_head_start(&self) -> u32 {
        // self.pos is positioned just after the `<name>!`. Find the start
        // of the head by walking back.
        let mut i = self.pos;
        // We consumed at minimum 2 tokens: `<ident>` and `!`.
        if i >= 2 {
            i -= 2;
        } else {
            return 0;
        }
        // Optional `diesel::` (2 tokens).
        if i >= 2
            && matches!(self.tokens.get(i - 1).map(|t| &t.kind), Some(TokenKind::ColonColon))
            && matches!(
                self.tokens.get(i - 2).map(|t| &t.kind),
                Some(TokenKind::Ident(s)) if s == "diesel"
            )
        {
            i -= 2;
        }
        self.tokens[i].span.start
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos).map(|t| &t.kind)
    }

    fn current_span(&self) -> ByteSpan {
        self.tokens.get(self.pos).map(|t| t.span).unwrap_or(ByteSpan::EMPTY)
    }

    fn expect_ident(&mut self, what: &str) -> Option<Ident> {
        let tok = self.tokens.get(self.pos)?.clone();
        if let TokenKind::Ident(name) = tok.kind {
            self.pos += 1;
            Some(Ident { name, span: tok.span })
        } else {
            self.diagnostics.push(SchemaDiagnostic::warning(
                DiagnosticCode::MalformedTable,
                format!("expected {what}"),
                tok.span,
            ));
            None
        }
    }

    fn expect_ident_silent(&mut self) -> Option<Ident> {
        let tok = self.tokens.get(self.pos)?.clone();
        if let TokenKind::Ident(name) = tok.kind {
            self.pos += 1;
            Some(Ident { name, span: tok.span })
        } else {
            None
        }
    }

    /// Skip `#[...]` and `#![...]` attribute blocks.
    fn skip_attributes_inside_macro(&mut self) {
        loop {
            if self.peek_kind() != Some(&TokenKind::Hash) {
                return;
            }
            self.pos += 1;
            // Optional `!` for inner attribute.
            if self.peek_kind() == Some(&TokenKind::Bang) {
                self.pos += 1;
            }
            if self.peek_kind() != Some(&TokenKind::LBracket) {
                return;
            }
            self.pos += 1;
            // Match brackets.
            let mut depth = 1usize;
            while depth > 0 && self.pos < self.tokens.len() {
                match self.peek_kind() {
                    Some(TokenKind::LBracket) => depth += 1,
                    Some(TokenKind::RBracket) => depth -= 1,
                    _ => {}
                }
                self.pos += 1;
                if depth == 0 {
                    break;
                }
            }
        }
    }

    /// Skip `use ...;` items and `#[...]` attributes inside a macro body
    /// before the table name appears.
    fn skip_use_and_attrs(&mut self) {
        loop {
            self.skip_attributes_inside_macro();
            if matches!(self.peek_kind(), Some(TokenKind::Ident(s)) if s == "use") {
                while self.pos < self.tokens.len()
                    && self.peek_kind() != Some(&TokenKind::Semicolon)
                {
                    self.pos += 1;
                }
                if self.peek_kind() == Some(&TokenKind::Semicolon) {
                    self.pos += 1;
                }
                continue;
            }
            return;
        }
    }

    /// Walk forward to the matching closer for whichever opener `target`
    /// is. Treats brace/paren/bracket nesting correctly. Returns the span
    /// of the matching closer, or None on EOF.
    fn skip_until_matched(&mut self, target: TokenKind) -> Option<ByteSpan> {
        let opener = match target {
            TokenKind::RParen => TokenKind::LParen,
            TokenKind::RBrace => TokenKind::LBrace,
            TokenKind::RBracket => TokenKind::LBracket,
            _ => return None,
        };
        let mut depth = 1usize;
        while self.pos < self.tokens.len() {
            let k = self.peek_kind().cloned();
            match k {
                Some(ref k) if *k == opener => depth += 1,
                Some(ref k) if *k == target => {
                    depth -= 1;
                    if depth == 0 {
                        let span = self.tokens[self.pos].span;
                        self.pos += 1;
                        return Some(span);
                    }
                }
                _ => {}
            }
            self.pos += 1;
        }
        None
    }

    /// Inside a column list, after a malformed column, skip ahead to the
    /// next comma (top-level) or closing `}`.
    fn skip_to_column_boundary(&mut self) {
        let mut depth_paren = 0i32;
        let mut depth_angle = 0i32;
        let mut depth_brace = 0i32;
        while self.pos < self.tokens.len() {
            match self.peek_kind() {
                Some(TokenKind::LParen) => depth_paren += 1,
                Some(TokenKind::RParen) => depth_paren -= 1,
                Some(TokenKind::LAngle) => depth_angle += 1,
                Some(TokenKind::RAngle) => depth_angle -= 1,
                Some(TokenKind::LBrace) => depth_brace += 1,
                Some(TokenKind::RBrace) => {
                    if depth_brace == 0 {
                        return;
                    }
                    depth_brace -= 1;
                }
                Some(TokenKind::Comma)
                    if depth_paren == 0 && depth_angle == 0 && depth_brace == 0 =>
                {
                    self.pos += 1;
                    return;
                }
                _ => {}
            }
            self.pos += 1;
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum MacroKind {
    Table,
    Joinable,
    AllowTablesTogether,
}

impl MacroKind {
    fn from_name(name: &str) -> Option<MacroKind> {
        match name {
            "table" => Some(MacroKind::Table),
            "joinable" => Some(MacroKind::Joinable),
            "allow_tables_to_appear_in_same_query" => Some(MacroKind::AllowTablesTogether),
            _ => None,
        }
    }
}

fn ident_text(tok: &Token) -> Option<&str> {
    if let TokenKind::Ident(s) = &tok.kind {
        Some(s.as_str())
    } else {
        None
    }
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out.trim().to_string()
}

pub fn parse_schema_file(source: &str) -> (SchemaFile, Vec<SchemaDiagnostic>) {
    let mut p = Parser::new(source);
    let f = p.parse_file();
    (f, p.into_diagnostics())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
// @generated automatically by Diesel CLI.

diesel::table! {
    users (id) {
        id -> Text,
        email -> Citext,
        password_hash -> Nullable<Text>,
        created_at_ms -> Int8,
    }
}

diesel::table! {
    messages (id) {
        id -> Text,
        channel_id -> Text,
        author_id -> Text,
        body -> Text,
    }
}

diesel::joinable!(messages -> users (author_id));

diesel::allow_tables_to_appear_in_same_query!(
    users,
    messages,
);
"#;

    #[test]
    fn parses_table_with_columns() {
        let (f, diags) = parse_schema_file(SAMPLE);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(f.tables.len(), 2);
        let users = &f.tables[0];
        assert_eq!(users.name.name, "users");
        assert_eq!(users.primary_keys.len(), 1);
        assert_eq!(users.primary_keys[0].name, "id");
        assert_eq!(users.columns.len(), 4);
        assert_eq!(users.columns[0].name.name, "id");
        assert_eq!(users.columns[0].sql_type.display, "Text");
        assert_eq!(users.columns[2].name.name, "password_hash");
        assert!(users.columns[2].sql_type.nullable);
        assert_eq!(users.columns[2].sql_type.display, "Nullable<Text>");
    }

    #[test]
    fn parses_joinables_and_allow_groups() {
        let (f, _) = parse_schema_file(SAMPLE);
        assert_eq!(f.joinables.len(), 1);
        assert_eq!(f.joinables[0].child.name, "messages");
        assert_eq!(f.joinables[0].parent.name, "users");
        assert_eq!(f.joinables[0].fk_column.name, "author_id");
        assert_eq!(f.allow_groups.len(), 1);
        let names: Vec<_> = f.allow_groups[0].tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["users", "messages"]);
    }

    #[test]
    fn handles_composite_primary_key() {
        let src = r#"
diesel::table! {
    channel_members (channel_id, user_id) {
        channel_id -> Text,
        user_id -> Text,
        role -> Text,
    }
}
"#;
        let (f, diags) = parse_schema_file(src);
        assert!(diags.is_empty());
        let t = &f.tables[0];
        assert_eq!(t.primary_keys.len(), 2);
        assert_eq!(t.primary_keys[0].name, "channel_id");
        assert_eq!(t.primary_keys[1].name, "user_id");
    }

    #[test]
    fn nested_generic_type_in_column() {
        let src = r#"
diesel::table! {
    t (id) {
        id -> Text,
        tags -> Array<Nullable<Text>>,
    }
}
"#;
        let (f, _) = parse_schema_file(src);
        let t = &f.tables[0];
        let col = &t.columns[1];
        assert_eq!(col.name.name, "tags");
        assert!(col.sql_type.array);
        assert_eq!(col.sql_type.display, "Array<Nullable<Text>>");
    }

    #[test]
    fn schema_qualified_table_name() {
        let src = r#"
diesel::table! {
    auth.users (id) {
        id -> Text,
    }
}
"#;
        let (f, _) = parse_schema_file(src);
        let t = &f.tables[0];
        assert_eq!(t.schema.as_ref().unwrap().name, "auth");
        assert_eq!(t.name.name, "users");
    }

    #[test]
    fn ignores_non_diesel_code() {
        let src = r#"
fn main() {
    let x = "diesel::table! { not actually a table }";
    println!("{}", x);
}

diesel::table! {
    real (id) {
        id -> Text,
    }
}
"#;
        let (f, _) = parse_schema_file(src);
        assert_eq!(f.tables.len(), 1);
        assert_eq!(f.tables[0].name.name, "real");
    }
}
