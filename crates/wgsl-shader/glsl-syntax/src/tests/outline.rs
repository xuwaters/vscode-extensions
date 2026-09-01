//! P3-08 — the outline read off the CST, and its parity with the old walk.
//!
//! The parity fixtures are the heuristic walk's *actual output* on the
//! extension's `examples/`, transcribed here as `(kind, name, name_span)`
//! triples. Transcribed rather than computed, because `glsl-syntax` does not
//! and must not depend on `wgsl-syntax`: the point of the gate is that the new
//! pipeline covers the old one, and a gate that imported the old one would go
//! green the day the old one broke.
//!
//! The bar is **cover**, not equal. The new outline may find more — and does:
//! a struct declared inside a body is a struct here and was a mislabelled
//! local there.
//!
//! The spans are byte offsets into files the old walk can no longer be run
//! against, so **the three example shaders must not move**: an edit that
//! changes a byte count in `test.vert`, `test.frag` or `test.comp` invalidates
//! the transcription rather than merely failing it. P6-01 rewrote a comment in
//! `test.frag` to the same length for exactly that reason; the dialect
//! examples added by that task are new files, which cannot disturb these.

use pretty_assertions::assert_eq;

use crate::outline::{Outline, SymbolKind, outline};
use crate::parse_source;

/// Run the whole pipeline over one of the extension's example shaders.
///
/// The examples are committed in this repository, so unlike the glslang corpus
/// they are a hard requirement rather than a skip.
fn example(name: &str) -> (String, Outline) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../extensions/wgsl-shader/examples")
        .join(name);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is missing: {e}", path.display()));
    let (pp, tree) = parse_source(&source);
    let outline = outline(&tree, &pp, &source);
    (source, outline)
}

fn parse(source: &str) -> Outline {
    let (pp, tree) = parse_source(source);
    outline(&tree, &pp, source)
}

/// Every `(kind, name)` the outline found, in source order.
fn kinds_and_names(outline: &Outline) -> Vec<(SymbolKind, &str)> {
    outline.symbols.iter().map(|s| (s.kind, s.name.as_str())).collect()
}

/// Assert the outline holds every symbol the old walk found, at the same span.
fn assert_covers(source: &str, outline: &Outline, expected: &[(SymbolKind, &str, u32, u32)]) {
    for (kind, name, start, end) in expected {
        let found = outline.symbols.iter().find(|s| {
            s.name == *name && s.name_span.start == *start && s.name_span.end == *end
        });
        let Some(found) = found else {
            panic!(
                "the outline is missing {name:?} at {start}..{end} (source: {:?})",
                &source[*start as usize..*end as usize]
            );
        };
        assert_eq!(found.kind, *kind, "wrong kind for {name:?}");
    }
}

// -- parity with the heuristic walk on the extension's examples -------------

#[test]
fn outline_parity_on_the_vertex_example() {
    let (source, outline) = example("test.vert");
    assert_covers(
        &source,
        &outline,
        &[
            (SymbolKind::Variable, "in_position", 43, 54),
            (SymbolKind::Variable, "in_normal", 85, 94),
            (SymbolKind::Variable, "in_uv", 125, 130),
            (SymbolKind::Block, "Camera", 161, 167),
            (SymbolKind::Field, "view", 179, 183),
            (SymbolKind::Field, "projection", 194, 204),
            (SymbolKind::Variable, "camera", 208, 214),
            (SymbolKind::Variable, "v_normal", 247, 255),
            (SymbolKind::Variable, "v_uv", 287, 291),
            (SymbolKind::EntryPoint, "main", 299, 303),
        ],
    );
}

#[test]
fn outline_parity_on_the_fragment_example() {
    let (source, outline) = example("test.frag");
    assert_covers(
        &source,
        &outline,
        &[
            (SymbolKind::Variable, "v_normal", 43, 51),
            (SymbolKind::Variable, "v_uv", 82, 86),
            (SymbolKind::Variable, "albedo", 289, 295),
            (SymbolKind::Variable, "albedo_sampler", 342, 356),
            (SymbolKind::Variable, "out_color", 389, 398),
            (SymbolKind::Constant, "LIGHT_DIRECTION", 412, 427),
            (SymbolKind::Function, "lambert", 458, 465),
            (SymbolKind::Parameter, "normal", 471, 477),
            (SymbolKind::Parameter, "light", 484, 489),
            (SymbolKind::EntryPoint, "main", 564, 568),
            (SymbolKind::Local, "base", 582, 586),
            (SymbolKind::Local, "diffuse", 649, 656),
        ],
    );
}

