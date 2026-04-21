//! Typed AST for proto3 files.
//!
//! Every node carries a [`ByteSpan`] for exact-range editor operations. The
//! parser is recovery-oriented, so any leaf may be a [`Missing`] — downstream
//! passes tolerate `Missing` and skip affected diagnostics rather than
//! aborting.

use crate::lexer::{Comment, CommentKind};
use crate::spans::ByteSpan;
use smol_str::SmolStr;

/// A named value with an attached span — the bread and butter of every node
/// that refers to something by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: SmolStr,
    pub span: ByteSpan,
}

/// A possibly-qualified name (`foo.bar.Baz`). The leading-dot bit carries
/// absolute-path semantics per the proto3 scoping rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedName {
    pub absolute: bool,
    pub parts: Vec<Ident>,
    pub span: ByteSpan,
}

impl QualifiedName {
    pub fn to_display(&self) -> String {
        let mut s = String::new();
        if self.absolute {
            s.push('.');
        }
        for (i, p) in self.parts.iter().enumerate() {
            if i > 0 {
                s.push('.');
            }
            s.push_str(&p.name);
        }
        s
    }
}

/// Either a scalar (`int32`, `string`, ...), a named user type, or the
/// special `map<K, V>` type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRef {
    Scalar(ScalarType, ByteSpan),
    Named(QualifiedName),
    Map(Box<MapType>),
    Missing(ByteSpan),
}

