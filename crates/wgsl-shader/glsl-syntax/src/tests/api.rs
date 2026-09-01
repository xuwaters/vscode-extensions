//! P2-06 — the `Preprocessed` output type as a whole.
//!
//! These are the round-trip tests: one realistic shader through the pipeline,
//! then every field of the answer checked against the source it came from. What
//! they really assert is that the five parts agree with each other — the token
//! stream, the directive record, the macro table, the inactive regions and the
//! diagnostics all describe the same file.

use pretty_assertions::assert_eq;

use analyzer_core::diagnostics::DiagnosticCode;

use super::{errors, texts};
use crate::diagnostics::PpCode;
use crate::preprocessor::{ExtensionBehaviour, Origin, PreprocessOptions, Profile, preprocess};
use crate::{preprocess_source, tokenize};

const SHADER: &str = "\
#version 310 es
#extension GL_EXT_shader_io_blocks : require
#pragma optimize(on)

#define MAX_LIGHTS 4
#define SCALE(v) ((v) * uScale)

precision highp float;
uniform float uScale;

#ifdef GL_ES
out vec4 fragColour;
#else
varying vec4 fragColour;
#endif

void main() {
    fragColour = vec4(SCALE(gl_FragCoord.x), 0.0, 0.0, float(MAX_LIGHTS));
}
";

#[test]
fn the_answer_describes_the_whole_file() {
    let pp = preprocess_source(SHADER);
    assert_eq!(errors(&pp), Vec::<&str>::new());

    // Directives.
    assert_eq!(pp.version(), 310);
    assert_eq!(pp.profile(), Profile::Es);
    assert!(pp.is_es());
    assert_eq!(pp.directives.extensions.len(), 1);
    assert_eq!(pp.directives.extensions[0].name, "GL_EXT_shader_io_blocks");
    assert_eq!(pp.directives.extensions[0].behaviour, ExtensionBehaviour::Require);
    assert_eq!(pp.directives.pragmas[0].text, "optimize(on)");

    // Macros, source-defined and predefined alike.
    assert!(pp.macros.lookup("MAX_LIGHTS").is_some());
    assert!(pp.macros.lookup("SCALE").is_some());
    assert!(pp.macros.lookup("GL_ES").is_some());

    // One inactive region: the `#else` the ES branch shut off.
    assert_eq!(pp.inactive.len(), 1);
    let dead = &SHADER[pp.inactive[0].span.start as usize..pp.inactive[0].span.end as usize];
    assert_eq!(dead, "varying vec4 fragColour;\n");

    // The live stream took the `#ifdef` branch and expanded both macros.
    let text = texts(&pp).join(" ");
    assert!(text.contains("out vec4 fragColour ;"), "{text}");
    assert!(!text.contains("varying"), "{text}");
    assert!(text.contains("( ( gl_FragCoord . x ) * uScale )"), "{text}");
    assert!(text.contains("float ( 4 )"), "{text}");
}

#[test]
fn every_token_span_is_inside_the_source_and_reads_back() {
    let pp = preprocess_source(SHADER);
    for token in &pp.tokens {
        assert!(
            token.span.end as usize <= SHADER.len(),
            "{token:?} points past the end of the source"
        );
        let text = &SHADER[token.span.start as usize..token.span.end as usize];
        // A source token spells itself; anything else points at the invocation
        // that produced it, which is real text either way.
        if token.origin == Origin::Source {
            assert_eq!(text, token.text, "{token:?}");
        } else {
            assert!(!text.is_empty(), "{token:?} has an empty provenance span");
        }
    }
}

#[test]
fn every_macro_definition_span_reads_back_as_its_directive() {
    let pp = preprocess_source(SHADER);
    for def in pp.macros.all().iter().filter(|d| !d.predefined) {
        let directive = &SHADER[def.span.start as usize..def.span.end as usize];
        assert!(directive.starts_with("#define"), "{directive:?}");
        let name = &SHADER[def.name_span.start as usize..def.name_span.end as usize];
        assert_eq!(name, def.name);
    }
}

#[test]
fn the_stream_holds_no_trivia() {
    let pp = preprocess_source(SHADER);
    assert!(pp.tokens.iter().all(|t| !t.kind.is_trivia()));
}

#[test]
fn leading_space_survives_into_the_stream() {
    let pp = preprocess_source("int a;int b;\n");
    let spaces: Vec<_> = pp.tokens.iter().map(|t| t.leading_space).collect();
    // `int a ; int b ;` — only the second `int` follows no whitespace.
    assert_eq!(spaces, [true, true, false, false, true, false]);
}

#[test]
fn preprocessing_a_lexed_source_matches_the_convenience_wrapper() {
    let tokens = tokenize(SHADER);
    let explicit = preprocess(SHADER, &tokens, &PreprocessOptions::default());
    let convenient = preprocess_source(SHADER);
    assert_eq!(explicit.tokens, convenient.tokens);
    assert_eq!(explicit.directives, convenient.directives);
    assert_eq!(explicit.inactive, convenient.inactive);
}

#[test]
fn diagnostic_codes_are_stable_and_unique() {
    let codes = [
        PpCode::UnknownDirective,
        PpCode::VersionNotFirst,
        PpCode::MalformedVersion,
        PpCode::MalformedExtension,
        PpCode::MalformedLine,
        PpCode::ErrorDirective,
        PpCode::ExtraTokens,
        PpCode::MissingMacroName,
        PpCode::BadMacroParameter,
        PpCode::DuplicateMacroParameter,
        PpCode::UnterminatedMacroParameters,
        PpCode::MacroRedefined,
        PpCode::ReservedMacroName,
        PpCode::ReservedMacroNameWarning,
        PpCode::UnmatchedConditional,
        PpCode::MisplacedElse,
        PpCode::UnterminatedConditional,
        PpCode::BadConditionalExpression,
        PpCode::DivisionByZero,
        PpCode::BadConditionalLiteral,
        PpCode::MacroArgumentCount,
        PpCode::UnterminatedMacroInvocation,
        PpCode::ExpansionLimit,
        PpCode::BadTokenPaste,
        PpCode::BadStringify,
    ];
    let mut seen: Vec<&str> = codes.iter().map(|c| c.as_str()).collect();
    seen.sort_unstable();
    let count = seen.len();
    seen.dedup();
    assert_eq!(seen.len(), count, "two diagnostics share a code");
    assert!(seen.iter().all(|c| c.starts_with("GLSL")), "{seen:?}");
}

#[test]
fn an_empty_source_yields_an_empty_answer() {
    let pp = preprocess_source("");
    assert!(pp.tokens.is_empty());
    assert!(pp.diagnostics.is_empty());
    assert!(pp.inactive.is_empty());
    assert_eq!(pp.version(), 110);
}

#[test]
fn a_source_that_is_nothing_but_broken_still_answers() {
    // Every kind of malformation at once. The contract is diagnostics, not
    // panics, and a token stream that still holds the readable parts.
    let source = "#version\n#define\n#if\n#else\n#else\n#elif\n#endif\n#endif\n#nope\n\
                  #define F(a,a) a##\nF(1\n\"unterminated\n/* unterminated\n";
    let pp = preprocess_source(source);
    assert!(!pp.diagnostics.is_empty());
    assert!(pp.diagnostics.iter().all(|d| d.span.end as usize <= source.len()));
}
