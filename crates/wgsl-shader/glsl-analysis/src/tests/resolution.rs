//! P4-01 — scopes, shadowing, redeclaration and what each name resolves to.

use crate::tests::{analyze, expect, expect_clean, type_of};
use crate::{SymbolKind, Target};

/// The kinds of the symbols the file declares, in declaration order.
fn kinds(source: &str) -> Vec<(String, SymbolKind)> {
    let analysis = analyze(source);
    analysis
        .symbols
        .iter()
        .map(|(_, symbol)| (symbol.name.clone(), symbol.kind))
        .collect()
}

#[test]
fn every_kind_of_declaration_is_recorded() {
    let source = "\
struct Light { vec3 colour; };
uniform mat4 view;
float lambert(vec3 n, vec3 l) {
  float d = dot(n, l);
  return d;
}
";
    let kinds = kinds(source);
    assert!(kinds.contains(&("Light".to_string(), SymbolKind::Struct)));
    assert!(kinds.contains(&("colour".to_string(), SymbolKind::Field)));
    assert!(kinds.contains(&("view".to_string(), SymbolKind::Global)));
    assert!(kinds.contains(&("lambert".to_string(), SymbolKind::Function)));
    assert!(kinds.contains(&("n".to_string(), SymbolKind::Parameter)));
    assert!(kinds.contains(&("d".to_string(), SymbolKind::Local)));
}

#[test]
fn a_name_resolves_to_the_innermost_declaration() {
    // The outer `x` is a float and the inner one an int; the type at each use
    // is what proves which declaration won.
    let source = "\
float x = 1.0;
void main() {
  {
    int x = 2;
    int y = «x»;
  }
}
";
    assert_eq!(type_of(source), "int");
    let outer = "\
float x = 1.0;
void main() {
  { int x = 2; }
  float y = «x»;
}
";
    assert_eq!(type_of(outer), "float");
}

#[test]
fn a_block_scope_ends_at_its_brace() {
    expect(
        "void main() {\n  { int inner = 1; }\n  inner = 2;\n}\n",
        &["GLSL0200"],
    );
}

#[test]
fn a_for_header_declares_over_the_whole_loop() {
    expect_clean("void main() {\n  for (int i = 0; i < 4; ++i) { int j = i; }\n}\n");
    // …and not past it.
    expect(
        "void main() {\n  for (int i = 0; i < 4; ++i) { }\n  i = 1;\n}\n",
        &["GLSL0200"],
    );
}

#[test]
fn a_condition_may_declare() {
    expect_clean(
        "bool next(out int value);\nvoid main() {\n  int v;\n\
         \x20 while (bool ok = next(v)) { v = v + 1; }\n}\n",
    );
}

#[test]
fn a_function_may_be_called_before_it_is_defined() {
    // GLSL wants a prototype first; an editor showing an error for a file
    // being typed top-down would be useless, so both passes see every
    // file-scope declaration.
    expect_clean("void main() { helper(); }\nvoid helper() { }\n");
}

#[test]
fn a_prototype_and_its_definition_are_not_a_redeclaration() {
    expect_clean("float f(float x);\nfloat f(float x) { return x; }\n");
}

#[test]
fn overloads_share_a_name() {
    expect_clean(
        "float f(float x) { return x; }\nfloat f(vec2 v) { return v.x; }\n\
         void main() { float a = f(1.0); float b = f(vec2(1.0)); }\n",
    );
}

#[test]
fn declaring_a_name_twice_in_one_scope_is_an_error() {
    expect("float x;\nfloat x;\n", &["GLSL0203"]);
    expect("void main() {\n  int a = 1;\n  int a = 2;\n}\n", &["GLSL0203"]);
    // A member declared twice in one struct, too.
    expect("struct S { int a; float a; };\n", &["GLSL0203"]);
    // Shadowing in a nested scope is not a redeclaration.
    expect_clean("float x;\nvoid main() { int x = 1; x = 2; }\n");
}

