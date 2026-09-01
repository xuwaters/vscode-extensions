//! Declarations — GLSL 4.60 §4 and §9's `external_declaration`.
//!
//! Everything at file scope is a declaration, so nothing here has to guess. The
//! guessing is [`looks_like_declaration`], which a *statement* asks before it
//! decides whether `foo bar;` declares something or evaluates something. Its
//! answer rests on the one thing GLSL guarantees: two adjacent identifiers
//! cannot occur in an expression.

use super::{Parser, is_qualifier, stmt};
use crate::cst::NodeKind;
use crate::diagnostics::ParseCode;
use crate::lexer::Punct;

/// One top-level declaration: a function, a variable, a block, a `precision`
/// line, or a stray `;`.
pub(crate) fn external_declaration(p: &mut Parser) {
    p.deeper(|p| {
        attributes(p);
        if p.at(Punct::Semi) {
            p.bump_as(NodeKind::EmptyDecl);
            return;
        }
        if p.at_word("precision") {
            precision(p);
            return;
        }
        declaration(p);
    });
}

/// Any run of `[[ … ]]` attribute groups in front of a declaration or a
/// statement — `GL_EXT_control_flow_attributes` and its relatives.
///
/// The contents are taken verbatim: they are extension vocabulary, and the
/// grammar's only job is to not lose the statement behind them.
pub(crate) fn attributes(p: &mut Parser) {
    while p.at(Punct::LBracket) && p.nth_is(1, Punct::LBracket) {
        p.open(NodeKind::Attribute);
        p.bump();
        p.bump();
        let mut depth = 0u32;
        while !p.at_end() {
            if p.at(Punct::RBracket) && p.nth_is(1, Punct::RBracket) && depth == 0 {
                p.bump();
                p.bump();
                break;
            }
            match p.punct() {
                Some(Punct::LParen | Punct::LBracket) => depth += 1,
                Some(Punct::RParen | Punct::RBracket) if depth > 0 => depth -= 1,
                // An unterminated group must not swallow the rest of the file.
                Some(Punct::Semi | Punct::LBrace | Punct::RBrace) if depth == 0 => break,
                _ => {}
            }
            p.bump();
        }
        p.close();
    }
}

/// `precision highp float;` — legal at file scope and, in ES, inside a body.
pub(crate) fn precision(p: &mut Parser) {
    p.open(NodeKind::PrecisionDecl);
    p.bump();
    if p.at_ident() {
        p.bump();
    } else {
        p.error(ParseCode::ExpectedIdentifier, "expected highp, mediump or lowp");
    }
    if p.at_ident() {
        type_spec(p);
    } else {
        p.error(ParseCode::ExpectedType, "expected the type this precision applies to");
    }
    p.expect_semi();
    p.close();
}

/// The whole of `declaration` and `function_definition`, which share their
/// first half and only diverge at the `(`.
pub(crate) fn declaration(p: &mut Parser) {
    let start = p.checkpoint();
    let qualified = qualifier_list(p);

    // `layout(local_size_x = 64) in;` — qualifiers and nothing else.
    if qualified && p.at(Punct::Semi) {
        p.open_before(start, NodeKind::QualifierDecl);
        p.bump();
        p.close();
        return;
    }
    // `invariant gl_Position, gl_PointSize;` — qualifiers over existing names.
    if qualified
        && p.at_ident()
        && (p.nth_is(1, Punct::Comma) || p.nth_is(1, Punct::Semi))
    {
        p.open_before(start, NodeKind::QualifierDecl);
        loop {
            p.bump_as(NodeKind::Name);
            if !p.eat(Punct::Comma) {
                break;
            }
            if !p.at_ident() {
                p.error(ParseCode::ExpectedIdentifier, "expected another name");
                break;
            }
        }
        p.expect_semi();
        p.close();
        return;
    }
    // `layout(std140) uniform Camera { … } camera;`
    if qualified && p.at_ident() && p.nth_is(1, Punct::LBrace) {
        interface_block(p, start);
        return;
    }

    if !at_type_start(p) {
        // At file scope only a declaration is legal, so this is not "a type is
        // missing" — it is a line we cannot read at all.
        p.error(ParseCode::UnreadableDeclaration, "expected a declaration");
        p.open_before(start, NodeKind::Error);
        p.recover_statement();
        p.close();
        return;
    }
    type_spec(p);

    // `struct S { … };` and `uniform Block;` declare no name of their own.
    if p.at(Punct::Semi) {
        p.open_before(start, NodeKind::Declaration);
        p.bump();
        p.close();
        return;
    }
    if p.at_ident() && p.nth_is(1, Punct::LParen) {
        function(p, start);
        return;
    }
    if p.at_ident() {
        p.open_before(start, NodeKind::Declaration);
        declarator_list(p);
        p.expect_semi();
        p.close();
        return;
    }
    p.open_before(start, NodeKind::Declaration);
    p.error(ParseCode::ExpectedIdentifier, "expected the name being declared");
    p.recover_statement();
    p.close();
}

