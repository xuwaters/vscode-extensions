//! Cap'n Proto AST. Nodes carry a [`ByteSpan`] for the full declaration and a
//! separate span for the name identifier (used for document-symbol selection
//! ranges). This AST is intentionally lossy: it captures what's needed for
//! document outlines and structural diagnostics, and falls back to "opaque
//! token range" for expressions it doesn't need to understand (field
//! defaults, annotation arguments, etc.).

use crate::spans::ByteSpan;
use smol_str::SmolStr;

#[derive(Debug, Clone)]
pub struct File {
    pub file_id: Option<FileId>,
    pub decls: Vec<Decl>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct FileId {
    /// The raw hex string, e.g. `0x9eb32e19f86ee174`.
    pub value: SmolStr,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub enum Decl {
    Using(Using),
    Struct(Struct),
    Enum(EnumDecl),
    Interface(Interface),
    Const(ConstDecl),
    Annotation(AnnotationDecl),
    /// A top-level annotation application like `$Cxx.namespace("foo");`.
    TopAnnotation(AnnotationApp),
}

impl Decl {
    pub fn span(&self) -> ByteSpan {
        match self {
            Decl::Using(u) => u.span,
            Decl::Struct(s) => s.span,
            Decl::Enum(e) => e.span,
            Decl::Interface(i) => i.span,
            Decl::Const(c) => c.span,
            Decl::Annotation(a) => a.span,
            Decl::TopAnnotation(a) => a.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Using {
    pub name: Option<Ident>,
    /// The imported path string if present (`import "foo.capnp"`). A plain
    /// `using Name = Something;` may have no import path.
    pub import_path: Option<StringLit>,
    /// Dotted path after the import string, e.g. `.Foo.Bar` in
    /// `using X = import "foo.capnp".Foo.Bar;`.
    pub import_target: Vec<Ident>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Struct {
    pub name: Ident,
    pub type_params: Vec<Ident>,
    pub members: Vec<StructMember>,
    pub annotations: Vec<AnnotationApp>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub enum StructMember {
    Field(Field),
    /// An anonymous `union { ... }`.
    AnonUnion(UnionBlock),
    Struct(Struct),
    Enum(EnumDecl),
    Interface(Interface),
    Const(ConstDecl),
    Annotation(AnnotationDecl),
    Using(Using),
}

impl StructMember {
    pub fn span(&self) -> ByteSpan {
        match self {
            StructMember::Field(f) => f.span,
            StructMember::AnonUnion(u) => u.span,
            StructMember::Struct(s) => s.span,
            StructMember::Enum(e) => e.span,
            StructMember::Interface(i) => i.span,
            StructMember::Const(c) => c.span,
            StructMember::Annotation(a) => a.span,
            StructMember::Using(u) => u.span,
        }
    }
}

/// A struct field. May be a slot (has `@N` ordinal and a type), a named
/// union (`name :union { ... }`), or a named group (`name :group { ... }`).
#[derive(Debug, Clone)]
pub struct Field {
    pub name: Ident,
    /// Present for slot fields, absent for named unions/groups.
    pub ordinal: Option<Ordinal>,
    pub body: FieldBody,
    pub annotations: Vec<AnnotationApp>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub enum FieldBody {
    Slot {
        ty: TypeRef,
        /// The default-expression token range, if any (`= ...`). Not parsed
        /// into structured form in this iteration.
        default_span: Option<ByteSpan>,
    },
    NamedUnion(UnionBlock),
    NamedGroup(GroupBlock),
}

#[derive(Debug, Clone)]
pub struct Ordinal {
    pub value: u32,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct UnionBlock {
    pub members: Vec<Field>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct GroupBlock {
    pub members: Vec<StructMember>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: Ident,
    pub enumerants: Vec<Enumerant>,
    pub annotations: Vec<AnnotationApp>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Enumerant {
    pub name: Ident,
    pub ordinal: Option<Ordinal>,
    pub annotations: Vec<AnnotationApp>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Interface {
    pub name: Ident,
    pub type_params: Vec<Ident>,
    pub superclasses: Vec<TypeRef>,
    pub methods: Vec<Method>,
    pub nested: Vec<StructMember>,
    pub annotations: Vec<AnnotationApp>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub name: Ident,
    pub ordinal: Option<Ordinal>,
    /// Parsed parameter list, if the method has a `(…)` params form.
    pub params: Option<Vec<MethodParam>>,
    /// Entire `(params)` span — preserved so hover/folding can reach it.
    pub params_span: Option<ByteSpan>,
    /// Parsed result list, if the method has a `-> (…)` results form.
    pub results: Option<Vec<MethodParam>>,
    pub results_span: Option<ByteSpan>,
    /// `true` when the method was declared with `-> stream;` (Cap'n Proto 0.8
    /// flow-control hint). Streaming methods have no explicit results form.
    pub streaming: bool,
    pub annotations: Vec<AnnotationApp>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct MethodParam {
    pub name: Ident,
    pub ty: TypeRef,
    pub default_span: Option<ByteSpan>,
    pub annotations: Vec<AnnotationApp>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct ConstDecl {
    pub name: Ident,
    pub ty: TypeRef,
    pub value_span: Option<ByteSpan>,
    pub annotations: Vec<AnnotationApp>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct AnnotationDecl {
    pub name: Ident,
    pub targets_span: Option<ByteSpan>,
    pub ty: Option<TypeRef>,
    pub annotations: Vec<AnnotationApp>,
    pub span: ByteSpan,
}

/// Application of an annotation to a declaration: `$Foo.bar(arg)`.
#[derive(Debug, Clone)]
pub struct AnnotationApp {
    pub path: Vec<Ident>,
    /// The parenthesised argument span, if any.
    pub args_span: Option<ByteSpan>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct TypeRef {
    /// When the reference opens with `import "foo.capnp"`, the string literal
    /// is captured here and [`path`](Self::path) holds the dotted tail
    /// (`Foo.Bar` in `import "foo.capnp".Foo.Bar`). A bare `import "foo.capnp"`
    /// has `path` empty.
    pub import_path: Option<StringLit>,
    /// Dotted name path: `Foo`, `Foo.Bar`, `List(Int32)` keeps the head `List`.
    pub path: Vec<Ident>,
    /// Type arguments inside `(...)`. Nested types are parsed recursively.
    pub args: Vec<TypeRef>,
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
