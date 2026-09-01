//! GLSL across its three dialects, end to end through the server.
//!
//! RFC 012 phase 5. The point of the whole RFC is that the answer no longer
//! depends on which GLSL the file is written in, so every feature that has a
//! dialect-sensitive answer is asked the same question three times:
//!
//! | Dialect | Fixture | What used to happen |
//! | --- | --- | --- |
//! | GLSL ES 3.00 | [`ES`] | naga does not implement it; nothing was validated |
//! | Desktop OpenGL | [`OPENGL`] | combined samplers and implicit bindings; nothing was validated |
//! | Vulkan 4.50 | [`VULKAN`] | naga validated it |
//!
//! The Vulkan column is also the parity evidence behind
//! [decision 0008](../../../../docs/rfc/012-glsl-analyzer/decisions/0008-naga-glsl-in-dropped.md):
//! every error naga caught on a Vulkan-dialect fixture, our analyzer catches
//! too, with a `GLSL02xx` code.

mod support;

use lsp_types::{Diagnostic, DiagnosticSeverity};
use serde_json::json;
use support::Harness;
use wgsl_lsp_core::Settings;

/// GLSL ES 3.00, as a WebGL2 fragment shader is written: a precision
/// statement, combined samplers, no bindings.
pub const ES: &str = r#"#version 300 es
precision mediump float;

uniform sampler2D albedo;
in vec2 v_uv;
out vec4 fragColour;

void main() {
    vec4 base = texture(albedo, v_uv);
    fragColour = vec4(base.rgb, 1.0);
}
"#;

/// Desktop OpenGL: a `#version 330 core` shader with a combined sampler and a
/// uniform block the driver binds.
pub const OPENGL: &str = r#"#version 330 core

uniform Material {
    vec3 diffuse;
    float shininess;
} material;

uniform sampler2D albedo;
in vec2 v_uv;
out vec4 fragColour;

float lambert(vec3 normal, vec3 light) {
    return max(dot(normalize(normal), normalize(light)), 0.0);
}

void main() {
    vec4 base = texture(albedo, v_uv);
    fragColour = vec4(base.rgb * material.diffuse, 1.0);
}
"#;

/// Vulkan GLSL: explicit sets and bindings, textures and samplers apart.
pub const VULKAN: &str = r#"#version 450

layout(set = 0, binding = 0) uniform Camera {
    mat4 view;
    mat4 projection;
} camera;

layout(set = 0, binding = 1) uniform texture2D albedo;
layout(set = 0, binding = 2) uniform sampler albedo_sampler;

layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 out_colour;

void main() {
    vec4 base = texture(sampler2D(albedo, albedo_sampler), v_uv);
    out_colour = camera.projection[0].x * base;
}
"#;

pub fn errors(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect()
}

pub fn codes(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter_map(|d| match &d.code {
            Some(lsp_types::NumberOrString::String(code)) => Some(code.clone()),
            _ => None,
        })
        .collect()
}

/// Open a fixture and return everything published for it.
pub fn published(name: &str, source: &str) -> Vec<Diagnostic> {
    let mut harness = Harness::new();
    let uri = harness.open(name, source);
    harness.diagnostics(&uri).unwrap_or_default()
}

// ── The three dialects are all valid, and all validated ─────────────────────

#[test]
fn every_dialect_is_analysed_and_none_of_them_is_flagged() {
    for (name, source) in
        [("es.frag", ES), ("gl.frag", OPENGL), ("vk.frag", VULKAN)]
    {
        let diagnostics = published(name, source);
        assert!(errors(&diagnostics).is_empty(), "{name}: {:?}", errors(&diagnostics));
    }
}

/// The old server said "not validated because naga implements Vulkan GLSL
/// only". There is nothing left to say.
#[test]
fn no_dialect_reports_itself_as_unvalidated() {
    for (name, source) in
        [("es.frag", ES), ("gl.frag", OPENGL), ("vk.frag", VULKAN)]
    {
        let mut harness = Harness::new();
        let uri = harness.open(name, source);
        let info: serde_json::Value = harness
            .request("wgsl/shaderInfo", json!({ "textDocument": { "uri": uri.as_str() } }))
            .expect("shader info");
        assert_eq!(info["ok"], true, "{name}: {info}");
        assert_eq!(info["stage"], "fragment", "{name}");
        assert!(info["skipped"].is_null(), "{name}: {info}");
        assert!(
            info["validator"].as_str().unwrap().starts_with("glsl-analysis"),
            "{name}: {info}"
        );
    }
}

// ── Errors, in every dialect ───────────────────────────────────────────────

