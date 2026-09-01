//! P3-01 — the tree's own contract, independent of any grammar rule.
//!
//! Everything here is a property that must hold for *every* input, which is
//! what makes it worth asserting separately from the fixtures that check what
//! a particular construct parses to. The corpus gate re-asserts all of it, on
//! ~1,700 real shaders.

use pretty_assertions::assert_eq;

use crate::cst::{Child, NodeId, NodeKind, PieceKind, SyntaxTree};
use crate::preprocessor::Preprocessed;

use super::{dump, parsed};

/// A cross-section: macros, conditionals, a block, a function, a loop.
const SHADER: &str = "\
#version 310 es
#define SCALE(v) ((v) * uScale)
uniform float uScale;
layout(binding = 0) uniform Camera { mat4 view; } camera;

#ifdef GL_ES
out vec4 colour;
#else
varying vec4 colour;
#endif

void main() {
    for (int i = 0; i < 4; ++i) {
        colour = vec4(SCALE(camera.view[i].x), 0.0, 0.0, 1.0);
    }
}
";

/// Every token of the stream is a leaf of the tree, exactly once, in order.
///
/// This is the invariant that makes "lossless" mean something: a parser that
/// quietly dropped what it did not understand would still round-trip the
/// source, because the dropped bytes would look like a gap.
fn assert_every_token_is_a_leaf(tree: &SyntaxTree) {
    let mut leaves: Vec<u32> = Vec::new();
    collect_leaves(tree, NodeId::ROOT, &mut leaves);
    let expected: Vec<u32> = (0..tree.token_count()).collect();
    assert_eq!(leaves, expected, "the tree does not hold every token exactly once");
}

/// The leaves of a subtree in the order a reader would meet them — which is
/// not the order the preorder node array gives, because a node's tokens and
/// its child nodes interleave.
fn collect_leaves(tree: &SyntaxTree, id: NodeId, out: &mut Vec<u32>) {
    for child in tree.children(id) {
        match *child {
            Child::Token(token) => out.push(token.0),
            Child::Node(node) => collect_leaves(tree, node, out),
        }
    }
}

fn assert_round_trips(tree: &SyntaxTree, pp: &Preprocessed, source: &str) {
    let pieces = tree.pieces(pp, source);
    let mut at = 0u32;
    for piece in &pieces {
        assert_eq!(piece.span.start, at, "the pieces do not tile the source");
        at = piece.span.end;
    }
    assert_eq!(at as usize, source.len(), "the pieces stop short of the end");
    assert_eq!(tree.reconstruct(pp, source), source);
}

#[test]
fn every_token_is_a_leaf_exactly_once() {
    let (_, tree) = parsed(SHADER);
    assert_every_token_is_a_leaf(&tree);
}

#[test]
fn the_tree_round_trips_the_source() {
    let (pp, tree) = parsed(SHADER);
    assert_round_trips(&tree, &pp, SHADER);
}

#[test]
fn a_directive_only_source_is_all_gap() {
    // Nothing reaches the parser, and every byte still comes back.
    let source = "#version 450\n#define A 1\n#if 0\nint dead;\n#endif\n";
    let (pp, tree) = parsed(source);
    assert!(pp.tokens.is_empty());
    assert_eq!(tree.token_count(), 0);
    let pieces = tree.pieces(&pp, source);
    assert!(pieces.iter().all(|p| p.kind == PieceKind::Gap));
    assert_eq!(tree.reconstruct(&pp, source), source);
}

#[test]
fn a_macro_invocation_is_covered_by_the_gap_around_it() {
    let source = "#define HALF 0.5\nfloat x = HALF;\n";
    let (pp, tree) = parsed(source);
    assert_round_trips(&tree, &pp, source);
    // The `0.5` was substituted, so it is not a piece of its own; the bytes
    // spelling `HALF` are.
    let tokens: Vec<&str> = tree
        .pieces(&pp, source)
        .iter()
        .filter(|p| matches!(p.kind, PieceKind::Token(_)))
        .map(|p| &source[p.span.start as usize..p.span.end as usize])
        .collect();
    assert_eq!(tokens, ["float", "x", "=", ";"]);
}

#[test]
fn an_argument_used_twice_stays_covered_once() {
    let source = "#define SQUARE(v) ((v) * (v))\nfloat x = SQUARE(y);\n";
    let (pp, tree) = parsed(source);
    assert_round_trips(&tree, &pp, source);
    assert_every_token_is_a_leaf(&tree);
}

