//! Parser tests.
//!
//! Fixtures mark the cursor with `|`, which [`at`] strips before parsing —
//! it reads far better in a test than a byte offset does, and it survives
//! edits to the fixture above it.

use analyzer_core::spans::ByteSpan;

use crate::lexer::{TokenKind, tokenize};
use crate::tree::SymbolKind;
use crate::{Language, Parsed, parse};

/// Parse a fixture whose cursor is marked with `|`, returning the parse and
/// the offset the marker stood at.
fn at(source: &str, language: Language) -> (Parsed, String, u32) {
    let offset = source.find('|').expect("fixture has no `|` cursor marker") as u32;
    let text = source.replacen('|', "", 1);
    let parsed = parse(&text, language);
    (parsed, text, offset)
}

fn names(parsed: &Parsed, kind: SymbolKind) -> Vec<&str> {
    parsed
        .symbols
        .iter()
        .filter(|s| s.kind == kind)
        .map(|s| s.name.as_str())
        .collect()
}

fn find<'a>(parsed: &'a Parsed, name: &str) -> &'a crate::Symbol {
    parsed
        .symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no symbol named {name}; have {:?}", names_of(parsed)))
}

fn names_of(parsed: &Parsed) -> Vec<&str> {
    parsed.symbols.iter().map(|s| s.name.as_str()).collect()
}

const WGSL: &str = r#"
struct Camera {
    view: mat4x4f,
    proj: mat4x4f,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var albedo: texture_2d<f32>;

const AMBIENT: f32 = 0.1;

alias Colour = vec4f;

fn attenuate(distance: f32, falloff: f32) -> f32 {
    let squared = distance * distance;
    return 1.0 / (squared * falloff);
}

@fragment
fn fs_main(@location(0) uv: vec2f) -> @location(0) Colour {
    var total = AMBIENT;
    for (var i = 0u; i < 4u; i = i + 1u) {
        total = total + attenuate(f32(i), 2.0);
    }
    return vec4f(total, 0.0, 0.0, 1.0);
}
"#;

const GLSL: &str = r#"
#version 450
#define MAX_LIGHTS 4

layout(std140, binding = 0) uniform Camera {
    mat4 view;
    mat4 proj;
} camera;

layout(location = 0) in vec3 position;
layout(location = 0) out vec4 fragColour;

struct Light {
    vec3 colour;
    float intensity;
};

uniform Light lights[MAX_LIGHTS];

const float AMBIENT = 0.1;

float attenuate(float distance, float falloff) {
    float squared = distance * distance;
    return 1.0 / (squared * falloff);
}

void main() {
    float total = AMBIENT;
    for (int i = 0; i < MAX_LIGHTS; i++) {
        total += attenuate(float(i), lights[i].intensity);
    }
    fragColour = vec4(total * position, 1.0);
}
"#;

// ── Lexing ─────────────────────────────────────────────────────────────────

#[test]
fn block_comments_nest_in_wgsl_but_not_in_glsl() {
    let source = "/* outer /* inner */ still outer */ x";
    let wgsl = tokenize(source, Language::Wgsl);
    assert_eq!(wgsl[0].kind, TokenKind::Comment);
    assert_eq!(wgsl[0].span.end as usize, source.len() - 2);
    assert_eq!(wgsl.len(), 2);

    // GLSL closes at the first `*/`, leaving `still outer */ x` as code.
    let glsl = tokenize(source, Language::Glsl);
    assert_eq!(glsl[0].kind, TokenKind::Comment);
    assert!(glsl.len() > 2);
}

#[test]
fn an_unterminated_block_comment_runs_to_the_end_rather_than_failing() {
    let tokens = tokenize("fn f() {} /* and then", Language::Wgsl);
    let last = tokens.last().unwrap();
    assert_eq!(last.kind, TokenKind::Comment);
    assert_eq!(last.span.end, 21);
}

#[test]
fn glsl_directives_lex_as_a_directive_plus_ordinary_tokens() {
    let tokens = tokenize("#version 450 core\n#  define PI 3.14\n", Language::Glsl);
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        [
            TokenKind::Preprocessor,
            TokenKind::Number,
            TokenKind::Ident,
            // `#  define` — whitespace after the hash is legal.
            TokenKind::Preprocessor,
            TokenKind::Ident,
            TokenKind::Number,
        ]
    );
    assert_eq!(tokens[3].line, 1);
}