/// `float lambert(vec3 n, vec3 l) { … }`, or the same without a body.
fn function(p: &mut Parser, start: u32) {
    p.open_before(start, NodeKind::FunctionDecl);
    p.bump_as(NodeKind::Name);
    parameter_list(p);
    if p.at(Punct::LBrace) {
        stmt::compound(p);
    } else {
        p.expect_semi();
    }
    p.close();
}

fn parameter_list(p: &mut Parser) {
    p.open(NodeKind::ParameterList);
    let opened = p.here();
    p.expect(Punct::LParen);
    while !p.at(Punct::RParen) && !p.at_end() {
        if p.at(Punct::RBrace) || p.at(Punct::Semi) {
            // The `)` is missing and the declaration has moved on without it.
            break;
        }
        let before = p.pos;
        parameter(p);
        if p.pos == before {
            p.skip_one_into_error("expected a parameter");
        }
        if !p.eat(Punct::Comma) {
            break;
        }
    }
    p.expect_closing(Punct::RParen, opened);
    p.close();
}

/// `const in highp vec3 light[2]`, or `void`, or a bare type.
fn parameter(p: &mut Parser) {
    p.open(NodeKind::Parameter);
    qualifier_list(p);
    if at_type_start(p) {
        type_spec(p);
        // A parameter may be unnamed: `float f(float);`
        if p.at_ident() {
            declarator(p, false);
        }
    } else {
        p.error(ParseCode::ExpectedType, "expected a parameter type");
    }
    p.close();
}

/// `layout(std140) uniform Camera { mat4 view; } camera[2];`
fn interface_block(p: &mut Parser, start: u32) {
    p.open_before(start, NodeKind::InterfaceBlock);
    p.bump_as(NodeKind::Name);
    field_list(p);
    if p.at_ident() {
        declarator(p, false);
    }
    p.expect_semi();
    p.close();
}

/// The `{ … }` of a struct or an interface block.
pub(crate) fn field_list(p: &mut Parser) {
    p.deeper(|p| {
        p.open(NodeKind::FieldList);
        let opened = p.here();
        p.expect(Punct::LBrace);
        while !p.at(Punct::RBrace) && !p.at_end() {
            let before = p.pos;
            field(p);
            if p.pos == before {
                p.skip_one_into_error("expected a member declaration");
            }
        }
        p.expect_closing(Punct::RBrace, opened);
        p.close();
    });
}

/// One `layout(offset = 4) mediump float a, b[2];` line inside a block.
fn field(p: &mut Parser) {
    p.open(NodeKind::FieldDecl);
    qualifier_list(p);
    if at_type_start(p) {
        type_spec(p);
        if p.at_ident() {
            declarator_list(p);
            p.expect_semi();
        } else if p.at(Punct::Semi) {
            // `struct { int a; };` nested with no member name of its own.
            p.bump();
        } else {
            p.error(ParseCode::ExpectedIdentifier, "expected a member name");
            p.recover_statement();
        }
    } else {
        p.error(ParseCode::ExpectedType, "expected a member type");
        p.recover_statement();
    }
    p.close();
}

/// `a, b[2], c = vec3(0.0)`.
fn declarator_list(p: &mut Parser) {
    loop {
        if !p.at_ident() {
            p.error(ParseCode::ExpectedIdentifier, "expected a name");
            break;
        }
        declarator(p, true);
        if !p.eat(Punct::Comma) {
            break;
        }
    }
}

/// One `name [array…] [= initialiser]`.
fn declarator(p: &mut Parser, allow_initialiser: bool) {
    p.open(NodeKind::Declarator);
    p.bump_as(NodeKind::Name);
    while p.at(Punct::LBracket) {
        array_spec(p);
    }
    if allow_initialiser && p.at(Punct::Eq) {
        p.open(NodeKind::Initializer);
        p.bump();
        initialiser(p);
        p.close();
    }
    p.close();
}

/// The right-hand side of an `=`: an expression, or a braced list (4.20+).
fn initialiser(p: &mut Parser) {
    if p.at(Punct::LBrace) {
        initialiser_list(p);
    } else {
        super::expr::assignment(p);
    }
}

fn initialiser_list(p: &mut Parser) {
    p.deeper(|p| {
        p.open(NodeKind::InitializerList);
        let opened = p.here();
        p.expect(Punct::LBrace);
        while !p.at(Punct::RBrace) && !p.at_end() {
            let before = p.pos;
            initialiser(p);
            if p.pos == before {
                p.skip_one_into_error("expected an initialiser");
            }
            if !p.eat(Punct::Comma) {
                break;
            }
        }
        p.expect_closing(Punct::RBrace, opened);
        p.close();
    });
}

/// One `[ … ]`, sized or not.
pub(crate) fn array_spec(p: &mut Parser) {
    p.open(NodeKind::ArraySpec);
    let opened = p.here();
    p.bump();
    if !p.at(Punct::RBracket) && !p.at_end() {
        super::expr::assignment(p);
    }
    if !p.at(Punct::RBracket) && !p.at_end() {
        p.error_at(
            ParseCode::MalformedArraySpecifier,
            "expected ']' after the array size",
            p.here(),
        );
        while !p.at(Punct::RBracket) && !p.at_boundary() {
            p.bump();
        }
    }
    p.expect_closing(Punct::RBracket, opened);
    p.close();
}

