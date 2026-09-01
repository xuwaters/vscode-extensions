//! P4-07 — version, profile and stage: which builtins a file actually has.
//!
//! This is the task decision 0007 closed. The rules under test:
//!
//! - a builtin the declared `#version` does not have is an error *with a hint*
//!   naming the spelling that version does have;
//! - the compatibility profile keeps the legacy names at any version;
//! - a version this crate does not recognise, or an `#extension` line, switches
//!   availability checking off rather than guessing;
//! - stage rules only fire when the host told us the stage, and only for the
//!   builtins that genuinely belong to one stage.

use glsl_spec::{DesktopVersion, EsVersion, Stage, Version};

use crate::{Options, analyze_source};

use crate::tests::{analyze, analyze_in, error_codes, expect, expect_clean, messages};

#[test]
fn a_modern_builtin_in_an_old_shader_names_the_old_spelling() {
    let analysis = analyze(
        "#version 110\nuniform sampler2D s;\nvoid main() { gl_FragColor = texture(s, \
         vec2(0.0)); }\n",
    );
    assert_eq!(error_codes(&analysis), &["GLSL0223"]);
    assert!(
        messages(&analysis).iter().any(|m| m.contains("texture2D")),
        "{:?}",
        messages(&analysis)
    );
    // The 1.10 spelling of the same shader is clean.
    expect_clean(
        "#version 110\nuniform sampler2D s;\nvoid main() { gl_FragColor = texture2D(s, \
         vec2(0.0)); }\n",
    );
}

#[test]
fn a_legacy_builtin_in_a_modern_shader_names_the_modern_spelling() {
    let analysis = analyze(
        "#version 330\nuniform sampler2D s;\nout vec4 colour;\n\
         void main() { colour = texture2D(s, vec2(0.0)); }\n",
    );
    assert_eq!(error_codes(&analysis), &["GLSL0223"]);
    assert!(
        messages(&analysis).iter().any(|m| m.contains("'texture'")),
        "{:?}",
        messages(&analysis)
    );
}

#[test]
fn gl_fragcolor_is_es_100_and_desktop_120_and_nothing_newer() {
    expect_clean("#version 100\nvoid main() { gl_FragColor = vec4(1.0); }\n");
    expect_clean("#version 120\nvoid main() { gl_FragColor = vec4(1.0); }\n");
    let analysis = analyze("#version 300 es\nvoid main() { gl_FragColor = vec4(1.0); }\n");
    assert_eq!(error_codes(&analysis), &["GLSL0223"]);
    assert!(
        messages(&analysis).iter().any(|m| m.contains("'out' variable")),
        "{:?}",
        messages(&analysis)
    );
}

#[test]
fn the_compatibility_profile_keeps_the_legacy_names() {
    // The same file that is an error in `core` is fine in `compatibility`.
    expect_clean(
        "#version 330 compatibility\nuniform sampler2D s;\n\
         void main() { gl_FragColor = texture2D(s, gl_TexCoord[0].st); }\n",
    );
    expect(
        "#version 330 core\nuniform sampler2D s;\n\
         void main() { gl_FragColor = texture2D(s, vec2(0.0)); }\n",
        &["GLSL0223", "GLSL0223"],
    );
}

#[test]
fn the_legacy_fixed_function_uniforms_resolve_in_a_compatibility_shader() {
    expect_clean(
        "#version 120\nvoid main() { gl_Position = gl_ModelViewProjectionMatrix * \
         gl_Vertex; }\n",
    );
    // …and they are typed, not merely tolerated: a mat4 times a vec4 is a vec4.
    assert_eq!(
        crate::tests::type_of(
            "#version 120\nvoid main() { gl_Position = «gl_ModelViewMatrix * gl_Vertex»; }\n"
        ),
        "vec4"
    );
}

#[test]
fn the_builtin_constants_are_const_ints_everywhere() {
    expect_clean("#version 330\nvoid main() { int n = gl_MaxDrawBuffers; }\n");
    expect_clean("#version 100\nvoid main() { int n = gl_MaxVertexAttribs; }\n");
    // A builtin constant is constant enough to size an array with.
    expect_clean("#version 330\nfloat weights[gl_MaxDrawBuffers];\nvoid main() { }\n");
}

#[test]
fn an_unrecognised_version_switches_gating_off() {
    // `#version 200` is no version of anything. Guessing a neighbour would
    // invent availability answers, so nothing is claimed.
    expect_clean("#version 200\nuniform sampler2D s;\nvoid main() { texture(s, vec2(0.0)); }\n");
}

#[test]
fn an_extension_line_switches_gating_off() {
    // An `#extension` can add any builtin at any version, and RFC 012 §2 N3
    // says we model none of them — so a file that enables one gets no
    // availability errors rather than wrong ones.
    expect_clean(
        "#version 110\n#extension GL_ARB_texture_rectangle : enable\n\
         uniform sampler2D s;\nvoid main() { gl_FragColor = texture(s, vec2(0.0)); }\n",
    );
}

#[test]
fn a_file_with_no_version_is_glsl_110() {
    // The spec's own rule, and it *is* a rule rather than a guess, so gating
    // applies.
    let analysis = analyze("uniform sampler2D s;\nvoid main() { texture(s, vec2(0.0)); }\n");
    assert_eq!(error_codes(&analysis), &["GLSL0223"]);
    assert!(analysis.context.version_known);
}