/// A `#` that is not at the start of a line is an operator, not a directive.
#[test]
fn a_hash_mid_line_is_not_a_directive() {
    let tokens = tokenize("int a = b # c;", Language::Glsl);
    assert!(tokens.iter().all(|t| t.kind != TokenKind::Preprocessor));
}

#[test]
fn wgsl_attributes_lex_as_one_token_including_the_at() {
    let tokens = tokenize("@workgroup_size(64) fn main() {}", Language::Wgsl);
    assert_eq!(tokens[0].kind, TokenKind::Attribute);
    assert_eq!(tokens[0].span, ByteSpan::new(0, 15));
}

#[test]
fn numbers_take_their_suffixes_and_leave_swizzles_alone() {
    let tokens = tokenize("1.0f 0x1p3 2u .5 v.xy", Language::Wgsl);
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        [
            TokenKind::Number,
            TokenKind::Number,
            TokenKind::Number,
            TokenKind::Number,
            TokenKind::Ident,
            TokenKind::Punct,
            TokenKind::Ident,
        ]
    );
}

#[test]
fn multibyte_text_does_not_split_a_scalar() {
    // The lexer indexes by byte; an em-dash in a comment must not panic.
    let source = "// naïve — try it\nfn f() {}";
    let tokens = tokenize(source, Language::Wgsl);
    assert_eq!(tokens[0].kind, TokenKind::Comment);
    for token in &tokens {
        assert!(source.is_char_boundary(token.span.start as usize));
        assert!(source.is_char_boundary(token.span.end as usize));
    }
}

/// The lexer must terminate on any byte sequence, valid or not.
#[test]
fn lexing_terminates_on_junk() {
    for source in ["", "\\", "\u{0}\u{1}", "///", "/*", "@", "#", "\"", "0x", "..."] {
        for language in [Language::Wgsl, Language::Glsl] {
            let tokens = tokenize(source, language);
            // Spans must tile the input in order, with no overlaps.
            let mut previous = 0;
            for token in &tokens {
                assert!(token.span.start >= previous, "{source:?} {language:?}");
                assert!(token.span.end > token.span.start, "{source:?} {language:?}");
                previous = token.span.end;
            }
        }
    }
}

// ── WGSL declarations ──────────────────────────────────────────────────────

#[test]
fn wgsl_module_scope_declarations_are_found() {
    let parsed = parse(WGSL, Language::Wgsl);
    assert_eq!(names(&parsed, SymbolKind::Struct), ["Camera"]);
    assert_eq!(names(&parsed, SymbolKind::Variable), ["camera", "albedo"]);
    assert_eq!(names(&parsed, SymbolKind::Constant), ["AMBIENT"]);
    assert_eq!(names(&parsed, SymbolKind::TypeAlias), ["Colour"]);
    assert_eq!(names(&parsed, SymbolKind::Function), ["attenuate"]);
    // `@fragment` promotes the function to an entry point.
    assert_eq!(names(&parsed, SymbolKind::EntryPoint), ["fs_main"]);
}

#[test]
fn wgsl_struct_members_are_children_of_the_struct() {
    let parsed = parse(WGSL, Language::Wgsl);
    let camera = find(&parsed, "Camera");
    let fields: Vec<&str> =
        camera.children.iter().map(|&i| parsed.symbols[i].name.as_str()).collect();
    assert_eq!(fields, ["view", "proj"]);
    assert_eq!(parsed.symbols[camera.children[0]].detail, "view: mat4x4f");
}

