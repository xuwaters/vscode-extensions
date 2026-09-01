//! The semantic diagnostics catalogue — `GLSL0200` onward.
//!
//! Codes are stable identifiers a user can search for and a test can assert on,
//! so they are never renumbered. `GLSL0001`–`GLSL0099` belongs to the
//! preprocessor and `GLSL0100`–`GLSL0199` to the parser; everything here is
//! phase 4's.
//!
//! The catalogue and the rule behind each entry are
//! `docs/rfc/012-glsl-analyzer/design/diagnostics.md`. Two properties are
//! enforced by tests rather than promised:
//!
//! - **Every code has a seeded fixture that produces exactly it**
//!   (`tests::catalogue`). A code nothing can emit is a code nobody can act on.
//! - **Nothing here fires on valid code.** The false-positive gate runs a
//!   curated list of valid corpus shaders and requires zero error-severity
//!   diagnostics (`tests::corpus`).

use analyzer_core::diagnostics::{Diagnostic, DiagnosticCode};

/// What semantic analysis found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticCode {
    /// A name that is declared nowhere the use can see.
    UnknownIdentifier,
    /// A call to a name that is neither a function nor a type.
    UnknownFunction,
    /// A type specifier naming something that is not a type.
    UnknownType,
    /// A second declaration of a name already declared in the same scope.
    Redeclaration,
    /// `.member` where the struct or block has no such member.
    UnknownMember,
    /// A swizzle with an unknown letter, mixed component sets, a repeat where
    /// an lvalue is required, or a component the vector does not have.
    BadSwizzle,
    /// `.member` on something that has no members.
    NotAStruct,
    /// `a[i]` where `a` is neither an array, a vector nor a matrix.
    NotIndexable,
    /// A constant index outside the bounds of what it indexes.
    IndexOutOfRange,
    /// A constructor whose arguments cannot build the type.
    BadConstructor,
    /// A call no overload of the function accepts.
    NoMatchingOverload,
    /// A call several overloads accept equally well.
    AmbiguousCall,
    /// The wrong number of arguments for a function with one signature.
    ArgumentCount,
    /// An argument whose type the parameter cannot accept.
    ArgumentType,
    /// An assignment, an increment, or an `out` argument that is not an lvalue.
    NotAnLvalue,
    /// A write to something the language makes read-only: a `const`, a
    /// `uniform`, a shader input.
    ReadOnly,
    /// An operator applied to operand types it is not defined for.
    BadOperand,
    /// Two operands, or two branches of a `?:`, whose types do not agree.
    TypeMismatch,
    /// An `if`/`while`/`for`/`?:` condition that is not a `bool`.
    ConditionNotBool,
    /// A `return` whose value does not match the function's return type, or a
    /// missing value, or a value in a `void` function.
    ReturnMismatch,
    /// `discard` outside a fragment shader.
    DiscardOutsideFragment,
    /// `break` or `continue` where the language has nothing for it to leave.
    MisplacedJump,
    /// A `const` with no initialiser, or one whose initialiser is not constant.
    ConstInitializer,
    /// A builtin, type or keyword the declared `#version` does not have.
    NotAvailableInVersion,
    /// A builtin variable that does not exist in this shader stage.
    NotAvailableInStage,
    /// An array size that is not a positive integer constant.
    BadArraySize,
    /// Code after a `return`, `break`, `continue` or `discard`. A warning.
    UnreachableCode,
    /// A non-`void` function whose body can reach its end. A warning.
    MissingReturn,
}

impl DiagnosticCode for SemanticCode {
    fn as_str(self) -> &'static str {
        match self {
            SemanticCode::UnknownIdentifier => "GLSL0200",
            SemanticCode::UnknownFunction => "GLSL0201",
            SemanticCode::UnknownType => "GLSL0202",
            SemanticCode::Redeclaration => "GLSL0203",
            SemanticCode::UnknownMember => "GLSL0204",
            SemanticCode::BadSwizzle => "GLSL0205",
            SemanticCode::NotAStruct => "GLSL0206",
            SemanticCode::NotIndexable => "GLSL0207",
            SemanticCode::IndexOutOfRange => "GLSL0208",
            SemanticCode::BadConstructor => "GLSL0209",
            SemanticCode::NoMatchingOverload => "GLSL0210",
            SemanticCode::AmbiguousCall => "GLSL0211",
            SemanticCode::ArgumentCount => "GLSL0212",
            SemanticCode::ArgumentType => "GLSL0213",
            SemanticCode::NotAnLvalue => "GLSL0214",
            SemanticCode::ReadOnly => "GLSL0215",
            SemanticCode::BadOperand => "GLSL0216",
            SemanticCode::TypeMismatch => "GLSL0217",
            SemanticCode::ConditionNotBool => "GLSL0218",
            SemanticCode::ReturnMismatch => "GLSL0219",
            SemanticCode::DiscardOutsideFragment => "GLSL0220",
            SemanticCode::MisplacedJump => "GLSL0221",
            SemanticCode::ConstInitializer => "GLSL0222",
            SemanticCode::NotAvailableInVersion => "GLSL0223",
            SemanticCode::NotAvailableInStage => "GLSL0224",
            SemanticCode::BadArraySize => "GLSL0225",
            SemanticCode::UnreachableCode => "GLSL0226",
            SemanticCode::MissingReturn => "GLSL0227",
        }
    }
}

