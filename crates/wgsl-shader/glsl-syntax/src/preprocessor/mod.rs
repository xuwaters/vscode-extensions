//! The GLSL preprocessor — GLSL 4.60 §3.3, with the rules of decision 0003.
//!
//! [`preprocess`] turns a lexed source into a [`Preprocessed`]: the live token
//! stream with every macro expanded and every token still pointing at real
//! source bytes, plus everything an editor wants to know *about* the
//! preprocessing — which version and extensions the file declared, what each
//! macro is and where it was defined, and which byte ranges the conditionals
//! switched off.
//!
//! Two things shape the implementation:
//!
//! - **Directives are line-oriented, code is not.** The driver walks the token
//!   stream one directive at a time and buffers the code between directives
//!   into a *segment*, which it expands as a unit. That is what lets a call
//!   spread over several lines while still guaranteeing that a `#define` on the
//!   line above is in force and one on the line below is not.
//! - **Inactive branches are recorded, not deleted.** Only the live branch
//!   reaches [`Preprocessed::tokens`], but every dead branch is in
//!   [`Preprocessed::inactive`] with its span, so folding, highlighting and the
//!   outline can still reach the code the user is editing inside an `#else`.
//!
//! Nothing here panics. Malformed input produces diagnostics and the most
//! useful stream we can still build from it.

pub mod expand;
pub mod expr;
pub mod macros;

use analyzer_core::spans::{ByteSpan, SpanTable};

use crate::diagnostics::{PpCode, PpDiagnostic};
use crate::lexer::{Punct, Token, TokenKind};
use expand::ExpandCtx;
use macros::{MacroDef, MacroKind, MacroTable, Reserved};

/// The version assumed when a file declares none — GLSL 1.10, per §3.3.
pub const DEFAULT_VERSION: i64 = 110;

/// Where a token in the expanded stream came from.
///
/// This is the provenance record decision 0003 requires, and it is what every
/// downstream feature reads before it decides whether a span is somewhere the
/// user can usefully be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Lexed straight out of the source; `span` is exactly this token.
    Source,
    /// Substituted from a macro body, or synthesised by `#`/`##`. `span` is the
    /// whole invocation that produced it, never the `#define`.
    MacroBody,
    /// Passed in as (part of) a macro argument. `span` is where the user wrote
    /// it, so it is as good as a source token.
    MacroArg,
}

impl Origin {
    /// Whether `span` is text the user actually wrote, which is the question
    /// rename and go-to-definition need answered.
    pub fn is_written(self) -> bool {
        matches!(self, Origin::Source | Origin::MacroArg)
    }
}

/// A token in the expanded stream, or in a macro body.
///
/// Carries its own text because expansion can synthesise tokens (`##`, `#`,
/// `__LINE__`) that exist in no source slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PpToken {
    pub kind: TokenKind,
    /// The source bytes this token is attributed to. Always a real range in the
    /// original source; see [`Origin`] for what it means.
    pub span: ByteSpan,
    pub origin: Origin,
    /// Whether whitespace, a comment or a newline separated this token from the
    /// one before it. Preserved because `#` stringification and macro
    /// redefinition comparison both depend on it.
    pub leading_space: bool,
    pub text: String,
}

/// Which profile a `#version` line named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Core,
    Compatibility,
    Es,
}

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Core => "core",
            Profile::Compatibility => "compatibility",
            Profile::Es => "es",
        }
    }
}

/// How a `#extension` line asked for an extension to be treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionBehaviour {
    Require,
    Enable,
    Warn,
    Disable,
}

