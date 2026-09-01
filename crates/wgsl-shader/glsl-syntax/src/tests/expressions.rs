//! P3-04 — statements and expressions.
//!
//! The precedence fixtures are written as one-line S-expressions rather than
//! full dumps: the whole point is the *grouping*, and a 40-line indented tree
//! makes an inverted precedence table harder to see, not easier.

use pretty_assertions::assert_eq;

use crate::cst::{Child, NodeId, NodeKind, SyntaxTree};
use crate::preprocessor::Preprocessed;

use super::{parse_errors, parsed};

/// The parse of one expression, as `(Kind a b)` with tokens spelled inline.
///
/// `x = a + b * c` becomes `(AssignExpr x = (BinaryExpr a + (BinaryExpr b * c)))`,
/// which is readable enough to check a precedence table against §5.1 by eye.
fn shape(expression: &str) -> String {
    let source = format!("void f() {{ {expression}; }}\n");
    let (pp, tree) = parsed(&source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new(), "{expression} did not parse");
    let statement = tree
        .nodes()
        .find(|(_, n)| n.kind == NodeKind::ExprStmt)
        .map(|(id, _)| id)
        .expect("no expression statement");
    let inner = tree.child_nodes(statement).next().expect("no expression");
    let mut out = String::new();
    write_shape(&tree, &pp, inner, &mut out);
    out
}

fn write_shape(tree: &SyntaxTree, pp: &Preprocessed, id: NodeId, out: &mut String) {
    let leaf = tree.children(id).iter().all(|c| matches!(c, Child::Token(_)));
    if !leaf {
        out.push('(');
        out.push_str(tree.kind(id).as_str());
        out.push(' ');
    }
    let mut first = true;
    for child in tree.children(id) {
        if !first {
            out.push(' ');
        }
        first = false;
        match *child {
            Child::Token(token) => match pp.tokens.get(token.index()) {
                Some(token) => out.push_str(&token.text),
                None => out.push('?'),
            },
            Child::Node(node) => write_shape(tree, pp, node, out),
        }
    }
    if !leaf {
        out.push(')');
    }
}

#[test]
fn multiplication_binds_tighter_than_addition() {
    assert_eq!(shape("a = b + c * d"), "(AssignExpr a = (BinaryExpr b + (BinaryExpr c * d)))");
    assert_eq!(shape("a = b * c + d"), "(AssignExpr a = (BinaryExpr (BinaryExpr b * c) + d))");
}

#[test]
fn arithmetic_is_left_associative() {
    assert_eq!(shape("a - b - c"), "(BinaryExpr (BinaryExpr a - b) - c)");
    assert_eq!(shape("a / b / c"), "(BinaryExpr (BinaryExpr a / b) / c)");
}

#[test]
fn the_whole_precedence_ladder_comes_out_in_order() {
    // One operator from every level of §5.1, loosest last. Each should end up
    // one step further out than the one below it.
    assert_eq!(
        shape("a || b ^^ c && d | e ^ f & g == h < i << j + k * l"),
        "(BinaryExpr a || (BinaryExpr b ^^ (BinaryExpr c && (BinaryExpr d | \
         (BinaryExpr e ^ (BinaryExpr f & (BinaryExpr g == (BinaryExpr h < \
         (BinaryExpr i << (BinaryExpr j + (BinaryExpr k * l)))))))))))"
    );
}

#[test]
fn assignment_is_right_associative_and_looser_than_the_conditional() {
    assert_eq!(shape("a = b = c"), "(AssignExpr a = (AssignExpr b = c))");
    assert_eq!(
        shape("a = b ? c : d"),
        "(AssignExpr a = (CondExpr b ? c : d))"
    );
    assert_eq!(
        shape("a += b < c ? d : e"),
        "(AssignExpr a += (CondExpr (BinaryExpr b < c) ? d : e))"
    );
}

#[test]
fn the_conditional_is_right_associative() {
    assert_eq!(shape("a ? b : c ? d : e"), "(CondExpr a ? b : (CondExpr c ? d : e))");
}

#[test]
fn the_comma_operator_is_looser_than_assignment() {
    assert_eq!(shape("a = b, c = d"), "(CommaExpr (AssignExpr a = b) , (AssignExpr c = d))");
}

#[test]
fn unary_binds_tighter_than_any_binary_operator() {
    assert_eq!(shape("-a * b"), "(BinaryExpr (UnaryExpr - a) * b)");
    assert_eq!(shape("!a && b"), "(BinaryExpr (UnaryExpr ! a) && b)");
    assert_eq!(shape("- - a"), "(UnaryExpr - (UnaryExpr - a))");
}

#[test]
fn postfix_binds_tighter_than_prefix() {
    assert_eq!(shape("-a.x"), "(UnaryExpr - (FieldExpr a . x))");
    assert_eq!(shape("++a[0]"), "(UnaryExpr ++ (IndexExpr a [ 0 ]))");
    assert_eq!(shape("a++ + b"), "(BinaryExpr (PostfixExpr a ++) + b)");
}