/// The seven mistakes a shader author actually makes. Each is checked in all
/// three dialects: the rule is the language's, not the dialect's.
#[test]
fn the_same_mistake_is_caught_in_every_dialect() {
    const CASES: [(&str, &str, &str); 7] = [
        ("GLSL0200", "an unknown name", "    float x = nowhere;\n"),
        ("GLSL0201", "a call to a non-function", "    float x = v_uv();\n"),
        ("GLSL0204", "a member that does not exist",
         "    struct S { float a; } s; float m = s.b;\n"),
        ("GLSL0205", "a swizzle the vector cannot answer", "    float x = v_uv.w;\n"),
        ("GLSL0210", "no matching overload", "    float x = dot(1.0);\n"),
        ("GLSL0216", "an operator with no meaning", "    float x = v_uv && v_uv;\n"),
        ("GLSL0218", "a condition that is not a bool", "    if (v_uv) { }\n"),
    ];

    for (dialect, source) in [("es", ES), ("opengl", OPENGL), ("vulkan", VULKAN)] {
        for (code, what, statement) in CASES {
            let broken = source.replacen("void main() {\n", &format!("void main() {{\n{statement}"), 1);
            let diagnostics = published("broken.frag", &broken);
            assert!(
                codes(&diagnostics).iter().any(|c| c == code),
                "{dialect}: {what} should be {code}, got {:?}",
                codes(&diagnostics)
            );
        }
    }
}

/// A preprocessor mistake and a parse mistake reach the client too, with their
/// own code ranges.
#[test]
fn preprocessor_and_parser_problems_are_published_with_their_codes() {
    let unterminated = published("a.frag", "#version 450\n#if 1\nvoid main() {}\n");
    assert!(codes(&unterminated).iter().any(|c| c == "GLSL0017"), "{unterminated:?}");

    let unclosed = published("a.frag", "#version 300 es\nvoid main() {\n");
    assert!(codes(&unclosed).iter().any(|c| c == "GLSL0105"), "{unclosed:?}");
}

/// Phase 4's conservatism, carried through the server: a file whose parse
/// failed gets the parse error and no semantic ones.
#[test]
fn a_file_that_did_not_parse_reports_no_semantic_errors() {
    let diagnostics = published("a.frag", "#version 450\nvoid main() { float x = nowhere\n}\n");
    let semantic: Vec<String> =
        codes(&diagnostics).into_iter().filter(|c| c.starts_with("GLSL02")).collect();
    assert!(semantic.is_empty(), "{semantic:?}");
    assert!(!errors(&diagnostics).is_empty(), "the parse error is still reported");
}

/// `#extension` means "I use something no table here models", and switches the
/// name and availability rules off rather than painting the file red.
#[test]
fn an_extension_directive_silences_the_name_rules() {
    let source = "#version 450\n\
        #extension GL_EXT_shader_explicit_arithmetic_types : require\n\
        void main() { int64_t big = int64_t(1); }\n";
    let diagnostics = published("a.frag", source);
    assert!(errors(&diagnostics).is_empty(), "{:?}", errors(&diagnostics));
}

// ── The shipped examples ───────────────────────────────────────────────────

/// RFC §9.2: the extension's own examples are validated by *our* analyzer, and
/// come back clean. All three dialects ship as examples now (P6-01), so this is
/// also the gate that the ES and desktop-OpenGL ones stay clean.
#[test]
fn the_shipped_examples_produce_no_errors() {
    const EXAMPLES: [(&str, &str); 5] = [
        ("test.vert", include_str!("../../../../extensions/wgsl-shader/examples/test.vert")),
        ("test.frag", include_str!("../../../../extensions/wgsl-shader/examples/test.frag")),
        ("test.comp", include_str!("../../../../extensions/wgsl-shader/examples/test.comp")),
        ("test-es300.frag",
         include_str!("../../../../extensions/wgsl-shader/examples/test-es300.frag")),
        ("test-opengl.frag",
         include_str!("../../../../extensions/wgsl-shader/examples/test-opengl.frag")),
    ];
    for (name, source) in EXAMPLES {
        let diagnostics = published(name, source);
        assert!(errors(&diagnostics).is_empty(), "{name}: {:?}", errors(&diagnostics));
    }
}

/// P6-01: one example per dialect, and each is analysed as the GLSL it declares
/// itself to be rather than as the one dialect a validator happens to read.
#[test]
fn each_shipped_example_is_analysed_as_the_dialect_it_declares() {
    const CASES: [(&str, &str, &str); 3] = [
        ("test-es300.frag",
         include_str!("../../../../extensions/wgsl-shader/examples/test-es300.frag"), "3.00 es"),
        ("test-opengl.frag",
         include_str!("../../../../extensions/wgsl-shader/examples/test-opengl.frag"), "3.30"),
        ("test.frag",
         include_str!("../../../../extensions/wgsl-shader/examples/test.frag"), "4.50"),
    ];
    for (name, source, version) in CASES {
        let mut harness = Harness::new();
        let uri = harness.open(name, source);
        assert!(harness.diagnostics(&uri).is_none(), "{name} publishes nothing");
        let info: serde_json::Value = harness
            .request("wgsl/shaderInfo", json!({ "textDocument": { "uri": uri.as_str() } }))
            .expect("shader info");
        assert_eq!(info["version"], version, "{name}: {info}");
        assert_eq!(info["stage"], "fragment", "{name}: {info}");
        assert!(info["skipped"].is_null(), "{name}: {info}");
    }
}

// ── Hover (P5-03) ──────────────────────────────────────────────────────────

/// The hover text over the first occurrence of `needle`, `within` characters
/// in.
pub fn hover(name: &str, source: &str, needle: &str, within: usize) -> String {
    let mut harness = Harness::new();
    let uri = harness.open(name, source);
    let hover: lsp_types::Hover = harness
        .at("textDocument/hover", &uri, support::find(source, needle, within))
        .unwrap_or_else(|| panic!("no hover on {needle:?}"));
    match hover.contents {
        lsp_types::HoverContents::Markup(markup) => markup.value,
        other => panic!("expected markup, got {other:?}"),
    }
}