impl ExtensionBehaviour {
    fn parse(text: &str) -> Option<ExtensionBehaviour> {
        match text {
            "require" => Some(ExtensionBehaviour::Require),
            "enable" => Some(ExtensionBehaviour::Enable),
            "warn" => Some(ExtensionBehaviour::Warn),
            "disable" => Some(ExtensionBehaviour::Disable),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ExtensionBehaviour::Require => "require",
            ExtensionBehaviour::Enable => "enable",
            ExtensionBehaviour::Warn => "warn",
            ExtensionBehaviour::Disable => "disable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionDirective {
    pub number: i64,
    /// The profile as written. `None` means the file named none, which is not
    /// the same as [`Preprocessed::profile`], where `#version 100` implies ES.
    pub profile: Option<Profile>,
    /// The whole directive line.
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionDirective {
    /// The extension name, or `all`.
    pub name: String,
    pub behaviour: ExtensionBehaviour,
    pub name_span: ByteSpan,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PragmaDirective {
    /// Everything after `#pragma`, verbatim.
    pub text: String,
    pub span: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDirective {
    /// The number the directive set. The *following* line takes this number.
    pub line: i64,
    pub source_string: Option<i64>,
    pub span: ByteSpan,
}

/// An `#include`, recorded and deliberately not followed (RFC 012 §10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeDirective {
    /// The path as written, quotes or angle brackets stripped.
    pub path: String,
    pub span: ByteSpan,
}

/// Everything the file declared about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Directives {
    pub version: Option<VersionDirective>,
    pub extensions: Vec<ExtensionDirective>,
    pub pragmas: Vec<PragmaDirective>,
    pub lines: Vec<LineDirective>,
    pub includes: Vec<IncludeDirective>,
}

/// A region a conditional switched off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InactiveRegion {
    /// The skipped bytes: from just after the controlling directive's newline
    /// up to the `#` of the directive that ends the branch.
    pub span: ByteSpan,
    /// The `#if`/`#elif`/`#else` line that switched it off.
    pub directive: ByteSpan,
}

/// Host-supplied preprocessing state — the LSP's equivalent of `-D`.
#[derive(Debug, Clone, Default)]
pub struct PreprocessOptions {
    /// Object-like macros defined before the first source line, as
    /// `(name, replacement text)`.
    pub predefines: Vec<(String, String)>,
}

/// The preprocessor's whole answer.
#[derive(Debug, Clone)]
pub struct Preprocessed {
    /// The live token stream, fully expanded, trivia removed. This is what the
    /// parser consumes.
    pub tokens: Vec<PpToken>,
    pub directives: Directives,
    /// Every macro the file defined, live or since retired, with the spans
    /// go-to-definition needs.
    pub macros: MacroTable,
    pub inactive: Vec<InactiveRegion>,
    pub diagnostics: Vec<PpDiagnostic>,
}

impl Preprocessed {
    /// The version in force, defaulting to GLSL 1.10 when the file declares
    /// none.
    pub fn version(&self) -> i64 {
        self.directives.version.as_ref().map_or(DEFAULT_VERSION, |v| v.number)
    }

    /// The profile in force. `#version 100` is ES whether or not it says so;
    /// every other version without a profile is core.
    pub fn profile(&self) -> Profile {
        match &self.directives.version {
            Some(v) => v
                .profile
                .unwrap_or(if v.number == 100 { Profile::Es } else { Profile::Core }),
            None => Profile::Core,
        }
    }

    pub fn is_es(&self) -> bool {
        self.profile() == Profile::Es
    }

    /// Whether `offset` falls inside a branch the conditionals switched off.
    pub fn is_inactive(&self, offset: u32) -> bool {
        self.inactive.iter().any(|r| r.span.contains_strict(offset))
    }
}

/// Preprocess a lexed source.
pub fn preprocess(source: &str, tokens: &[Token], options: &PreprocessOptions) -> Preprocessed {
    Preprocessor::new(source, tokens, options).run()
}

/// One open `#if` group.
struct Frame {
    /// The `#if`/`#ifdef`/`#ifndef` that opened the group.
    open: ByteSpan,
    /// Whether any branch of this group has already been taken.
    taken: bool,
    /// Whether the branch we are in right now is live, ignoring ancestors.
    live: bool,
    /// Whether everything outside this group is live.
    parent_live: bool,
    seen_else: bool,
    /// Where the current dead branch's content starts, when it is dead and
    /// visible (a dead branch inside a dead branch is covered by the outer
    /// region already).
    dead_from: Option<u32>,
    /// The directive that switched the current dead branch off.
    dead_directive: ByteSpan,
}

struct Preprocessor<'a> {
    src: &'a str,
    tokens: &'a [Token],
    lines: SpanTable,
    out: Vec<PpToken>,
    /// Code tokens buffered since the last directive.
    segment: Vec<PpToken>,
    macros: MacroTable,
    frames: Vec<Frame>,
    directives: Directives,
    inactive: Vec<InactiveRegion>,
    diagnostics: Vec<PpDiagnostic>,
    version: i64,
    profile: Option<Profile>,
    line_offset: i64,
    file_number: i64,
    /// Whether `#version` may still legally appear.
    version_still_allowed: bool,
}

impl<'a> Preprocessor<'a> {
    fn new(src: &'a str, tokens: &'a [Token], options: &PreprocessOptions) -> Self {
        let mut pp = Preprocessor {
            src,
            tokens,
            lines: SpanTable::new(src),
            out: Vec::with_capacity(tokens.len() / 2 + 1),
            segment: Vec::new(),
            macros: MacroTable::new(),
            frames: Vec::new(),
            directives: Directives::default(),
            inactive: Vec::new(),
            diagnostics: Vec::new(),
            version: DEFAULT_VERSION,
            profile: None,
            line_offset: 0,
            file_number: 0,
            version_still_allowed: true,
        };
        for name in macros::DYNAMIC_MACROS {
            pp.predefine(name, MacroKind::Dynamic, Vec::new());
        }
        for (name, body) in &options.predefines {
            let body = pp.tokens_of(body);
            pp.predefine(name, MacroKind::Object, body);
        }
        pp
    }