#[test]
fn a_postfix_chain_is_read_left_to_right() {
    assert_eq!(
        shape("v.xyz[1].w"),
        "(FieldExpr (IndexExpr (FieldExpr v . xyz) [ 1 ]) . w)"
    );
    assert_eq!(
        shape("f(1)(2)"),
        "(CallExpr (CallExpr f (ArgumentList ( 1 ))) (ArgumentList ( 2 )))"
    );
}

#[test]
fn a_call_a_constructor_and_an_array_constructor_are_all_calls() {
    // Syntax cannot tell them apart, and says so by using one node kind.
    assert_eq!(shape("f(a)"), "(CallExpr f (ArgumentList ( a )))");
    assert_eq!(shape("vec4(a)"), "(CallExpr vec4 (ArgumentList ( a )))");
    assert_eq!(
        shape("float[2](a, b)"),
        "(CallExpr (IndexExpr float [ 2 ]) (ArgumentList ( a , b )))"
    );
}

#[test]
fn parentheses_override_precedence_and_keep_their_own_node() {
    assert_eq!(
        shape("(a + b) * c"),
        "(BinaryExpr (ParenExpr ( (BinaryExpr a + b) )) * c)"
    );
}

#[test]
fn an_argument_list_splits_on_commas_not_on_the_comma_operator() {
    assert_eq!(
        shape("f(a, b)"),
        "(CallExpr f (ArgumentList ( a , b )))"
    );
    // Parenthesised, the comma *is* the operator again.
    assert_eq!(
        shape("f((a, b))"),
        "(CallExpr f (ArgumentList ( (ParenExpr ( (CommaExpr a , b) )) )))"
    );
}

#[test]
fn every_statement_form_has_its_own_node() {
    let source = "\
void f() {
    ;
    { }
    if (a) b(); else c();
    switch (k) { case 1: break; default: discard; }
    while (a) continue;
    do { } while (a);
    for (int i = 0; i < 4; ++i) { }
    return;
}
";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let kinds: Vec<&str> = tree
        .nodes()
        .map(|(_, n)| n.kind)
        .filter(|k| {
            matches!(
                k,
                NodeKind::EmptyStmt
                    | NodeKind::IfStmt
                    | NodeKind::ElseClause
                    | NodeKind::SwitchStmt
                    | NodeKind::CaseLabel
                    | NodeKind::BreakStmt
                    | NodeKind::DiscardStmt
                    | NodeKind::WhileStmt
                    | NodeKind::ContinueStmt
                    | NodeKind::DoWhileStmt
                    | NodeKind::ForStmt
                    | NodeKind::ReturnStmt
            )
        })
        .map(|k| k.as_str())
        .collect();
    assert_eq!(
        kinds,
        [
            "EmptyStmt",
            "IfStmt",
            "ElseClause",
            "SwitchStmt",
            "CaseLabel",
            "BreakStmt",
            "CaseLabel",
            "DiscardStmt",
            "WhileStmt",
            "ContinueStmt",
            "DoWhileStmt",
            "ForStmt",
            "ReturnStmt"
        ]
    );
}

#[test]
fn a_declaration_and_an_expression_in_a_body_are_told_apart() {
    let source = "\
void f() {
    vec3 a;
    a = b;
    a[0] = 1.0;
    g(a);
    float[2] c;
    sample = 2.0;
}
";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let kinds: Vec<&str> = tree
        .nodes()
        .map(|(_, n)| n.kind)
        .filter(|k| matches!(k, NodeKind::DeclStmt | NodeKind::ExprStmt))
        .map(|k| k.as_str())
        .collect();
    // `sample` is a qualifier keyword being used as a name, which pre-1.30
    // sources do; it is still an assignment.
    assert_eq!(
        kinds,
        ["DeclStmt", "ExprStmt", "ExprStmt", "ExprStmt", "DeclStmt", "ExprStmt"]
    );
}

#[test]
fn a_condition_may_declare() {
    let source = "void f() { while (bool ok = next()) { } }\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let condition = tree
        .nodes()
        .find(|(_, n)| n.kind == NodeKind::Condition)
        .map(|(id, _)| id)
        .unwrap();
    assert!(tree.child_of_kind(condition, NodeKind::TypeSpec).is_some());
    assert!(tree.child_of_kind(condition, NodeKind::Declarator).is_some());
}

#[test]
fn a_for_header_keeps_its_three_parts_apart() {
    let source = "void f() { for (int i = 0; i < 4; ++i) s += i; }\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    let loop_node = tree
        .nodes()
        .find(|(_, n)| n.kind == NodeKind::ForStmt)
        .map(|(id, _)| id)
        .unwrap();
    let parts: Vec<&str> =
        tree.child_nodes(loop_node).map(|n| tree.kind(n).as_str()).collect();
    assert_eq!(parts, ["DeclStmt", "Condition", "UnaryExpr", "ExprStmt"]);
}

#[test]
fn an_empty_for_header_parses() {
    let source = "void f() { for (;;) { } }\n";
    let (_, tree) = parsed(source);
    assert_eq!(parse_errors(&tree), Vec::<&str>::new());
    assert_eq!(tree.nodes().filter(|(_, n)| n.kind == NodeKind::ForStmt).count(), 1);
}