#[test]
fn outline_parity_on_the_compute_example() {
    let (source, outline) = example("test.comp");
    assert_covers(
        &source,
        &outline,
        &[
            (SymbolKind::Block, "Values", 116, 122),
            // No instance name, so the block's members are the globals.
            (SymbolKind::Variable, "values", 135, 141),
            (SymbolKind::Block, "Params", 177, 183),
            (SymbolKind::Field, "scale", 196, 201),
            (SymbolKind::Field, "count", 212, 217),
            (SymbolKind::Variable, "params", 221, 227),
            (SymbolKind::EntryPoint, "main", 235, 239),
            (SymbolKind::Local, "index", 253, 258),
        ],
    );
}

#[test]
fn the_examples_details_read_the_way_the_old_walk_wrote_them() {
    // The detail string is what an outline row and a hover show, so it is part
    // of the parity, not decoration.
    let (_, vert) = example("test.vert");
    let detail = |name: &str| {
        vert.symbols
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.detail.clone())
            .unwrap_or_default()
    };
    assert_eq!(detail("in_position"), "layout(location = 0) in vec3 in_position");
    assert_eq!(detail("Camera"), "layout(binding = 0) uniform Camera");
    assert_eq!(detail("view"), "mat4 view");
    assert_eq!(detail("camera"), "Camera camera");
    assert_eq!(detail("main"), "void main()");

    let (_, frag) = example("test.frag");
    let detail = |name: &str| {
        frag.symbols
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.detail.clone())
            .unwrap_or_default()
    };
    assert_eq!(detail("lambert"), "float lambert(vec3 normal, vec3 light)");
    assert_eq!(detail("normal"), "vec3 normal");
    assert_eq!(detail("LIGHT_DIRECTION"), "const vec3 LIGHT_DIRECTION");
    assert_eq!(detail("base"), "vec4 base");

    let (_, comp) = example("test.comp");
    let values = comp.symbols.iter().find(|s| s.name == "values").unwrap();
    assert_eq!(values.detail, "float values[]");
}

#[test]
fn the_examples_scopes_match_the_old_walk() {
    let (source, frag) = example("test.frag");
    let file = source.len() as u32;
    let symbol = |name: &str| frag.symbols.iter().find(|s| s.name == name).unwrap();

    // File-scope names are visible over the whole file.
    assert_eq!(symbol("lambert").scope.start, 0);
    assert_eq!(symbol("lambert").scope.end, file);
    // A parameter is visible over its function.
    assert_eq!(symbol("normal").scope, symbol("lambert").full_span);
    // A local is visible from where it is declared to the end of its block.
    let main = symbol("main");
    let base = symbol("base");
    assert_eq!(base.scope.start, 577, "from the start of its declaration");
    assert_eq!(base.scope.end, main.full_span.end, "to the closing brace");
}

// -- the outline in its own right -------------------------------------------

#[test]
fn a_struct_declares_itself_its_fields_and_its_instance() {
    let source = "struct S { vec3 a; float b; } s;\n";
    let outline = parse(source);
    assert_eq!(
        kinds_and_names(&outline),
        [
            (SymbolKind::Struct, "S"),
            (SymbolKind::Field, "a"),
            (SymbolKind::Field, "b"),
            (SymbolKind::Variable, "s"),
        ]
    );
    let s = &outline.symbols[0];
    assert_eq!(s.detail, "struct S");
    assert_eq!(outline.symbols[3].detail, "S s");
    assert_eq!(outline.symbols[1].parent, Some(0));
}

#[test]
fn a_macro_is_a_symbol_in_force_from_its_definition_onwards() {
    let source = "float a;\n#define PI 3.14159\nfloat b = PI;\n";
    let outline = parse(source);
    let macro_symbol = outline.symbols.iter().find(|s| s.kind == SymbolKind::Macro).unwrap();
    assert_eq!(macro_symbol.name, "PI");
    assert_eq!(macro_symbol.detail, "#define PI 3.14159");
    assert_eq!(macro_symbol.scope.start, source.find("#define").unwrap() as u32);
    assert_eq!(macro_symbol.scope.end, source.len() as u32);
    // And it is not in force above its own `#define`.
    assert!(outline.resolve("PI", 0).is_none());
}

#[test]
fn a_predefined_macro_is_not_a_symbol() {
    let outline = parse("#version 300 es\nfloat a;\n");
    assert!(!outline.symbols.iter().any(|s| s.kind == SymbolKind::Macro));
}

