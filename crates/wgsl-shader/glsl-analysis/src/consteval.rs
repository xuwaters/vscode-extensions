//! Just enough constant folding for array sizes and `const` initialisers.
//!
//! RFC 012 §2 N1: this is not a compiler, and nothing here exists to optimise.
//! Two questions need answering and no more — *what integer is this* (an array
//! size, an index against a bound) and *could this possibly be constant* (a
//! `const` initialiser). Both answer conservatively: an expression this module
//! cannot fold is not thereby non-constant, and only
//! [`certainly_not_constant`] — which needs a resolved, non-`const` name or a
//! call to a user function to say yes — may be built on.

use glsl_syntax::{NodeId, NodeKind};

use crate::analyzer::Analyzer;
use crate::symbols::SymbolKind;

/// How deep folding will recurse. Far past anything a size expression needs.
const MAX_DEPTH: u32 = 32;

/// The integer an expression evaluates to, when it plainly evaluates to one.
pub fn const_int(analyzer: &Analyzer, node: NodeId) -> Option<i64> {
    fold(analyzer, node, 0)
}

fn fold(analyzer: &Analyzer, node: NodeId, depth: u32) -> Option<i64> {
    if depth >= MAX_DEPTH {
        return None;
    }
    match analyzer.tree.kind(node) {
        NodeKind::LiteralExpr => parse_int(analyzer.node_text(node)),
        NodeKind::NameExpr => {
            let name = analyzer.node_text(node);
            let id = analyzer.scopes.lookup_one(name)?;
            analyzer.symbols.get(id)?.const_value
        }
        NodeKind::ParenExpr => {
            let inner = analyzer.child_expression(node)?;
            fold(analyzer, inner, depth + 1)
        }
        NodeKind::UnaryExpr => {
            let operand = analyzer.child_expression(node)?;
            let value = fold(analyzer, operand, depth + 1)?;
            match analyzer.operator(node) {
                "-" => value.checked_neg(),
                "+" => Some(value),
                "~" => Some(!value),
                _ => None,
            }
        }
        NodeKind::BinaryExpr => {
            let operands = analyzer.child_expressions(node);
            let left = fold(analyzer, *operands.first()?, depth + 1)?;
            let right = fold(analyzer, *operands.get(1)?, depth + 1)?;
            match analyzer.operator(node) {
                "+" => left.checked_add(right),
                "-" => left.checked_sub(right),
                "*" => left.checked_mul(right),
                "/" => left.checked_div(right),
                "%" => left.checked_rem(right),
                "<<" => u32::try_from(right).ok().and_then(|by| left.checked_shl(by)),
                ">>" => u32::try_from(right).ok().and_then(|by| left.checked_shr(by)),
                "&" => Some(left & right),
                "|" => Some(left | right),
                "^" => Some(left ^ right),
                _ => None,
            }
        }
        // `int(2.0)` and `uint(N)` — the only constructors a size uses.
        NodeKind::CallExpr => {
            let callee = analyzer.tree.child_nodes(node).next()?;
            if analyzer.tree.kind(callee) != NodeKind::NameExpr {
                return None;
            }
            if !matches!(analyzer.node_text(callee), "int" | "uint") {
                return None;
            }
            let list = analyzer.tree.child_of_kind(node, NodeKind::ArgumentList)?;
            let arguments = analyzer.child_expressions(list);
            if arguments.len() != 1 {
                return None;
            }
            fold(analyzer, arguments[0], depth + 1)
        }
        _ => None,
    }
}

/// A GLSL integer literal, in any base, with its suffix.
pub fn parse_int(text: &str) -> Option<i64> {
    let text = text.trim_end_matches(['u', 'U', 'l', 'L']);
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok();
    }
    if text.len() > 1 && text.starts_with('0') && text.bytes().all(|b| b.is_ascii_digit()) {
        return i64::from_str_radix(&text[1..], 8).ok();
    }
    text.parse::<i64>().ok()
}

/// Whether an expression is *certainly* not constant.
///
/// The only two things that make it so are a name that resolves to storage the
/// program can write — a variable, a parameter, a `uniform` — and a call to a
/// function the file declares. Everything else, unresolved names included,
/// answers false, because the alternative is telling a user their array size is
/// not constant when the truth is that we could not read it.
pub fn certainly_not_constant(analyzer: &Analyzer, node: NodeId) -> bool {
    for descendant in analyzer.tree.descendants(node) {
        match analyzer.tree.kind(descendant) {
            NodeKind::NameExpr => {
                let name = analyzer.node_text(descendant);
                let Some(id) = analyzer.scopes.lookup_one(name) else {
                    continue;
                };
                let Some(symbol) = analyzer.symbols.get(id) else {
                    continue;
                };
                if matches!(
                    symbol.kind,
                    SymbolKind::Global | SymbolKind::Local | SymbolKind::Parameter
                ) && !symbol.qualifiers.is_const
                {
                    return true;
                }
            }
            NodeKind::CallExpr => {
                // A constructor is fine; a call to a declared function is not.
                let Some(callee) = analyzer.tree.child_nodes(descendant).next() else {
                    continue;
                };
                if analyzer.tree.kind(callee) != NodeKind::NameExpr {
                    continue;
                }
                let name = analyzer.node_text(callee);
                if analyzer
                    .scopes
                    .lookup(name)
                    .iter()
                    .filter_map(|id| analyzer.symbols.get(*id))
                    .any(|s| s.kind == SymbolKind::Function)
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}
