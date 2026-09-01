//! The GLSL side of the server.
//!
//! RFC 012 phase 5. For a `Language::Glsl` document the server holds a real
//! preprocessed token stream, a real CST and a real semantic analysis instead
//! of the heuristic walk plus naga that came before —
//! [design/architecture.md](../../../../../docs/rfc/012-glsl-analyzer/design/architecture.md)
//! "Integration into `wgsl-lsp-core`".
//!
//! Two layers, and the split is what keeps the change small:
//!
//! - [`GlslDocument`] holds everything the new pipeline produces: the lossless
//!   token stream, the [`Preprocessed`] record, the [`SyntaxTree`], the
//!   [`Outline`] and the [`glsl_analysis::Analysis`].
//! - [`adapter`] projects the outline onto the shapes the feature layer
//!   already consumes for WGSL — a [`wgsl_syntax::Parsed`] of symbols, scopes,
//!   references and blocks. The kinds line up name for name, so it is a
//!   mapping rather than a translation.
//!
//! Features then answer structural questions from the projection exactly as
//! they did before, and reach *into* [`GlslDocument`] for the answers only the
//! new pipeline can give: typed hovers, real overload sets, availability
//! filtering, resolved semantic-token kinds.

pub mod adapter;

use analyzer_core::spans::ByteSpan;
use glsl_analysis::{Analysis as SemanticAnalysis, Context, Options};
use glsl_spec::{Stage, Version};
use glsl_syntax::outline::Outline;
use glsl_syntax::{Preprocessed, SyntaxTree, Token};
use wgsl_syntax::Parsed;

use crate::analysis::stage;

/// The `#version` a `glsl.defaultVersion` setting names, as the directive
/// spells it: `450`, `330 core`, `300 es`.
///
/// An empty or unreadable value is `None`, which means "follow the spec" —
/// GLSL 1.10. Unreadable rather than rejected on purpose: a typo in a setting
/// must not stop the server analysing anything.
pub fn parse_version(text: &str) -> Option<Version> {
    let mut words = text.split_whitespace();
    let number: u16 = words.next()?.parse().ok()?;
    let profile = words.next();
    Version::from_directive(number, profile == Some("es"))
}

/// Everything the GLSL pipeline knows about one open document.
pub struct GlslDocument {
    /// The lossless token stream: every byte of the source belongs to exactly
    /// one entry, comments and whitespace included.
    pub raw: Vec<Token>,
    pub pp: Preprocessed,
    pub tree: SyntaxTree,
    pub outline: Outline,
    pub analysis: SemanticAnalysis,
    /// Whether the stage was told to us — by `#pragma shader_stage` or the file
    /// extension — rather than guessed from the builtins the source uses.
    pub stage_known: bool,
}

impl GlslDocument {
    /// Preprocess, parse and analyse a source, and project it onto the shapes
    /// the feature layer consumes.
    ///
    /// `extension` is the file's extension without the dot; it is where the
    /// stage comes from when no `#pragma shader_stage` says otherwise.
    pub fn build(
        source: &str,
        extension: &str,
        default_version: Option<Version>,
    ) -> (GlslDocument, Parsed) {
        let told = stage::resolve_stage(source, extension);
        let raw = glsl_syntax::tokenize(source);
        let pp = glsl_syntax::preprocess(
            source,
            &raw,
            &glsl_syntax::PreprocessOptions::default(),
        );
        let tree = glsl_syntax::parse(&pp);
        let outline = glsl_syntax::outline::outline(&tree, &pp, source);
        let analysis =
            glsl_analysis::analyze(&tree, &pp, &Options { stage: told, default_version });
        let parsed = adapter::to_parsed(source, &raw, &outline);
        let document = GlslDocument {
            raw,
            pp,
            tree,
            outline,
            analysis,
            stage_known: told.is_some(),
        };
        (document, parsed)
    }

