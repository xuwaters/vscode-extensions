//! P4-06 — the overload gauntlet: user functions, builtins, generic families,
//! ranking and ambiguity.

use crate::tests::{expect, expect_clean, type_of};

/// The type a call answers, written against some declarations.
fn call(declarations: &str, expression: &str) -> String {
    type_of(&format!("{declarations}void main() {{ «{expression}»; }}\n"))
}

// ── Builtins and their generic families ──────────────────────────────────

#[test]
fn a_family_binds_once_and_the_return_follows_it() {
    // `#version 450` because the integer and double families arrived in 1.30
    // and 4.00, and the version a file declares decides which overloads it has.
    let declarations = "#version 450\nvec3 a; vec3 b; float f; ivec3 i3; dvec3 d3;\n";
    let cases: &[(&str, &str)] = &[
        // genType binds to the argument, and the return is the same family.
        ("sin(f)", "float"),
        ("sin(a)", "vec3"),
        ("normalize(a)", "vec3"),
        // …even when the return is concrete.
        ("length(a)", "float"),
        ("dot(a, b)", "float"),
        // A family used twice must agree, and the third argument is its own.
        ("mix(a, b, f)", "vec3"),
        ("clamp(a, 0.0, 1.0)", "vec3"),
        // genIType and genBType follow genType's *shape*.
        ("lessThan(a, b)", "bvec3"),
        ("abs(i3)", "ivec3"),
        ("floatBitsToInt(a)", "ivec3"),
        // The double family is its own.
        ("abs(d3)", "dvec3"),
    ];
    for (expression, expected) in cases {
        assert_eq!(&call(declarations, expression), expected, "{expression}");
    }
}

#[test]
fn a_sampler_family_decides_the_result_family() {
    // `gvec4 texture(gsampler2D, vec2)`: the sampler's class picks the vector's.
    let source = "#version 330\nuniform sampler2D s;\nuniform isampler2D is;\n\
                  uniform usampler2D us;\nin vec2 uv;\n";
    assert_eq!(call(source, "texture(s, uv)"), "vec4");
    assert_eq!(call(source, "texture(is, uv)"), "ivec4");
    assert_eq!(call(source, "texture(us, uv)"), "uvec4");
    // A shadow sampler answers a plain float, whatever the family says.
    let shadow = "#version 330\nuniform sampler2DShadow s;\nin vec3 uvw;\n";
    assert_eq!(call(shadow, "texture(s, uvw)"), "float");
}

#[test]
fn an_optional_parameter_may_be_left_out() {
    let source = "#version 330\nuniform sampler2D s;\nin vec2 uv;\n";
    assert_eq!(call(source, "texture(s, uv)"), "vec4");
    assert_eq!(call(source, "texture(s, uv, 0.5)"), "vec4");
    expect_clean(&format!("{source}out vec4 c;\nvoid main() {{ c = texture(s, uv, -0.5); }}\n"));
}

#[test]
fn arguments_are_converted_the_way_the_spec_ranks_them() {
    // `max` has genType, genIType, genUType and genDType forms; an `int`
    // argument must pick the integer one rather than converting to float.
    let version = "#version 450\n";
    assert_eq!(call(version, "max(1, 2)"), "int");
    assert_eq!(call(version, "max(1.0, 2.0)"), "float");
    assert_eq!(call(version, "max(1u, 2u)"), "uint");
    // A mixed call converts the one that has to convert.
    assert_eq!(call(version, "max(1, 2.0)"), "float");
    // …and GLSL 1.10 has only the float family, so the same call answers a
    // float there. Which overloads exist is a property of the version.
    assert_eq!(call("", "max(1, 2)"), "float");
}

#[test]
fn no_overload_of_a_builtin_takes_these_arguments() {
    expect("void main() { float f = sin(true); }\n", &["GLSL0210"]);
    expect("#version 450\nvoid main() { float f = sin(true); }\n", &["GLSL0210"]);
    expect(
        "#version 330\nuniform sampler2D s;\nvoid main() { vec4 c = texture(s); }\n",
        &["GLSL0210"],
    );
}

