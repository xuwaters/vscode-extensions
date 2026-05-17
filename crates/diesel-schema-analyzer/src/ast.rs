//! AST for a parsed diesel `schema.rs` file.
//!
//! The grammar surface we model is intentionally narrow — only the three
//! diesel macros that carry the structural information feature providers
//! need:
//!
//! * `diesel::table! { [schema.]name (pk_cols,*) { col -> Type, ... } }`
//! * `diesel::joinable!(child -> parent (fk_col));`
//! * `diesel::allow_tables_to_appear_in_same_query!(t1, t2, ...);`
//!
//! Everything else in the file is ignored (we do *not* try to model
//! `use` items, attributes, or other Rust constructs).

use crate::spans::ByteSpan;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaFile {
    pub tables: Vec<Table>,
    pub joinables: Vec<Joinable>,
    pub allow_groups: Vec<AllowGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// Span of the entire `diesel::table! { ... }` invocation.
    pub span: ByteSpan,
    /// Optional `schema.` qualifier — e.g. `auth.users`.
    pub schema: Option<Ident>,
    pub name: Ident,
    /// Primary-key column names (zero or more — diesel defaults to `id`
    /// when omitted, but we keep the parsed list as-is).
    pub primary_keys: Vec<Ident>,
    pub columns: Vec<Column>,
    /// Span of the column-list braces (inclusive of both `{` and `}`),
    /// used for folding ranges.
    pub body_span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: Ident,
    /// The full type expression as written in the source — e.g. `Text`,
    /// `Nullable<Int8>`, `Array<Nullable<Text>>`. Preserved verbatim for
    /// hover display.
    pub sql_type: TypeExpr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeExpr {
    pub span: ByteSpan,
    /// The raw source text of the type expression, with all interior
    /// whitespace collapsed to single spaces.
    pub display: String,
    /// The outer name (e.g. `Nullable` for `Nullable<Int8>`, `Text` for `Text`).
    pub outer: String,
    /// True if the outer constructor is `Nullable`.
    pub nullable: bool,
    /// True if the outer constructor is `Array`.
    pub array: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joinable {
    /// Span of the entire `diesel::joinable!(...)` invocation.
    pub span: ByteSpan,
    pub child: Ident,
    pub parent: Ident,
    pub fk_column: Ident,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowGroup {
    /// Span of the entire `diesel::allow_tables_to_appear_in_same_query!(...)` invocation.
    pub span: ByteSpan,
    pub tables: Vec<Ident>,
}