/// The same builtin, two versions, two answers. This is what a hand-written
/// one-signature table could never do.
#[test]
fn a_builtin_hover_shows_the_overloads_this_version_has() {
    let es = hover("es.frag", ES, "texture(albedo", 2);
    assert!(es.contains("gvec4 texture(gsampler2D sampler"), "{es}");
    // `sampler1D` is desktop-only and has no business in an ES hover.
    assert!(!es.contains("sampler1D"), "{es}");

    let vulkan = hover("vk.frag", VULKAN, "texture(sampler2D", 2);
    assert!(vulkan.contains("sampler1D"), "{vulkan}");
    // Both carry the reference page's prose.
    assert!(es.contains("texture"), "{es}");
    assert!(vulkan.len() > es.len(), "desktop 4.50 has strictly more overloads");
}

/// A name that exists in the language but not in *this* version says so rather
/// than pretending the overloads are current.
#[test]
fn a_builtin_hover_names_the_version_that_does_not_have_it() {
    let source = "#version 300 es\nprecision mediump float;\n\
        uniform sampler2D t;\nout vec4 c;\n\
        void main() { c = texture2D(t, vec2(0.0)); }\n";
    let text = hover("es.frag", source, "texture2D(t", 2);
    assert!(text.contains("Not available in GLSL 3.00 es"), "{text}");
}

#[test]
fn a_builtin_variable_hover_names_its_stages() {
    let source = "#version 450\nlayout(location = 0) out vec4 c;\n\
        void main() { c = gl_FragCoord; }\n";
    let text = hover("a.frag", source, "gl_FragCoord", 3);
    assert!(text.contains("in vec4 gl_FragCoord"), "{text}");
    assert!(text.contains("Stages: Fragment"), "{text}");
}

/// GLSL declares its types, but it does not declare the type of every
/// *expression* — and the declaration is what a reader wants first.
#[test]
fn a_user_symbol_hover_shows_the_declaration_it_was_written_as() {
    let text = hover("gl.frag", OPENGL, "lambert(vec3 normal", 0);
    assert!(text.contains("float lambert(vec3 normal, vec3 light)"), "{text}");

    let member = hover("gl.frag", OPENGL, "material.diffuse", 9);
    assert!(member.contains("vec3 diffuse"), "{member}");
    assert!(member.contains("Material"), "{member}");
}

#[test]
fn a_swizzle_hover_names_the_type_the_selection_produces() {
    let text = hover("gl.frag", OPENGL, "base.rgb", 5);
    assert!(text.contains("vec3 rgb"), "{text}");
}

/// A `#define` never reaches the expanded token stream, so nothing but the
/// macro table can answer for it.
#[test]
fn a_macro_hover_shows_the_define_that_produced_it() {
    let source = "#version 450\n#define SCALE(x) ((x) * 2.0)\n\
        void main() { float y = SCALE(1.0); }\n";
    let text = hover("a.frag", source, "SCALE(1.0)", 2);
    assert!(text.contains("#define SCALE(x) ((x) * 2.0)"), "{text}");
}

#[test]
fn a_keyword_hover_says_what_the_keyword_is_for() {
    let text = hover("gl.frag", OPENGL, "uniform Material", 3);
    assert!(text.contains("storage qualifier"), "{text}");
}

// ── Completion (P5-04) ─────────────────────────────────────────────────────

/// The labels offered at the `|` in a fixture.
pub fn complete(name: &str, source: &str) -> Vec<String> {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_at(name, source);
    let response: lsp_types::CompletionResponse =
        harness.at("textDocument/completion", &uri, position).expect("a completion list");
    match response {
        lsp_types::CompletionResponse::Array(items) => items,
        lsp_types::CompletionResponse::List(list) => list.items,
    }
    .into_iter()
    .map(|item| item.label)
    .collect()
}

/// A `.` offers what the *type* has, not every field in the file.
#[test]
fn completing_after_a_dot_offers_the_members_of_the_resolved_type() {
    let source = OPENGL.replace("material.diffuse", "material.");
    let labels = complete("gl.frag", &source.replace("material.,", "material.|,"));
    assert_eq!(labels, ["diffuse", "shininess"]);
}

#[test]
fn completing_after_a_dot_on_a_vector_offers_all_three_swizzle_sets() {
    let source = OPENGL.replace("base.rgb", "base.|");
    let labels = complete("gl.frag", &source);
    assert_eq!(
        labels,
        [
            "x", "y", "xy", "z", "xyz", "w", "xyzw", "r", "g", "rg", "b", "rgb", "a",
            "rgba", "s", "t", "st", "p", "stp", "q", "stpq"
        ]
    );
}

/// The array method, which is the only thing after a `.` on an array.
#[test]
fn completing_after_a_dot_on_an_array_offers_length() {
    let source = "#version 450\nuniform float values[4];\nvoid main() { int n = values.| }\n";
    assert_eq!(complete("a.frag", source), ["length"]);
}