#[test]
fn wgsl_details_stop_before_the_body_and_the_initialiser() {
    let parsed = parse(WGSL, Language::Wgsl);
    assert_eq!(
        find(&parsed, "attenuate").detail,
        "fn attenuate(distance: f32, falloff: f32) -> f32"
    );
    assert_eq!(find(&parsed, "camera").detail, "@group(0) @binding(0) var<uniform> camera: Camera");
    assert_eq!(find(&parsed, "AMBIENT").detail, "const AMBIENT: f32");
    // The template arguments of `texture_2d<f32>` must not end the declaration.
    assert_eq!(find(&parsed, "albedo").detail, "@group(0) @binding(1) var albedo: texture_2d<f32>");
}

#[test]
fn wgsl_parameters_and_locals_belong_to_their_function() {
    let parsed = parse(WGSL, Language::Wgsl);
    let attenuate = find(&parsed, "attenuate");
    let children: Vec<&str> =
        attenuate.children.iter().map(|&i| parsed.symbols[i].name.as_str()).collect();
    assert_eq!(children, ["distance", "falloff", "squared"]);
    assert_eq!(parsed.symbols[attenuate.children[2]].kind, SymbolKind::Local);
}

// ── GLSL declarations ──────────────────────────────────────────────────────

#[test]
fn glsl_module_scope_declarations_are_found() {
    let parsed = parse(GLSL, Language::Glsl);
    assert_eq!(names(&parsed, SymbolKind::Macro), ["MAX_LIGHTS"]);
    assert_eq!(names(&parsed, SymbolKind::Block), ["Camera"]);
    assert_eq!(names(&parsed, SymbolKind::Struct), ["Light"]);
    assert_eq!(
        names(&parsed, SymbolKind::Variable),
        ["camera", "position", "fragColour", "lights"]
    );
    assert_eq!(names(&parsed, SymbolKind::Constant), ["AMBIENT"]);
    assert_eq!(names(&parsed, SymbolKind::Function), ["attenuate"]);
    assert_eq!(names(&parsed, SymbolKind::EntryPoint), ["main"]);
}

#[test]
fn glsl_details_carry_the_qualifiers_and_the_type() {
    let parsed = parse(GLSL, Language::Glsl);
    assert_eq!(find(&parsed, "position").detail, "layout(location = 0) in vec3 position");
    assert_eq!(find(&parsed, "AMBIENT").detail, "const float AMBIENT");
    assert_eq!(find(&parsed, "lights").detail, "uniform Light lights[MAX_LIGHTS]");
    assert_eq!(
        find(&parsed, "attenuate").detail,
        "float attenuate(float distance, float falloff)"
    );
}

/// An interface block with an instance name puts its members behind that name;
/// one without puts them straight into global scope. The parse has to say which.
#[test]
fn a_named_interface_block_makes_its_members_fields() {
    let parsed = parse(GLSL, Language::Glsl);
    let block = find(&parsed, "Camera");
    assert_eq!(block.kind, SymbolKind::Block);
    let fields: Vec<SymbolKind> =
        block.children.iter().map(|&i| parsed.symbols[i].kind).collect();
    assert_eq!(fields, [SymbolKind::Field, SymbolKind::Field]);
    // …and the instance is a variable in its own right.
    assert_eq!(find(&parsed, "camera").kind, SymbolKind::Variable);
}

#[test]
fn an_anonymous_interface_block_puts_its_members_in_global_scope() {
    let source = "#version 450\nlayout(binding = 0) uniform Matrices {\n  mat4 view;\n};\n";
    let parsed = parse(source, Language::Glsl);
    let view = find(&parsed, "view");
    assert_eq!(view.kind, SymbolKind::Variable);
    // Resolvable by bare name from anywhere in the file, which is the GLSL rule.
    assert!(parsed.resolve_name("view", source.len() as u32 - 1).is_some());
}

#[test]
fn a_glsl_declarator_list_declares_every_name() {
    let source = "float a, b = 1.0, c[4];\n";
    let parsed = parse(source, Language::Glsl);
    assert_eq!(names_of(&parsed), ["a", "b", "c"]);
    assert_eq!(find(&parsed, "c").detail, "float c[4]");
    assert_eq!(find(&parsed, "b").detail, "float b");
}

