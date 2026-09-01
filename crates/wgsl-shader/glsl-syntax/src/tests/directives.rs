//! P2-02 — directive recognition and the directive record.

use pretty_assertions::assert_eq;

use super::{codes, errors, texts};
use crate::diagnostics::PpCode;
use crate::preprocessor::{ExtensionBehaviour, Profile};
use crate::preprocess_source;

#[test]
fn version_with_a_profile() {
    let source = "#version 320 es\n";
    let pp = preprocess_source(source);
    let version = pp.directives.version.as_ref().expect("a #version");
    assert_eq!(version.number, 320);
    assert_eq!(version.profile, Some(Profile::Es));
    assert_eq!(&source[version.span.start as usize..version.span.end as usize], "#version 320 es");
    assert!(pp.is_es());
    assert_eq!(errors(&pp), Vec::<&str>::new());
}

#[test]
fn version_without_a_profile_defaults_by_number() {
    // `#version 100` is ES whether or not it says so; everything else is core.
    assert_eq!(preprocess_source("#version 100\n").profile(), Profile::Es);
    assert_eq!(preprocess_source("#version 460\n").profile(), Profile::Core);
    // …and a file with no `#version` at all is GLSL 1.10 core.
    let bare = preprocess_source("void main() {}\n");
    assert_eq!(bare.version(), 110);
    assert_eq!(bare.profile(), Profile::Core);
}

#[test]
fn version_records_compatibility_and_core_verbatim() {
    let pp = preprocess_source("#version 440 compatibility\n");
    assert_eq!(pp.profile(), Profile::Compatibility);
    assert_eq!(pp.directives.version.unwrap().profile, Some(Profile::Compatibility));
}

#[test]
fn version_after_code_is_an_error_but_still_recorded() {
    let pp = preprocess_source("int a;\n#version 460\n");
    assert_eq!(codes(&pp), [PpCode::VersionNotFirst]);
    assert_eq!(pp.version(), 460);
}

#[test]
fn version_after_a_comment_is_fine() {
    let pp = preprocess_source("// license header\n\n#version 460 core\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(pp.version(), 460);
}

#[test]
fn malformed_version_diagnoses_without_panicking() {
    assert_eq!(codes(&preprocess_source("#version\n")), [PpCode::MalformedVersion]);
    assert_eq!(codes(&preprocess_source("#version core\n")), [PpCode::MalformedVersion]);
    assert_eq!(codes(&preprocess_source("#version 460 turbo\n")), [PpCode::MalformedVersion]);
    assert_eq!(codes(&preprocess_source("#version 460 core extra\n")), [PpCode::ExtraTokens]);
}

#[test]
fn extension_records_name_and_behaviour() {
    let source = "#version 460\n#extension GL_ARB_gpu_shader5 : require\n#extension all : warn\n";
    let pp = preprocess_source(source);
    assert_eq!(errors(&pp), Vec::<&str>::new());
    let names: Vec<_> = pp.directives.extensions.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["GL_ARB_gpu_shader5", "all"]);
    let behaviours: Vec<_> = pp.directives.extensions.iter().map(|e| e.behaviour).collect();
    assert_eq!(behaviours, [ExtensionBehaviour::Require, ExtensionBehaviour::Warn]);
    // The name span is the name alone, which is what go-to-documentation wants.
    let first = &pp.directives.extensions[0];
    assert_eq!(
        &source[first.name_span.start as usize..first.name_span.end as usize],
        "GL_ARB_gpu_shader5"
    );
}

#[test]
fn every_extension_behaviour_parses() {
    for behaviour in ["require", "enable", "warn", "disable"] {
        let pp = preprocess_source(&format!("#extension GL_X : {behaviour}\n"));
        assert_eq!(errors(&pp), Vec::<&str>::new(), "for {behaviour}");
        assert_eq!(pp.directives.extensions[0].behaviour.as_str(), behaviour);
    }
}

#[test]
fn malformed_extension_diagnoses_without_panicking() {
    assert_eq!(codes(&preprocess_source("#extension\n")), [PpCode::MalformedExtension]);
    assert_eq!(codes(&preprocess_source("#extension GL_X\n")), [PpCode::MalformedExtension]);
    assert_eq!(codes(&preprocess_source("#extension GL_X :\n")), [PpCode::MalformedExtension]);
    let bad_behaviour = preprocess_source("#extension GL_X : maybe\n");
    assert_eq!(codes(&bad_behaviour), [PpCode::MalformedExtension]);
}

#[test]
fn pragma_keeps_its_text_verbatim() {
    let pp = preprocess_source("#pragma optimize(off)\n#pragma STDGL invariant(all)\n");
    let texts: Vec<_> = pp.directives.pragmas.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, ["optimize(off)", "STDGL invariant(all)"]);
    assert_eq!(errors(&pp), Vec::<&str>::new());
}

