//! P3-06 — dialect variances.
//!
//! The parser is deliberately **version-blind**: it parses `attribute` at 4.60
//! and `layout` at 1.10 alike, and lets analysis judge. That is not laxity, it
//! is the only way an editor stays useful while a `#version` line is being
//! edited — and it is why every fixture here asserts a *clean* parse of a
//! construct some other version would reject.

use pretty_assertions::assert_eq;

use crate::cst::NodeKind;
use crate::preprocessor::Profile;

use super::{parse_errors, parsed};

/// Parse and insist nothing was reported.
fn clean(source: &str) {
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new(), "parsing {source:?}");
    assert!(
        !tree.nodes().any(|(_, n)| n.kind == NodeKind::Error),
        "an Error node in {source:?}"
    );
}

#[test]
fn a_webgl1_era_shader_parses() {
    clean(
        "\
#version 100
precision mediump float;
attribute vec3 aPosition;
attribute vec2 aTexCoord;
varying vec2 vTexCoord;
uniform mat4 uMvp;
void main() {
    vTexCoord = aTexCoord;
    gl_Position = uMvp * vec4(aPosition, 1.0);
}
",
    );
}

#[test]
fn a_webgl1_fragment_shader_with_the_legacy_builtins_parses() {
    clean(
        "\
#version 100
precision highp float;
uniform sampler2D uTexture;
varying vec2 vTexCoord;
void main() {
    gl_FragColor = texture2D(uTexture, vTexCoord);
}
",
    );
}

#[test]
fn a_shader_with_no_version_line_at_all_parses() {
    // Which is GLSL 1.10 by §3.3, and by far the most common thing to open.
    clean("varying vec4 colour;\nvoid main() { gl_FragColor = colour; }\n");
}

#[test]
fn an_es_300_shader_parses() {
    let source = "\
#version 300 es
precision highp float;
layout(location = 0) in vec3 aPosition;
out vec4 vColour;
uniform Block { mat4 mvp; } block;
void main() {
    vColour = vec4(aPosition, 1.0);
    gl_Position = block.mvp * vColour;
}
";
    clean(source);
    let (pp, _) = parsed(source);
    assert_eq!(pp.profile(), Profile::Es);
}

#[test]
fn precision_statements_are_legal_wherever_they_appear() {
    // ES allows them at file scope and inside a body, and repeated.
    clean(
        "\
#version 310 es
precision highp float;
precision mediump int;
void f() {
    precision lowp float;
    float x = 1.0;
}
",
    );
}

#[test]
fn a_desktop_460_shader_parses() {
    clean(
        "\
#version 460 core
layout(local_size_x = 32, local_size_y = 1, local_size_z = 1) in;
layout(std430, binding = 0) restrict buffer Data { double values[]; } data;
shared float scratch[32];
void main() {
    uint i = gl_GlobalInvocationID.x;
    scratch[gl_LocalInvocationIndex] = float(data.values[i]);
    barrier();
}
",
    );
}

#[test]
fn a_compatibility_profile_declaration_parses() {
    clean(
        "\
#version 130 compatibility
attribute vec4 position;
varying vec4 colour;
void main() {
    gl_Position = gl_ModelViewProjectionMatrix * position;
    colour = gl_Color;
}
",
    );
}

#[test]
fn geometry_and_tessellation_layouts_parse() {
    clean(
        "\
#version 450
layout(triangles, invocations = 2) in;
layout(triangle_strip, max_vertices = 3) out;
layout(vertices = 3) out;
in gl_PerVertex { vec4 gl_Position; } gl_in[];
out gl_PerVertex { vec4 gl_Position; };
void main() {
    for (int i = 0; i < 3; ++i) {
        gl_Position = gl_in[i].gl_Position;
        EmitVertex();
    }
    EndPrimitive();
}
",
    );
}

#[test]
fn the_parser_does_not_care_which_version_the_file_claims() {
    // The same body under four `#version` lines must give the same tree.
    let body = "attribute vec3 a;\nlayout(location = 0) out vec4 c;\nvoid main() { c = \
                vec4(a, 1.0); }\n";
    let shapes: Vec<String> = ["", "#version 100\n", "#version 330 core\n", "#version 460\n"]
        .iter()
        .map(|version| {
            let source = format!("{version}{body}");
            let (pp, tree) = parsed(&source);
            assert_eq!(parse_errors(&tree), Vec::<&str>::new(), "{source:?}");
            tree.dump(&pp, &source)
        })
        .collect();
    assert!(shapes.windows(2).all(|pair| pair[0] == pair[1]), "the trees differ by version");
}

#[test]
fn extension_storage_qualifiers_do_not_derail_the_file() {
    // Ray tracing and mesh shading put words in qualifier position that core
    // GLSL never had. Parsing them costs nothing; whether they are *allowed*
    // is Phase 4's question.
    clean(
        "\
#version 460
#extension GL_EXT_ray_tracing : require
layout(location = 0) rayPayloadEXT vec3 payload;
hitAttributeEXT vec2 attribs;
void main() {
    payload = vec3(attribs, 1.0);
}
",
    );
    clean(
        "\
#version 450
#extension GL_EXT_nonuniform_qualifier : enable
nonuniformEXT int index;
void main() { index = nonuniformEXT(index); }
",
    );
}

#[test]
fn a_conditional_hides_the_dead_branch_from_the_parser_but_not_the_file() {
    let source = "\
#version 300 es
#ifdef GL_ES
out vec4 colour;
#else
varying vec4 colour;
#endif
";
    let (pp, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    // One declaration reached the parser…
    assert_eq!(tree.child_nodes(tree.root()).count(), 1);
    // …and the branch that did not is still on the record, and still covered.
    assert_eq!(pp.inactive.len(), 1);
    assert_eq!(tree.reconstruct(&pp, source), source);
}

#[test]
fn a_macro_that_expands_to_a_declaration_parses_as_one() {
    let source = "#define UNIFORM(t, n) uniform t n\nUNIFORM(vec4, colour);\n";
    let (pp, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let declaration = tree.child_nodes(tree.root()).next().unwrap();
    assert_eq!(tree.kind(declaration), NodeKind::Declaration);
    assert_eq!(tree.spelling(&pp, declaration), "uniform vec4 colour;");
    // Everything in it points at the invocation, which is text the user wrote.
    assert_eq!(tree.text(source, declaration), "UNIFORM(vec4, colour);");
}
