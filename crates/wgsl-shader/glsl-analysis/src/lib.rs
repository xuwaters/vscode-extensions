//! Semantic analysis for GLSL.
//!
//! RFC 012, Phase 4 (docs/rfc/012-glsl-analyzer). Consumes glsl-syntax trees
//! and the glsl-spec tables; produces resolved references, expression types
//! and the diagnostics catalogue.
//!
//! ```
//! use analyzer_core::diagnostics::DiagnosticCode;
//! use glsl_analysis::{Options, analyze_source};
//!
//! let analysis = analyze_source(
//!     "#version 300 es\nprecision mediump float;\n\
//!      uniform sampler2D tex;\nin vec2 uv;\nout vec4 colour;\n\
//!      void main() { colour = texture(tex, uv); }\n",
//!     &Options::default(),
//! );
//! assert!(analysis.errors().next().is_none());
//!
//! // …and the same shader written for WebGL1 knows that `texture` is not
//! // the spelling that version has.
//! let old = analyze_source(
//!     "#version 100\nuniform sampler2D tex;\nvarying vec2 uv;\n\
//!      void main() { gl_FragColor = texture(tex, uv); }\n",
//!     &Options::default(),
//! );
//! let first = old.errors().next().unwrap();
//! assert_eq!(first.code.as_str(), "GLSL0223");
//! assert!(first.message.contains("texture2D"));
//! ```
//!
//! ## What it promises
//!
//! - **It never panics.** Any input, any nesting; the corpus gate proves it.
//! - **It stays quiet when it is unsure.** A file whose parse or preprocessing
//!   failed, or which `#include`s something we never followed, gets types and
//!   resolution and no error-severity diagnostics at all. A false error in an
//!   editor is worse than a missed one, and every rule here is written that way
//!   round: unknown types, extension names and unmodelled builtins all mean
//!   silence.
//! - **Every diagnostic has a stable `GLSL02xx` code** with a seeded fixture
//!   that produces exactly it. See [`diagnostics`] and design/diagnostics.md.

mod analyzer;
mod body;
mod builtins;
mod calls;
mod consteval;
mod context;
mod conversions;
mod diagnostics;
mod expr;
mod symbols;
mod types;

#[cfg(test)]
mod tests;

use analyzer_core::diagnostics::Severity;
use analyzer_core::spans::ByteSpan;
use glsl_spec::{BuiltinFunction, BuiltinVariable, LegacyFunction, LegacyVariable};
use glsl_syntax::{Preprocessed, SyntaxTree};

pub use builtins::{FoundFunction, FoundVariable, is_builtin, lookup_function, lookup_variable};
pub use context::{Context, Options};
pub use conversions::{Constructed, common_type, construct, conversion_cost,
    implicitly_convertible};
pub use diagnostics::{SemanticCode, SemanticDiagnostic};
pub use expr::{Const, Value};
pub use symbols::{
    Parameter, Qualifiers, Signature, Symbol, SymbolId, SymbolKind, SymbolTable,
};
pub use types::{Field, Scalar, StructDef, StructId, StructTable, Type};

/// What an identifier occurrence turned out to name.
#[derive(Debug, Clone)]
pub enum Target {
    /// Something the file declares.
    Symbol(SymbolId),
    BuiltinFunction(&'static BuiltinFunction),
    BuiltinVariable(&'static BuiltinVariable),
    /// A compatibility-profile or ES 1.00 builtin, from the hand-written table
    /// (decision 0007).
    LegacyFunction(&'static LegacyFunction),
    LegacyVariable(&'static LegacyVariable),
    /// A type name — the callee of a constructor, or a type specifier.
    Type(Type),
    /// A member of a struct or an interface block.
    Field { owner: StructId, index: usize },
    /// A vector component selection, which names no declaration at all.
    Swizzle,
    /// A name this analysis could not place. Recorded rather than dropped: an
    /// editor still has to answer for it, and a `gl_`-prefixed or
    /// extension-suffixed name lands here without ever being an error.
    Unresolved,
}

/// One identifier occurrence and what it names.
#[derive(Debug, Clone)]
pub struct ResolvedRef {
    /// Real source bytes — never a macro body's, per decision 0003.
    pub span: ByteSpan,
    pub target: Target,
}

/// Everything semantic analysis knows about a file.
#[derive(Debug, Clone)]
pub struct Analysis {
    /// The dialect the file was analysed as.
    pub context: Context,
    pub symbols: SymbolTable,
    pub structs: StructTable,
    /// Sorted by span.
    pub diagnostics: Vec<SemanticDiagnostic>,
    /// One entry per tree node, indexed by `NodeId`: the type of the expression
    /// there, or [`Type::Unknown`].
    pub types: Vec<Type>,
    /// Sorted by span, one entry per identifier occurrence.
    pub references: Vec<ResolvedRef>,
}

impl Analysis {
    /// Just the errors — what an editor paints red.
    pub fn errors(&self) -> impl Iterator<Item = &SemanticDiagnostic> {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error)
    }

    /// The type of the expression at a node, if it has one.
    pub fn type_at(&self, node: glsl_syntax::NodeId) -> Option<&Type> {
        self.types.get(node.index()).filter(|ty| !ty.is_unknown())
    }

    /// What the identifier at this offset names.
    pub fn reference_at(&self, offset: u32) -> Option<&ResolvedRef> {
        self.references.iter().find(|r| r.span.contains(offset))
    }

    /// The symbol at this offset, declaration site or use.
    pub fn symbol_at(&self, offset: u32) -> Option<(SymbolId, &Symbol)> {
        match self.reference_at(offset)?.target {
            Target::Symbol(id) => self.symbols.get(id).map(|symbol| (id, symbol)),
            _ => None,
        }
    }

    /// A type's printable name, which needs the struct table this analysis
    /// built.
    pub fn type_name(&self, ty: &Type) -> String {
        ty.name(&self.structs)
    }
}

/// Analyse a parsed source.
///
/// The source text itself is not needed: every span comes from the tree and
/// every spelling from the preprocessor's token stream, which is the only one
/// that knows what a macro expanded to.
pub fn analyze(tree: &SyntaxTree, pp: &Preprocessed, options: &Options) -> Analysis {
    let mut analyzer = analyzer::Analyzer::new(tree, pp, options);
    analyzer.collect_globals();
    analyzer.walk_bodies();
    analyzer.finish()
}

/// Preprocess, parse and analyse in one step. The convenience every test and
/// every doctest uses; the server holds the tree and calls [`analyze`].
pub fn analyze_source(source: &str, options: &Options) -> Analysis {
    let (pp, tree) = glsl_syntax::parse_source(source);
    analyze(&tree, &pp, options)
}
