//! Typed AST for a Makefile.
//!
//! Every declaration node owns a [`ByteSpan`] covering its full extent in
//! the source and a `name_span` for the specific identifier that should
//! be highlighted as the "selection range" in the outline view.

use crate::spans::ByteSpan;

#[derive(Debug, Clone)]
pub struct File {
    pub items: Vec<Item>,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub enum Item {
    Rule(Rule),
    Assignment(Assignment),
    Define(Define),
    Include(Include),
    Conditional(Conditional),
    Directive(Directive),
}

impl Item {
    pub fn span(&self) -> ByteSpan {
        match self {
            Item::Rule(r) => r.span,
            Item::Assignment(a) => a.span,
            Item::Define(d) => d.span,
            Item::Include(i) => i.span,
            Item::Conditional(c) => c.span,
            Item::Directive(d) => d.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Identifier {
    pub name: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub targets: Vec<Identifier>,
    pub prerequisites: Vec<Identifier>,
    pub is_double_colon: bool,
    /// True if any target contains a `%` pattern character.
    pub is_pattern: bool,
    /// True if this rule's first target was previously declared `.PHONY`.
    pub is_phony: bool,
    pub recipe_lines: Vec<RecipeLine>,
    pub span: ByteSpan,
    pub name_span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct RecipeLine {
    pub text: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Assignment {
    pub name: Identifier,
    pub op: AssignOp,
    pub value: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    /// `=`
    Recursive,
    /// `:=`
    Simple,
    /// `::=` (POSIX)
    Immediate,
    /// `?=`
    Conditional,
    /// `+=`
    Append,
    /// `!=`
    Shell,
}

impl AssignOp {
    pub fn as_str(self) -> &'static str {
        match self {
            AssignOp::Recursive => "=",
            AssignOp::Simple => ":=",
            AssignOp::Immediate => "::=",
            AssignOp::Conditional => "?=",
            AssignOp::Append => "+=",
            AssignOp::Shell => "!=",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Define {
    pub name: Identifier,
    pub op: Option<AssignOp>,
    pub body: String,
    pub span: ByteSpan,
    pub name_span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Include {
    pub paths: Vec<String>,
    /// True for `-include` and `sinclude` (missing files are not an error).
    pub optional: bool,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Conditional {
    pub kind: ConditionalKind,
    pub condition: String,
    pub then_branch: Vec<Item>,
    pub else_branch: Vec<Item>,
    pub span: ByteSpan,
    pub name_span: ByteSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalKind {
    Ifeq,
    Ifneq,
    Ifdef,
    Ifndef,
}

impl ConditionalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ConditionalKind::Ifeq => "ifeq",
            ConditionalKind::Ifneq => "ifneq",
            ConditionalKind::Ifdef => "ifdef",
            ConditionalKind::Ifndef => "ifndef",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Directive {
    pub kind: DirectiveKind,
    pub arguments: String,
    pub span: ByteSpan,
    pub name_span: ByteSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveKind {
    Export,
    Unexport,
    Override,
    Private,
    Undefine,
    VPath,
}

impl DirectiveKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DirectiveKind::Export => "export",
            DirectiveKind::Unexport => "unexport",
            DirectiveKind::Override => "override",
            DirectiveKind::Private => "private",
            DirectiveKind::Undefine => "undefine",
            DirectiveKind::VPath => "vpath",
        }
    }
}