#[test]
fn an_interface_block_with_an_instance_is_reached_through_it() {
    let source = "\
uniform Camera { mat4 view; mat4 proj; } camera;
void main() { mat4 m = «camera.view»; }
";
    assert_eq!(type_of(source), "mat4");
    expect_clean(
        "uniform Camera { mat4 view; } camera;\nvoid main() { mat4 m = camera.view; }\n",
    );
    // The block *name* is not a variable — it is what the API binds against —
    // so it has no type and nothing is claimed about a use of it.
    let analysis = analyze(
        "uniform Camera { mat4 view; } camera;\nvoid main() { mat4 m = Camera.view; }\n",
    );
    assert!(crate::tests::error_codes(&analysis).is_empty());
    assert!(matches!(
        analysis.reference_at("uniform ".len() as u32).map(|r| &r.target),
        Some(Target::Symbol(_))
    ));
}

#[test]
fn an_anonymous_block_puts_its_members_in_global_scope() {
    let source = "\
uniform Camera { mat4 view; mat4 proj; };
void main() { mat4 m = «view»; }
";
    assert_eq!(type_of(source), "mat4");
    expect_clean("uniform Camera { mat4 view; };\nvoid main() { mat4 m = view; }\n");
}

#[test]
fn a_struct_name_is_a_type_and_a_constructor() {
    expect_clean(
        "struct Light { vec3 colour; float power; };\n\
         Light make() { return Light(vec3(1.0), 2.0); }\n",
    );
    let source = "\
struct Light { vec3 colour; };
void main() { vec3 c = «Light(vec3(1.0)).colour»; }
";
    assert_eq!(type_of(source), "vec3");
}

#[test]
fn a_struct_may_be_declared_inside_a_body() {
    expect_clean("void main() {\n  struct Local { int a; };\n  Local l = Local(1);\n}\n");
}

#[test]
fn an_undeclared_name_is_reported_once_and_recorded_as_unresolved() {
    let analysis = analyze("void main() { float x = missing; }\n");
    assert_eq!(crate::tests::error_codes(&analysis), &["GLSL0200"]);
    let offset = "void main() { float x = ".len() as u32;
    let reference = analysis.reference_at(offset).expect("the name is still recorded");
    assert!(matches!(reference.target, Target::Unresolved));
}

#[test]
fn a_gl_prefixed_or_extension_name_is_never_an_unknown_identifier() {
    // The one rule that keeps every ray-tracing, mesh and subgroup shader in
    // the corpus from lighting up: a name the language reserves for itself or
    // an extension is a name this analysis has no opinion about.
    expect_clean("void main() { float x = gl_SomethingNobodyModelled; }\n");
    expect_clean("void main() { float x = somethingEXT; }\n");
    expect_clean("void main() { float x = subgroupSomethingNV; }\n");
}

#[test]
fn resolution_survives_a_file_that_did_not_parse() {
    // A broken file still resolves — hover and go-to-definition have to work
    // between two keystrokes — and reports no *error* about its semantics.
    let analysis = analyze("uniform mat4 view;\nvoid main() { view * ; }\n");
    assert!(crate::tests::error_codes(&analysis).is_empty());
    let offset = "uniform mat4 view;\nvoid main() { ".len() as u32;
    assert!(matches!(
        analysis.reference_at(offset).map(|r| &r.target),
        Some(Target::Symbol(_))
    ));
}

#[test]
fn a_type_name_resolves_to_a_type() {
    let analysis = analyze("uniform mat4 view;\n");
    let reference = analysis.reference_at("uniform ".len() as u32).unwrap();
    assert!(matches!(reference.target, Target::Type(_)));
}

#[test]
fn a_builtin_resolves_to_the_spec_table() {
    let analysis = analyze("void main() { float x = dot(vec3(1.0), vec3(2.0)); }\n");
    let reference = analysis.reference_at("void main() { float x = ".len() as u32).unwrap();
    match reference.target {
        Target::BuiltinFunction(function) => assert_eq!(function.name, "dot"),
        ref other => panic!("dot resolved to {other:?}"),
    }
}