    /// Lex a replacement text that is not part of the source. Its tokens carry
    /// empty spans; expansion attributes them to the invocation anyway.
    fn tokens_of(&self, text: &str) -> Vec<PpToken> {
        crate::lexer::tokenize(text)
            .iter()
            .filter(|t| !t.kind.is_trivia())
            .enumerate()
            .map(|(i, t)| PpToken {
                kind: t.kind,
                span: ByteSpan::EMPTY,
                origin: Origin::MacroBody,
                leading_space: i > 0,
                text: t.text(text).into_owned(),
            })
            .collect()
    }

    fn predefine(&mut self, name: &str, kind: MacroKind, body: Vec<PpToken>) {
        let id = self.macros.id_for(name);
        self.macros.define(MacroDef {
            id,
            name: name.to_string(),
            kind,
            params: Vec::new(),
            body,
            span: ByteSpan::EMPTY,
            name_span: ByteSpan::EMPTY,
            predefined: true,
            undefined_at: None,
        });
    }

    fn run(mut self) -> Preprocessed {
        let mut i = 0;
        let mut leading_space = true;
        while i < self.tokens.len() {
            let token = self.tokens[i];
            if token.kind.is_trivia() {
                leading_space = true;
                i += 1;
                continue;
            }
            if token.at_line_start && token.kind == TokenKind::Punct(Punct::Hash) {
                i = self.directive(i);
                leading_space = true;
                continue;
            }
            if self.active() {
                self.version_still_allowed = false;
                let text = token.text(self.src).into_owned();
                self.segment.push(PpToken {
                    kind: token.kind,
                    span: token.span,
                    origin: Origin::Source,
                    leading_space,
                    text,
                });
            }
            leading_space = false;
            i += 1;
        }
        self.flush_segment();
        let end = self.src.len() as u32;
        while let Some(frame) = self.frames.last() {
            let open = frame.open;
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::UnterminatedConditional,
                "this conditional is never closed by an '#endif'",
                open,
            ));
            self.end_branch(end);
            self.frames.pop();
        }
        Preprocessed {
            tokens: self.out,
            directives: self.directives,
            macros: self.macros,
            inactive: self.inactive,
            diagnostics: self.diagnostics,
        }
    }

    /// Whether code at the cursor is inside a live branch.
    fn active(&self) -> bool {
        self.frames.last().is_none_or(|f| f.parent_live && f.live)
    }

    /// Expand and emit everything buffered since the last directive.
    fn flush_segment(&mut self) {
        if self.segment.is_empty() {
            return;
        }
        let segment = std::mem::take(&mut self.segment);
        let ctx = ExpandCtx {
            source: self.src,
            lines: &self.lines,
            line_offset: self.line_offset,
            file_number: self.file_number,
            version: self.version,
        };
        let expanded = expand::expand(&self.macros, &ctx, segment, &mut self.diagnostics);
        self.out.extend(expanded);
    }

    /// Handle the directive whose `#` is at token index `hash`. Returns the
    /// index of the first token on the next line.
    fn directive(&mut self, hash: usize) -> usize {
        self.flush_segment();
        let hash_span = self.tokens[hash].span;
        let mut args: Vec<PpToken> = Vec::new();
        let mut i = hash + 1;
        let mut leading_space = false;
        let mut end = hash_span.end;
        while i < self.tokens.len() {
            let token = self.tokens[i];
            if token.kind == TokenKind::Newline {
                i += 1;
                break;
            }
            if token.kind.is_trivia() {
                leading_space = true;
                i += 1;
                continue;
            }
            end = token.span.end;
            args.push(PpToken {
                kind: token.kind,
                span: token.span,
                origin: Origin::Source,
                leading_space,
                text: token.text(self.src).into_owned(),
            });
            leading_space = false;
            i += 1;
        }
        // Where the skipped content of a branch this directive opens begins.
        let next_line = self.tokens.get(i).map_or(self.src.len() as u32, |t| t.span.start);
        let span = ByteSpan::new(hash_span.start, end);
        let name = match args.first() {
            Some(t) if t.kind == TokenKind::Ident => t.text.clone(),
            // A bare `#` is the null directive, and legal.
            None => return i,
            Some(t) => {
                if self.active() {
                    self.diagnostics.push(PpDiagnostic::error(
                        PpCode::UnknownDirective,
                        format!("'#{}' is not a preprocessor directive", t.text),
                        span,
                    ));
                }
                return i;
            }
        };
        let body = &args[1..];
        match name.as_str() {
            "if" | "ifdef" | "ifndef" => self.open_conditional(&name, body, span, next_line),
            "elif" => self.elif(body, span, next_line, hash_span.start),
            "else" => self.else_branch(body, span, next_line, hash_span.start),
            "endif" => self.endif(body, span, hash_span.start),
            // Everything else is inert inside a branch that is switched off:
            // a dead `#error` must not fire and a dead `#define` must not land.
            _ if !self.active() => {}
            "define" => self.define(body, span),
            "undef" => self.undef(body, span),
            "version" => self.version_directive(body, span),
            "extension" => self.extension(body, span),
            "pragma" => self.pragma(body, span),
            "line" => self.line_directive(body, span),
            "error" => self.error_directive(body, span),
            "include" => self.include(body, span),
            other => self.diagnostics.push(PpDiagnostic::error(
                PpCode::UnknownDirective,
                format!("'#{other}' is not a preprocessor directive"),
                span,
            )),
        }
        if !matches!(name.as_str(), "version") {
            self.version_still_allowed = false;
        }
        i
    }

    fn extra_tokens(&mut self, rest: &[PpToken], directive: &str) {
        if let Some(first) = rest.first() {
            self.diagnostics.push(PpDiagnostic::warning(
                PpCode::ExtraTokens,
                format!("'#{directive}' ignores everything after this"),
                ByteSpan::new(first.span.start, rest[rest.len() - 1].span.end),
            ));
        }
    }

    // -- conditionals ------------------------------------------------------

    fn evaluate(&mut self, args: &[PpToken], span: ByteSpan) -> bool {
        let ctx = ExpandCtx {
            source: self.src,
            lines: &self.lines,
            line_offset: self.line_offset,
            file_number: self.file_number,
            version: self.version,
        };
        expr::condition(args, &self.macros, &ctx, span, &mut self.diagnostics)
    }

    fn open_conditional(&mut self, name: &str, args: &[PpToken], span: ByteSpan, next_line: u32) {
        let parent_live = self.active();
        let live = if !parent_live {
            false
        } else if name == "if" {
            self.evaluate(args, span)
        } else {
            let defined = match args.first() {
                Some(t) if t.kind == TokenKind::Ident => {
                    self.extra_tokens(&args[1..], name);
                    self.macros.is_defined(&t.text)
                }
                _ => {
                    self.diagnostics.push(PpDiagnostic::error(
                        PpCode::MissingMacroName,
                        format!("'#{name}' must be followed by a macro name"),
                        span,
                    ));
                    false
                }
            };
            if name == "ifdef" { defined } else { !defined }
        };
        self.frames.push(Frame {
            open: span,
            taken: live,
            live,
            parent_live,
            seen_else: false,
            dead_from: None,
            dead_directive: span,
        });
        self.begin_branch(span, next_line);
    }

    fn elif(&mut self, args: &[PpToken], span: ByteSpan, next_line: u32, hash: u32) {
        let Some(frame) = self.frames.last() else {
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::UnmatchedConditional,
                "'#elif' without a matching '#if'",
                span,
            ));
            return;
        };
        let (parent_live, taken, seen_else) = (frame.parent_live, frame.taken, frame.seen_else);
        self.end_branch(hash);
        if seen_else {
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::MisplacedElse,
                "'#elif' after '#else' in the same conditional",
                span,
            ));
        }
        // A branch whose group is already decided is not evaluated at all, so
        // a division by zero in a dead `#elif` stays quiet.
        let live = parent_live && !taken && self.evaluate(args, span);
        if let Some(frame) = self.frames.last_mut() {
            frame.live = live;
            frame.taken |= live;
        }
        self.begin_branch(span, next_line);
    }

    fn else_branch(&mut self, args: &[PpToken], span: ByteSpan, next_line: u32, hash: u32) {
        self.extra_tokens(args, "else");
        let Some(frame) = self.frames.last() else {
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::UnmatchedConditional,
                "'#else' without a matching '#if'",
                span,
            ));
            return;
        };
        let (parent_live, taken, seen_else) = (frame.parent_live, frame.taken, frame.seen_else);
        self.end_branch(hash);
        if seen_else {
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::MisplacedElse,
                "a conditional can only have one '#else'",
                span,
            ));
        }
        let live = parent_live && !taken;
        if let Some(frame) = self.frames.last_mut() {
            frame.live = live;
            frame.taken |= live;
            frame.seen_else = true;
        }
        self.begin_branch(span, next_line);
    }

    fn endif(&mut self, args: &[PpToken], span: ByteSpan, hash: u32) {
        self.extra_tokens(args, "endif");
        if self.frames.is_empty() {
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::UnmatchedConditional,
                "'#endif' without a matching '#if'",
                span,
            ));
            return;
        }
        self.end_branch(hash);
        self.frames.pop();
    }

    /// Start recording a dead branch, if this one is dead and visible.
    fn begin_branch(&mut self, directive: ByteSpan, next_line: u32) {
        if let Some(frame) = self.frames.last_mut() {
            if frame.parent_live && !frame.live {
                frame.dead_from = Some(next_line);
                frame.dead_directive = directive;
            }
        }
    }

    /// Close the dead branch being recorded, if any, at byte `at`.
    fn end_branch(&mut self, at: u32) {
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        let Some(start) = frame.dead_from.take() else {
            return;
        };
        let directive = frame.dead_directive;
        if at > start {
            self.inactive.push(InactiveRegion { span: ByteSpan::new(start, at), directive });
        }
    }

    // -- declarations ------------------------------------------------------

    fn version_directive(&mut self, args: &[PpToken], span: ByteSpan) {
        if !self.version_still_allowed {
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::VersionNotFirst,
                "'#version' must be the first thing in the file, comments aside",
                span,
            ));
        }
        self.version_still_allowed = false;
        let Some(number) = args.first().filter(|t| t.kind == TokenKind::Int) else {
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::MalformedVersion,
                "'#version' must be followed by a version number",
                span,
            ));
            return;
        };
        let Ok(number) = number.text.trim_end_matches(['u', 'U']).parse::<i64>() else {
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::MalformedVersion,
                format!("'{}' is not a version number", number.text),
                number.span,
            ));
            return;
        };
        let mut profile = None;
        let mut rest = &args[1..];
        if let Some(token) = rest.first() {
            match token.text.as_str() {
                "core" => profile = Some(Profile::Core),
                "compatibility" => profile = Some(Profile::Compatibility),
                "es" => profile = Some(Profile::Es),
                other => self.diagnostics.push(PpDiagnostic::error(
                    PpCode::MalformedVersion,
                    format!("'{other}' is not a profile; expected core, compatibility or es"),
                    token.span,
                )),
            }
            rest = &rest[1..];
        }
        self.extra_tokens(rest, "version");
        self.version = number;
        self.profile = profile.or(if number == 100 { Some(Profile::Es) } else { None });
        if self.profile == Some(Profile::Es) {
            let body = self.tokens_of("1");
            self.predefine("GL_ES", MacroKind::Object, body);
        }
        self.directives.version = Some(VersionDirective { number, profile, span });
    }

    fn extension(&mut self, args: &[PpToken], span: ByteSpan) {
        let malformed = |pp: &mut Self, at: ByteSpan| {
            pp.diagnostics.push(PpDiagnostic::error(
                PpCode::MalformedExtension,
                "'#extension' takes a name, a ':' and one of require, enable, warn or disable",
                at,
            ));
        };
        let Some(name) = args.first().filter(|t| t.kind == TokenKind::Ident) else {
            malformed(self, span);
            return;
        };
        if !matches!(args.get(1).map(|t| t.kind), Some(TokenKind::Punct(Punct::Colon))) {
            malformed(self, span);
            return;
        }
        let Some(behaviour) = args.get(2).and_then(|t| ExtensionBehaviour::parse(&t.text)) else {
            malformed(self, args.get(2).map_or(span, |t| t.span));
            return;
        };
        self.extra_tokens(&args[3..], "extension");
        self.directives.extensions.push(ExtensionDirective {
            name: name.text.clone(),
            behaviour,
            name_span: name.span,
            span,
        });
    }

    fn pragma(&mut self, args: &[PpToken], span: ByteSpan) {
        let text = match (args.first(), args.last()) {
            (Some(first), Some(last)) => {
                self.src[first.span.start as usize..last.span.end as usize].to_string()
            }
            _ => String::new(),
        };
        self.directives.pragmas.push(PragmaDirective { text, span });
    }

    fn line_directive(&mut self, args: &[PpToken], span: ByteSpan) {
        let ctx = ExpandCtx {
            source: self.src,
            lines: &self.lines,
            line_offset: self.line_offset,
            file_number: self.file_number,
            version: self.version,
        };
        let (line, source_string) =
            expr::line_arguments(args, &self.macros, &ctx, span, &mut self.diagnostics);
        let Some(line) = line else {
            return;
        };
        // The line *after* the directive takes the number, so the offset is
        // relative to the physical line after this one. glslang settles this;
        // `Test/preprocessor.line.vert` is the fixture that pins it down.
        let physical = self.lines.offset_to_line_col(self.src, span.start).line as i64 + 1;
        self.line_offset = line - physical - 1;
        if let Some(number) = source_string {
            self.file_number = number;
        }
        self.directives.lines.push(LineDirective { line, source_string, span });
    }

    fn error_directive(&mut self, args: &[PpToken], span: ByteSpan) {
        let message = match (args.first(), args.last()) {
            (Some(first), Some(last)) => {
                self.src[first.span.start as usize..last.span.end as usize].trim().to_string()
            }
            _ => String::new(),
        };
        self.diagnostics.push(PpDiagnostic::error(
            PpCode::ErrorDirective,
            if message.is_empty() { "#error".to_string() } else { message },
            span,
        ));
    }

    fn include(&mut self, args: &[PpToken], span: ByteSpan) {
        // Recorded, never followed — RFC 012 §10. The path is whatever the line
        // says, whether quoted or in angle brackets, because we only ever show
        // it back to the user.
        let path = match (args.first(), args.last()) {
            (Some(first), Some(last)) => self.src
                [first.span.start as usize..last.span.end as usize]
                .trim()
                .trim_matches(['"', '<', '>'])
                .to_string(),
            _ => String::new(),
        };
        self.directives.includes.push(IncludeDirective { path, span });
    }

    // -- macros ------------------------------------------------------------

    /// Report a `#define`/`#undef` of a name the language reserves. Returns
    /// whether the definition may still go ahead.
    fn check_reserved(&mut self, name: &PpToken, directive: &str) -> bool {
        match macros::reserved(&name.text) {
            Reserved::No => true,
            Reserved::Warning => {
                self.diagnostics.push(PpDiagnostic::warning(
                    PpCode::ReservedMacroNameWarning,
                    format!(
                        "'{}' contains '__', which the language reserves; '#{directive}' of it \
                         is undefined behaviour",
                        name.text
                    ),
                    name.span,
                ));
                true
            }
            Reserved::Error => {
                self.diagnostics.push(PpDiagnostic::error(
                    PpCode::ReservedMacroName,
                    format!("'{}' is reserved and cannot be '#{directive}'d", name.text),
                    name.span,
                ));
                false
            }
        }
    }

    fn define(&mut self, args: &[PpToken], span: ByteSpan) {
        let Some(name) = args.first().filter(|t| t.kind == TokenKind::Ident) else {
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::MissingMacroName,
                "'#define' must be followed by a macro name",
                span,
            ));
            return;
        };
        if !self.check_reserved(name, "define") {
            return;
        }
        let mut rest = &args[1..];
        let mut kind = MacroKind::Object;
        let mut params: Vec<String> = Vec::new();
        // `#define F(x)` is function-like; `#define F (x)` defines `F` as `(x)`.
        // The only difference is the space, which is why tokens carry it.
        let opens_params = rest
            .first()
            .is_some_and(|t| t.kind == TokenKind::Punct(Punct::LParen) && !t.leading_space);
        if opens_params {
            kind = MacroKind::Function;
            rest = &rest[1..];
            let mut expect_name = true;
            loop {
                let Some(token) = rest.first() else {
                    self.diagnostics.push(PpDiagnostic::error(
                        PpCode::UnterminatedMacroParameters,
                        format!("the parameter list of '{}' has no ')'", name.text),
                        span,
                    ));
                    return;
                };
                rest = &rest[1..];
                match token.kind {
                    TokenKind::Punct(Punct::RParen) if expect_name && params.is_empty() => break,
                    TokenKind::Punct(Punct::RParen) if !expect_name => break,
                    TokenKind::Punct(Punct::Comma) if !expect_name => expect_name = true,
                    TokenKind::Ident if expect_name => {
                        if params.contains(&token.text) {
                            self.diagnostics.push(PpDiagnostic::error(
                                PpCode::DuplicateMacroParameter,
                                format!("'{}' is already a parameter of this macro", token.text),
                                token.span,
                            ));
                        } else {
                            params.push(token.text.clone());
                        }
                        expect_name = false;
                    }
                    _ => {
                        self.diagnostics.push(PpDiagnostic::error(
                            PpCode::BadMacroParameter,
                            format!("'{}' is not a macro parameter name", token.text),
                            token.span,
                        ));
                        return;
                    }
                }
            }
        }
        let mut body: Vec<PpToken> = rest.to_vec();
        if let Some(first) = body.first_mut() {
            first.leading_space = false;
        }
        let id = self.macros.id_for(&name.text);
        let def = MacroDef {
            id,
            name: name.text.clone(),
            kind,
            params,
            body,
            span,
            name_span: name.span,
            predefined: false,
            undefined_at: None,
        };
        if let Some(existing) = self.macros.lookup(&def.name) {
            if !existing.is_identical_to(&def) {
                let previous = existing.predefined;
                self.diagnostics.push(PpDiagnostic::error(
                    PpCode::MacroRedefined,
                    if previous {
                        format!("'{}' is predefined and this redefines it", def.name)
                    } else {
                        format!(
                            "'{}' is already defined differently; '#undef' it first",
                            def.name
                        )
                    },
                    name.span,
                ));
            }
        }
        self.macros.define(def);
    }

    fn undef(&mut self, args: &[PpToken], span: ByteSpan) {
        let Some(name) = args.first().filter(|t| t.kind == TokenKind::Ident) else {
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::MissingMacroName,
                "'#undef' must be followed by a macro name",
                span,
            ));
            return;
        };
        if !self.check_reserved(name, "undef") {
            return;
        }
        self.extra_tokens(&args[1..], "undef");
        self.macros.undefine(&name.text, span);
    }
}