// ── Scopes and resolution ──────────────────────────────────────────────────

#[test]
fn a_local_resolves_to_its_own_declaration_not_a_global_of_the_same_name() {
    let source = "\
var total: f32 = 0.0;
fn f() {
    let total = 1.0;
    let x = to|tal;
}
";
    let (parsed, text, offset) = at(source, Language::Wgsl);
    let resolved = parsed.resolve_at(&text, offset).expect("resolves");
    assert_eq!(parsed.symbols[resolved].kind, SymbolKind::Local);
}

#[test]
fn a_global_resolves_from_inside_a_function() {
    let source = "\
var total: f32 = 0.0;
fn f() { let x = to|tal; }
";
    let (parsed, text, offset) = at(source, Language::Wgsl);
    let resolved = parsed.resolve_at(&text, offset).expect("resolves");
    assert_eq!(parsed.symbols[resolved].kind, SymbolKind::Variable);
}

/// A local declared in a nested block must not be visible after it closes.
#[test]
fn block_scope_ends_at_the_closing_brace() {
    let source = "\
fn f() {
    if (true) {
        let inner = 1.0;
    }
    let after = 2|.0;
}
";
    let (parsed, _, offset) = at(source, Language::Wgsl);
    assert!(parsed.resolve_name("inner", offset).is_none());
    assert!(parsed.resolve_name("after", offset).is_some());
}

/// The loop variable outlives the header but not the statement.
#[test]
fn a_for_loop_variable_is_scoped_to_the_loop() {
    let source = "\
fn f() {
    for (var i = 0u; i < 4u; i = i + 1u) {
        let inside = i;
    }
    let outside = 1|.0;
}
";
    let (parsed, _, offset) = at(source, Language::Wgsl);
    let i = find(&parsed, "i");
    assert!(i.scope.contains(source.find("let inside").unwrap() as u32));
    assert!(parsed.resolve_name("i", offset).is_none());
}

#[test]
fn glsl_for_loop_variables_are_scoped_the_same_way() {
    let source = "\
void main() {
    for (int i = 0; i < 4; i++) {
        float x = float(i);
    }
    int after = 1|;
}
";
    let (parsed, _, offset) = at(source, Language::Glsl);
    assert!(parsed.resolve_name("i", offset).is_none());
    assert!(find(&parsed, "i").scope.contains(source.find("float x").unwrap() as u32));
}

#[test]
fn a_parameter_is_visible_throughout_its_function_and_no_further() {
    let source = "fn f(a: f32) -> f32 { return a; }\nfn g() -> f32 { return 0|.0; }\n";
    let (parsed, _, offset) = at(source, Language::Wgsl);
    assert!(parsed.resolve_name("a", offset).is_none());
    assert!(parsed.resolve_name("a", source.find("return a").unwrap() as u32 + 7).is_some());
}

#[test]
fn a_member_access_does_not_resolve_in_scope() {
    let source = "\
struct S { view: f32 }
var view: f32 = 0.0;
fn f(s: S) -> f32 { return s.vi|ew; }
";
    let (parsed, text, offset) = at(source, Language::Wgsl);
    let reference = parsed.reference_at(offset).expect("on an identifier");
    assert!(reference.is_member);
    // The global `view` must not be offered as the definition of `s.view`.
    assert!(parsed.resolve_at(&text, offset).is_none());
}

#[test]
fn a_glsl_macro_is_in_force_from_its_definition_onwards() {
    let source = "#define PI 3.14\nfloat a = P|I;\n";
    let (parsed, text, offset) = at(source, Language::Glsl);
    let resolved = parsed.resolve_at(&text, offset).expect("resolves");
    assert_eq!(parsed.symbols[resolved].kind, SymbolKind::Macro);
    assert_eq!(parsed.symbols[resolved].detail, "#define PI 3.14");
}

