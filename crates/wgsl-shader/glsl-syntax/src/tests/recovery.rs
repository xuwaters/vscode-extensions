//! P3-05 — recovery: the "keeps working while the source does not parse"
//! contract.
//!
//! The headline is the prefix property: **every prefix of a valid file must
//! parse without panicking, and must still hold every token it was given.**
//! That is not a synthetic worry — a prefix is exactly what a file looks like
//! while it is being typed, and the editor asks for an outline at every one of
//! them.

use pretty_assertions::assert_eq;

use crate::cst::{Child, NodeId, NodeKind, SyntaxTree};

use super::{parse_errors, parsed};

/// A cross-section of the language, used as the seed for the prefix property.
const VALID: &[&str] = &[
    "#version 450 core\nlayout(location = 0) in vec3 pos;\nvoid main() { gl_Position = \
     vec4(pos, 1.0); }\n",
    "#version 310 es\nprecision highp float;\nuniform Camera { mat4 view; } camera;\n\
     out vec4 colour;\nvoid main() { colour = camera.view[0]; }\n",
    "struct S { vec3 a; float b[4]; };\nS make(float x) { S s; s.b[0] = x; return s; }\n",
    "#define SCALE(v) ((v) * 2.0)\nfloat f(float x) { return SCALE(x); }\n",
    "void f() { for (int i = 0; i < 4; ++i) { if (i > 1) continue; else break; } }\n",
    "void f() { switch (k) { case 1: case 2: discard; default: return; } }\n",
    "layout(std430, binding = 0) buffer B { float values[]; };\nvoid main() { \
     values[0] = 1.0; }\n",
];

fn collect_leaves(tree: &SyntaxTree, id: NodeId, out: &mut Vec<u32>) {
    for child in tree.children(id) {
        match *child {
            Child::Token(token) => out.push(token.0),
            Child::Node(node) => collect_leaves(tree, node, out),
        }
    }
}

/// The invariants that hold no matter how broken the input is.
fn assert_survives(source: &str) {
    let (pp, tree) = parsed(source);
    let mut leaves = Vec::new();
    collect_leaves(&tree, NodeId::ROOT, &mut leaves);
    let expected: Vec<u32> = (0..pp.tokens.len() as u32).collect();
    assert_eq!(leaves, expected, "tokens lost parsing {source:?}");
    assert_eq!(tree.reconstruct(&pp, source), source, "no round-trip for {source:?}");
    for (_, node) in tree.nodes() {
        assert!(node.span.end as usize <= source.len(), "span past the end of {source:?}");
    }
}

#[test]
fn every_prefix_of_a_valid_file_parses_without_panicking() {
    for source in VALID {
        for end in 0..=source.len() {
            if !source.is_char_boundary(end) {
                continue;
            }
            assert_survives(&source[..end]);
        }
    }
}

#[test]
fn every_suffix_of_a_valid_file_parses_too() {
    // The other half of the same worry: a file whose head is being deleted, or
    // one the user opened scrolled into the middle of a paste.
    for source in VALID {
        for start in 0..=source.len() {
            if !source.is_char_boundary(start) {
                continue;
            }
            assert_survives(&source[start..]);
        }
    }
}

#[test]
fn a_missing_semicolon_does_not_cost_the_declarations_around_it() {
    let source = "float a;\nfloat b\nfloat c;\n";
    let (_, tree) = parsed(source);
    let names: Vec<&str> = tree
        .nodes()
        .filter(|(_, n)| n.kind == NodeKind::Name)
        .map(|(id, _)| tree.text(source, id))
        .collect();
    assert_eq!(names, ["a", "b", "c"]);
    assert!(!parse_errors(&tree).is_empty(), "the missing ';' should be reported");
}

#[test]
fn a_half_typed_member_line_keeps_its_block() {
    let source = "struct S {\n    vec3 nor\n    float b;\n};\n";
    let (_, tree) = parsed(source);
    let fields = tree
        .nodes()
        .find(|(_, n)| n.kind == NodeKind::FieldList)
        .map(|(id, _)| id)
        .unwrap();
    let names: Vec<&str> = tree
        .descendants(fields)
        .filter(|n| tree.kind(*n) == NodeKind::Name)
        .map(|n| tree.text(source, n))
        .collect();
    assert_eq!(names, ["nor", "b"]);
}

