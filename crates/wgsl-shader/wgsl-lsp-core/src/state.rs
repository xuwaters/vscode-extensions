//! Per-document server state.
//!
//! Two clocks, and keeping them apart is what makes the server responsive:
//!
//! | State | Rebuilt | Feeds |
//! | --- | --- | --- |
//! | the syntax layer | every edit, always | completion, hover, definition, symbols, folding, tokens |
//! | [`Analysis`] (naga, WGSL only) | lazily, on first ask after an edit | diagnostics, types, signature detail |
//!
//! The syntax parse is cheap and never fails, so it happens eagerly. naga is
//! neither, so it happens only when something actually needs it — which, with
//! `validate.onType` off, means on save rather than on keystroke.
//!
//! Which syntax layer depends on the language, and that is the one branch in
//! the whole document model:
//!
//! - **WGSL** parses with [`wgsl_syntax`] and validates with naga.
//! - **GLSL** preprocesses, parses and analyses with the `glsl-*` crates
//!   ([`crate::glsl`]), and naga never runs. The GLSL document also produces a
//!   [`Parsed`] projection so the feature layer's structural queries are the
//!   same code for both languages.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use analyzer_core::spans::{ByteSpan, SpanTable};
use lsp_types::{Position, Range, SemanticToken, Uri};
use naga::{Function, Module};
use wgsl_syntax::{Language, Parsed};

use glsl_spec::Version;

use crate::analysis::{Analysis, types};
use crate::convert;
use crate::glsl::GlslDocument;

/// An open document.
pub struct Document {
    pub uri: Uri,
    pub language: Language,
    pub version: i32,
    text: String,
    /// The file's extension without the dot, which is where a GLSL source's
    /// stage comes from when no `#pragma shader_stage` says otherwise.
    extension: String,
    /// The `#version` to analyse a GLSL file that declares none as, from
    /// `glsl.defaultVersion`. `None` follows the spec, which says 1.10.
    default_version: Option<Version>,
    lines: SpanTable,
    /// The structural view every feature reads. For WGSL it *is* the parse;
    /// for GLSL it is [`GlslDocument::parsed`], projected from the CST.
    parsed: Parsed,
    /// The GLSL pipeline's answers. `None` for a WGSL document.
    glsl: Option<Box<GlslDocument>>,
    analysis: OnceCell<Analysis>,
    /// The most recent module naga *did* parse.
    ///
    /// Load-bearing, not an optimisation. Completion after a `.` is requested
    /// with the document in a state no parser accepts — `camera.` is not a
    /// WGSL expression — so a server that only ever consults the current text
    /// can never type the value before the dot. The module from one keystroke
    /// ago names the same fields.
    last_good: RefCell<Option<Rc<Module>>>,
    pub tokens: TokenCache,
}

