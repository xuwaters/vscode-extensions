//! P3-03 — functions: prototypes, definitions, parameter lists, `subroutine`.

use pretty_assertions::assert_eq;

use crate::cst::NodeKind;

use super::{dump, parse_errors, parsed};

#[test]
fn a_definition_and_a_prototype_differ_only_by_their_body() {
    let source = "float f(float x);\nfloat f(float x) { return x; }\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let functions: Vec<_> = tree
        .child_nodes(tree.root())
        .filter(|n| tree.kind(*n) == NodeKind::FunctionDecl)
        .collect();
    assert_eq!(functions.len(), 2);
    assert!(tree.child_of_kind(functions[0], NodeKind::CompoundStmt).is_none());
    assert!(tree.child_of_kind(functions[1], NodeKind::CompoundStmt).is_some());
}

#[test]
fn a_parameter_list_keeps_qualifiers_types_arrays_and_names() {
    assert_eq!(
        dump("float f(in vec3 a, out float b[2], float);\n"),
        "\
SourceFile
  FunctionDecl
    TypeSpec
      · float
    Name
      · f
    ParameterList
      · (
      Parameter
        QualifierList
          · in
        TypeSpec
          · vec3
        Declarator
          Name
            · a
      · ,
      Parameter
        QualifierList
          · out
        TypeSpec
          · float
        Declarator
          Name
            · b
          ArraySpec
            · [
            LiteralExpr
              · 2
            · ]
      · ,
      Parameter
        TypeSpec
          · float
      · )
    · ;
"
    );
}

#[test]
fn void_in_the_parameter_list_names_no_parameter() {
    let source = "void main(void) { }\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let params = tree
        .nodes()
        .find(|(_, n)| n.kind == NodeKind::ParameterList)
        .map(|(id, _)| id)
        .unwrap();
    let parameter = tree.child_of_kind(params, NodeKind::Parameter).unwrap();
    assert!(tree.child_of_kind(parameter, NodeKind::Declarator).is_none());
}

#[test]
fn an_empty_parameter_list_holds_no_parameter_node() {
    let source = "void main() { }\n";
    let (_, tree) = parsed(source);
    let params = tree
        .nodes()
        .find(|(_, n)| n.kind == NodeKind::ParameterList)
        .map(|(id, _)| id)
        .unwrap();
    assert_eq!(tree.child_nodes(params).count(), 0);
}

#[test]
fn a_qualified_return_type_stays_with_the_function() {
    let source = "precise float f() { return 1.0; }\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let function = tree.child_nodes(tree.root()).next().unwrap();
    assert_eq!(tree.kind(function), NodeKind::FunctionDecl);
    assert!(tree.child_of_kind(function, NodeKind::QualifierList).is_some());
}

#[test]
fn a_function_returning_an_array_parses() {
    let source = "float[4] table() { return float[4](1.0, 2.0, 3.0, 4.0); }\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let function = tree.child_nodes(tree.root()).next().unwrap();
    assert_eq!(tree.kind(function), NodeKind::FunctionDecl);
    let type_spec = tree.child_of_kind(function, NodeKind::TypeSpec).unwrap();
    assert_eq!(tree.text(source, type_spec), "float[4]");
}

#[test]
fn subroutine_is_tolerated_in_all_three_of_its_spellings() {
    let source = "\
subroutine void fnType(int a);
subroutine(fnType) void impl(int a) { }
subroutine uniform fnType f;
";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let kinds: Vec<_> =
        tree.child_nodes(tree.root()).map(|n| tree.kind(n).as_str()).collect();
    assert_eq!(kinds, ["FunctionDecl", "FunctionDecl", "Declaration"]);
    let qualifiers: Vec<_> = tree
        .nodes()
        .filter(|(_, n)| n.kind == NodeKind::SubroutineQualifier)
        .map(|(id, _)| tree.text(source, id))
        .collect();
    assert_eq!(qualifiers, ["subroutine", "subroutine(fnType)", "subroutine"]);
}

#[test]
fn a_struct_may_be_declared_in_a_return_type_or_a_parameter() {
    let source = "struct S { int a; } f(struct T { int b; } t) { return S(1); }\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    assert_eq!(tree.nodes().filter(|(_, n)| n.kind == NodeKind::StructSpec).count(), 2);
}

#[test]
fn overloads_are_separate_functions_not_a_redefinition() {
    // Syntax has no opinion about overloading; it just has to produce two.
    let source = "float f(float x) { return x; }\nfloat f(int x) { return 0.0; }\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    assert_eq!(
        tree.child_nodes(tree.root())
            .filter(|n| tree.kind(*n) == NodeKind::FunctionDecl)
            .count(),
        2
    );
}