impl SemanticCode {
    /// Every code, for the catalogue test and for documentation.
    pub const ALL: &'static [SemanticCode] = &[
        SemanticCode::UnknownIdentifier,
        SemanticCode::UnknownFunction,
        SemanticCode::UnknownType,
        SemanticCode::Redeclaration,
        SemanticCode::UnknownMember,
        SemanticCode::BadSwizzle,
        SemanticCode::NotAStruct,
        SemanticCode::NotIndexable,
        SemanticCode::IndexOutOfRange,
        SemanticCode::BadConstructor,
        SemanticCode::NoMatchingOverload,
        SemanticCode::AmbiguousCall,
        SemanticCode::ArgumentCount,
        SemanticCode::ArgumentType,
        SemanticCode::NotAnLvalue,
        SemanticCode::ReadOnly,
        SemanticCode::BadOperand,
        SemanticCode::TypeMismatch,
        SemanticCode::ConditionNotBool,
        SemanticCode::ReturnMismatch,
        SemanticCode::DiscardOutsideFragment,
        SemanticCode::MisplacedJump,
        SemanticCode::ConstInitializer,
        SemanticCode::NotAvailableInVersion,
        SemanticCode::NotAvailableInStage,
        SemanticCode::BadArraySize,
        SemanticCode::UnreachableCode,
        SemanticCode::MissingReturn,
    ];

    /// One line, for the design document and for a "what does GLSL0207 mean"
    /// answer in the editor.
    pub const fn summary(self) -> &'static str {
        match self {
            SemanticCode::UnknownIdentifier => "a name that is declared nowhere in scope",
            SemanticCode::UnknownFunction => "a call to something that is not a function",
            SemanticCode::UnknownType => "a type specifier that names no type",
            SemanticCode::Redeclaration => "a name declared twice in one scope",
            SemanticCode::UnknownMember => "a member the struct or block does not have",
            SemanticCode::BadSwizzle => "a swizzle the vector cannot answer",
            SemanticCode::NotAStruct => "'.member' on something with no members",
            SemanticCode::NotIndexable => "'[]' on something that cannot be indexed",
            SemanticCode::IndexOutOfRange => "a constant index outside the bounds",
            SemanticCode::BadConstructor => "a constructor these arguments cannot build",
            SemanticCode::NoMatchingOverload => "no overload accepts these arguments",
            SemanticCode::AmbiguousCall => "several overloads accept these arguments equally",
            SemanticCode::ArgumentCount => "the wrong number of arguments",
            SemanticCode::ArgumentType => "an argument the parameter cannot accept",
            SemanticCode::NotAnLvalue => "a write to something that cannot be assigned",
            SemanticCode::ReadOnly => "a write to a const, uniform or shader input",
            SemanticCode::BadOperand => "an operator applied to types it has no meaning for",
            SemanticCode::TypeMismatch => "two operands whose types do not agree",
            SemanticCode::ConditionNotBool => "a condition that is not a bool",
            SemanticCode::ReturnMismatch => "a 'return' that does not match the function",
            SemanticCode::DiscardOutsideFragment => "'discard' outside a fragment shader",
            SemanticCode::MisplacedJump => "'break' or 'continue' with nothing to leave",
            SemanticCode::ConstInitializer => "a 'const' without a constant initialiser",
            SemanticCode::NotAvailableInVersion => "a name this '#version' does not have",
            SemanticCode::NotAvailableInStage => "a builtin this shader stage does not have",
            SemanticCode::BadArraySize => "an array size that is not a positive constant",
            SemanticCode::UnreachableCode => "code that can never run",
            SemanticCode::MissingReturn => "a non-void function that can end without a value",
        }
    }
}

/// The carrier for everything semantic analysis reports.
pub type SemanticDiagnostic = Diagnostic<SemanticCode>;