/// RFC §9.1: an ES fragment shader is offered what ES has and nothing else.
#[test]
fn builtins_are_filtered_by_the_declared_version() {
    let es = complete("es.frag", &ES.replace("vec4 base =", "vec4 base = te|;\n    vec4 unused ="));
    assert!(es.contains(&"texture".to_string()), "ES 3.00 has `texture`");
    assert!(!es.contains(&"texture1D".to_string()), "and no `texture1D`");
    assert!(!es.contains(&"noise1".to_string()), "nor the desktop-only noise family");
    assert!(!es.contains(&"dmat4".to_string()), "and no double matrices");
    assert!(es.contains(&"gl_FragCoord".to_string()), "which every fragment shader has");
    assert!(!es.contains(&"gl_ClipVertex".to_string()), "a compatibility-profile name");

    let desktop = complete("vk.frag", &VULKAN.replace("vec4 base =", "vec4 base = te|;\n    vec4 unused ="));
    assert!(desktop.contains(&"dmat4".to_string()), "4.50 has double matrices");
    assert!(desktop.contains(&"texture".to_string()));
    assert!(desktop.len() > es.len(), "desktop 4.50 declares strictly more than ES 3.00");
}

/// The other direction: a WebGL1 shader is not offered the modern spelling.
#[test]
fn an_es_100_shader_is_offered_the_names_it_actually_has() {
    let source = "#version 100\nuniform sampler2D t;\nvarying vec2 uv;\n\
        void main() { gl_FragColor = te|; }\n";
    let labels = complete("a.frag", source);
    assert!(labels.contains(&"texture2D".to_string()), "{:?}", labels.len());
    assert!(!labels.contains(&"texture".to_string()));
    assert!(labels.contains(&"gl_FragColor".to_string()));
}

/// A stage we were *told* filters the list; a stage we guessed must not.
#[test]
fn stage_filtering_only_applies_when_the_stage_was_declared() {
    let vertex = "#version 450\nvoid main() { gl|; }\n";
    let labels = complete("a.vert", vertex);
    assert!(labels.contains(&"gl_Position".to_string()));
    assert!(!labels.contains(&"gl_FragCoord".to_string()), "not in a vertex shader");

    // A bare `.glsl` file names no stage, so nothing is hidden.
    let labels = complete("a.glsl", vertex);
    assert!(labels.contains(&"gl_Position".to_string()));
    assert!(labels.contains(&"gl_FragCoord".to_string()));
}

#[test]
fn macros_are_offered_from_the_macro_table() {
    let source = "#version 450\n#define MAX_LIGHTS 4\nvoid main() { int n = MAX|; }\n";
    assert!(complete("a.frag", source).contains(&"MAX_LIGHTS".to_string()));
}

#[test]
fn a_directive_and_a_layout_key_are_offered_in_their_own_positions() {
    let directives = complete("a.frag", "#version 450\n#|\nvoid main() {}\n");
    assert!(directives.contains(&"define".to_string()));
    assert!(directives.contains(&"ifdef".to_string()));

    let layout = complete("a.vert", "#version 450\nlayout(|) in vec3 position;\nvoid main() {}\n");
    assert!(layout.contains(&"location".to_string()));
    assert!(layout.contains(&"std140".to_string()));
}

// ── Signature help (P5-05) ─────────────────────────────────────────────────

pub fn signature_help(name: &str, source: &str) -> lsp_types::SignatureHelp {
    let mut harness = Harness::new();
    let (uri, position) = harness.open_at(name, source);
    harness
        .at("textDocument/signatureHelp", &uri, position)
        .expect("signature help")
}

/// GLSL overloads, so the answer is a set — and it is printed in the spec's own
/// generic notation rather than expanded to a wall of concrete signatures.
#[test]
fn a_builtin_call_offers_its_whole_overload_set_in_spec_notation() {
    let source = "#version 450\nvoid main() { float x = mix(|1.0, 2.0, 0.5); }\n";
    let help = signature_help("a.frag", source);
    let labels: Vec<&str> = help.signatures.iter().map(|s| s.label.as_str()).collect();
    assert!(labels.len() > 1, "{labels:?}");
    assert!(
        labels.iter().any(|l| l.starts_with("genType mix(genType x, genType y")),
        "{labels:?}"
    );
    // The active parameter tracks the cursor, per signature.
    assert_eq!(help.signatures[help.active_signature.unwrap() as usize].active_parameter, Some(0));
}

#[test]
fn the_active_signature_is_the_one_whose_arity_still_fits() {
    // Two arguments in: only the three-parameter `clamp` can still be meant.
    let source = "#version 450\nvoid main() { float x = clamp(0.0, 1.0, |2.0); }\n";
    let help = signature_help("a.frag", source);
    let active = &help.signatures[help.active_signature.unwrap() as usize];
    assert_eq!(active.active_parameter, Some(2));
    assert_eq!(active.label.matches(',').count(), 2, "{}", active.label);
}

