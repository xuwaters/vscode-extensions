//! Mojom AST. Nodes carry a [`ByteSpan`] for the full declaration and a
//! separate span for the name identifier (used for document-symbol selection
//! ranges and go-to-definition targets).
//!
//! This AST is intentionally lossy: it captures declaration structure
//! (names, ordinals, type references, nesting) — enough for diagnostics,
//! document outlines and cross-file name resolution — and falls back to an
//! opaque token-range span for value expressions (const/default values,
//! enum-value expressions) it doesn't need to understand.

use crate::spans::ByteSpan;
use smol_str::SmolStr;

#[derive(Debug, Clone)]
pub struct File {
    pub module: Option<Module>,
    pub imports: Vec<Import>,
    pub decls: Vec<Decl>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Module {
    /// The dotted module name, e.g. `foo.bar`.
    pub name: SmolStr,
    pub name_span: ByteSpan,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub path: StringLit,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub enum Decl {
    Struct(Struct),
    Union(Union),
    Interface(Interface),
    Enum(EnumDecl),
    Const(ConstDecl),
}

impl Decl {
    pub fn span(&self) -> ByteSpan {
        match self {
            Decl::Struct(s) => s.span,
            Decl::Union(u) => u.span,
            Decl::Interface(i) => i.span,
            Decl::Enum(e) => e.span,
            Decl::Const(c) => c.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Struct {
    pub name: Ident,
    pub members: Vec<StructMember>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub enum StructMember {
    Field(Field),
    Const(ConstDecl),
    Enum(EnumDecl),
}

#[derive(Debug, Clone)]
pub struct Field {
    pub ty: TypeRef,
    pub name: Ident,
    pub ordinal: Option<Ordinal>,
    /// Span of the `= <value>` default expression, if present.
    pub default_span: Option<ByteSpan>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Union {
    pub name: Ident,
    pub fields: Vec<Field>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Interface {
    pub name: Ident,
    pub members: Vec<InterfaceMember>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub enum InterfaceMember {
    Method(Method),
    Const(ConstDecl),
    Enum(EnumDecl),
}

#[derive(Debug, Clone)]
pub struct Method {
    pub name: Ident,
    pub ordinal: Option<Ordinal>,
    pub params: Vec<Param>,
    pub params_span: ByteSpan,
    /// The `=> (…)` response parameter list, if present.
    pub response: Option<Vec<Param>>,
    pub response_span: Option<ByteSpan>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub ty: TypeRef,
    pub name: Ident,
    pub ordinal: Option<Ordinal>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: Ident,
    pub values: Vec<EnumValue>,
    /// `true` for a bodyless `[Native] enum Foo;` declaration.
    pub bodyless: bool,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct EnumValue {
    pub name: Ident,
    /// Span of the `= <value>` initialiser expression, if present.
    pub value_span: Option<ByteSpan>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct ConstDecl {
    pub ty: TypeRef,
    pub name: Ident,
    /// Span of the `= <value>` expression, if present.
    pub value_span: Option<ByteSpan>,
    pub span: ByteSpan,
}

/// A type reference. The structure of the type (array, map, pending_remote,
/// …) is flattened down to two things the LSP layer cares about: a display
/// `label` reconstructed from the source, and the set of *named* (i.e.
/// user-defined, non-builtin) references contained within it.
#[derive(Debug, Clone)]
pub struct TypeRef {
    /// Display text of the whole type, e.g. `array<Foo>?`.
    pub label: String,
    /// User-defined references nested inside this type. Builtin scalar and
    /// container names (`bool`, `int32`, `string`, `handle`, `array`, …) are
    /// not recorded here.
    pub refs: Vec<NamedRef>,
    pub span: ByteSpan,
}

/// A dotted reference to a user-defined type (`Foo`, `foo.bar.Baz`).
#[derive(Debug, Clone)]
pub struct NamedRef {
    pub path: Vec<Ident>,
    pub span: ByteSpan,
}

impl NamedRef {
    pub fn dotted(&self) -> String {
        self.path.iter().map(|i| i.text.as_str()).collect::<Vec<_>>().join(".")
    }
}

#[derive(Debug, Clone)]
pub struct Ordinal {
    pub value: u32,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Ident {
    pub text: SmolStr,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct StringLit {
    pub value: String,
    pub span: ByteSpan,
}

/// The Mojom builtin scalar and container type names. References to these are
/// never treated as unresolved user types.
pub fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "int8" | "uint8"
            | "int16" | "uint16"
            | "int32" | "uint32"
            | "int64" | "uint64"
            | "float" | "double"
            | "string"
            | "handle"
            | "array" | "map"
            | "pending_remote" | "pending_receiver"
            | "pending_associated_remote" | "pending_associated_receiver"
            | "associated"
    )
}