#[test]
fn roots_come_back_in_source_order() {
    let source = "#define A 1\nfloat b;\nvoid f() { }\n";
    let outline = parse(source);
    let names: Vec<&str> =
        outline.roots.iter().map(|&i| outline.symbols[i].name.as_str()).collect();
    assert_eq!(names, ["A", "b", "f"]);
}

#[test]
fn shadowing_resolves_to_the_smallest_scope() {
    let source = "float x;\nvoid f() {\n    float x;\n    x = 1.0;\n}\n";
    let outline = parse(source);
    let use_site = source.rfind("x = 1.0").unwrap() as u32;
    let resolved = outline.resolve("x", use_site).unwrap();
    assert_eq!(outline.symbols[resolved].kind, SymbolKind::Local);
    // At file scope, the global is the only candidate.
    assert_eq!(outline.symbols[outline.resolve("x", 0).unwrap()].kind, SymbolKind::Variable);
}

#[test]
fn a_for_loop_variable_is_visible_over_the_whole_loop() {
    let source = "void f() {\n    for (int i = 0; i < 4; ++i) {\n        g(i);\n    }\n    \
                  h();\n}\n";
    let outline = parse(source);
    let i = outline.symbols.iter().find(|s| s.name == "i").unwrap();
    let inside = source.find("g(i)").unwrap() as u32 + 2;
    let after = source.find("h()").unwrap() as u32;
    assert!(i.scope.contains(inside), "visible in the body");
    assert!(!i.scope.contains(after), "gone after the loop");
    assert_eq!(i.scope.start, source.find("for").unwrap() as u32);
}

#[test]
fn a_reference_is_recorded_for_every_written_identifier() {
    let source = "void f(float a) { a = a + 1.0; }\n";
    let outline = parse(source);
    let names: Vec<&str> = outline
        .references
        .iter()
        .map(|r| &source[r.span.start as usize..r.span.end as usize])
        .collect();
    assert_eq!(names, ["void", "f", "float", "a", "a", "a"]);
    assert!(outline.references[3].is_declaration, "the parameter's own name");
    assert!(!outline.references[4].is_declaration);
}

#[test]
fn a_member_access_is_marked_as_one() {
    let source = "void f() { x = camera.view; }\n";
    let outline = parse(source);
    let view = outline
        .references
        .iter()
        .find(|r| &source[r.span.start as usize..r.span.end as usize] == "view")
        .unwrap();
    assert!(view.is_member);
    let camera = outline
        .references
        .iter()
        .find(|r| &source[r.span.start as usize..r.span.end as usize] == "camera")
        .unwrap();
    assert!(!camera.is_member);
}

#[test]
fn a_macro_invocation_is_one_reference_at_its_name() {
    // The tokens a macro body produced spell names that exist nowhere in the
    // source, so they contribute nothing; the invocation they all point at is
    // one reference to the macro, and the `#define` is its declaration.
    let source = "#define BODY colour = vec4(1.0)\nvoid main() { BODY; }\n";
    let outline = parse(source);
    let names: Vec<&str> = outline
        .references
        .iter()
        .map(|r| &source[r.span.start as usize..r.span.end as usize])
        .collect();
    assert_eq!(names, ["BODY", "void", "main", "BODY"]);
    assert!(outline.references[0].is_declaration, "the '#define BODY'");
    assert!(!outline.references[3].is_declaration, "the use");
}

#[test]
fn a_function_like_invocation_refers_to_its_name_not_its_arguments() {
    let source = "#define SQUARE(v) ((v) * (v))\nvoid f() { x = SQUARE(y); }\n";
    let outline = parse(source);
    let named = |name: &str| {
        outline
            .references
            .iter()
            .filter(|r| &source[r.span.start as usize..r.span.end as usize] == name)
            .count()
    };
    // Once for the `#define`, once for the call — not once per body token.
    assert_eq!(named("SQUARE"), 2);
    // And the argument keeps the span it was written at, exactly once.
    assert_eq!(named("y"), 1);
}

#[test]
fn a_half_typed_file_still_produces_an_outline() {
    let source = "uniform float scale;\nvoid main() {\n    float x = sca\n";
    let outline = parse(source);
    let names: Vec<&str> = outline.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["scale", "main", "x"]);
}

#[test]
fn an_empty_source_has_an_empty_outline() {
    let outline = parse("");
    assert!(outline.symbols.is_empty());
    assert!(outline.roots.is_empty());
    assert!(outline.references.is_empty());
}