#[test]
fn an_out_argument_must_be_something_that_can_be_written() {
    expect_clean("#version 450\nvoid main() { float whole; float part = modf(1.5, whole); }\n");
    expect(
        "#version 450\nvoid main() { float part = modf(1.5, 2.0); }\n",
        &["GLSL0214"],
    );
    expect(
        "#version 450\nvoid main() { const float c = 0.0; float p = modf(1.5, c); }\n",
        &["GLSL0215"],
    );
}

// ── User functions ────────────────────────────────────────────────────────

#[test]
fn a_user_call_answers_its_return_type() {
    assert_eq!(call("vec3 shade(float t) { return vec3(t); }\n", "shade(1.0)"), "vec3");
}

#[test]
fn the_best_overload_wins() {
    let declarations = "\
int  pick(int x) { return x; }
float pick(float x) { return x; }
double pick(double x) { return x; }
";
    // Exact matches, each to its own.
    assert_eq!(call(declarations, "pick(1)"), "int");
    assert_eq!(call(declarations, "pick(1.0)"), "float");
    assert_eq!(call(declarations, "pick(1.0lf)"), "double");
    // A `uint` converts, and float beats double because it is the closer rank.
    assert_eq!(call(declarations, "pick(1u)"), "float");
}

#[test]
fn a_call_two_declarations_fit_equally_is_ambiguous() {
    expect(
        "int  g(float x, int y);\nfloat g(int x, float y);\n\
         void main() { g(1, 1); }\n",
        &["GLSL0211"],
    );
    // Two that fit *exactly* are not ambiguous — they are the same call
    // written twice, and the first is taken.
    expect_clean(
        "float h(float x, float y) { return x; }\nfloat h(float x, float y);\n\
         void main() { h(1.0, 1.0); }\n",
    );
}

#[test]
fn the_wrong_number_of_arguments_names_the_number() {
    let analysis = crate::tests::analyze(
        "float f(float a, float b) { return a; }\nvoid main() { f(1.0); }\n",
    );
    assert_eq!(crate::tests::error_codes(&analysis), &["GLSL0212"]);
    assert!(
        analysis.errors().any(|d| d.message.contains("2 arguments")),
        "{:?}",
        crate::tests::messages(&analysis)
    );
}

#[test]
fn an_argument_the_parameter_cannot_take_names_the_parameter() {
    let analysis = crate::tests::analyze(
        "float f(vec3 direction) { return direction.x; }\nvoid main() { f(1.0); }\n",
    );
    assert_eq!(crate::tests::error_codes(&analysis), &["GLSL0213"]);
    assert!(
        analysis.errors().any(|d| d.message.contains("direction")),
        "{:?}",
        crate::tests::messages(&analysis)
    );
}

#[test]
fn no_declaration_of_an_overloaded_name_fits() {
    expect(
        "float f(vec3 a) { return a.x; }\nfloat f(vec2 a) { return a.x; }\n\
         void main() { f(true); }\n",
        &["GLSL0210"],
    );
}

#[test]
fn calling_something_that_is_not_a_function() {
    expect("float x;\nvoid main() { x(1.0); }\n", &["GLSL0201"]);
    expect("void main() { missing(1.0); }\n", &["GLSL0201"]);
    // …and a name an extension might own is left alone.
    expect_clean("void main() { subgroupBarrierNV(); }\n");
}

#[test]
fn an_out_parameter_of_a_user_function_needs_an_lvalue() {
    expect_clean("void f(out float x) { x = 1.0; }\nvoid main() { float v; f(v); }\n");
    expect("void f(out float x) { x = 1.0; }\nvoid main() { f(1.0); }\n", &["GLSL0214"]);
}

#[test]
fn a_call_with_an_unknown_argument_says_nothing() {
    expect_clean("void main() { float f = sin(gl_SomethingUnmodelled); }\n");
    expect_clean("void main() { float f = someExtensionCallEXT(1.0, 2.0, 3.0); }\n");
}

#[test]
fn void_is_not_an_argument() {
    expect_clean("void f() { }\nvoid main() { f(); }\n");
    expect_clean("void f(void) { }\nvoid main() { f(); }\n");
}