#[test]
fn an_unclosed_brace_at_end_of_file_is_a_warning_not_a_loss() {
    let source = "void main() {\n    float x = 1.0;\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new(), "a file being typed has no errors");
    assert_eq!(
        super::parse_codes(&tree),
        ["GLSL0105"],
        "one unclosed-delimiter warning, naming the '{{'"
    );
    let names: Vec<&str> = tree
        .nodes()
        .filter(|(_, n)| n.kind == NodeKind::Name)
        .map(|(id, _)| tree.text(source, id))
        .collect();
    assert_eq!(names, ["main", "x"]);
}

#[test]
fn an_unclosed_paren_at_end_of_file_is_tolerated() {
    for source in ["void main(", "void main() { f(", "float a[", "void f() { g(a,"] {
        let (_, tree) = parsed(source);
        assert_eq!(parse_errors(&tree), Vec::<&str>::new(), "{source:?}");
        assert_survives(source);
    }
}

#[test]
fn garbage_between_declarations_is_confined_to_an_error_node() {
    let source = "float a;\n@ $ %\nfloat b;\n";
    let (_, tree) = parsed(source);
    let names: Vec<&str> = tree
        .nodes()
        .filter(|(_, n)| n.kind == NodeKind::Name)
        .map(|(id, _)| tree.text(source, id))
        .collect();
    assert_eq!(names, ["a", "b"]);
    assert!(tree.nodes().any(|(_, n)| n.kind == NodeKind::Error));
    assert_survives(source);
}

#[test]
fn a_statement_the_parser_cannot_read_ends_at_the_next_semicolon() {
    let source = "void f() {\n    ] ] ];\n    float ok = 1.0;\n}\n";
    let (_, tree) = parsed(source);
    let names: Vec<&str> = tree
        .nodes()
        .filter(|(_, n)| n.kind == NodeKind::Name)
        .map(|(id, _)| tree.text(source, id))
        .collect();
    assert_eq!(names, ["f", "ok"]);
}

#[test]
fn deep_nesting_stops_at_the_limit_instead_of_the_stack() {
    // Well past `MAX_DEPTH`, and nothing here may recurse that far.
    let depth = 2_000;
    let source = format!("void f() {{ int x = {}1{}; }}\n", "(".repeat(depth), ")".repeat(depth));
    let (_, tree) = parsed(&source);
    assert!(
        tree.diagnostics.iter().any(|d| d.code == crate::diagnostics::ParseCode::NestingLimit),
        "the nesting limit should be reported"
    );
    assert_survives(&source);
}

#[test]
fn deeply_nested_blocks_stop_at_the_limit_too() {
    let depth = 2_000;
    let source = format!("void f() {}{}\n", "{".repeat(depth), "}".repeat(depth));
    assert_survives(&source);
}

#[test]
fn a_source_of_nothing_but_punctuation_still_answers() {
    for source in [";;;;", "}}}}", "))))", "[[[[", "((((", "....", ",,,,", "= = =", "#"] {
        assert_survives(source);
    }
}

#[test]
fn every_parser_diagnostic_has_a_source_that_produces_it() {
    // A code nothing can emit is a code nobody can act on. One seed each; the
    // assertion is that the code appears, not that it is the only one.
    let seeds: &[(&str, &str)] = &[
        ("GLSL0100", "void f() { if a) { } }\n"),
        ("GLSL0101", "float 1;\n"),
        ("GLSL0102", "precision highp;\n"),
        ("GLSL0103", "void f() { x = ; }\n"),
        ("GLSL0104", "void f() { g(,); }\n"),
        ("GLSL0105", "void f() {\n"),
        ("GLSL0106", "layout(0) in vec4 a;\n"),
        ("GLSL0107", "float a[2 3];\n"),
        ("GLSL0108", "@ $ %\n"),
        ("GLSL0109", "float a\n"),
        ("GLSL0110", "void f() { int x = ((((((((((((((((((((((((((((((((((((((((((((((((\
                      ((((((((((((((((((((1)))))))))))))))))))))))))))))))))))))))))))))\
                      )))))))))))))))))))))); }\n"),
    ];
    for (code, source) in seeds {
        let (_, tree) = parsed(source);
        assert!(
            super::parse_codes(&tree).contains(code),
            "{source:?} produced {:?}, not {code}",
            super::parse_codes(&tree)
        );
        assert_survives(source);
    }
}

#[test]
fn a_broken_file_never_reports_a_span_outside_itself() {
    let source = "struct { void f( float ; } ] main(){{{\n";
    let (_, tree) = parsed(source);
    for diagnostic in &tree.diagnostics {
        assert!(
            diagnostic.span.end as usize <= source.len(),
            "{diagnostic:?} points past the end"
        );
    }
    assert_survives(source);
}