#[test]
fn node_ids_are_preorder_and_subtrees_are_id_ranges() {
    let (_, tree) = parsed(SHADER);
    for (id, node) in tree.nodes() {
        assert!(node.parent <= id, "a parent must be allocated before its child");
        assert!(node.end.0 > id.0, "a subtree always holds at least its own node");
        for child in tree.child_nodes(id) {
            assert!(tree.contains(id, child), "{child:?} is not inside {id:?}");
            assert_eq!(tree.node(child).parent, id);
        }
    }
}

#[test]
fn the_root_is_the_source_file_and_is_its_own_parent() {
    let (_, tree) = parsed(SHADER);
    assert_eq!(tree.kind(NodeId::ROOT), NodeKind::SourceFile);
    assert_eq!(tree.node(NodeId::ROOT).parent, NodeId::ROOT);
    assert_eq!(tree.node(NodeId::ROOT).end.0 as usize, tree.node_count());
}

#[test]
fn node_spans_nest_inside_their_parents() {
    let (_, tree) = parsed(SHADER);
    for (id, node) in tree.nodes() {
        if node.span.is_empty() {
            continue;
        }
        for child in tree.child_nodes(id) {
            let child_span = tree.span(child);
            if child_span.is_empty() {
                continue;
            }
            assert!(
                child_span.start >= node.span.start && child_span.end <= node.span.end,
                "{:?} {child_span:?} escapes its parent {:?} {:?}",
                tree.kind(child),
                node.kind,
                node.span
            );
        }
    }
}

#[test]
fn node_at_finds_the_innermost_node_on_the_cursor() {
    let source = "void main() { float x = 1.0; }\n";
    let (_, tree) = parsed(source);
    let offset = source.find("1.0").unwrap() as u32;
    let node = tree.node_at(offset);
    assert_eq!(tree.kind(node), NodeKind::LiteralExpr);
    // And the chain above it is the declaration it belongs to.
    let kinds: Vec<&str> = tree.ancestors(node).map(|n| tree.kind(n).as_str()).collect();
    assert_eq!(
        kinds,
        [
            "LiteralExpr",
            "Initializer",
            "Declarator",
            "Declaration",
            "DeclStmt",
            "CompoundStmt",
            "FunctionDecl",
            "SourceFile"
        ]
    );
}

#[test]
fn an_empty_source_still_has_a_root() {
    let (pp, tree) = parsed("");
    assert_eq!(tree.node_count(), 1);
    assert_eq!(tree.kind(NodeId::ROOT), NodeKind::SourceFile);
    assert!(tree.diagnostics.is_empty());
    assert_eq!(tree.reconstruct(&pp, ""), "");
}

#[test]
fn the_dump_names_every_node_kind_it_prints() {
    // A guard on `NodeKind::as_str`: a kind added without a name would print
    // as something the fixtures could not match on.
    let text = dump("void main() { int a = 1 + 2; }\n");
    assert!(text.starts_with("SourceFile\n"), "{text}");
    assert!(text.contains("BinaryExpr"), "{text}");
    assert!(!text.contains("<gone>"), "{text}");
}

#[test]
fn parser_diagnostic_codes_are_stable_unique_and_in_their_own_range() {
    use analyzer_core::diagnostics::DiagnosticCode;

    use crate::diagnostics::ParseCode;

    let codes = [
        ParseCode::ExpectedToken,
        ParseCode::ExpectedIdentifier,
        ParseCode::ExpectedType,
        ParseCode::ExpectedExpression,
        ParseCode::UnexpectedToken,
        ParseCode::UnclosedDelimiter,
        ParseCode::MalformedLayout,
        ParseCode::MalformedArraySpecifier,
        ParseCode::UnreadableDeclaration,
        ParseCode::UnexpectedEndOfFile,
        ParseCode::NestingLimit,
    ];
    let mut seen: Vec<&str> = codes.iter().map(|c| c.as_str()).collect();
    seen.sort_unstable();
    let count = seen.len();
    seen.dedup();
    assert_eq!(seen.len(), count, "two diagnostics share a code");
    // `GLSL0100`–`GLSL0199` is the parser's range; the preprocessor keeps
    // everything below it.
    for code in &seen {
        let number: u32 = code.trim_start_matches("GLSL").parse().expect("{code} is malformed");
        assert!((100..200).contains(&number), "{code} is outside the parser's range");
    }
}

#[test]
fn spelling_reads_the_expanded_stream() {
    let source = "#define HALF 0.5\nfloat x = HALF;\n";
    let (pp, tree) = parsed(source);
    let offset = source.rfind("HALF").unwrap() as u32;
    let node = tree.node_at(offset);
    // The tree spells what the macro produced, and points at what was written.
    assert_eq!(tree.spelling(&pp, node), "0.5");
    assert_eq!(tree.text(source, node), "HALF");
}

