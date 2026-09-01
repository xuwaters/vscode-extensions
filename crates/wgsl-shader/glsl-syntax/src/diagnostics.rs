//! Diagnostic codes emitted by the token, preprocessor and parser layers.
//!
//! Codes are stable identifiers a user can search for and a test can assert on,
//! so they are never renumbered. `GLSL0001`–`GLSL0099` is the preprocessor's
//! range, `GLSL0100`–`GLSL0199` the parser's; semantic analysis (phase 4) takes
//! `GLSL0200` onward.
//!
//! Severity follows glslang where glslang has an opinion, because it is the
//! behavioural oracle for this layer — notably, `GL_`-prefixed macro names are
//! an error while `__`-containing ones are only a warning.

use analyzer_core::diagnostics::{Diagnostic, DiagnosticCode};

/// What went wrong in the token or preprocessor layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PpCode {
    /// A `#` followed by a word that is not a directive.
    UnknownDirective,
    /// `#version` was not the first thing in the file.
    VersionNotFirst,
    /// `#version` without a readable number, or with an unknown profile.
    MalformedVersion,
    /// `#extension` without `name : behaviour`.
    MalformedExtension,
    /// `#line` without a readable line number.
    MalformedLine,
    /// The message of an `#error` directive, verbatim.
    ErrorDirective,
    /// Tokens after a directive that was already complete.
    ExtraTokens,
    /// `#define` or `#undef` without a name after it.
    MissingMacroName,
    /// A `#define` parameter list containing something that is not a name.
    BadMacroParameter,
    /// The same parameter name twice in one `#define`.
    DuplicateMacroParameter,
    /// A `#define` parameter list with no closing parenthesis.
    UnterminatedMacroParameters,
    /// A `#define` that changes an existing definition without an `#undef`.
    MacroRedefined,
    /// `#define`/`#undef` of a name the language reserves (`GL_…`, `defined`).
    ReservedMacroName,
    /// `#define`/`#undef` of a name containing `__`, which is reserved but
    /// only advisory outside ES.
    ReservedMacroNameWarning,
    /// `#else`, `#elif` or `#endif` with no `#if` group open.
    UnmatchedConditional,
    /// A second `#else`, or an `#elif` after `#else`, in one group.
    MisplacedElse,
    /// An `#if` group still open at end of file.
    UnterminatedConditional,
    /// An `#if`/`#elif` expression that does not parse.
    BadConditionalExpression,
    /// `/` or `%` by zero inside an `#if` expression.
    DivisionByZero,
    /// An integer literal an `#if` expression cannot read.
    BadConditionalLiteral,
    /// A function-like macro invoked with the wrong number of arguments.
    MacroArgumentCount,
    /// A function-like macro invocation whose `)` never arrives.
    UnterminatedMacroInvocation,
    /// Expansion hit its step budget — self-referential input, or simply an
    /// enormous one. The remaining tokens are passed through unexpanded.
    ExpansionLimit,
    /// `##` at the start or end of a macro body, or a paste that does not
    /// produce a single token.
    BadTokenPaste,
    /// `#` in a macro body not followed by one of that macro's parameters.
    BadStringify,
}

impl DiagnosticCode for PpCode {
    fn as_str(self) -> &'static str {
        match self {
            PpCode::UnknownDirective => "GLSL0001",
            PpCode::VersionNotFirst => "GLSL0002",
            PpCode::MalformedVersion => "GLSL0003",
            PpCode::MalformedExtension => "GLSL0004",
            PpCode::MalformedLine => "GLSL0005",
            PpCode::ErrorDirective => "GLSL0006",
            PpCode::ExtraTokens => "GLSL0007",
            PpCode::MissingMacroName => "GLSL0008",
            PpCode::BadMacroParameter => "GLSL0009",
            PpCode::DuplicateMacroParameter => "GLSL0010",
            PpCode::UnterminatedMacroParameters => "GLSL0011",
            PpCode::MacroRedefined => "GLSL0012",
            PpCode::ReservedMacroName => "GLSL0013",
            PpCode::ReservedMacroNameWarning => "GLSL0014",
            PpCode::UnmatchedConditional => "GLSL0015",
            PpCode::MisplacedElse => "GLSL0016",
            PpCode::UnterminatedConditional => "GLSL0017",
            PpCode::BadConditionalExpression => "GLSL0018",
            PpCode::DivisionByZero => "GLSL0019",
            PpCode::BadConditionalLiteral => "GLSL0020",
            PpCode::MacroArgumentCount => "GLSL0021",
            PpCode::UnterminatedMacroInvocation => "GLSL0022",
            PpCode::ExpansionLimit => "GLSL0023",
            PpCode::BadTokenPaste => "GLSL0024",
            PpCode::BadStringify => "GLSL0025",
        }
    }
}

/// The carrier for everything the preprocessor reports.
pub type PpDiagnostic = Diagnostic<PpCode>;

/// What went wrong in the parser — GLSL 4.60 §9, as read by
/// [`crate::parser::parse`].
///
/// The parser recovers from all of these, so a diagnostic here never means the
/// tree is unusable; it means one construct in it is an
/// [`crate::cst::NodeKind::Error`] or is missing a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParseCode {
    /// A specific token was required here and something else was found.
    ExpectedToken,
    /// A declaration or a member needs a name and there is none.
    ExpectedIdentifier,
    /// A type specifier was required — after `precision`, or before a
    /// declarator.
    ExpectedType,
    /// An expression was required and the next token cannot start one.
    ExpectedExpression,
    /// A token that cannot begin what is expected here. Skipped into an
    /// `Error` node.
    UnexpectedToken,
    /// A `(`, `[` or `{` that the file ends without closing.
    UnclosedDelimiter,
    /// `layout(…)` holding something that is not `name` or `name = value`.
    MalformedLayout,
    /// An array specifier with no `]`, or with something unreadable inside.
    MalformedArraySpecifier,
    /// A whole declaration or statement the parser could not read.
    UnreadableDeclaration,
    /// The token stream ended in the middle of a construct.
    UnexpectedEndOfFile,
    /// The source nests deeper than the parser will recurse. The remainder of
    /// the construct becomes one `Error` node.
    NestingLimit,
}

impl DiagnosticCode for ParseCode {
    fn as_str(self) -> &'static str {
        match self {
            ParseCode::ExpectedToken => "GLSL0100",
            ParseCode::ExpectedIdentifier => "GLSL0101",
            ParseCode::ExpectedType => "GLSL0102",
            ParseCode::ExpectedExpression => "GLSL0103",
            ParseCode::UnexpectedToken => "GLSL0104",
            ParseCode::UnclosedDelimiter => "GLSL0105",
            ParseCode::MalformedLayout => "GLSL0106",
            ParseCode::MalformedArraySpecifier => "GLSL0107",
            ParseCode::UnreadableDeclaration => "GLSL0108",
            ParseCode::UnexpectedEndOfFile => "GLSL0109",
            ParseCode::NestingLimit => "GLSL0110",
        }
    }
}

/// The carrier for everything the parser reports.
pub type SyntaxDiagnostic = Diagnostic<ParseCode>;