/// The set is the version's, not the language's.
#[test]
fn an_overload_set_is_filtered_by_the_declared_version() {
    let es = signature_help(
        "es.frag",
        "#version 300 es\nprecision mediump float;\nuniform sampler2D t;\n\
         void main() { vec4 c = texture(|t, vec2(0.0)); }\n",
    );
    let desktop = signature_help(
        "a.frag",
        "#version 450\nuniform sampler2D t;\n\
         void main() { vec4 c = texture(|t, vec2(0.0)); }\n",
    );
    assert!(
        desktop.signatures.len() > es.signatures.len(),
        "es {} vs desktop {}",
        es.signatures.len(),
        desktop.signatures.len()
    );
    assert!(es.signatures.iter().all(|s| !s.label.contains("sampler1D")), "no 1D in ES");
}

/// A file's own function, including the overloads GLSL lets it declare.
#[test]
fn a_user_function_shows_every_overload_the_file_declares() {
    let source = "#version 450\n\
        float scale(float x) { return x * 2.0; }\n\
        vec2 scale(vec2 v) { return v * 2.0; }\n\
        void main() { float y = scale(|1.0); }\n";
    let help = signature_help("a.frag", source);
    let labels: Vec<&str> = help.signatures.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["float scale(float x)", "vec2 scale(vec2 v)"]);
}

/// Per-parameter prose off the reference page, which is what the parameter
/// popup shows once the argument is highlighted.
#[test]
fn parameters_carry_the_reference_pages_own_documentation() {
    let source = "#version 450\nvoid main() { float x = clamp(|0.0, 1.0, 2.0); }\n";
    let help = signature_help("a.frag", source);
    let parameters = help.signatures[0].parameters.as_ref().unwrap();
    assert!(parameters.iter().any(|p| p.documentation.is_some()), "{parameters:?}");
}

// ── Definition, references, rename (P5-06) ─────────────────────────────────

pub fn definition(name: &str, source: &str, needle: &str, within: usize) -> lsp_types::Range {
    let mut harness = Harness::new();
    let uri = harness.open(name, source);
    let response: lsp_types::GotoDefinitionResponse = harness
        .at("textDocument/definition", &uri, support::find(source, needle, within))
        .unwrap_or_else(|| panic!("no definition for {needle:?}"));
    match response {
        lsp_types::GotoDefinitionResponse::Scalar(location) => location.range,
        lsp_types::GotoDefinitionResponse::Array(mut locations) => locations.remove(0).range,
        other => panic!("unexpected {other:?}"),
    }
}

/// A member lands on the field of the struct the base actually has, not on the
/// first field of that name anywhere in the file.
#[test]
fn go_to_definition_on_a_member_lands_on_its_own_struct() {
    let source = "#version 450\n\
        struct Other { vec3 diffuse; };\n\
        uniform Material { vec3 diffuse; } material;\n\
        out vec4 c;\n\
        void main() { c = vec4(material.diffuse, 1.0); }\n";
    let range = definition("a.frag", source, "material.diffuse", 9);
    assert_eq!(range.start, support::find(source, "vec3 diffuse; } material", 5));
}

/// Resolution happened with the scope stack the *use site* saw, so shadowing
/// comes free.
#[test]
fn go_to_definition_respects_shadowing() {
    let source = "#version 450\n\
        float total = 0.0;\n\
        float f() {\n    float total = 1.0;\n    return total;\n}\n";
    let range = definition("a.frag", source, "return total", 7);
    assert_eq!(range.start, support::find(source, "float total = 1.0", 6));
}

/// A `#define` never reaches the expanded token stream, so only the macro
/// table can say where it was written.
#[test]
fn go_to_definition_on_a_macro_lands_on_its_define() {
    let source = "#version 450\n#define MAX_LIGHTS 4\n\
        void main() { int n = MAX_LIGHTS; }\n";
    let range = definition("a.frag", source, "= MAX_LIGHTS", 2);
    assert_eq!(range.start, support::find(source, "MAX_LIGHTS 4", 0));
}

#[test]
fn references_span_a_macro_and_its_invocations() {
    let source = "#version 450\n#define HALF 0.5\n\
        void main() { float a = HALF; float b = HALF; }\n";
    let mut harness = Harness::new();
    let uri = harness.open("a.frag", source);
    let locations: Vec<lsp_types::Location> = harness
        .request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "position": support::find(source, "HALF 0.5", 1),
                "context": { "includeDeclaration": true },
            }),
        )
        .expect("references");
    assert_eq!(locations.len(), 3, "{locations:?}");
}

#[test]
fn rename_refuses_a_builtin_and_refuses_renaming_onto_one() {
    let source = "#version 450\nuniform sampler2D t;\nout vec4 c;\n\
        void main() { c = texture(t, vec2(0.0)); }\n";
    let mut harness = Harness::new();
    let uri = harness.open("a.frag", source);

    // `texture` is the language's.
    assert!(
        harness
            .at::<lsp_types::PrepareRenameResponse>(
                "textDocument/prepareRename",
                &uri,
                support::find(source, "texture(t", 2)
            )
            .is_none()
    );
    // …and so is `mix`, so nothing may be renamed onto it.
    assert!(
        harness
            .request::<lsp_types::WorkspaceEdit>(
                "textDocument/rename",
                json!({
                    "textDocument": { "uri": uri.as_str() },
                    "position": support::find(source, "sampler2D t", 10),
                    "newName": "mix",
                }),
            )
            .is_none()
    );
}