    /// The structural projection alone: lex, preprocess, parse, outline — no
    /// semantic analysis.
    ///
    /// What the workspace index wants. It keeps names and spans for files the
    /// editor never opened, and paying for type inference on a thousand
    /// shaders to produce a symbol list nobody has asked about would be the
    /// wrong trade.
    pub fn project(source: &str) -> Parsed {
        // Indexing needs names and spans, and a version decides neither.
        let raw = glsl_syntax::tokenize(source);
        let pp = glsl_syntax::preprocess(
            source,
            &raw,
            &glsl_syntax::PreprocessOptions::default(),
        );
        let tree = glsl_syntax::parse(&pp);
        let outline = glsl_syntax::outline::outline(&tree, &pp, source);
        adapter::to_parsed(source, &raw, &outline)
    }

    /// The dialect the file was analysed as.
    pub fn context(&self) -> Context {
        self.analysis.context
    }

    pub fn version(&self) -> Version {
        self.analysis.context.version
    }

    pub fn stage(&self) -> Stage {
        self.analysis.context.stage
    }

    /// The stage as the status bar and `#pragma shader_stage` spell it.
    pub fn stage_label(&self) -> &'static str {
        self.analysis.context.stage.label()
    }

    /// The `#extension` names the file enabled, which relax several rules.
    pub fn extensions(&self) -> impl Iterator<Item = &str> {
        self.pp.directives.extensions.iter().map(|e| e.name.as_str())
    }

    /// The macro definition whose name is at `offset`, or which is named
    /// there.
    pub fn macro_at(&self, name: &str) -> Option<&glsl_syntax::preprocessor::macros::MacroDef> {
        self.pp.macros.all().iter().find(|def| def.name == name && !def.predefined)
    }

    /// Whether `offset` lies inside a region a conditional switched off.
    pub fn is_inactive(&self, offset: u32) -> bool {
        self.pp.is_inactive(offset)
    }

    /// The innermost CST node covering `offset`, in *source* bytes.
    ///
    /// Macro-expanded tokens all carry their invocation's span, so an offset
    /// inside an invocation lands on the node the expansion produced — which
    /// is the node a hover over that invocation should describe.
    pub fn node_at(&self, offset: u32) -> glsl_syntax::NodeId {
        self.tree.node_at(offset)
    }

    /// The type analysis gave the expression at `offset`, if it gave one.
    ///
    /// Walks outwards from the innermost node: a bare identifier is a
    /// `NodeKind::Name` inside an `IdentExpr`, and it is the expression that
    /// carries the type.
    pub fn type_at(&self, offset: u32) -> Option<&glsl_analysis::Type> {
        let mut node = self.tree.node_at(offset);
        for _ in 0..4 {
            if let Some(ty) = self.analysis.type_at(node) {
                return Some(ty);
            }
            let parent = self.tree.node(node).parent;
            if parent == node {
                break;
            }
            node = parent;
        }
        None
    }

    /// A type's printable name.
    pub fn type_name(&self, ty: &glsl_analysis::Type) -> String {
        self.analysis.type_name(ty)
    }

    /// The source a span covers, for the adapter's own bookkeeping.
    pub fn slice<'a>(&self, source: &'a str, span: ByteSpan) -> &'a str {
        source.get(span.start as usize..span.end as usize).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glsl_spec::{DesktopVersion, EsVersion};

    #[test]
    fn a_default_version_setting_is_read_the_way_the_directive_is_written() {
        assert_eq!(parse_version("450"), Some(Version::Desktop(DesktopVersion::V450)));
        assert_eq!(parse_version("330 core"), Some(Version::Desktop(DesktopVersion::V330)));
        assert_eq!(parse_version("300 es"), Some(Version::Es(EsVersion::V300)));
        // Nothing, and nonsense, both mean "follow the spec".
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("nonsense"), None);
        assert_eq!(parse_version("999"), None);
    }

    /// The setting only speaks for a file that declares nothing itself.
    #[test]
    fn a_declared_version_always_beats_the_default() {
        let told = Some(Version::Es(EsVersion::V300));
        let (glsl, _) = GlslDocument::build("void main() {}\n", "frag", told);
        assert_eq!(glsl.version(), Version::Es(EsVersion::V300));

        let (glsl, _) = GlslDocument::build("#version 450\nvoid main() {}\n", "frag", told);
        assert_eq!(glsl.version(), Version::Desktop(DesktopVersion::V450));
    }
}
