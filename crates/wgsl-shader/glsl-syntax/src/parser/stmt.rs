//! Statements — GLSL 4.60 §6, as the §9 grammar spells them.
//!
//! The only ambiguity in a body is declaration versus expression, and
//! [`decl::looks_like_declaration`] settles it before anything is consumed.
//! Everything else is keyword-led.

use super::{Parser, decl, expr};
use crate::cst::NodeKind;
use crate::diagnostics::ParseCode;
use crate::lexer::Punct;

/// One statement of any kind.
pub(crate) fn statement(p: &mut Parser) {
    p.deeper(|p| {
        decl::attributes(p);
        statement_body(p);
    });
}

fn statement_body(p: &mut Parser) {
    match () {
        _ if p.at(Punct::LBrace) => compound(p),
        _ if p.at(Punct::Semi) => p.bump_as(NodeKind::EmptyStmt),
        _ if p.at_word("if") => if_statement(p),
        _ if p.at_word("switch") => switch_statement(p),
        _ if p.at_word("case") || p.at_word("default") => case_label(p),
        _ if p.at_word("while") => while_statement(p),
        _ if p.at_word("do") => do_statement(p),
        _ if p.at_word("for") => for_statement(p),
        _ if p.at_word("return") => return_statement(p),
        _ if p.at_word("break") => jump(p, NodeKind::BreakStmt),
        _ if p.at_word("continue") => jump(p, NodeKind::ContinueStmt),
        _ if p.at_word("discard") => jump(p, NodeKind::DiscardStmt),
        _ if p.at_word("precision") => decl::precision(p),
        _ if decl::looks_like_declaration(p) => declaration_statement(p),
        _ => expression_statement(p),
    }
}

/// `{ … }`.
pub(crate) fn compound(p: &mut Parser) {
    p.deeper(|p| {
        p.open(NodeKind::CompoundStmt);
        let opened = p.here();
        p.expect(Punct::LBrace);
        while !p.at(Punct::RBrace) && !p.at_end() {
            let before = p.pos;
            statement(p);
            if p.pos == before {
                p.skip_one_into_error("expected a statement");
            }
        }
        p.expect_closing(Punct::RBrace, opened);
        p.close();
    });
}

fn declaration_statement(p: &mut Parser) {
    p.open(NodeKind::DeclStmt);
    decl::declaration(p);
    p.close();
}

fn expression_statement(p: &mut Parser) {
    p.open(NodeKind::ExprStmt);
    let before = p.pos;
    expr::expression(p);
    if p.pos == before {
        // Nothing an expression can start with — `primary` has already said
        // so. Skip the line rather than leave the caller's loop to discover
        // that nothing moved.
        p.recover_statement();
        p.close();
        return;
    }
    p.expect_semi();
    p.close();
}

/// `if (cond) stmt` with an optional `else`.
fn if_statement(p: &mut Parser) {
    p.open(NodeKind::IfStmt);
    p.bump();
    header(p);
    statement(p);
    if p.at_word("else") {
        p.open(NodeKind::ElseClause);
        p.bump();
        statement(p);
        p.close();
    }
    p.close();
}

fn switch_statement(p: &mut Parser) {
    p.open(NodeKind::SwitchStmt);
    p.bump();
    header(p);
    if p.at(Punct::LBrace) {
        compound(p);
    } else {
        p.error(ParseCode::ExpectedToken, "expected '{' to open the switch body");
        p.recover_statement();
    }
    p.close();
}

/// `case expr:` and `default:`, which §6 makes statements of their own.
fn case_label(p: &mut Parser) {
    p.open(NodeKind::CaseLabel);
    let is_case = p.at_word("case");
    p.bump();
    if is_case {
        expr::expression(p);
    }
    p.expect(Punct::Colon);
    p.close();
}

fn while_statement(p: &mut Parser) {
    p.open(NodeKind::WhileStmt);
    p.bump();
    header(p);
    statement(p);
    p.close();
}

fn do_statement(p: &mut Parser) {
    p.open(NodeKind::DoWhileStmt);
    p.bump();
    statement(p);
    if !p.eat_word("while") {
        p.error(ParseCode::ExpectedToken, "expected 'while' to close this 'do'");
    }
    header(p);
    p.expect_semi();
    p.close();
}

/// `for (init; condition; increment) body`.
fn for_statement(p: &mut Parser) {
    p.open(NodeKind::ForStmt);
    p.bump();
    let opened = p.here();
    p.expect(Punct::LParen);

    // The initialiser is a declaration, an expression, or nothing.
    if p.at(Punct::Semi) {
        p.bump_as(NodeKind::EmptyStmt);
    } else if decl::looks_like_declaration(p) {
        declaration_statement(p);
    } else {
        expression_statement(p);
    }

    if !p.at(Punct::Semi) && !p.at(Punct::RParen) && !p.at_end() {
        condition(p);
    }
    p.eat(Punct::Semi);

    if !p.at(Punct::RParen) && !p.at_end() {
        let before = p.pos;
        expr::expression(p);
        if p.pos == before {
            p.skip_one_into_error("expected the loop increment");
        }
    }
    p.expect_closing(Punct::RParen, opened);
    statement(p);
    p.close();
}

fn return_statement(p: &mut Parser) {
    p.open(NodeKind::ReturnStmt);
    p.bump();
    if !p.at(Punct::Semi) && !p.at_end() {
        expr::expression(p);
    }
    p.expect_semi();
    p.close();
}

/// `break;`, `continue;`, `discard;`.
fn jump(p: &mut Parser, kind: NodeKind) {
    p.open(kind);
    p.bump();
    p.expect_semi();
    p.close();
}

/// The parenthesised `( condition )` of an `if`, `while` or `switch`.
fn header(p: &mut Parser) {
    let opened = p.here();
    if !p.expect(Punct::LParen) {
        return;
    }
    if !p.at(Punct::RParen) && !p.at_end() {
        condition(p);
    }
    p.expect_closing(Punct::RParen, opened);
}

/// A condition, which §9 lets declare: `while (bool ok = next())`.
fn condition(p: &mut Parser) {
    p.open(NodeKind::Condition);
    let before = p.pos;
    if decl::looks_like_declaration(p) {
        // Without the `;` a declaration statement would consume — a condition
        // ends at its `)` or `;`, not at a semicolon of its own.
        decl::qualifier_list(p);
        if decl::at_type_start(p) {
            decl::type_spec(p);
        }
        if p.at_ident() {
            p.open(NodeKind::Declarator);
            p.bump_as(NodeKind::Name);
            while p.at(Punct::LBracket) {
                decl::array_spec(p);
            }
            if p.at(Punct::Eq) {
                p.open(NodeKind::Initializer);
                p.bump();
                expr::assignment(p);
                p.close();
            }
            p.close();
        } else {
            p.error(ParseCode::ExpectedIdentifier, "expected the name being declared");
        }
    } else {
        expr::expression(p);
    }
    if p.pos == before {
        p.error(ParseCode::ExpectedExpression, "expected a condition");
        // Skip to the `)` this condition sits in, without leaving the header.
        p.open(NodeKind::Error);
        while !p.at(Punct::RParen) && !p.at_boundary() {
            p.bump();
        }
        p.close();
    }
    p.close();
}