#[test]
fn a_pragma_with_nothing_after_it_is_still_a_pragma() {
    let pp = preprocess_source("#pragma\n");
    assert_eq!(pp.directives.pragmas.len(), 1);
    assert_eq!(pp.directives.pragmas[0].text, "");
    assert_eq!(errors(&pp), Vec::<&str>::new());
}

#[test]
fn line_sets_the_number_of_the_following_line() {
    // glslang's `Test/preprocessor.line.vert` is the oracle: after `#line N`
    // the *next* line is line N.
    let pp = preprocess_source("#line 10\nint a = __LINE__;\n");
    assert_eq!(texts(&pp), ["int", "a", "=", "10", ";"]);
}

#[test]
fn line_takes_a_constant_expression_and_a_source_string_number() {
    let pp = preprocess_source("#define X 4\n#line X * 2 3\nint a = __LINE__; int b = __FILE__;\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "8", ";", "int", "b", "=", "3", ";"]);
    let directive = pp.directives.lines.last().expect("a #line");
    assert_eq!(directive.line, 8);
    assert_eq!(directive.source_string, Some(3));
}

#[test]
fn line_accepts_a_filename_in_place_of_a_number() {
    let pp = preprocess_source("#line 42 \"shader.glsl\"\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(pp.directives.lines[0].line, 42);
    assert_eq!(pp.directives.lines[0].source_string, None);
}

#[test]
fn malformed_line_diagnoses_without_panicking() {
    assert_eq!(codes(&preprocess_source("#line\n")), [PpCode::MalformedLine]);
    assert!(codes(&preprocess_source("#line +\n")).contains(&PpCode::MalformedLine));
}

#[test]
fn error_directive_becomes_a_diagnostic_carrying_its_message() {
    let pp = preprocess_source("#error this build needs GL_ARB_gpu_shader5\n");
    assert_eq!(codes(&pp), [PpCode::ErrorDirective]);
    assert_eq!(pp.diagnostics[0].message, "this build needs GL_ARB_gpu_shader5");
}

#[test]
fn error_inside_a_dead_branch_stays_quiet() {
    let pp = preprocess_source("#if 0\n#error never\n#endif\n");
    assert_eq!(codes(&pp), Vec::<PpCode>::new());
}

#[test]
fn include_is_recorded_and_not_followed() {
    let pp = preprocess_source("#include \"common.glsl\"\n#include <lib/util.h>\nint a;\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    let paths: Vec<_> = pp.directives.includes.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(paths, ["common.glsl", "lib/util.h"]);
    // Nothing was pulled in: the stream is only what the file itself holds.
    assert_eq!(texts(&pp), ["int", "a", ";"]);
}

#[test]
fn a_bare_hash_is_the_null_directive() {
    let pp = preprocess_source("#\n#   \nint a;\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(texts(&pp), ["int", "a", ";"]);
}

#[test]
fn an_unknown_directive_is_reported_and_skipped() {
    let pp = preprocess_source("#nonsense whatever\nint a;\n");
    assert_eq!(codes(&pp), [PpCode::UnknownDirective]);
    assert_eq!(texts(&pp), ["int", "a", ";"]);
}

#[test]
fn a_hash_that_is_not_at_the_start_of_a_line_is_an_operator() {
    // Nonsense in code, but it must not be mistaken for a directive.
    let pp = preprocess_source("int a = b # c;\n");
    assert_eq!(codes(&pp), Vec::<PpCode>::new());
    assert_eq!(texts(&pp), ["int", "a", "=", "b", "#", "c", ";"]);
}

#[test]
fn a_directive_may_have_whitespace_and_comments_around_its_name() {
    let pp = preprocess_source("  #  /* here */ version /* and */ 460 core\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(pp.version(), 460);
}

#[test]
fn a_directive_may_be_spread_over_lines_with_continuations() {
    let pp = preprocess_source("#version \\\n 460 \\\n core\nint a;\n");
    assert_eq!(errors(&pp), Vec::<&str>::new());
    assert_eq!(pp.version(), 460);
    assert_eq!(texts(&pp), ["int", "a", ";"]);
}

#[test]
fn version_defines_gl_es_only_for_the_es_profile() {
    let es = preprocess_source("#version 300 es\n#ifdef GL_ES\nint es;\n#endif\n");
    assert_eq!(texts(&es), ["int", "es", ";"]);
    let desktop = preprocess_source("#version 460 core\n#ifdef GL_ES\nint es;\n#endif\n");
    assert_eq!(texts(&desktop), Vec::<&str>::new());
}
