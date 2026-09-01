//! P3-02 — declarations: qualifiers, `layout`, `precision`, structs, interface
//! blocks, arrays and initialisers.
//!
//! Fixtures assert the tree's *shape*, because that is what the layers above
//! read. A fixture that only checked "no diagnostics" would pass with every
//! declaration parsed as one flat `Error`.

use pretty_assertions::assert_eq;

use crate::cst::NodeKind;

use super::{dump, parse_errors, parsed};

/// Assert a source parses to exactly this dump, with no diagnostics.
fn assert_tree(source: &str, expected: &str) {
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new(), "unexpected diagnostics");
    assert_eq!(dump(source), expected.trim_start_matches('\n'));
}

#[test]
fn a_qualified_global_carries_its_layout_and_its_type() {
    assert_tree(
        "layout(location = 0) in vec3 pos;\n",
        "
SourceFile
  Declaration
    QualifierList
      LayoutQualifier
        · layout
        · (
        LayoutItem
          · location
          · =
          LiteralExpr
            · 0
        · )
      · in
    TypeSpec
      · vec3
    Declarator
      Name
        · pos
    · ;
",
    );
}

#[test]
fn a_layout_takes_bare_names_and_name_equals_value_alike() {
    let source = "layout(std430, binding = 0, local_size_x = 8 * 8) buffer B { int i; };\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let items: Vec<_> = tree
        .nodes()
        .filter(|(_, n)| n.kind == NodeKind::LayoutItem)
        .map(|(id, _)| tree.text(source, id))
        .collect();
    assert_eq!(items, ["std430", "binding = 0", "local_size_x = 8 * 8"]);
}

#[test]
fn every_qualifier_sequence_the_language_allows_parses() {
    // §4.10's order, all at once, plus the legacy and memory qualifiers.
    let source = "\
const int a = 1;
in float b;
centroid out float c;
flat in int d;
noperspective smooth in float e;
attribute vec3 f;
varying vec4 g;
uniform highp mat4 h;
buffer Data { int i; } data;
shared float j[16];
layout(binding = 0) coherent volatile restrict readonly writeonly buffer K { int k; };
invariant precise out vec4 l;
patch in float m;
sample in vec2 n;
";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let roots = tree.child_nodes(tree.root()).count();
    assert_eq!(roots, 14, "one declaration per line");
}

#[test]
fn a_precision_statement_declares_nothing_and_still_parses() {
    assert_tree(
        "precision mediump float;\n",
        "
SourceFile
  PrecisionDecl
    · precision
    · mediump
    TypeSpec
      · float
    · ;
",
    );
}

#[test]
fn qualifiers_with_no_type_are_their_own_declaration() {
    // `invariant gl_Position;` re-qualifies a name that already exists, and
    // `layout(…) in;` qualifies the stage itself. Neither has a type.
    assert_tree(
        "invariant gl_Position;\n",
        "
SourceFile
  QualifierDecl
    QualifierList
      · invariant
    Name
      · gl_Position
    · ;
",
    );
    let (_, tree) = parsed("layout(local_size_x = 64) in;\n");
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    assert_eq!(
        tree.kind(tree.child_nodes(tree.root()).next().unwrap()),
        NodeKind::QualifierDecl
    );
}

#[test]
fn a_struct_is_a_type_specifier_with_a_body() {
    assert_tree(
        "struct Light { vec3 dir; float power; } light;\n",
        "
SourceFile
  Declaration
    TypeSpec
      StructSpec
        · struct
        Name
          · Light
        FieldList
          · {
          FieldDecl
            TypeSpec
              · vec3
            Declarator
              Name
                · dir
            · ;
          FieldDecl
            TypeSpec
              · float
            Declarator
              Name
                · power
            · ;
          · }
    Declarator
      Name
        · light
    · ;
",
    );
}

#[test]
fn a_struct_with_no_instance_and_a_struct_with_no_name_both_parse() {
    let (_, tree) = parsed("struct S { int a; };\nstruct { int b; } anon;\n");
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let structs = tree.nodes().filter(|(_, n)| n.kind == NodeKind::StructSpec).count();
    assert_eq!(structs, 2);
}

#[test]
fn an_interface_block_keeps_its_name_body_and_instance_apart() {
    let source =
        "layout(std140, binding = 0) uniform Camera { mat4 view; mat4 proj; } camera;\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let block = tree.child_nodes(tree.root()).next().unwrap();
    assert_eq!(tree.kind(block), NodeKind::InterfaceBlock);
    let name = tree.child_of_kind(block, NodeKind::Name).unwrap();
    assert_eq!(tree.text(source, name), "Camera");
    let instance = tree.child_of_kind(block, NodeKind::Declarator).unwrap();
    assert_eq!(tree.text(source, instance), "camera");
    let fields = tree.child_of_kind(block, NodeKind::FieldList).unwrap();
    assert_eq!(tree.child_nodes(fields).count(), 2);
}

