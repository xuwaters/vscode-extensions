//! P4-08 — statement rules: returns, jumps, conditions, `const` and array
//! sizes.

use crate::tests::{analyze, codes, expect, expect_clean};

#[test]
fn a_return_must_match_its_function() {
    expect_clean("float f() { return 1.0; }\n");
    // An implicit conversion is a match.
    expect_clean("float f() { return 1; }\n");
    expect_clean("void g() { return; }\n");
    expect("float f() { return vec3(1.0); }\n", &["GLSL0219"]);
    expect("float f() { return; }\n", &["GLSL0219"]);
    expect("void g() { return 1.0; }\n", &["GLSL0219"]);
    // A struct return type is checked like any other.
    expect_clean("struct S { int a; };\nS f() { return S(1); }\n");
    expect("struct S { int a; };\nS f() { return 1; }\n", &["GLSL0219"]);
}

#[test]
fn a_non_void_function_with_no_return_at_all_is_a_warning() {
    // A warning, not an error: it is what a half-typed function looks like,
    // and painting it red between two keystrokes helps nobody.
    let analysis = analyze("float f() { float x = 1.0; }\n");
    assert_eq!(codes(&analysis), &["GLSL0227"]);
    assert!(analysis.errors().next().is_none());
    // One `return` anywhere is enough to say nothing.
    expect_clean("float f(bool c) { if (c) { return 1.0; } return 2.0; }\n");
    expect_clean("void g() { }\n");
}

#[test]
fn code_after_a_return_is_a_warning() {
    let analysis = analyze("float f() { return 1.0; float x = 2.0; }\n");
    assert_eq!(codes(&analysis), &["GLSL0226"]);
    assert!(analysis.errors().next().is_none());
    // Reported once per block, however much follows.
    let long = analyze("void f() { return; int a = 1; int b = 2; int c = 3; }\n");
    assert_eq!(codes(&long), &["GLSL0226"]);
}

#[test]
fn break_and_continue_need_something_to_leave() {
    expect_clean("void main() { for (int i = 0; i < 4; ++i) { break; } }\n");
    expect_clean("void main() { while (true) { continue; } }\n");
    expect_clean("void main() { int i = 0; switch (i) { case 0: break; } }\n");
    expect("void main() { break; }\n", &["GLSL0221"]);
    expect("void main() { continue; }\n", &["GLSL0221"]);
    // A `switch` is something to break out of but not something to continue.
    expect("void main() { int i = 0; switch (i) { case 0: continue; } }\n", &["GLSL0221"]);
    // And the loop's scope ends with it.
    expect(
        "void main() { for (int i = 0; i < 4; ++i) { } break; }\n",
        &["GLSL0221"],
    );
}

#[test]
fn a_condition_must_be_a_bool() {
    expect_clean("void main() { if (true) { } }\n");
    expect_clean("uniform float f;\nvoid main() { if (f > 0.0) { } }\n");
    expect_clean("void main() { for (int i = 0; i < 4; ++i) { } }\n");
    expect("void main() { if (1.0) { } }\n", &["GLSL0218"]);
    expect("void main() { while (1) { } }\n", &["GLSL0218"]);
    expect("void main() { for (int i = 0; i; ++i) { } }\n", &["GLSL0218"]);
    // A `switch` selects on an integer, and must not be asked for a bool.
    expect_clean("void main() { int i = 0; switch (i) { case 0: break; } }\n");
}

#[test]
fn a_const_needs_a_constant_value() {
    expect_clean("const float pi = 3.14159;\nvoid main() { }\n");
    expect_clean("const int n = 2 * 3 + 1;\nvoid main() { }\n");
    expect("const float pi;\nvoid main() { }\n", &["GLSL0222"]);
    expect("uniform float f;\nconst float c = f;\nvoid main() { }\n", &["GLSL0222"]);
    expect(
        "float f() { return 1.0; }\nvoid main() { const float c = f(); }\n",
        &["GLSL0222"],
    );
    // A builtin call is not something this crate folds, and an unfoldable
    // initialiser is not thereby non-constant.
    expect_clean("const float c = sin(1.0);\nvoid main() { }\n");
}

#[test]
fn an_array_size_must_be_a_positive_constant() {
    expect_clean("void main() { float a[4]; }\n");
    expect_clean("const int N = 4;\nvoid main() { float a[N]; }\n");
    expect("void main() { float a[0]; }\n", &["GLSL0225"]);
    expect("void main() { float a[-1]; }\n", &["GLSL0225"]);
    expect("uniform int n;\nvoid main() { float a[n]; }\n", &["GLSL0225"]);
    // An unsized array is not a bad size; it is a size the declaration leaves
    // open, which is how a buffer's tail and an `in` array are written.
    expect_clean("buffer B { float data[]; };\nvoid main() { }\n");
    expect_clean("in float heights[];\nvoid main() { }\n");
}

#[test]
fn an_initialiser_must_convert_to_what_it_initialises() {
    expect_clean("void main() { float f = 1; vec3 v = vec3(1.0); }\n");
    expect("void main() { int i = 1.0; }\n", &["GLSL0217"]);
    expect("void main() { vec3 v = vec2(1.0); }\n", &["GLSL0217"]);
    // A braced initialiser list is deliberately not shape-checked.
    expect_clean("#version 420\nvoid main() { float a[2] = { 1.0, 2.0 }; }\n");
}

#[test]
fn a_body_walks_even_when_a_statement_is_unreadable() {
    // A file with a parse error still resolves, and reports no semantic error.
    let analysis = analyze("uniform float f;\nvoid main() { float g = f + ; }\n");
    assert!(analysis.errors().next().is_none());
}

#[test]
fn deeply_nested_statements_do_not_overflow() {
    let depth = 300;
    let mut source = String::from("void main() {\n");
    for _ in 0..depth {
        source.push_str("if (true) {\n");
    }
    source.push_str("int x = 1;\n");
    for _ in 0..depth {
        source.push_str("}\n");
    }
    source.push_str("}\n");
    // The only requirement is an answer rather than a stack overflow.
    let analysis = analyze(&source);
    let _ = analysis.diagnostics.len();
}
