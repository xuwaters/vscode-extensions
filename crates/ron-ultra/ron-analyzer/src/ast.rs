//! Lossless AST for RON.
//!
//! Scalars never store a normalized form of their own bytes: the formatter
//! reprints the source slice, so `0xFF`, `1_000`, `r#"..."#` and every
//! escape survive exactly as written. Strings additionally keep their
//! decoded text, which is what duplicate-key detection compares.
//!
//! Comments attach to what they annotate. Comments on the lines before an
//! entry are its `leading` set; comments on the same line after its value
//! (or its comma) are `trailing`; comments between the last entry and the
//! closing bracket are the container's `dangling` set. A comment inside an
//! entry — between a key and its `:`, or after the `:` — rides on the
//! value as `prefix`.

use crate::spans::ByteSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    Line,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub span: ByteSpan,
    pub kind: CommentKind,
    /// A newline separates this comment from whatever precedes it.
    pub own_line: bool,
    /// A blank line separates this comment from whatever precedes it.
    pub blank_before: bool,
}

impl Comment {
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.span.start as usize..self.span.end as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub span: ByteSpan,
    /// The name with any `r#` prefix removed.
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    /// `[a, b]`
    List,
    /// `{k: v}`
    Map,
    /// `(a, b)`, `Name(a, b)`, and `()` — positional values in parentheses.
    Tuple,
    /// `(a: 1)`, `Name(a: 1)` — named fields in parentheses.
    Struct,
}

impl ContainerKind {
    pub fn open(self) -> &'static str {
        match self {
            ContainerKind::List => "[",
            ContainerKind::Map => "{",
            ContainerKind::Tuple | ContainerKind::Struct => "(",
        }
    }

    pub fn close(self) -> &'static str {
        match self {
            ContainerKind::List => "]",
            ContainerKind::Map => "}",
            ContainerKind::Tuple | ContainerKind::Struct => ")",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntryKey {
    /// `field:` in a struct.
    Field(Ident),
    /// `key:` in a map; any value can be a key.
    Map(Box<Value>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: Option<EntryKey>,
    pub value: Value,
    pub leading: Vec<Comment>,
    pub trailing: Vec<Comment>,
    /// A blank line sits directly before the entry's first token (its
    /// leading comments record their own gaps).
    pub blank_before: bool,
}

impl Entry {
    /// From the key (or the value, for a positional entry) to the end of
    /// the value. Comments are not included.
    pub fn span(&self) -> ByteSpan {
        let start = match &self.key {
            Some(EntryKey::Field(id)) => id.span.start,
            Some(EntryKey::Map(k)) => k.span.start,
            None => self.value.span.start,
        };
        ByteSpan::new(start.min(self.value.span.start), self.value.span.end)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub kind: ContainerKind,
    pub open: ByteSpan,
    /// `None` when the closing bracket is missing (always diagnosed).
    pub close: Option<ByteSpan>,
    pub entries: Vec<Entry>,
    pub dangling: Vec<Comment>,
    /// The author put a newline right after the opening bracket. The
    /// formatter keeps such a container expanded, which is how a reader
    /// chooses between `(x: 1, y: 2)` on one line and one field per line.
    pub broken: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValueKind {
    Bool(bool),
    Integer,
    /// Includes `inf`, `NaN` and their signed forms.
    Float,
    /// `decoded` is `None` when the literal had an invalid escape.
    String { decoded: Option<String> },
    ByteString,
    Char,
    /// `Name`, `Name(...)` — a struct, a tuple struct, a unit struct or an
    /// enum variant, including `Some(...)` and `None`.
    Named { name: Ident, body: Option<Container> },
    /// An anonymous `[...]`, `{...}`, `(...)`, or `()`.
    Container(Container),
    /// A hole left by error recovery; the file always carries an error.
    Missing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub span: ByteSpan,
    pub kind: ValueKind,
    pub prefix: Vec<Comment>,
}

impl Value {
    pub fn raw<'a>(&self, source: &'a str) -> &'a str {
        &source[self.span.start as usize..self.span.end as usize]
    }

    /// The parenthesized, bracketed or braced body, named or not.
    pub fn container(&self) -> Option<&Container> {
        match &self.kind {
            ValueKind::Container(c) => Some(c),
            ValueKind::Named { body: Some(c), .. } => Some(c),
            _ => None,
        }
    }

    pub fn name(&self) -> Option<&Ident> {
        match &self.kind {
            ValueKind::Named { name, .. } => Some(name),
            _ => None,
        }
    }

    /// A short description for hovers and outline details, such as
    /// "struct `Window`" or "list".
    pub fn describe(&self) -> String {
        match &self.kind {
            ValueKind::Bool(_) => "bool".into(),
            ValueKind::Integer => "integer".into(),
            ValueKind::Float => "float".into(),
            ValueKind::String { .. } => "string".into(),
            ValueKind::ByteString => "byte string".into(),
            ValueKind::Char => "char".into(),
            ValueKind::Missing => "missing value".into(),
            ValueKind::Container(c) => match c.kind {
                ContainerKind::List => "list".into(),
                ContainerKind::Map => "map".into(),
                ContainerKind::Tuple if c.entries.is_empty() => "unit".into(),
                ContainerKind::Tuple => "tuple".into(),
                ContainerKind::Struct => "struct".into(),
            },
            ValueKind::Named { name, body } => match (name.name.as_str(), body) {
                ("None", None) => "option `None`".into(),
                ("Some", Some(_)) => "option `Some`".into(),
                (n, None) => format!("unit struct or enum variant `{n}`"),
                (n, Some(c)) if c.kind == ContainerKind::Struct => format!("struct `{n}`"),
                (n, Some(_)) => format!("tuple struct or enum variant `{n}`"),
            },
        }
    }
}

/// `#![enable(...)]`, kept verbatim for the formatter.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub span: ByteSpan,
    pub leading: Vec<Comment>,
    /// Comments after the `]` on the same line.
    pub trailing: Vec<Comment>,
    /// A blank line sits directly before the `#`.
    pub blank_before: bool,
    /// The attribute's name (`enable`), if it got that far.
    pub name: Option<Ident>,
    /// The extensions listed inside `enable(...)`.
    pub extensions: Vec<Ident>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct File {
    pub attributes: Vec<Attribute>,
    /// Comments before the value (after any attributes).
    pub leading: Vec<Comment>,
    pub value: Option<Value>,
    /// A blank line sits directly before the value's first token.
    pub value_blank_before: bool,
    /// Comments on the value's last line, after it.
    pub value_trailing: Vec<Comment>,
    /// Comments after that, to the end of the file.
    pub trailing: Vec<Comment>,
}
