//! Typed AST for Protocol Buffers text format.

use crate::spans::ByteSpan;
use smol_str::SmolStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: SmolStr,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedName {
    pub parts: Vec<Ident>,
    pub span: ByteSpan,
}

impl QualifiedName {
    pub fn to_display(&self) -> String {
        let mut s = String::new();
        for (i, p) in self.parts.iter().enumerate() {
            if i > 0 {
                s.push('.');
            }
            s.push_str(&p.name);
        }
        s
    }
}

/// A full textproto file: a sequence of top-level fields, plus the collected
/// `#` header annotations that bind the document to a proto message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub header: HeaderHints,
    pub fields: Vec<Field>,
    pub span: ByteSpan,
}

/// Conventional header annotations read from leading `#` comments.
///
/// The format recognised (as used by `txtpbfmt` and the public specs):
///   `# proto-file: path/to/schema.proto`
///   `# proto-message: pkg.MessageName`
///   `# proto-import: path/to/other.proto` (repeatable)
///   `# proto-syntax: editions | proto2 | proto3`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderHints {
    pub proto_file: Option<HeaderAnnotation>,
    pub proto_message: Option<HeaderAnnotation>,
    pub proto_import: Vec<HeaderAnnotation>,
    pub proto_syntax: Option<HeaderAnnotation>,
    /// Unrecognised `proto-*` keys — reported as a hint-level diagnostic.
    pub unknown: Vec<HeaderAnnotation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderAnnotation {
    pub key: SmolStr,
    pub key_span: ByteSpan,
    pub value: String,
    pub value_span: ByteSpan,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: FieldName,
    /// True when the source used `: ` between name and value. Absent for
    /// message fields that use `name { … }` form.
    pub has_colon: bool,
    pub value: Value,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldName {
    Ident(Ident),
    /// `[pkg.ext_name]` extension field.
    Extension { name: QualifiedName, span: ByteSpan },
    /// `[host.domain/pkg.Type]` Any type-URL field.
    Any { url: AnyTypeUrl, span: ByteSpan },
}

impl FieldName {
    pub fn span(&self) -> ByteSpan {
        match self {
            FieldName::Ident(i) => i.span,
            FieldName::Extension { span, .. } => *span,
            FieldName::Any { span, .. } => *span,
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            FieldName::Ident(i) => i.name.to_string(),
            FieldName::Extension { name, .. } => format!("[{}]", name.to_display()),
            FieldName::Any { url, .. } => format!("[{}/{}]", url.host, url.type_name.to_display()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnyTypeUrl {
    pub host: String,
    pub host_span: ByteSpan,
    pub type_name: QualifiedName,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String {
        /// Concatenated value (possibly across adjacent quoted literals).
        value: String,
        span: ByteSpan,
    },
    Integer {
        raw: SmolStr,
        negative: bool,
        span: ByteSpan,
    },
    Float {
        raw: SmolStr,
        negative: bool,
        span: ByteSpan,
    },
    /// A bare identifier used as an enum or boolean value.
    Ident(Ident),
    /// A `-ident` — `-inf` or a negated enum alias like `-MAX`. The parser is
    /// deliberately lenient; semantic checks decide whether this is valid.
    SignedIdent {
        ident: Ident,
        span: ByteSpan,
    },
    Message {
        fields: Vec<Field>,
        is_angle: bool,
        span: ByteSpan,
    },
    List {
        elements: Vec<Value>,
        span: ByteSpan,
    },
    Missing(ByteSpan),
}

impl Value {
    pub fn span(&self) -> ByteSpan {
        match self {
            Value::String { span, .. } => *span,
            Value::Integer { span, .. } => *span,
            Value::Float { span, .. } => *span,
            Value::Ident(i) => i.span,
            Value::SignedIdent { span, .. } => *span,
            Value::Message { span, .. } => *span,
            Value::List { span, .. } => *span,
            Value::Missing(s) => *s,
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            Value::String { .. } => "string",
            Value::Integer { .. } => "integer",
            Value::Float { .. } => "float",
            Value::Ident(_) | Value::SignedIdent { .. } => "identifier",
            Value::Message { .. } => "message",
            Value::List { .. } => "list",
            Value::Missing(_) => "missing",
        }
    }
}