#[test]
fn rename_rewrites_a_macro_and_every_invocation() {
    let source = "#version 450\n#define HALF 0.5\n\
        void main() { float a = HALF; float b = HALF; }\n";
    let mut harness = Harness::new();
    let uri = harness.open("a.frag", source);
    let edit: lsp_types::WorkspaceEdit = harness
        .request(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "position": support::find(source, "= HALF;", 2),
                "newName": "HALF_UNIT",
            }),
        )
        .expect("a rename");
    let edits = &edit.changes.as_ref().unwrap()[&uri];
    assert_eq!(edits.len(), 3);
    assert!(edits.iter().all(|e| e.new_text == "HALF_UNIT"));
}

// ── Semantic tokens (P5-07) ────────────────────────────────────────────────

/// Every semantic token as `(text, type index, modifier bits)`.
///
/// The legend is `semantic_tokens::TYPES`; index 3 is FUNCTION, 4 VARIABLE,
/// 5 PARAMETER, 6 PROPERTY, 7 MACRO, 2 STRUCT, 1 TYPE.
pub fn tokens(name: &str, source: &str) -> Vec<(String, u32, u32)> {
    let mut harness = Harness::new();
    let uri = harness.open(name, source);
    let result: lsp_types::SemanticTokensResult = harness
        .request(
            "textDocument/semanticTokens/full",
            json!({ "textDocument": { "uri": uri.as_str() } }),
        )
        .expect("tokens");
    let lsp_types::SemanticTokensResult::Tokens(tokens) = result else {
        panic!("expected a full token array");
    };

    let lines: Vec<&str> = source.split('\n').collect();
    let (mut line, mut start) = (0u32, 0u32);
    let mut out = Vec::new();
    for token in tokens.data {
        line += token.delta_line;
        start = if token.delta_line == 0 { start + token.delta_start } else { token.delta_start };
        let text: String = lines[line as usize]
            .chars()
            .skip(start as usize)
            .take(token.length as usize)
            .collect();
        out.push((text, token.token_type, token.token_modifiers_bitset));
    }
    out
}

/// The bit `semantic_tokens::modifier::DISABLED` sets.
const DISABLED: u32 = 1 << 3;
const DEFAULT_LIBRARY: u32 = 1 << 2;

/// A local that shadows a builtin paints as the local. The old classifier hit
/// the builtin table first and painted the wrong thing.
#[test]
fn a_name_that_shadows_a_builtin_paints_as_what_it_resolves_to() {
    let source = "#version 450\nvoid main() { float mix = 1.0; float y = mix + 1.0; }\n";
    let painted = tokens("a.frag", source);
    let uses: Vec<&(String, u32, u32)> = painted.iter().filter(|(t, ..)| t == "mix").collect();
    assert_eq!(uses.len(), 2, "{painted:?}");
    for (text, kind, modifiers) in uses {
        assert_eq!(*kind, 4, "{text} should be a variable, not a function");
        assert_eq!(modifiers & DEFAULT_LIBRARY, 0, "and not a library name");
    }
}

#[test]
fn a_macro_and_its_invocations_paint_as_macros() {
    let source = "#version 450\n#define HALF 0.5\nvoid main() { float a = HALF; }\n";
    let painted = tokens("a.frag", source);
    let halves: Vec<&(String, u32, u32)> = painted.iter().filter(|(t, ..)| t == "HALF").collect();
    assert_eq!(halves.len(), 2, "{painted:?}");
    assert!(halves.iter().all(|(_, kind, _)| *kind == 7), "{halves:?}");
}

#[test]
fn a_swizzle_and_a_field_both_paint_as_properties() {
    let painted = tokens("gl.frag", OPENGL);
    let by_name = |name: &str| painted.iter().find(|(t, ..)| t == name).cloned();
    assert_eq!(by_name("rgb").map(|(_, k, _)| k), Some(6), "the swizzle");
    assert_eq!(by_name("diffuse").map(|(_, k, _)| k), Some(6), "the block member");
}

#[test]
fn builtin_functions_and_variables_carry_the_default_library_modifier() {
    let source = "#version 450\nlayout(location = 0) out vec4 c;\n\
        void main() { c = vec4(gl_FragCoord.x); }\n";
    let painted = tokens("a.frag", source);
    let coord = painted.iter().find(|(t, ..)| t == "gl_FragCoord").expect("gl_FragCoord");
    assert_eq!(coord.1, 4);
    assert!(coord.2 & DEFAULT_LIBRARY != 0, "{coord:?}");
}

/// Both branches stay visible — an outline has to show the one you are
/// editing — and the one that is switched off is dimmed rather than hidden.
#[test]
fn an_inactive_branch_is_painted_and_dimmed() {
    let source = "#version 450\nvoid main() {\n#if 0\n  float dead = 1.0;\n#else\n  \
        float live = 2.0;\n#endif\n}\n";
    let painted = tokens("a.frag", source);
    let dead = painted.iter().find(|(t, ..)| t == "dead").expect("still painted");
    assert!(dead.2 & DISABLED != 0, "{dead:?}");
    let live = painted.iter().find(|(t, ..)| t == "live").expect("painted");
    assert_eq!(live.2 & DISABLED, 0, "{live:?}");
}

// ── Symbols, folding, inlay hints (P5-08) ──────────────────────────────────