impl Document {
    pub fn new(
        uri: Uri,
        language: Language,
        version: i32,
        text: String,
        default_version: Option<Version>,
    ) -> Document {
        let extension = extension_of(&uri);
        let lines = SpanTable::new(&text);
        let (parsed, glsl) = build(&text, language, &extension, default_version);
        Document {
            uri,
            language,
            version,
            text,
            extension,
            default_version,
            lines,
            parsed,
            glsl,
            analysis: OnceCell::new(),
            last_good: RefCell::new(None),
            tokens: TokenCache::default(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The file's extension without the dot, which is what a GLSL source's
    /// stage falls back to when no `#pragma shader_stage` says otherwise.
    pub fn extension(&self) -> &str {
        &self.extension
    }

    pub fn lines(&self) -> &SpanTable {
        &self.lines
    }

    pub fn parsed(&self) -> &Parsed {
        &self.parsed
    }

    /// The GLSL pipeline's answers, for the features that can use more than
    /// the structural projection. `None` for a WGSL document.
    pub fn glsl(&self) -> Option<&GlslDocument> {
        self.glsl.as_deref()
    }

    /// Adopt a new `glsl.defaultVersion`, re-analysing under it.
    ///
    /// The version is read where the analysis is *made*, so a document that
    /// kept its answer would go on reporting the previous setting until its
    /// next edit. Returns whether anything changed.
    pub fn set_default_version(&mut self, default_version: Option<Version>) -> bool {
        if self.default_version == default_version {
            return false;
        }
        self.default_version = default_version;
        self.reparse();
        true
    }

    /// Replace the whole document.
    pub fn replace(&mut self, text: String, version: i32) {
        self.text = text;
        self.version = version;
        self.reparse();
    }

    /// Apply one incremental change.
    ///
    /// The range is resolved against the text as it stands *before* this edit,
    /// which is what LSP specifies when a `didChange` carries several.
    ///
    /// A position past the end of the document clamps to the end, as the
    /// protocol requires — VS Code sends one whenever the cursor is in virtual
    /// space. The guard below is for the case clamping cannot fix: a range
    /// that lands mid-scalar, which `replace_range` would panic on.
    pub fn edit(&mut self, range: Range, replacement: &str) {
        let span = convert::range_to_span(&self.text, &self.lines, range);
        let (start, end) = (span.start as usize, span.end as usize);
        if start > self.text.len()
            || end > self.text.len()
            || !self.text.is_char_boundary(start)
            || !self.text.is_char_boundary(end)
        {
            return;
        }
        self.text.replace_range(start..end, replacement);
        self.reparse();
    }

    fn reparse(&mut self) {
        self.lines = SpanTable::new(&self.text);
        let (parsed, glsl) =
            build(&self.text, self.language, &self.extension, self.default_version);
        self.parsed = parsed;
        self.glsl = glsl;
        // The naga analysis is now stale by definition; the next asker pays
        // for a fresh one.
        self.analysis = OnceCell::new();
    }

    /// What naga makes of the document as it currently stands.
    ///
    /// Authoritative for WGSL, and therefore what diagnostics publish: if the
    /// document does not parse right now, this says so. Empty for GLSL, which
    /// [`crate::glsl`] answers for instead.
    pub fn analysis(&self) -> &Analysis {
        let (text, language) = (&self.text, self.language);
        let last_good = &self.last_good;
        self.analysis.get_or_init(|| {
            if language != Language::Wgsl {
                return Analysis::empty();
            }
            let analysis = Analysis::run(text);
            if let Some(module) = &analysis.module {
                *last_good.borrow_mut() = Some(Rc::clone(module));
            }
            analysis
        })
    }

    /// A module to answer type questions from: the current one, or the last
    /// one that parsed.
    ///
    /// Everything *except* diagnostics wants this. A stale module names the
    /// same structs and fields as the current text in all but the rarest case,
    /// and the alternative — no answer at all whenever a line is half-typed —
    /// is the state completion is always requested in.
    pub fn module(&self) -> Option<Rc<Module>> {
        match &self.analysis().module {
            Some(module) => Some(Rc::clone(module)),
            None => self.last_good.borrow().clone(),
        }
    }

    /// The naga `Function` for the function containing `offset`.
    ///
    /// Going through the *name* rather than naga's spans is deliberate: the
    /// module may be a keystroke out of date, and a name survives edits that
    /// invalidate every span in the arena.
    pub fn naga_function_at<'a>(
        &self,
        module: &'a Module,
        offset: u32,
    ) -> Option<&'a Function> {
        let index = self.parsed.enclosing_function(offset)?;
        types::function_named(module, &self.parsed.symbols[index].name)
    }

    pub fn offset(&self, position: Position) -> u32 {
        convert::position_to_offset(&self.text, &self.lines, position)
    }

    pub fn position(&self, offset: u32) -> Position {
        convert::offset_to_position(&self.text, &self.lines, offset)
    }

    pub fn range(&self, span: ByteSpan) -> Range {
        convert::span_to_range(&self.text, &self.lines, span)
    }

    pub fn span(&self, range: Range) -> ByteSpan {
        convert::range_to_span(&self.text, &self.lines, range)
    }

    /// The source a span covers.
    pub fn slice(&self, span: ByteSpan) -> &str {
        self.text.get(span.start as usize..span.end as usize).unwrap_or("")
    }

    /// The whole document as one range, for a full-document edit.
    pub fn whole(&self) -> Range {
        self.range(ByteSpan::new(0, self.text.len() as u32))
    }
}

/// Run the syntax layer a language calls for.
///
/// GLSL builds the whole `glsl-*` pipeline and keeps the projection beside it;
/// WGSL parses as it always did.
fn build(
    text: &str,
    language: Language,
    extension: &str,
    default_version: Option<Version>,
) -> (Parsed, Option<Box<GlslDocument>>) {
    match language {
        Language::Wgsl => (wgsl_syntax::parse(text, language), None),
        Language::Glsl => {
            let (glsl, parsed) = GlslDocument::build(text, extension, default_version);
            (parsed, Some(Box::new(glsl)))
        }
    }
}

/// The extension of a `file:`-style URI, lower-cased and without the dot.
///
/// Query strings and fragments are stripped first: an embedded shader arrives
/// as `wgsl-embedded:/…/main.rs?block=2#frag`, and taking the extension from
/// the tail would produce nonsense.
fn extension_of(uri: &Uri) -> String {
    let text = uri.as_str();
    let text = text.split(['?', '#']).next().unwrap_or(text);
    let last = text.rsplit(['/', '\\']).next().unwrap_or(text);
    match last.rsplit_once('.') {
        Some((_, extension)) if !extension.is_empty() => extension.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// The previous semantic token array for a document, so `full/delta` can send
/// edits instead of thousands of tokens on every keystroke.
#[derive(Debug, Clone, Default)]
pub struct TokenCache {
    /// The id the client will quote when it asks for a delta.
    result_id: u64,
    /// The tokens that id refers to.
    pub tokens: Vec<SemanticToken>,
}

impl TokenCache {
    /// Store a new token array and return its result id.
    pub fn store(&mut self, tokens: Vec<SemanticToken>) -> String {
        self.result_id = self.result_id.wrapping_add(1);
        self.tokens = tokens;
        self.result_id.to_string()
    }

    /// Whether the client's quoted id still matches what we hold.
    pub fn matches(&self, result_id: &str) -> bool {
        result_id == self.result_id.to_string()
    }

    pub fn current_id(&self) -> String {
        self.result_id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(text: &str) -> Uri {
        text.parse().unwrap()
    }

    fn document(text: &str) -> Document {
        Document::new(uri("file:///shaders/test.wgsl"), Language::Wgsl, 1, text.to_string(), None)
    }

    /// The one branch in the document model: a GLSL document holds the whole
    /// `glsl-*` pipeline and never troubles naga.
    #[test]
    fn a_glsl_document_holds_the_glsl_pipeline_and_no_naga_module() {
        let document = Document::new(
            uri("file:///shaders/test.frag"),
            Language::Glsl,
            1,
            "#version 300 es\nprecision mediump float;\nuniform sampler2D albedo;\n\
             void main() {}\n"
                .to_string(),
            None,
        );
        let glsl = document.glsl().expect("the GLSL pipeline ran");
        assert_eq!(glsl.version().label(), "3.00 es");
        assert_eq!(glsl.stage_label(), "fragment");
        assert!(glsl.stage_known, "a .frag extension says which stage it is");
        assert!(glsl.analysis.errors().next().is_none());

        // naga is not consulted, so there is nothing for it to be wrong about.
        assert!(document.analysis().problems.is_empty());
        assert!(document.module().is_none());
        // …and the projection still answers the feature layer.
        assert_eq!(document.parsed().symbols[0].name, "albedo");
    }

    #[test]
    fn an_extension_comes_off_the_last_path_segment() {
        assert_eq!(extension_of(&uri("file:///a/b/test.frag")), "frag");
        assert_eq!(extension_of(&uri("file:///a/b/TEST.FRAG")), "frag");
        assert_eq!(extension_of(&uri("file:///a/b/noext")), "");
        assert_eq!(extension_of(&uri("file:///a.dir/noext")), "");
        // The query and fragment an embedded document carries must not count.
        assert_eq!(extension_of(&uri("wgsl-embedded:/a/main.rs?block=2")), "rs");
    }

    #[test]
    fn an_edit_splices_and_reparses() {
        let mut document = document("fn a() {}\n");
        let range = Range {
            start: Position { line: 0, character: 3 },
            end: Position { line: 0, character: 4 },
        };
        document.edit(range, "renamed");
        assert_eq!(document.text(), "fn renamed() {}\n");
        assert_eq!(document.parsed().symbols[0].name, "renamed");
    }

    /// The protocol clamps a position past the end of the document rather than
    /// rejecting it, so an edit there appends. Version tracking, not range
    /// checking, is what catches a client that has genuinely drifted.
    #[test]
    fn an_edit_past_the_end_clamps_to_the_end() {
        let mut document = document("fn a() {}\n");
        let range = Range {
            start: Position { line: 40, character: 0 },
            end: Position { line: 50, character: 0 },
        };
        document.edit(range, "fn b() {}\n");
        assert_eq!(document.text(), "fn a() {}\nfn b() {}\n");
        assert_eq!(document.parsed().symbols.len(), 2);
    }

    #[test]
    fn the_naga_analysis_is_recomputed_after_an_edit() {
        let mut document = document("@fragment fn f() -> @location(0) vec4f { return vec4f(1.0); }");
        assert!(document.analysis().problems.is_empty());

        document.replace("fn f() -> {".to_string(), 2);
        assert!(!document.analysis().problems.is_empty());
    }

    #[test]
    fn the_enclosing_naga_function_is_found_by_name() {
        let source = "fn helper(x: f32) -> f32 { let y = x * 2.0; return y; }\n";
        let document = document(source);
        let offset = source.find("let y").unwrap() as u32;
        let module = document.module().expect("naga parsed it");
        let function = document.naga_function_at(&module, offset).expect("inside helper");
        assert_eq!(function.name.as_deref(), Some("helper"));
    }

    /// Completion after a `.` is always requested on a document no parser
    /// accepts. Without a remembered module there would be nothing to type
    /// the value before the dot with.
    #[test]
    fn a_module_survives_an_edit_that_breaks_the_parse() {
        let mut document = document("struct S { a: f32 }\nvar s: S;\n");
        assert!(document.module().is_some());

        document.replace("struct S { a: f32 }\nvar s: S;\nfn f() { let x = s.".to_string(), 2);
        // The current analysis has nothing…
        assert!(document.analysis().module.is_none());
        assert!(!document.analysis().problems.is_empty());
        // …and the remembered one still names `S`.
        let module = document.module().expect("the last good module");
        assert!(module.types.iter().any(|(_, ty)| ty.name.as_deref() == Some("S")));
    }

    #[test]
    fn the_token_cache_only_matches_the_id_it_last_handed_out() {
        let mut cache = TokenCache::default();
        let first = cache.store(Vec::new());
        assert!(cache.matches(&first));
        let second = cache.store(Vec::new());
        assert!(!cache.matches(&first));
        assert!(cache.matches(&second));
    }
}