#[test]
fn a_block_can_have_no_instance_name_at_all() {
    assert_tree(
        "buffer Values { float values[]; };\n",
        "
SourceFile
  InterfaceBlock
    QualifierList
      · buffer
    Name
      · Values
    FieldList
      · {
      FieldDecl
        TypeSpec
          · float
        Declarator
          Name
            · values
          ArraySpec
            · [
            · ]
        · ;
      · }
    · ;
",
    );
}

#[test]
fn a_block_instance_may_be_an_array() {
    let source = "uniform Lights { vec4 colour; } lights[4];\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let block = tree.child_nodes(tree.root()).next().unwrap();
    let instance = tree.child_of_kind(block, NodeKind::Declarator).unwrap();
    assert_eq!(tree.text(source, instance), "lights[4]");
    assert!(tree.child_of_kind(instance, NodeKind::ArraySpec).is_some());
}

#[test]
fn both_array_syntaxes_parse_and_land_in_different_places() {
    // `float[4] a` puts the size on the type; `float a[4]` puts it on the
    // declarator. GLSL means the same thing by both, and the tree says which
    // was written.
    let source = "float[4] a;\nfloat b[4];\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let decls: Vec<_> = tree.child_nodes(tree.root()).collect();

    let on_type = tree.child_of_kind(decls[0], NodeKind::TypeSpec).unwrap();
    assert!(tree.child_of_kind(on_type, NodeKind::ArraySpec).is_some());
    let first = tree.child_of_kind(decls[0], NodeKind::Declarator).unwrap();
    assert!(tree.child_of_kind(first, NodeKind::ArraySpec).is_none());

    let second = tree.child_of_kind(decls[1], NodeKind::Declarator).unwrap();
    assert!(tree.child_of_kind(second, NodeKind::ArraySpec).is_some());
}

#[test]
fn a_multidimensional_array_gets_one_specifier_per_dimension() {
    let source = "uniform mat4 m[2][3];\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let declarator = tree
        .nodes()
        .find(|(_, n)| n.kind == NodeKind::Declarator)
        .map(|(id, _)| id)
        .unwrap();
    let sizes: Vec<_> = tree
        .child_nodes(declarator)
        .filter(|c| tree.kind(*c) == NodeKind::ArraySpec)
        .map(|c| tree.text(source, c))
        .collect();
    assert_eq!(sizes, ["[2]", "[3]"]);
}

#[test]
fn a_declarator_list_declares_every_name_with_its_own_initialiser() {
    let source = "const float PI = 3.14, TAU = 6.28, E;\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let names: Vec<_> = tree
        .nodes()
        .filter(|(_, n)| n.kind == NodeKind::Name)
        .map(|(id, _)| tree.text(source, id))
        .collect();
    assert_eq!(names, ["PI", "TAU", "E"]);
    let initialisers =
        tree.nodes().filter(|(_, n)| n.kind == NodeKind::Initializer).count();
    assert_eq!(initialisers, 2);
}

#[test]
fn a_braced_initialiser_list_nests() {
    let source = "const mat2 m = { { 1.0, 0.0 }, { 0.0, 1.0 } };\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let lists =
        tree.nodes().filter(|(_, n)| n.kind == NodeKind::InitializerList).count();
    assert_eq!(lists, 3, "the outer list plus one per row");
}

#[test]
fn a_stray_semicolon_at_file_scope_is_legal() {
    assert_tree(
        ";\n",
        "
SourceFile
  EmptyDecl
    · ;
",
    );
}

#[test]
fn an_extension_attribute_does_not_swallow_what_follows_it() {
    // `[[…]]` is GL_EXT_control_flow_attributes, and the corpus is full of it.
    let source = "void f() { [[unroll]] for (int i = 0; i < 4; ++i) { } }\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    assert_eq!(tree.nodes().filter(|(_, n)| n.kind == NodeKind::Attribute).count(), 1);
    assert_eq!(tree.nodes().filter(|(_, n)| n.kind == NodeKind::ForStmt).count(), 1);
}

/// `is_qualifier` binary-searches an order built by a `const` insertion sort,
/// and a binary search is only correct over a strictly increasing one. A
/// repeated word would also mean the table lists a qualifier twice.
#[test]
fn the_qualifier_order_is_strictly_sorted_and_finds_every_word() {
    use crate::parser::{QUALIFIER_ORDER, QUALIFIERS, is_qualifier};

    let sorted: Vec<&str> = QUALIFIER_ORDER.iter().map(|&i| QUALIFIERS[i as usize]).collect();
    assert_eq!(sorted.len(), QUALIFIERS.len());
    for pair in sorted.windows(2) {
        assert!(pair[0] < pair[1], "{pair:?} is out of order or repeated");
    }
    for word in QUALIFIERS {
        assert!(is_qualifier(word), "'{word}' is in the table and was not found");
    }
    for word in ["helper", "lambert", "vec3", "normalize", "insample", "cons", ""] {
        assert!(!is_qualifier(word), "'{word}' is not a qualifier");
    }
}