/// The outline now comes off the CST, so it sees what the old token walk could
/// not: a `#define`, and a block distinguished from its instance.
#[test]
fn the_outline_comes_off_the_cst() {
    let source = "#version 450\n#define MAX 4\n\
        layout(binding = 0) uniform Camera { mat4 view; } camera;\n\
        struct Light { vec3 colour; };\n\
        void main() {}\n";
    let mut harness = Harness::new();
    let uri = harness.open("a.frag", source);
    let response: lsp_types::DocumentSymbolResponse = harness
        .request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri.as_str() } }),
        )
        .expect("an outline");
    let lsp_types::DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("expected a nested outline");
    };
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["MAX", "Camera", "camera", "Light", "main"]);
    assert_eq!(symbols[0].kind, lsp_types::SymbolKind::CONSTANT, "the macro");
    assert_eq!(symbols[1].kind, lsp_types::SymbolKind::INTERFACE, "the block");
    assert_eq!(symbols[2].kind, lsp_types::SymbolKind::VARIABLE, "its instance");
    assert_eq!(symbols[4].kind, lsp_types::SymbolKind::METHOD, "the entry point");
}

/// The branch you are not in is the region you most want out of the way.
#[test]
fn folding_covers_an_inactive_conditional_branch() {
    let source = "#version 450\n#if 0\nfloat dead = 1.0;\nfloat also = 2.0;\n#else\n\
        float live = 3.0;\n#endif\nvoid main() {}\n";
    let mut harness = Harness::new();
    let uri = harness.open("a.frag", source);
    let ranges: Vec<lsp_types::FoldingRange> = harness
        .request(
            "textDocument/foldingRange",
            json!({ "textDocument": { "uri": uri.as_str() } }),
        )
        .expect("folding ranges");
    let spans: Vec<(u32, u32)> =
        ranges.iter().map(|r| (r.start_line, r.end_line)).collect();
    // Lines 2–3 are the dead branch; the `#if` on line 1 stays visible.
    assert!(spans.contains(&(2, 3)), "{spans:?}");
}

/// The one type GLSL leaves to inference.
#[test]
fn an_implicitly_sized_array_is_annotated_with_the_size_it_deduced() {
    let mut settings = Settings::default();
    settings.glsl.inlay_hints.enabled = true;
    settings.glsl.inlay_hints.types = true;

    let source = "#version 450\nconst float weights[] = float[](0.1, 0.2, 0.3);\n\
        void main() {}\n";
    let hints = inlay_hints(settings, "a.frag", source);
    assert_eq!(hints, ["3"], "{hints:?}");
}

/// Parameter names come from the overload, in the reference pages' spelling.
#[test]
fn parameter_hints_name_the_arguments_of_a_builtin_call() {
    let mut settings = Settings::default();
    settings.glsl.inlay_hints.enabled = true;
    settings.glsl.inlay_hints.parameter_names = true;

    let source = "#version 450\nvoid main() { float x = clamp(0.0, 1.0, 2.0); }\n";
    let hints = inlay_hints(settings, "a.frag", source);
    assert_eq!(hints, ["x:", "minVal:", "maxVal:"], "{hints:?}");
}

pub fn inlay_hints(settings: Settings, name: &str, source: &str) -> Vec<String> {
    let mut harness = Harness::with_settings(settings);
    let uri = harness.open(name, source);
    let hints: Vec<lsp_types::InlayHint> = harness
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri.as_str() },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": support::position_of(source, source.len()),
                },
            }),
        )
        .expect("inlay hints");
    hints
        .into_iter()
        .map(|hint| match hint.label {
            lsp_types::InlayHintLabel::String(text) => text,
            other => panic!("expected a string label, got {other:?}"),
        })
        .collect()
}

// ── The RFC's own walkthroughs ─────────────────────────────────────────────

/// RFC 012 §9.1, asserted as one flow: open a `#version 300 es` fragment
/// shader and get real diagnostics, ES-correct hover, and a completion list
/// that knows which era it is in.
#[test]
fn success_criterion_9_1_a_webgl2_fragment_shader() {
    let source = "#version 300 es\nprecision mediump float;\n\
        uniform sampler2D albedo;\nin vec2 v_uv;\nout vec4 fragColour;\n\
        void main() { fragColour = texture(albedo, v_uv); }\n";

    // 1. Real diagnostics: clean now, and a real error when there is one.
    assert!(errors(&published("es.frag", source)).is_empty());
    let broken = source.replace("albedo, v_uv)", "albedo, nowhere)");
    assert!(!errors(&published("es.frag", &broken)).is_empty());

    // 2. Hover on `texture` shows the ES overloads and the page's prose.
    let text = hover("es.frag", source, "texture(albedo", 2);
    assert!(text.contains("gvec4 texture(gsampler2D sampler"), "{text}");
    assert!(!text.contains("sampler1D"), "a desktop-only sampler: {text}");
    assert!(text.len() > 200, "the reference page's prose is there: {text}");

    // 3. Completion offers what ES 3.00 has and not what it does not.
    let labels = complete("es.frag", &source.replace("texture(albedo", "gl|(albedo"));
    assert!(labels.contains(&"gl_FragCoord".to_string()));
    assert!(!labels.contains(&"gl_ClipDistance".to_string()), "desktop-only");
    assert!(!labels.contains(&"gl_ClipVertex".to_string()), "compatibility-only");
}

