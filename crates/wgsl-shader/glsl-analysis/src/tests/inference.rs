//! P4-05 — the operators: component-wise semantics, matrix `*`, comparisons,
//! logic, and the ternary.

use crate::tests::{expect, expect_clean, type_of};

/// The type of an expression, given some declarations to write it against.
fn typed(declarations: &str, expression: &str) -> String {
    type_of(&format!("{declarations}void main() {{ «{expression}»; }}\n"))
}

/// The standard cast of operands every table below is written against.
const DECLARATIONS: &str = "\
float f; int i; uint u; double d; bool b;
vec2 v2; vec3 v3; vec4 v4; ivec3 i3; uvec3 u3; bvec3 b3; dvec3 d3;
mat3 m3; mat4 m4; mat4x2 m4x2; mat2x4 m2x4;
";

#[track_caller]
fn table(cases: &[(&str, &str)]) {
    for (expression, expected) in cases {
        assert_eq!(&typed(DECLARATIONS, expression), expected, "{expression}");
    }
}

#[test]
fn literals_carry_their_own_type() {
    table(&[
        ("1", "int"),
        ("1u", "uint"),
        ("1.0", "float"),
        ("1.5e3", "float"),
        ("1.0lf", "double"),
        ("true", "bool"),
        ("0x10", "int"),
    ]);
}

#[test]
fn arithmetic_is_component_wise_with_scalars_broadcasting() {
    table(&[
        ("f + f", "float"),
        ("v3 + v3", "vec3"),
        ("v3 * f", "vec3"),
        ("f * v3", "vec3"),
        ("v3 / 2.0", "vec3"),
        ("m3 + m3", "mat3"),
        ("m3 * 2.0", "mat3"),
        ("-v3", "vec3"),
        // The wider component type wins.
        ("i + f", "float"),
        ("i + u", "uint"),
        ("f + d", "double"),
        ("i3 + v3", "vec3"),
        ("v3 + d3", "dvec3"),
    ]);
}

#[test]
fn matrix_multiplication_is_linear_algebra() {
    table(&[
        ("m4 * v4", "vec4"),
        ("v4 * m4", "vec4"),
        ("m3 * m3", "mat3"),
        // matAxB * matCxA is matCxB: 4×2 columns×rows times 2×4 is 2 columns
        // of 2 rows.
        ("m4x2 * m2x4", "mat2"),
        ("m2x4 * m4x2", "mat4"),
        // A mat4x2 has four columns, so it takes a vec4 and answers a vec2.
        ("m4x2 * v4", "vec2"),
        ("v2 * m4x2", "vec4"),
    ]);
    // …and the shapes that do not line up are an error, not a guess.
    expect("mat4x2 a; mat2 b;\nvoid main() { mat2 c = a * b; }\n", &["GLSL0217"]);
    expect("mat4x2 a; vec2 b;\nvoid main() { vec2 c = a * b; }\n", &["GLSL0217"]);
}

#[test]
fn comparisons_and_logic_answer_bool() {
    table(&[
        ("f < f", "bool"),
        ("i <= 2", "bool"),
        ("v3 == v3", "bool"),
        ("v3 != v3", "bool"),
        ("b && b", "bool"),
        ("b || b", "bool"),
        ("b ^^ b", "bool"),
        ("!b", "bool"),
    ]);
}

#[test]
fn the_ordering_comparisons_are_scalars_only() {
    // `v3 < v3` is what `lessThan` is for; the operator has no vector form.
    expect("vec3 a;\nvoid main() { bool b = a < a; }\n", &["GLSL0216"]);
    expect_clean("vec3 a;\nvoid main() { bvec3 b = lessThan(a, a); }\n");
}

#[test]
fn logic_needs_bools_and_arithmetic_needs_numbers() {
    expect("void main() { bool b = 1.0 && true; }\n", &["GLSL0216"]);
    expect("void main() { bool b = !1.0; }\n", &["GLSL0216"]);
    expect("void main() { float f = true + 1.0; }\n", &["GLSL0216"]);
    expect("void main() { bool b = true; b = -b; }\n", &["GLSL0216"]);
}

#[test]
fn the_integer_operators_reject_floats() {
    table(&[
        ("i % 2", "int"),
        ("i << 2", "int"),
        ("i3 & i3", "ivec3"),
        ("~i", "int"),
        ("u | 1u", "uint"),
    ]);
    expect("void main() { float f = 1.0 % 2.0; }\n", &["GLSL0216"]);
    expect("void main() { float f = 1.0; float g = f << 1; }\n", &["GLSL0216"]);
    expect("void main() { float f = ~1.0; }\n", &["GLSL0216"]);
}

#[test]
fn operands_of_different_shapes_do_not_combine() {
    expect("vec2 a; vec3 b;\nvoid main() { vec3 c = a + b; }\n", &["GLSL0217"]);
    expect("vec3 a; bvec3 b;\nvoid main() { vec3 c = a + b; }\n", &["GLSL0216"]);
}

#[test]
fn the_ternary_unifies_its_branches() {
    table(&[
        ("b ? f : f", "float"),
        ("b ? i : f", "float"),
        ("b ? v3 : v3", "vec3"),
    ]);
    expect("bool b;\nvoid main() { float f = b ? 1.0 : true; }\n", &["GLSL0217"]);
    expect("void main() { float f = 1.0 ? 1.0 : 2.0; }\n", &["GLSL0218"]);
}

#[test]
fn assignment_checks_the_conversion_and_answers_the_target_type() {
    expect_clean("void main() { float f; f = 1; f += 2; f *= 2.0; }\n");
    expect_clean("void main() { vec3 v; v *= 2.0; v += vec3(1.0); }\n");
    // int ← float narrows, which GLSL does not do implicitly.
    expect("void main() { int i; i = 1.0; }\n", &["GLSL0217"]);
    // …and a compound assignment applies its operator's rules first.
    expect("void main() { vec3 v; v %= 2; }\n", &["GLSL0216"]);
}

#[test]
fn writing_to_something_that_cannot_be_written_is_an_error() {
    expect("void main() { 1.0 = 2.0; }\n", &["GLSL0214"]);
    expect("void main() { const float f = 1.0; f = 2.0; }\n", &["GLSL0215"]);
    expect("uniform float f;\nvoid main() { f = 1.0; }\n", &["GLSL0215"]);
    expect("in float f;\nvoid main() { f = 1.0; }\n", &["GLSL0215"]);
    expect("void main() { ++1.0; }\n", &["GLSL0214"]);
    // A parameter is a copy the function owns, whatever it was passed.
    expect_clean("float f(float x) { x = x + 1.0; return x; }\n");
    // …and `varying` is an input or an output depending on the stage, so it is
    // never treated as read-only.
    expect_clean("varying float v;\nvoid main() { v = 1.0; }\n");
}

#[test]
fn an_unknown_operand_makes_every_operator_quiet() {
    expect_clean("void main() { float f = gl_SomethingUnmodelled + 1.0; }\n");
    expect_clean("void main() { bool b = gl_SomethingUnmodelled && true; }\n");
}

#[test]
fn a_long_operator_chain_does_not_overflow() {
    // The parser counts assignments, not precedence rungs, so a chain like
    // this is one statement and hundreds of nested nodes.
    let chain = std::iter::repeat_n("1.0", 2_000).collect::<Vec<_>>().join(" + ");
    let source = format!("void main() {{ float f = {chain}; }}\n");
    let analysis = crate::tests::analyze(&source);
    // The only requirement is that it answers at all.
    assert!(analysis.errors().next().is_none());
}
