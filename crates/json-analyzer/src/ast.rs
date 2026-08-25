//! Lossless AST for the JSON family.
//!
//! Scalars never store a normalized form of their own bytes — a number
//! is the `raw` slice of the source, a string keeps both its decoded
//! `value` (for key comparison and table cells) and its span (so the
//! formatter reprints the author's quoting and escapes verbatim).
//!
//! Comments attach to the member or element they annotate: everything on
//! the lines before an entry is `leading`, a comment on the same line
//! after the value (or its comma) is `trailing`. Comments between the
//! last entry and a closing bracket are the container's `dangling` set.
//! Sorting an object moves each member's comments with it.

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteKind {
    Double,
    Single,
    /// An unquoted JSON5 key (`{key: 1}`) or word literal.
    Bare,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StringLit {
    /// The decoded text, escapes resolved.
    pub value: String,
    pub quote: QuoteKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub span: ByteSpan,
    /// Decoded key text, used for duplicate detection and sorting.
    pub name: String,
    pub quote: QuoteKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub key: Key,
    pub value: Value,
    pub leading: Vec<Comment>,
    pub trailing: Option<Comment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub value: Value,
    pub leading: Vec<Comment>,
    pub trailing: Option<Comment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub members: Vec<Member>,
    /// Comments between the final member and the closing `}`.
    pub dangling: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Array {
    pub elements: Vec<Element>,
    pub dangling: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValueKind {
    Null,
    Bool(bool),
    /// Raw text lives in the span; the parser has already validated it.
    Number,
    String(StringLit),
    Object(Object),
    Array(Array),
    /// A hole left by error recovery. The formatter refuses files
    /// containing one (they always carry an error diagnostic).
    Missing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub span: ByteSpan,
    pub kind: ValueKind,
}

impl Value {
    pub fn raw<'a>(&self, source: &'a str) -> &'a str {
        &source[self.span.start as usize..self.span.end as usize]
    }

    /// A short lowercase noun for hovers and symbol details.
    pub fn type_name(&self) -> &'static str {
        match &self.kind {
            ValueKind::Null => "null",
            ValueKind::Bool(_) => "boolean",
            ValueKind::Number => "number",
            ValueKind::String(_) => "string",
            ValueKind::Object(_) => "object",
            ValueKind::Array(_) => "array",
            ValueKind::Missing => "missing",
        }
    }
}

/// A single-document file (`json` / `jsonc` / `json5`).
#[derive(Debug, Clone, PartialEq)]
pub struct Root {
    pub leading: Vec<Comment>,
    pub value: Option<Value>,
    pub trailing: Vec<Comment>,
}

/// One line of a JSON Lines file that holds a value.
#[derive(Debug, Clone, PartialEq)]
pub struct LineRecord {
    /// Zero-based source line.
    pub line: u32,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    Single(Root),
    Lines(Vec<LineRecord>),
}