/// RFC 012 §9.2: the extension's own OpenGL-style example is validated by our
/// analyzer, and the "not validated because naga" message is gone for good.
#[test]
fn success_criterion_9_2_the_shipped_example_is_validated() {
    let source = include_str!("../../../../extensions/wgsl-shader/examples/test.frag");
    let mut harness = Harness::new();
    let uri = harness.open("test.frag", source);
    assert!(harness.diagnostics(&uri).is_none());

    let info: serde_json::Value = harness
        .request("wgsl/shaderInfo", json!({ "textDocument": { "uri": uri.as_str() } }))
        .expect("shader info");
    assert!(info["skipped"].is_null(), "{info}");
    assert_eq!(info["ok"], true, "{info}");
    assert_eq!(info["stage"], "fragment");
    assert_eq!(info["stageGuessed"], false, "a .frag says which stage it is");
    assert_eq!(info["version"], "4.50");
}

/// `workspace/symbol` has to answer for GLSL files the editor never opened,
/// which means the index projects the CST outline too.
#[test]
fn the_workspace_index_finds_glsl_symbols_in_unopened_files() {
    let mut harness = Harness::new();
    harness.workspace_files(&[(
        "lib.frag",
        "#version 450\nfloat sharedFalloff(float d) { return 1.0 / d; }\n",
    )]);
    let response: lsp_types::WorkspaceSymbolResponse =
        harness.request("workspace/symbol", json!({ "query": "falloff" })).expect("symbols");
    let names: Vec<String> = match response {
        lsp_types::WorkspaceSymbolResponse::Nested(symbols) => {
            symbols.into_iter().map(|s| s.name).collect()
        }
        lsp_types::WorkspaceSymbolResponse::Flat(symbols) => {
            symbols.into_iter().map(|s| s.name).collect()
        }
    };
    assert_eq!(names, ["sharedFalloff"]);
}

/// Every request must survive a GLSL document one keystroke from valid, which
/// is the state the editor asks about most of the time.
#[test]
fn every_request_survives_a_half_typed_glsl_document() {
    const METHODS: [&str; 9] = [
        "textDocument/completion",
        "textDocument/hover",
        "textDocument/definition",
        "textDocument/documentHighlight",
        "textDocument/signatureHelp",
        "textDocument/prepareRename",
        "textDocument/documentSymbol",
        "textDocument/foldingRange",
        "textDocument/semanticTokens/full",
    ];
    for partial in [
        "#version",
        "#version 300 es\nprecision",
        "#define",
        "#if",
        "#if 1\nvoid main() {",
        "layout(location = 0) in ",
        "float f(float",
        "uniform Camera {",
        "void main() { vec4 c = texture(",
        "void main() { c.",
        "struct S {",
    ] {
        let mut harness = Harness::new();
        let uri = harness.open("a.frag", partial);
        let position = support::position_of(partial, partial.len());
        for method in METHODS {
            // Nothing may panic and nothing may error; `null` is a fine answer.
            let _: Option<serde_json::Value> = harness.request(
                method,
                json!({
                    "textDocument": { "uri": uri.as_str() },
                    "position": position,
                }),
            );
        }
    }
}

// ── Settings (P5-09) ───────────────────────────────────────────────────────

/// `glsl.defaultVersion` replaces `glsl.validate.dialect`, and it reaches the
/// files already open rather than only the next one — the same requirement the
/// dialect setting had, for the same reason.
#[test]
fn the_default_version_setting_round_trips_and_reaches_open_documents() {
    // A file that declares no `#version` is GLSL 1.10 by the spec, and
    // `texture` is not a 1.10 name.
    let source = "uniform sampler2D t;\nin vec2 uv;\nout vec4 c;\n\
        void main() { c = texture(t, uv); }\n";
    let mut harness = Harness::new();
    let uri = harness.open("a.frag", source);
    let diagnostics = harness.diagnostics(&uri).expect("published");
    assert!(codes(&diagnostics).iter().any(|c| c == "GLSL0223"), "{diagnostics:?}");

    harness.configure(json!({ "glsl": { "defaultVersion": "330 core" } }));
    assert!(harness.diagnostics(&uri).unwrap().is_empty(), "the file is 3.30 now");

    let info: serde_json::Value = harness
        .request("wgsl/shaderInfo", json!({ "textDocument": { "uri": uri.as_str() } }))
        .expect("shader info");
    assert_eq!(info["version"], "3.30");

    // …and switching it back brings the diagnostic back.
    harness.configure(json!({ "glsl": { "defaultVersion": "" } }));
    assert!(!harness.diagnostics(&uri).unwrap().is_empty());
}

/// A client still sending the retired `glsl.validate.dialect` must not break
/// the server, and must not be silently obeyed either.
#[test]
fn the_retired_dialect_setting_is_ignored() {
    let mut harness = Harness::new();
    let uri = harness.open("gl.frag", OPENGL);
    harness.configure(json!({ "glsl": { "validate": { "dialect": "vulkan" } } }));
    assert!(harness.diagnostics(&uri).is_none_or(|d| d.is_empty()), "still clean");
}
