//! Expressions — the GLSL 4.60 §5.1 precedence table, by climbing.
//!
//! Each level parses the level above it and then, while it sees one of its own
//! operators, wraps what it already has. The wrapping is
//! [`Parser::open_before`], which is what keeps `a - b - c` left-associative
//! without an `O(n²)` insert per operator.
//!
//! Nothing here decides what an expression *means*. `vec4(1.0)`, `f(1.0)` and
//! `float[2](a, b)` are all [`NodeKind::CallExpr`]; `a[i]` is
//! [`NodeKind::IndexExpr`] whether `a` is an array or a type. Telling a
//! constructor from a call needs a symbol table, and a syntax tree that guessed
//! would be a syntax tree that lies.

use super::Parser;
use crate::cst::NodeKind;
use crate::diagnostics::ParseCode;
use crate::lexer::{Punct, TokenKind};

/// The binary operators, loosest binding first. §5.1 exactly.
const LEVELS: &[&[Punct]] = &[
    &[Punct::OrOr],
    &[Punct::XorXor],
    &[Punct::AndAnd],
    &[Punct::Pipe],
    &[Punct::Caret],
    &[Punct::Amp],
    &[Punct::EqEq, Punct::Ne],
    &[Punct::Lt, Punct::Gt, Punct::Le, Punct::Ge],
    &[Punct::Shl, Punct::Shr],
    &[Punct::Plus, Punct::Minus],
    &[Punct::Star, Punct::Slash, Punct::Percent],
];

/// The assignment operators, which are right-associative and bind loosest of
/// all but the comma.
const ASSIGN: &[Punct] = &[
    Punct::Eq,
    Punct::PlusEq,
    Punct::MinusEq,
    Punct::StarEq,
    Punct::SlashEq,
    Punct::PercentEq,
    Punct::ShlEq,
    Punct::ShrEq,
    Punct::AmpEq,
    Punct::CaretEq,
    Punct::PipeEq,
];

/// The prefix operators.
const PREFIX: &[Punct] = &[
    Punct::PlusPlus,
    Punct::MinusMinus,
    Punct::Plus,
    Punct::Minus,
    Punct::Tilde,
    Punct::Bang,
];

/// A full expression, comma operator included.
pub(crate) fn expression(p: &mut Parser) {
    let start = p.checkpoint();
    assignment(p);
    if p.at(Punct::Comma) {
        p.open_before(start, NodeKind::CommaExpr);
        while p.eat(Punct::Comma) {
            assignment(p);
        }
        p.close();
    }
}

/// One assignment expression — the unit an argument list and an initialiser
/// are made of, and the point on every recursive cycle where depth is counted.
pub(crate) fn assignment(p: &mut Parser) {
    p.deeper(|p| {
        let start = p.checkpoint();
        conditional(p);
        if p.punct().is_some_and(|punct| ASSIGN.contains(&punct)) {
            p.open_before(start, NodeKind::AssignExpr);
            p.bump();
            assignment(p);
            p.close();
        }
    });
}

/// `c ? a : b`, right-associative.
fn conditional(p: &mut Parser) {
    let start = p.checkpoint();
    binary(p, 0);
    if p.at(Punct::Question) {
        p.open_before(start, NodeKind::CondExpr);
        p.bump();
        expression(p);
        p.expect(Punct::Colon);
        assignment(p);
        p.close();
    }
}

/// One rung of the precedence ladder.
fn binary(p: &mut Parser, level: usize) {
    let Some(operators) = LEVELS.get(level) else {
        unary(p);
        return;
    };
    let start = p.checkpoint();
    binary(p, level + 1);
    while p.punct().is_some_and(|punct| operators.contains(&punct)) {
        p.open_before(start, NodeKind::BinaryExpr);
        p.bump();
        binary(p, level + 1);
        p.close();
    }
}

/// The prefix operators, iteratively — `!!!!x` must not cost stack.
fn unary(p: &mut Parser) {
    let mut opened = 0usize;
    while p.punct().is_some_and(|punct| PREFIX.contains(&punct)) {
        p.open(NodeKind::UnaryExpr);
        p.bump();
        opened += 1;
    }
    postfix(p);
    for _ in 0..opened {
        p.close();
    }
}

/// `[…]`, `(…)`, `.member`, `++` and `--`, applied left to right.
fn postfix(p: &mut Parser) {
    let start = p.checkpoint();
    primary(p);
    loop {
        if p.at(Punct::LBracket) {
            p.open_before(start, NodeKind::IndexExpr);
            let opened = p.here();
            p.bump();
            if !p.at(Punct::RBracket) && !p.at_end() {
                expression(p);
            }
            p.expect_closing(Punct::RBracket, opened);
            p.close();
            continue;
        }
        if p.at(Punct::LParen) {
            p.open_before(start, NodeKind::CallExpr);
            arguments(p);
            p.close();
            continue;
        }
        if p.at(Punct::Dot) {
            p.open_before(start, NodeKind::FieldExpr);
            p.bump();
            if p.at_ident() {
                p.bump();
            } else {
                p.error(ParseCode::ExpectedIdentifier, "expected a member or swizzle name");
            }
            p.close();
            continue;
        }
        if p.at(Punct::PlusPlus) || p.at(Punct::MinusMinus) {
            p.open_before(start, NodeKind::PostfixExpr);
            p.bump();
            p.close();
            continue;
        }
        return;
    }
}

/// `( a, b )` at a call site. `f(void)` is spelled with a `void` argument, which
/// is a name like any other here.
fn arguments(p: &mut Parser) {
    p.open(NodeKind::ArgumentList);
    let opened = p.here();
    p.bump();
    while !p.at(Punct::RParen) && !p.at_end() {
        let before = p.pos;
        assignment(p);
        if p.pos == before {
            // Neither an argument nor a `)`. One token into an `Error` keeps
            // the list moving instead of spinning.
            p.skip_one_into_error("expected an argument");
        }
        if !p.eat(Punct::Comma) {
            break;
        }
    }
    p.expect_closing(Punct::RParen, opened);
    p.close();
}

/// A name, a literal, or a parenthesised expression.
///
/// Consumes nothing when the cursor is on something that cannot start an
/// expression: the caller is always either a loop with its own guard or a
/// statement that is about to recover, and eating a `;` here would take the
/// boundary they need.
fn primary(p: &mut Parser) {
    match p.nth_kind(0) {
        Some(TokenKind::Int | TokenKind::Float | TokenKind::Str) => {
            p.bump_as(NodeKind::LiteralExpr)
        }
        Some(TokenKind::Ident) => p.bump_as(NodeKind::NameExpr),
        Some(TokenKind::Punct(Punct::LParen)) => {
            p.open(NodeKind::ParenExpr);
            let opened = p.here();
            p.bump();
            if !p.at(Punct::RParen) && !p.at_end() {
                expression(p);
            }
            p.expect_closing(Punct::RParen, opened);
            p.close();
        }
        _ => {
            p.error(ParseCode::ExpectedExpression, "expected an expression");
            p.open(NodeKind::Error);
            p.close();
        }
    }
}