/// A type specifier: a word, or an inline `struct { … }`, plus any C-style
/// array suffix — the `[4]` of `float[4] x`.
pub(crate) fn type_spec(p: &mut Parser) {
    p.open(NodeKind::TypeSpec);
    if p.at_word("struct") {
        struct_spec(p);
    } else if p.at_ident() {
        p.bump();
    } else {
        p.error(ParseCode::ExpectedType, "expected a type");
    }
    while p.at(Punct::LBracket) {
        array_spec(p);
    }
    p.close();
}

/// `struct Name { … }` — the name is optional, the body is not.
fn struct_spec(p: &mut Parser) {
    p.open(NodeKind::StructSpec);
    p.bump();
    if p.at_ident() {
        p.bump_as(NodeKind::Name);
    }
    if p.at(Punct::LBrace) {
        field_list(p);
    } else {
        p.error(ParseCode::ExpectedToken, "expected '{' to open the struct body");
    }
    p.close();
}

/// The qualifier sequence in front of a declaration. Returns whether there was
/// one, which is what tells `invariant a;` from `a;`.
pub(crate) fn qualifier_list(p: &mut Parser) -> bool {
    if !at_qualifier(p) {
        return false;
    }
    p.open(NodeKind::QualifierList);
    while at_qualifier(p) {
        if p.at_word("layout") {
            layout_qualifier(p);
            continue;
        }
        if p.at_word("subroutine") {
            subroutine_qualifier(p);
            continue;
        }
        p.bump();
    }
    p.close();
    true
}

fn at_qualifier(p: &Parser) -> bool {
    p.at_ident() && is_qualifier(p.nth_text(0))
}

/// `layout(location = 0, binding = 1)`.
fn layout_qualifier(p: &mut Parser) {
    p.open(NodeKind::LayoutQualifier);
    p.bump();
    if p.at(Punct::LParen) {
        let opened = p.here();
        p.bump();
        while !p.at(Punct::RParen) && !p.at_end() {
            if p.eat(Punct::Comma) {
                continue;
            }
            if p.at_ident() {
                p.open(NodeKind::LayoutItem);
                p.bump();
                if p.eat(Punct::Eq) {
                    super::expr::assignment(p);
                }
                p.close();
                continue;
            }
            // `layout(0)` and friends: not legal, not worth losing the file over.
            let span = p.here();
            p.error_at(
                ParseCode::MalformedLayout,
                "a layout qualifier is a name, optionally followed by '= value'",
                span,
            );
            p.open(NodeKind::LayoutItem);
            p.bump();
            p.close();
        }
        p.expect_closing(Punct::RParen, opened);
    } else {
        p.error(ParseCode::ExpectedToken, "expected '(' after 'layout'");
    }
    p.close();
}

/// `subroutine` on its own, or `subroutine(TypeA, TypeB)`.
fn subroutine_qualifier(p: &mut Parser) {
    p.open(NodeKind::SubroutineQualifier);
    p.bump();
    if p.at(Punct::LParen) {
        let opened = p.here();
        p.bump();
        while !p.at(Punct::RParen) && !p.at_end() {
            if p.at_ident() || p.at(Punct::Comma) {
                p.bump();
                continue;
            }
            break;
        }
        p.expect_closing(Punct::RParen, opened);
    }
    p.close();
}

/// Whether a type specifier can start here.
pub(crate) fn at_type_start(p: &Parser) -> bool {
    p.at_ident()
}

/// Whether the statement at the cursor declares something rather than
/// evaluating something. Pure lookahead — see design/cst.md §4.
pub(crate) fn looks_like_declaration(p: &Parser) -> bool {
    if p.at_word("precision") || p.at_word("struct") {
        return true;
    }
    let mut i = 0usize;
    let mut qualifiers = 0usize;
    while p.nth_is_ident(i) && is_qualifier(p.nth_text(i)) {
        let word = p.nth_text(i);
        i += 1;
        qualifiers += 1;
        // `layout(…)` and `subroutine(…)` carry a group the scan steps over.
        if (word == "layout" || word == "subroutine") && p.nth_is(i, Punct::LParen) {
            i = p.past_group(i);
        }
    }
    if qualifiers > 0 {
        if p.nth_is(i, Punct::Semi) {
            return true;
        }
        if p.nth_is_ident(i) && (p.nth_is(i + 1, Punct::Comma) || p.nth_is(i + 1, Punct::Semi)) {
            return true;
        }
    }
    if p.nth_is_word(i, "struct") {
        return true;
    }
    if !p.nth_is_ident(i) {
        return false;
    }
    // A type, possibly `float[4]`, followed by a name. Two adjacent
    // identifiers never occur in a GLSL expression, so this is decisive.
    let mut j = i + 1;
    while p.nth_is(j, Punct::LBracket) {
        j = p.past_group(j);
    }
    p.nth_is_ident(j)
}