#[test]
fn a_stage_rule_needs_the_host_to_have_named_the_stage() {
    // Guessed stage: nothing is claimed.
    expect_clean("#version 330\nvoid main() { gl_FragDepth = 1.0; }\n");
    // Told stage: `gl_FragDepth` belongs to the fragment shader alone.
    let analysis = analyze_in(
        "#version 330\nvoid main() { gl_FragDepth = 1.0; }\n",
        Stage::Vertex,
    );
    assert_eq!(error_codes(&analysis), &["GLSL0224"]);
    let fragment = analyze_in(
        "#version 330\nvoid main() { gl_FragDepth = 1.0; }\n",
        Stage::Fragment,
    );
    assert!(fragment.errors().next().is_none());
}

#[test]
fn a_builtin_that_lives_in_several_stages_is_never_a_stage_error() {
    // `gl_ClipDistance`'s reference page names four stages and forgets that
    // 4.30 made it readable in the fragment shader too. A stage error is only
    // ever raised for a builtin that belongs to exactly one, so prose that is
    // incomplete cannot become a false positive.
    let analysis = analyze_in(
        "#version 450\nvoid main() { float d = gl_ClipDistance[0]; }\n",
        Stage::Fragment,
    );
    assert!(analysis.errors().next().is_none(), "{:?}", messages(&analysis));
}

#[test]
fn discard_belongs_to_the_fragment_shader() {
    let analysis =
        analyze_in("#version 330\nvoid main() { discard; }\n", Stage::Vertex);
    assert_eq!(error_codes(&analysis), &["GLSL0220"]);
    let fragment =
        analyze_in("#version 330\nvoid main() { discard; }\n", Stage::Fragment);
    assert!(fragment.errors().next().is_none());
    // And with no stage known, nothing is claimed.
    expect_clean("#version 330\nvoid main() { discard; }\n");
}

#[test]
fn the_stage_is_read_from_the_file_extension() {
    use crate::Options;
    assert_eq!(Options::for_path("shader.frag").stage, Some(Stage::Fragment));
    assert_eq!(Options::for_path("/a/b/shader.vert").stage, Some(Stage::Vertex));
    assert_eq!(Options::for_path("shader.comp").stage, Some(Stage::Compute));
    assert_eq!(Options::for_path("shader.tesc").stage, Some(Stage::TessControl));
    // A `.glsl` names no stage, and neither does anything unknown.
    assert_eq!(Options::for_path("shader.glsl").stage, None);
    assert_eq!(Options::for_path("shader.rgen").stage, None);
}

#[test]
fn es_and_desktop_are_tracked_separately() {
    // `texture` is ES 3.00 and desktop 1.30; the two profiles have their own
    // masks and neither is inferred from the other.
    expect_clean(
        "#version 300 es\nprecision mediump float;\nuniform sampler2D s;\nin vec2 uv;\n\
         out vec4 colour;\nvoid main() { colour = texture(s, uv); }\n",
    );
    let analysis = analyze(
        "#version 100\nuniform sampler2D s;\nvarying vec2 uv;\n\
         void main() { gl_FragColor = texture(s, uv); }\n",
    );
    assert_eq!(error_codes(&analysis), &["GLSL0223"]);
    // `texture2D` is the other way round.
    expect_clean(
        "#version 100\nuniform sampler2D s;\nvarying vec2 uv;\n\
         void main() { gl_FragColor = texture2D(s, uv); }\n",
    );
    expect(
        "#version 300 es\nprecision mediump float;\nuniform sampler2D s;\nin vec2 uv;\n\
         out vec4 colour;\nvoid main() { colour = texture2D(s, uv); }\n",
        &["GLSL0223"],
    );
}

/// A file that declares no `#version` is GLSL 1.10 by the spec, and that is
/// what the analyzer holds it to. A host that knows better — a project whose
/// shaders are assembled from `#version`-less fragments — says so through
/// [`Options::default_version`] (P5-09), and the availability rules move with
/// it rather than switching off.
#[test]
fn the_host_can_supply_the_version_a_file_does_not_declare() {
    const SOURCE: &str = "uniform sampler2D s;\nin vec2 uv;\nout vec4 colour;\n\
        void main() { colour = texture(s, uv); }\n";

    // 1.10 is the spec's answer, and `texture` is not a 1.10 name.
    let analysis = analyze_source(
        SOURCE,
        &Options { stage: Some(Stage::Fragment), ..Options::default() },
    );
    assert_eq!(analysis.context.version, Version::DEFAULT);
    assert_eq!(error_codes(&analysis), &["GLSL0223"]);

    // Told otherwise, the same file is clean — and still *known*, so a name
    // that 3.30 really lacks would still be caught.
    let analysis = analyze_source(
        SOURCE,
        &Options {
            stage: Some(Stage::Fragment),
            default_version: Some(Version::Desktop(DesktopVersion::V330)),
        },
    );
    assert_eq!(analysis.context.version, Version::Desktop(DesktopVersion::V330));
    assert!(analysis.context.version_known);
    assert!(analysis.errors().next().is_none(), "{:?}", error_codes(&analysis));

    // A `#version` in the file always wins over the host's default.
    let analysis = analyze_source(
        &format!("#version 100\n{SOURCE}"),
        &Options {
            stage: Some(Stage::Fragment),
            default_version: Some(Version::Desktop(DesktopVersion::V330)),
        },
    );
    assert_eq!(analysis.context.version, Version::Es(EsVersion::V100));
}