#[test]
fn occurrences_finds_every_use_including_the_declaration() {
    let source = "fn f(a: f32) -> f32 { return a + a; }\n";
    let parsed = parse(source, Language::Wgsl);
    let occurrences: Vec<_> = parsed.occurrences(source, "a").collect();
    assert_eq!(occurrences.len(), 3);
    assert_eq!(occurrences.iter().filter(|r| r.is_declaration).count(), 1);
}

// ── Blocks and folding ─────────────────────────────────────────────────────

#[test]
fn brace_paren_and_bracket_blocks_are_all_recorded() {
    let parsed = parse("fn f(a: array<f32, 4>) { let b = a[0]; }", Language::Wgsl);
    let kinds: Vec<_> = parsed.blocks.iter().map(|b| b.kind).collect();
    assert!(kinds.contains(&crate::BlockKind::Brace));
    assert!(kinds.contains(&crate::BlockKind::Paren));
    assert!(kinds.contains(&crate::BlockKind::Bracket));
}

#[test]
fn runs_of_line_comments_fold_together_but_a_lone_one_does_not() {
    let source = "// one\n// two\n// three\nfn f() {}\n// alone\n";
    let parsed = parse(source, Language::Wgsl);
    let comments: Vec<_> = parsed
        .blocks
        .iter()
        .filter(|b| b.kind == crate::BlockKind::Comment)
        .collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].span, ByteSpan::new(0, 22));
}

#[test]
fn a_multi_line_block_comment_folds() {
    let parsed = parse("/* one\n   two */\nfn f() {}", Language::Wgsl);
    assert_eq!(
        parsed.blocks.iter().filter(|b| b.kind == crate::BlockKind::Comment).count(),
        1
    );
    // A single-line one has nothing to fold.
    let parsed = parse("/* one */\nfn f() {}", Language::Wgsl);
    assert_eq!(
        parsed.blocks.iter().filter(|b| b.kind == crate::BlockKind::Comment).count(),
        0
    );
}

// ── Resilience ─────────────────────────────────────────────────────────────

#[test]
fn an_unclosed_brace_is_reported_and_the_outline_survives() {
    let parsed = parse("fn a() {}\nfn b() {\n  let x = 1;\n", Language::Wgsl);
    assert_eq!(names(&parsed, SymbolKind::Function), ["a", "b"]);
    assert!(parsed.diagnostics.iter().any(|d| d.message.contains("unclosed `{`")));
}

#[test]
fn an_unmatched_close_is_reported_without_derailing_the_rest() {
    let parsed = parse("fn a() { }\n}\nfn b() { }\n", Language::Wgsl);
    assert_eq!(names(&parsed, SymbolKind::Function), ["a", "b"]);
    assert!(parsed.diagnostics.iter().any(|d| d.message.contains("unmatched `}`")));
}

/// The single most common state a file is in: a declaration being typed. The
/// declarations above and below it must keep their symbols.
#[test]
fn a_half_typed_declaration_does_not_lose_its_neighbours() {
    for partial in [
        "fn ",
        "fn half",
        "fn half(",
        "fn half(x",
        "fn half(x: f3",
        "fn half(x: f32) ->",
        "fn half(x: f32) -> f32 {",
        "var<uni",
        "struct ",
        "struct Half {",
        "@grou",
        "@group(0) @binding(0) var<uniform> ",
    ] {
        let source = format!("fn before() {{}}\n{partial}\nfn after() {{}}\n");
        let parsed = parse(&source, Language::Wgsl);
        assert!(
            parsed.symbols.iter().any(|s| s.name == "before"),
            "lost `before` on {partial:?}: {:?}",
            names_of(&parsed)
        );
    }
}

#[test]
fn a_half_typed_glsl_declaration_does_not_lose_its_neighbours() {
    for partial in [
        "vec3",
        "vec3 half",
        "vec3 half(",
        "layout(location = 0)",
        "layout(location = 0) in",
        "layout(location = 0) in vec3",
        "uniform Camera {",
        "struct ",
        "#define",
        "#version",
    ] {
        let source = format!("void before() {{}}\n{partial}\nvoid after() {{}}\n");
        let parsed = parse(&source, Language::Glsl);
        assert!(
            parsed.symbols.iter().any(|s| s.name == "before"),
            "lost `before` on {partial:?}: {:?}",
            names_of(&parsed)
        );
    }
}

