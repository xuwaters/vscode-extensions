//! The slice of a Rust file the schema needs: modules, imports, and the
//! shapes of structs, enums and type aliases with their serde attributes.
//! Functions, impls, traits and everything else are skipped by the parser
//! and never reach this tree.

use crate::spans::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RIdent {
    pub name: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathSegment {
    pub name: String,
    /// Generic type arguments; lifetimes, const arguments and associated
    /// type bindings are dropped.
    pub args: Vec<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    /// `a::b::C<T>`; `global` for a leading `::`.
    Path { global: bool, segments: Vec<PathSegment> },
    /// `(A, B)`; `()` when empty.
    Tuple(Vec<TypeExpr>),
    /// `[T; N]`, with `N` when it is an integer literal.
    Array(Box<TypeExpr>, Option<u64>),
    /// `[T]`.
    Slice(Box<TypeExpr>),
    /// `&T`, `&'a mut T`, `*const T`.
    Ref(Box<TypeExpr>),
    /// Anything the schema cannot reason about: `dyn Trait`, `impl Trait`,
    /// `fn(..)`, `_`, `!`, qualified paths.
    Other,
}

/// What `#[serde(...)]` (including inside `#[cfg_attr(..., serde(...))]`)
/// says about an item, a field or a variant. Only deserialize-side
/// settings matter: `rename(deserialize = "x")` lands in `rename`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SerdeAttrs {
    pub rename: Option<String>,
    pub aliases: Vec<String>,
    pub rename_all: Option<String>,
    pub rename_all_fields: Option<String>,
    /// `default` or `default = "path"`.
    pub default: bool,
    /// `skip` or `skip_deserializing`.
    pub skip: bool,
    pub flatten: bool,
    pub transparent: bool,
    pub deny_unknown_fields: bool,
    pub untagged: bool,
    pub tag: Option<String>,
    pub content: Option<String>,
    /// `#[serde(other)]` on a unit variant.
    pub other: bool,
    /// `from = "T"` or `try_from = "T"`: deserialized as `T` instead.
    pub from: Option<String>,
    /// `with`, `deserialize_with`: the shape is someone else's business.
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// `None` for a tuple-struct field.
    pub name: Option<RIdent>,
    pub ty: TypeExpr,
    /// The type as written, whitespace-normalized, for messages and hovers.
    pub ty_text: String,
    pub serde: SerdeAttrs,
    pub docs: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fields {
    Named(Vec<Field>),
    Tuple(Vec<Field>),
    Unit,
}

impl Fields {
    pub fn list(&self) -> &[Field] {
        match self {
            Fields::Named(f) | Fields::Tuple(f) => f,
            Fields::Unit => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: RIdent,
    pub fields: Fields,
    pub serde: SerdeAttrs,
    pub docs: String,
    pub span: ByteSpan,
}

/// One name a `use` brings into scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseImport {
    /// Leading `::`.
    pub global: bool,
    pub path: Vec<String>,
    /// The name bound in scope: the alias, or the last segment. `None` for
    /// a glob and for `as _`.
    pub binding: Option<String>,
    pub glob: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    /// `mod name;` (`body: None`) or `mod name { ... }`.
    Mod { body: Option<Vec<Item>> },
    Use(Vec<UseImport>),
    Struct { generics: Vec<String>, fields: Fields },
    Enum { generics: Vec<String>, variants: Vec<Variant> },
    TypeAlias { generics: Vec<String>, ty: TypeExpr },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// `None` only for `use`.
    pub name: Option<RIdent>,
    pub kind: ItemKind,
    pub serde: SerdeAttrs,
    pub docs: String,
    /// `#[path = "..."]` on a `mod`.
    pub path_attr: Option<String>,
    pub span: ByteSpan,
}

impl Item {
    pub fn name(&self) -> &str {
        self.name.as_ref().map_or("", |n| n.name.as_str())
    }

    pub fn generics(&self) -> &[String] {
        match &self.kind {
            ItemKind::Struct { generics, .. }
            | ItemKind::Enum { generics, .. }
            | ItemKind::TypeAlias { generics, .. } => generics,
            _ => &[],
        }
    }

    /// The keyword a hover shows: `struct`, `enum`, `type`, `mod`.
    pub fn keyword(&self) -> &'static str {
        match self.kind {
            ItemKind::Mod { .. } => "mod",
            ItemKind::Use(_) => "use",
            ItemKind::Struct { .. } => "struct",
            ItemKind::Enum { .. } => "enum",
            ItemKind::TypeAlias { .. } => "type",
        }
    }
}
