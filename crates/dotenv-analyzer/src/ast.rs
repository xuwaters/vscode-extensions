//! AST for a parsed `.env` file.
//!
//! The grammar is flat: a file is a list of [`Entry`] items, one per
//! logical line. An entry is either an assignment, a standalone comment,
//! or a blank line. Assignments record exact byte ranges for the key,
//! the `=`, and the value — feature providers use these for completion,
//! hover, and references.

use crate::spans::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Assignment(Assignment),
    Comment(Comment),
    Blank(Blank),
}

impl Entry {
    pub fn span(&self) -> ByteSpan {
        match self {
            Entry::Assignment(a) => a.span,
            Entry::Comment(c) => c.span,
            Entry::Blank(b) => b.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    /// Full span of the logical line (including any leading whitespace,
    /// `export ` prefix, and trailing newline if present).
    pub span: ByteSpan,
    /// Set when the line started with the `export ` keyword.
    pub export: bool,
    pub name: Identifier,
    /// Span of the `=` (or `:=`/etc — only `=` is supported for now).
    pub equals_span: ByteSpan,
    /// The raw value as it appears in source. May contain quotes; the
    /// `value_kind` distinguishes how it should be interpreted.
    pub value_span: ByteSpan,
    pub value_kind: ValueKind,
    /// Variable references found inside the value (only for unquoted /
    /// double-quoted values; single-quoted values are literal).
    pub references: Vec<VarRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier {
    pub name: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Unquoted,
    SingleQuoted,
    DoubleQuoted,
    /// Quote opened but never closed before EOF.
    UnclosedDouble,
    UnclosedSingle,
    /// `KEY=` with empty value.
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarRef {
    pub name: String,
    /// Span of the whole reference including the leading `$` and any
    /// surrounding braces.
    pub span: ByteSpan,
    /// Span of just the identifier (without `$` / `${}`). Used for
    /// reporting "unknown variable" diagnostics precisely.
    pub name_span: ByteSpan,
    pub braced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub span: ByteSpan,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blank {
    pub span: ByteSpan,
}