/// Parsing must terminate and produce well-formed spans for any input at all.
#[test]
fn parsing_terminates_and_produces_spans_inside_the_source() {
    let sources = [
        WGSL,
        GLSL,
        "",
        "{{{{{{",
        "}}}}}}",
        "((((((",
        "fn fn fn fn",
        "struct struct {",
        "a < b > c < d",
        "var<",
        "#define\n#define\n#",
        "float float float(",
        "/*",
        "\u{0}\u{1}\u{2}",
    ];
    for source in sources {
        for language in [Language::Wgsl, Language::Glsl] {
            let parsed = parse(source, language);
            let len = source.len() as u32;
            for symbol in &parsed.symbols {
                assert!(symbol.name_span.end <= len, "{source:?} {language:?}");
                assert!(symbol.full_span.end <= len, "{source:?} {language:?}");
                assert!(symbol.scope.end <= len, "{source:?} {language:?}");
                assert!(
                    symbol.scope.contains(symbol.name_span.start),
                    "{source:?} {language:?}: {} is not visible at its own declaration",
                    symbol.name
                );
            }
            for block in &parsed.blocks {
                assert!(block.span.end <= len, "{source:?} {language:?}");
            }
        }
    }
}

/// Angle brackets are comparison operators as often as they are template
/// brackets, and mistaking one for the other used to swallow the file.
#[test]
fn comparisons_are_not_mistaken_for_template_arguments() {
    let source = "fn f(a: i32, b: i32) -> bool { return a < b; }\nfn g() {}\n";
    let parsed = parse(source, Language::Wgsl);
    assert_eq!(names(&parsed, SymbolKind::Function), ["f", "g"]);
}

#[test]
fn nested_template_arguments_close_correctly() {
    let source = "var<storage, read> data: array<vec2<f32>>;\nfn f() {}\n";
    let parsed = parse(source, Language::Wgsl);
    assert_eq!(names_of(&parsed), ["data", "f"]);
    assert_eq!(find(&parsed, "data").detail, "var<storage, read> data: array<vec2<f32>>");
}

#[test]
fn a_duplicate_declaration_in_one_scope_is_reported() {
    let parsed = parse("var a: f32 = 0.0;\nvar a: f32 = 1.0;\n", Language::Wgsl);
    assert!(parsed.diagnostics.iter().any(|d| d.message.contains("already declared")));
    // GLSL overloading is legal, so repeated function names are not reported.
    let parsed = parse("float f(float x) { return x; }\nfloat f(int x) { return 0.0; }\n", Language::Glsl);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
}

// ── Cursor queries ─────────────────────────────────────────────────────────

#[test]
fn token_before_skips_comments() {
    let source = "a. /* hm */ b";
    let parsed = parse(source, Language::Wgsl);
    let before = parsed.token_before(source.len() as u32 - 1).unwrap();
    assert_eq!(parsed.text(source, before.span), ".");
}

#[test]
fn visible_at_orders_the_innermost_scope_first() {
    let source = "\
var x: f32 = 0.0;
fn f() {
    let y = 1|.0;
}
";
    let (parsed, _, offset) = at(source, Language::Wgsl);
    let visible: Vec<&str> =
        parsed.visible_at(offset).iter().map(|&i| parsed.symbols[i].name.as_str()).collect();
    assert_eq!(visible.first(), Some(&"y"));
    assert!(visible.contains(&"x"));
    assert!(visible.contains(&"f"));
}

#[test]
fn the_enclosing_function_is_the_innermost_one() {
    let source = "fn outer() {\n    let a = 1|.0;\n}\n";
    let (parsed, _, offset) = at(source, Language::Wgsl);
    let function = parsed.enclosing_function(offset).expect("inside a function");
    assert_eq!(parsed.symbols[function].name, "outer");
}