impl TypeRef {
    pub fn span(&self) -> ByteSpan {
        match self {
            TypeRef::Scalar(_, s) => *s,
            TypeRef::Named(q) => q.span,
            TypeRef::Map(m) => m.span,
            TypeRef::Missing(s) => *s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    Double,
    Float,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    Sfixed32,
    Sfixed64,
    Bool,
    String,
    Bytes,
}

impl ScalarType {
    pub fn as_str(self) -> &'static str {
        match self {
            ScalarType::Double => "double",
            ScalarType::Float => "float",
            ScalarType::Int32 => "int32",
            ScalarType::Int64 => "int64",
            ScalarType::Uint32 => "uint32",
            ScalarType::Uint64 => "uint64",
            ScalarType::Sint32 => "sint32",
            ScalarType::Sint64 => "sint64",
            ScalarType::Fixed32 => "fixed32",
            ScalarType::Fixed64 => "fixed64",
            ScalarType::Sfixed32 => "sfixed32",
            ScalarType::Sfixed64 => "sfixed64",
            ScalarType::Bool => "bool",
            ScalarType::String => "string",
            ScalarType::Bytes => "bytes",
        }
    }

    pub fn is_integer(self) -> bool {
        !matches!(
            self,
            ScalarType::Double | ScalarType::Float | ScalarType::Bool | ScalarType::String | ScalarType::Bytes
        )
    }

    pub fn is_valid_map_key(self) -> bool {
        matches!(
            self,
            ScalarType::Int32
                | ScalarType::Int64
                | ScalarType::Uint32
                | ScalarType::Uint64
                | ScalarType::Sint32
                | ScalarType::Sint64
                | ScalarType::Fixed32
                | ScalarType::Fixed64
                | ScalarType::Sfixed32
                | ScalarType::Sfixed64
                | ScalarType::Bool
                | ScalarType::String
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapType {
    pub key: TypeRef,
    pub value: TypeRef,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Syntax {
    Proto2,
    Proto3,
    Edition(SmolStr),
    Unspecified,
}

/// Top-level parsed `.proto` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub syntax: Syntax,
    pub syntax_span: Option<ByteSpan>,
    pub package: Option<Package>,
    pub imports: Vec<Import>,
    pub options: Vec<OptionDecl>,
    pub items: Vec<TopLevelItem>,
    pub span: ByteSpan,
    pub leading_comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: QualifiedName,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub path: String,
    pub path_span: ByteSpan,
    pub modifier: ImportModifier,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportModifier {
    None,
    Public,
    Weak,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopLevelItem {
    Message(Message),
    Enum(EnumDecl),
    Service(Service),
    Extend(Extend),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub name: Ident,
    pub fields: Vec<FieldDecl>,
    pub oneofs: Vec<Oneof>,
    pub nested_messages: Vec<Message>,
    pub nested_enums: Vec<EnumDecl>,
    pub reserved: Vec<Reserved>,
    pub extensions: Vec<ExtensionsDecl>,
    pub options: Vec<OptionDecl>,
    pub span: ByteSpan,
    pub leading_comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDecl {
    pub name: Ident,
    pub values: Vec<EnumValue>,
    pub reserved: Vec<Reserved>,
    pub options: Vec<OptionDecl>,
    pub span: ByteSpan,
    pub leading_comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumValue {
    pub name: Ident,
    pub number: IntValue,
    pub options: Vec<OptionDecl>,
    pub span: ByteSpan,
    pub leading_comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: Ident,
    pub methods: Vec<Rpc>,
    pub options: Vec<OptionDecl>,
    pub span: ByteSpan,
    pub leading_comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rpc {
    pub name: Ident,
    pub input: RpcType,
    pub output: RpcType,
    pub options: Vec<OptionDecl>,
    pub span: ByteSpan,
    pub leading_comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcType {
    pub streaming: bool,
    pub ty: QualifiedName,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDecl {
    pub label: FieldLabel,
    pub ty: TypeRef,
    pub name: Ident,
    pub number: IntValue,
    pub options: Vec<OptionDecl>,
    pub span: ByteSpan,
    pub leading_comments: Vec<Comment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldLabel {
    None,
    Repeated,
    Optional,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Oneof {
    pub name: Ident,
    pub fields: Vec<FieldDecl>,
    pub options: Vec<OptionDecl>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reserved {
    pub items: Vec<ReservedItem>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservedItem {
    Name(String, ByteSpan),
    Number(IntValue),
    Range {
        from: IntValue,
        to: ReservedRangeEnd,
        span: ByteSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservedRangeEnd {
    Value(IntValue),
    Max(ByteSpan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionsDecl {
    pub ranges: Vec<ExtensionRange>,
    pub options: Vec<OptionDecl>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionRange {
    pub from: IntValue,
    pub to: ReservedRangeEnd,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extend {
    pub ty: QualifiedName,
    pub fields: Vec<FieldDecl>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionDecl {
    pub name: OptionName,
    pub value: OptionValue,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionName {
    pub parts: Vec<OptionNamePart>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionNamePart {
    pub is_extension: bool,
    pub name: QualifiedName,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionValue {
    String(String, ByteSpan),
    Int(IntValue),
    Float(SmolStr, ByteSpan),
    Bool(bool, ByteSpan),
    Ident(Ident),
    Message(Vec<MessageLiteralField>, ByteSpan),
    List(Vec<OptionValue>, ByteSpan),
    Missing(ByteSpan),
}

impl OptionValue {
    pub fn span(&self) -> ByteSpan {
        match self {
            OptionValue::String(_, s) => *s,
            OptionValue::Int(v) => v.span,
            OptionValue::Float(_, s) => *s,
            OptionValue::Bool(_, s) => *s,
            OptionValue::Ident(i) => i.span,
            OptionValue::Message(_, s) => *s,
            OptionValue::List(_, s) => *s,
            OptionValue::Missing(s) => *s,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLiteralField {
    pub name: Ident,
    pub value: OptionValue,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntValue {
    pub raw: SmolStr,
    pub negative: bool,
    pub span: ByteSpan,
}

impl IntValue {
    /// Parses the literal as `i64`. Returns `None` if out of range or not a
    /// valid number (in which case a diagnostic is already responsible for
    /// reporting the problem).
    pub fn as_i64(&self) -> Option<i64> {
        let raw = self.raw.as_str();
        let parsed: Option<i64> = if let Some(rest) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
            i64::from_str_radix(rest, 16).ok()
        } else if raw.len() > 1 && raw.starts_with('0') && raw.chars().all(|c| c.is_ascii_digit()) {
            i64::from_str_radix(raw, 8).ok()
        } else {
            raw.parse().ok()
        };
        parsed.map(|v| if self.negative { v.wrapping_neg() } else { v })
    }
}

pub fn doc_comment_text(comments: &[Comment]) -> Option<String> {
    if comments.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    for c in comments {
        match c.kind {
            CommentKind::Line => {
                let body = c.text.trim_start_matches("//").trim();
                lines.push(body.to_string());
            }
            CommentKind::Block => {
                let body = c
                    .text
                    .trim_start_matches("/*")
                    .trim_end_matches("*/")
                    .trim();
                for l in body.lines() {
                    lines.push(l.trim().trim_start_matches('*').trim().to_string());
                }
            }
        }
    }
    if lines.iter().all(|l| l.is_empty()) {
        None
    } else {
        Some(lines.join("\n"))
    }
}
