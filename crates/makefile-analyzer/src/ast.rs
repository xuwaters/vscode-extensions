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
    /// True for grouped targets (`a b &: deps`).
    pub is_grouped: bool,
    /// The target pattern of a static pattern rule (`objs: %.o: %.c`).
    pub static_pattern: Option<Identifier>,
    /// Prerequisites after `|`.
    pub order_only: Vec<Identifier>,
    /// Recipe text after `;` on the header line.
    pub inline_recipe: Option<String>,
    pub recipe_lines: Vec<RecipeLine>,
    pub span: ByteSpan,
    pub name_span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct RecipeLine {
    /// Source text of the line, recipe prefix and backslash-newlines
    /// included.
    pub text: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone)]
pub struct Assignment {
    pub modifiers: Vec<Modifier>,
    /// Non-empty for a target-specific variable (`targets: VAR = value`).
    pub targets: Vec<Identifier>,
    pub name: Identifier,
    pub op: AssignOp,
    pub value: String,
    pub span: ByteSpan,
}

/// Keywords that may precede a variable assignment, `define` or
/// `undefine`, in any order and combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Export,
    Unexport,
    Override,
    Private,
}

impl Modifier {
    pub fn as_str(self) -> &'static str {
        match self {
            Modifier::Export => "export",
            Modifier::Unexport => "unexport",
            Modifier::Override => "override",
            Modifier::Private => "private",
        }
    }

    pub fn from_word(word: &str) -> Option<Modifier> {
        match word {
            "export" => Some(Modifier::Export),
            "unexport" => Some(Modifier::Unexport),
            "override" => Some(Modifier::Override),
            "private" => Some(Modifier::Private),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    /// `=`
    Recursive,
    /// `:=`
    Simple,
    /// `::=` (POSIX)
    Immediate,
    /// `:::=` (POSIX immediate-with-escape)
    ImmediateExpand,
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
            AssignOp::ImmediateExpand => ":::=",
            AssignOp::Conditional => "?=",
            AssignOp::Append => "+=",
            AssignOp::Shell => "!=",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Define {
    pub modifiers: Vec<Modifier>,
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
    /// `load` / `-load`
    Load,
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
            DirectiveKind::Load => "load",
        }
    }
}
