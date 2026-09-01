//! P4-04 — swizzles, struct members, `length()` and indexing.

use crate::tests::{expect, expect_clean, type_of};

/// Wrap an expression in a shader and ask for its type.
fn in_main(declarations: &str, expression: &str) -> String {
    type_of(&format!("{declarations}void main() {{ float unused_ = 0.0; «{expression}»; }}\n"))
}

#[test]
fn a_swizzle_answers_as_many_components_as_it_names() {
    let cases: &[(&str, &str)] = &[
        ("v.x", "float"),
        ("v.xy", "vec2"),
        ("v.xyz", "vec3"),
        ("v.xyzw", "vec4"),
        ("v.wzyx", "vec4"),
        // The three sets are interchangeable and answer the same thing.
        ("v.r", "float"),
        ("v.rgb", "vec3"),
        ("v.st", "vec2"),
        // A repeat is legal in a value position.
        ("v.xxxx", "vec4"),
    ];
    for (expression, expected) in cases {
        assert_eq!(&in_main("vec4 v;\n", expression), expected, "{expression}");
    }
    // The component type follows the vector's.
    assert_eq!(in_main("ivec3 v;\n", "v.xy"), "ivec2");
    assert_eq!(in_main("bvec2 v;\n", "v.y"), "bool");
}

#[test]
fn a_swizzle_the_vector_cannot_answer_is_an_error() {
    expect("vec2 v;\nvoid main() { float f = v.z; }\n", &["GLSL0205"]);
    expect("vec4 v;\nvoid main() { float f = v.q; }\n", &[]);
    expect("vec2 v;\nvoid main() { float f = v.p; }\n", &["GLSL0205"]);
    // A letter no set has.
    expect("vec4 v;\nvoid main() { float f = v.k; }\n", &["GLSL0205"]);
    // Mixing sets.
    expect("vec4 v;\nvoid main() { vec2 f = v.xr; }\n", &["GLSL0205"]);
    // More than four.
    expect("vec4 v;\nvoid main() { vec4 f = v.xyzwx; }\n", &["GLSL0205"]);
}

#[test]
fn a_repeated_swizzle_cannot_be_assigned_to() {
    expect_clean("void main() { vec4 v; v.xy = vec2(1.0); }\n");
    expect("void main() { vec4 v; v.xx = vec2(1.0); }\n", &["GLSL0214"]);
}

#[test]
fn struct_members_answer_their_declared_type() {
    let declarations = "struct Light { vec3 colour; float power; };\nLight light;\n";
    assert_eq!(in_main(declarations, "light.colour"), "vec3");
    assert_eq!(in_main(declarations, "light.power"), "float");
    assert_eq!(in_main(declarations, "light.colour.g"), "float");
}

#[test]
fn a_member_the_struct_does_not_have_is_an_error() {
    expect(
        "struct Light { vec3 colour; };\nLight light;\nvoid main() { float f = light.power; }\n",
        &["GLSL0204"],
    );
}

#[test]
fn a_member_of_something_with_no_members_is_an_error() {
    expect("void main() { mat4 m; float f = m.x; }\n", &["GLSL0206"]);
    expect("float f;\nvoid main() { float g = f.member; }\n", &["GLSL0206"]);
}

#[test]
fn length_is_the_one_method() {
    assert_eq!(in_main("float a[4];\n", "a.length()"), "int");
    assert_eq!(in_main("vec3 v;\n", "v.length()"), "int");
    assert_eq!(in_main("mat4 m;\n", "m.length()"), "int");
    expect_clean("void main() { float a[4]; int n = a.length(); }\n");
}

#[test]
fn indexing_answers_the_element_type() {
    assert_eq!(in_main("float a[4];\n", "a[0]"), "float");
    assert_eq!(in_main("vec4 v;\n", "v[1]"), "float");
    // A matrix indexes to a column, which is as long as it has rows.
    assert_eq!(in_main("mat4x2 m;\n", "m[0]"), "vec2");
    assert_eq!(in_main("mat4x2 m;\n", "m[0][1]"), "float");
    assert_eq!(in_main("vec3 a[2];\n", "a[1].y"), "float");
}

#[test]
fn indexing_something_that_is_not_indexable_is_an_error() {
    expect("void main() { float f = 1.0; float g = f[0]; }\n", &["GLSL0207"]);
    expect(
        "struct S { int a; };\nS s;\nvoid main() { int i = s[0]; }\n",
        &["GLSL0207"],
    );
}

#[test]
fn a_constant_index_outside_the_bounds_is_an_error() {
    expect("void main() { vec3 v; float f = v[3]; }\n", &["GLSL0208"]);
    expect("void main() { float a[2]; float f = a[2]; }\n", &["GLSL0208"]);
    expect("void main() { mat3 m; vec3 c = m[3]; }\n", &["GLSL0208"]);
    // A constant folded from a `const int` counts.
    expect(
        "const int N = 4;\nvoid main() { vec3 v; float f = v[N]; }\n",
        &["GLSL0208"],
    );
    // And a variable index says nothing at all.
    expect_clean("uniform int i;\nvoid main() { vec3 v; float f = v[i]; }\n");
    // Nor does an index into an array whose size nobody stated.
    expect_clean("buffer B { float data[]; };\nvoid main() { float f = data[99]; }\n");
}

#[test]
fn an_array_size_from_a_const_is_a_real_size() {
    assert_eq!(in_main("const int N = 3;\nfloat a[N];\n", "a"), "float[3]");
    assert_eq!(in_main("const int N = 3;\nfloat a[N * 2];\n", "a"), "float[6]");
}

#[test]
fn members_of_an_unknown_type_say_nothing() {
    // The rule that keeps every unmodelled extension type quiet.
    expect_clean("void main() { float f = gl_LightSource[0].diffuse.r; }\n");
}
