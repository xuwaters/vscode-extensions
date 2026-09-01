//! GLSL syntax: lexer, preprocessor and recovering parser.
//!
//! RFC 012 (docs/rfc/012-glsl-analyzer). Contracts live in
//! design/architecture.md; the preprocessor's provenance rules in decision
//! 0003. Tokenising never fails; parsing recovers and never panics.
//!
//! Three layers:
//!
//! - [`lexer::tokenize`] — lossless. Every byte of the source belongs to
//!   exactly one token or one piece of trivia, and no input can make it fail.
//! - [`preprocessor::preprocess`] — the real preprocessor: macros expanded,
//!   conditionals evaluated, `#version`/`#extension`/`#pragma`/`#line`
//!   recorded, and every token in the result still pointing at source bytes the
//!   user can see.
//! - [`parser::parse`] — the GLSL 4.60 grammar into the flat CST of
//!   [`cst::SyntaxTree`]. Recovers at `;`/`}` boundaries, tolerates unclosed
//!   groups, loses no token, and never panics on any input.
//!
//! ```
//! use glsl_syntax::preprocess_source;
//!
//! let pp = preprocess_source("#version 300 es\n#define HALF 0.5\nfloat x = HALF;\n");
//! assert!(pp.is_es());
//! // The `0.5` is attributed to the `HALF` that produced it, not to the
//! // `#define` the user is not looking at.
//! let half = pp.tokens.iter().find(|t| t.text == "0.5").unwrap();
//! assert_eq!(&"#version 300 es\n#define HALF 0.5\nfloat x = HALF;\n"
//!     [half.span.start as usize..half.span.end as usize], "HALF");
//! ```

pub mod cst;
pub mod diagnostics;
pub mod lexer;
pub mod outline;
pub mod parser;
pub mod preprocessor;

#[cfg(test)]
mod tests;

pub use cst::{Child, Node, NodeId, NodeKind, Piece, PieceKind, SyntaxTree, TokenId};
pub use diagnostics::{ParseCode, PpCode, PpDiagnostic, SyntaxDiagnostic};
pub use lexer::{Punct, Token, TokenKind, tokenize};
pub use outline::{Outline, Reference, Symbol, SymbolKind};
pub use parser::parse;
pub use preprocessor::{
    Directives, ExtensionBehaviour, ExtensionDirective, InactiveRegion, IncludeDirective,
    LineDirective, Origin, PpToken, PragmaDirective, PreprocessOptions, Preprocessed, Profile,
    VersionDirective, preprocess,
};

/// Lex and preprocess a source with no host-supplied predefines.
pub fn preprocess_source(source: &str) -> Preprocessed {
    let tokens = tokenize(source);
    preprocess(source, &tokens, &PreprocessOptions::default())
}

/// Lex, preprocess and parse a source in one step.
///
/// Both halves of the answer come back because they belong together: the tree
/// holds token *indices*, and only the [`Preprocessed`] knows what those tokens
/// say and which macro or conditional they came through.
pub fn parse_source(source: &str) -> (Preprocessed, SyntaxTree) {
    let pp = preprocess_source(source);
    let tree = parse(&pp);
    (pp, tree)
}
