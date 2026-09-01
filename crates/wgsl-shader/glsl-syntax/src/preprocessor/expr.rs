//! The `#if` constant-expression evaluator — GLSL 4.60 §3.3.
//!
//! §3.3 says the expression is an integer constant expression over the C
//! operators, and leaves the arithmetic to the implementation. glslang is the
//! oracle, so: everything is evaluated in **32-bit signed** arithmetic that
//! wraps rather than trapping, shifts by a count outside `0..32` are defined
//! away, and `INT_MIN / -1` is zero. There is no `?:` and no comma operator —
//! glslang's operator table has neither, and neither does the grammar in §3.3.
//!
//! `defined X` is resolved *before* macro expansion, over the tokens as
//! written. An identifier that survives expansion is an undefined macro and
//! stands for zero, which is what makes `#if UNSET_FLAG` false rather than an
//! error.

use analyzer_core::spans::ByteSpan;

use super::expand::{self, ExpandCtx};
use super::macros::MacroTable;
use super::{Origin, PpToken};
use crate::diagnostics::{PpCode, PpDiagnostic};
use crate::lexer::{Punct, TokenKind};

/// Precedence levels, lowest binding first. `UNARY` sits above every binary
/// operator so `-a * b` groups as `(-a) * b`.
const P_MIN: u8 = 0;
const P_UNARY: u8 = 11;

/// The binary operators, with their precedence.
fn binop(punct: Punct) -> Option<(u8, Punct)> {
    let precedence = match punct {
        Punct::OrOr => 1,
        Punct::AndAnd => 2,
        Punct::Pipe => 3,
        Punct::Caret => 4,
        Punct::Amp => 5,
        Punct::EqEq | Punct::Ne => 6,
        Punct::Lt | Punct::Gt | Punct::Le | Punct::Ge => 7,
        Punct::Shl | Punct::Shr => 8,
        Punct::Plus | Punct::Minus => 9,
        Punct::Star | Punct::Slash | Punct::Percent => 10,
        _ => return None,
    };
    Some((precedence, punct))
}

/// Evaluate an `#if`/`#elif` condition. Anything unreadable is false, with a
/// diagnostic — never a panic.
pub(crate) fn condition(
    tokens: &[PpToken],
    macros: &MacroTable,
    ctx: &ExpandCtx<'_>,
    span: ByteSpan,
    diagnostics: &mut Vec<PpDiagnostic>,
) -> bool {
    if tokens.is_empty() {
        diagnostics.push(PpDiagnostic::error(
            PpCode::BadConditionalExpression,
            "this directive needs a condition",
            span,
        ));
        return false;
    }
    let prepared = resolve_defined(tokens, macros, span, diagnostics);
    let expanded = expand::expand(macros, ctx, prepared, diagnostics);
    let mut eval = Eval { tokens: &expanded, pos: 0, span, failed: false, diagnostics };
    let value = eval.expr(P_MIN, false);
    eval.expect_end();
    // An expression we could not read is false, not "whatever we got as far as
    // reading" — `#if (1` must not take its branch.
    !eval.failed && value != 0
}

/// Evaluate the one or two constant expressions a `#line` directive carries.
///
/// `#line` allows a filename string in place of the second number under
/// `GL_GOOGLE_cpp_style_line_directive`; that form yields `None` for the source
/// string number rather than an error, because the directive is still valid.
pub(crate) fn line_arguments(
    tokens: &[PpToken],
    macros: &MacroTable,
    ctx: &ExpandCtx<'_>,
    span: ByteSpan,
    diagnostics: &mut Vec<PpDiagnostic>,
) -> (Option<i64>, Option<i64>) {
    let prepared = resolve_defined(tokens, macros, span, diagnostics);
    let expanded = expand::expand(macros, ctx, prepared, diagnostics);
    if expanded.is_empty() {
        diagnostics.push(PpDiagnostic::error(
            PpCode::MalformedLine,
            "'#line' must be followed by a line number",
            span,
        ));
        return (None, None);
    }
    let mut eval = Eval { tokens: &expanded, pos: 0, span, failed: false, diagnostics };
    let line = eval.expr(P_MIN, false);
    if eval.failed {
        eval.diagnostics.push(PpDiagnostic::error(
            PpCode::MalformedLine,
            "'#line' must be followed by a line number",
            span,
        ));
        return (None, None);
    }
    if eval.at_end() {
        return (Some(line), None);
    }
    if eval.peek().is_some_and(|t| t.kind == TokenKind::Str) {
        eval.pos += 1;
        eval.expect_end();
        return (Some(line), None);
    }
    let source_string = eval.expr(P_MIN, false);
    let failed = eval.failed;
    eval.expect_end();
    (Some(line), (!failed).then_some(source_string))
}

/// Replace every `defined X` and `defined ( X )` with `1` or `0`, before any
/// macro expansion can eat the name.
fn resolve_defined(
    tokens: &[PpToken],
    macros: &MacroTable,
    span: ByteSpan,
    diagnostics: &mut Vec<PpDiagnostic>,
) -> Vec<PpToken> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        if token.kind != TokenKind::Ident || token.text != "defined" {
            out.push(token.clone());
            i += 1;
            continue;
        }
        let leading_space = token.leading_space;
        let mut j = i + 1;
        let parenthesised = matches!(
            tokens.get(j).map(|t| t.kind),
            Some(TokenKind::Punct(Punct::LParen))
        );
        if parenthesised {
            j += 1;
        }
        let Some(name) = tokens.get(j).filter(|t| t.kind == TokenKind::Ident) else {
            diagnostics.push(PpDiagnostic::error(
                PpCode::BadConditionalExpression,
                "'defined' must be followed by a macro name",
                token.span,
            ));
            out.push(literal(0, token.span, leading_space));
            i = j;
            continue;
        };
        let defined = macros.is_defined(&name.text);
        j += 1;
        if parenthesised {
            match tokens.get(j) {
                Some(t) if t.kind == TokenKind::Punct(Punct::RParen) => j += 1,
                _ => diagnostics.push(PpDiagnostic::error(
                    PpCode::BadConditionalExpression,
                    "'defined(' is missing its ')'",
                    span,
                )),
            }
        }
        let covered = token.span.join(tokens[j - 1].span);
        out.push(literal(i64::from(defined), covered, leading_space));
        i = j;
    }
    out
}

fn literal(value: i64, span: ByteSpan, leading_space: bool) -> PpToken {
    PpToken {
        kind: TokenKind::Int,
        span,
        origin: Origin::MacroBody,
        leading_space,
        text: value.to_string(),
    }
}

struct Eval<'a> {
    tokens: &'a [PpToken],
    pos: usize,
    /// The directive, for diagnostics that have no better home.
    span: ByteSpan,
    failed: bool,
    diagnostics: &'a mut Vec<PpDiagnostic>,
}

impl Eval<'_> {
    fn peek(&self) -> Option<&PpToken> {
        self.tokens.get(self.pos)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn fail(&mut self, message: &str, span: ByteSpan) -> i32 {
        if !self.failed {
            self.failed = true;
            self.diagnostics.push(PpDiagnostic::error(
                PpCode::BadConditionalExpression,
                message.to_string(),
                span,
            ));
        }
        0
    }

    fn expect_end(&mut self) {
        if self.failed || self.at_end() {
            return;
        }
        let span = self.tokens[self.pos].span;
        self.diagnostics.push(PpDiagnostic::warning(
            PpCode::ExtraTokens,
            "tokens after the end of this expression are ignored",
            span,
        ));
    }

    /// Precedence climbing. `short_circuit` suppresses the arithmetic
    /// complaints of a subexpression whose value `&&`/`||` has already decided.
    fn expr(&mut self, min_precedence: u8, short_circuit: bool) -> i64 {
        let mut lhs = self.unary(short_circuit);
        while !self.failed {
            let Some((precedence, op)) = self.peek().and_then(|t| match t.kind {
                TokenKind::Punct(p) => binop(p),
                _ => None,
            }) else {
                break;
            };
            if precedence <= min_precedence {
                break;
            }
            self.pos += 1;
            let decided = (op == Punct::OrOr && lhs != 0) || (op == Punct::AndAnd && lhs == 0);
            let rhs = self.expr(precedence, short_circuit || decided);
            lhs = self.apply(op, lhs, rhs, short_circuit || decided);
        }
        lhs
    }

    fn unary(&mut self, short_circuit: bool) -> i64 {
        let Some(token) = self.peek().cloned() else {
            return i64::from(self.fail("this expression ends too early", self.span));
        };
        match token.kind {
            TokenKind::Int => {
                self.pos += 1;
                match parse_int(&token.text) {
                    Some(value) => value,
                    None => {
                        self.diagnostics.push(PpDiagnostic::error(
                            PpCode::BadConditionalLiteral,
                            format!("'{}' is not an integer this expression can use", token.text),
                            token.span,
                        ));
                        0
                    }
                }
            }
            // An identifier that survived expansion is an undefined macro,
            // and an undefined macro is zero.
            TokenKind::Ident => {
                self.pos += 1;
                0
            }
            TokenKind::Punct(Punct::LParen) => {
                self.pos += 1;
                let value = self.expr(P_MIN, short_circuit);
                match self.peek() {
                    Some(t) if t.kind == TokenKind::Punct(Punct::RParen) => self.pos += 1,
                    _ => {
                        self.fail("this expression is missing a ')'", token.span);
                    }
                }
                value
            }
            TokenKind::Punct(p @ (Punct::Plus | Punct::Minus | Punct::Tilde | Punct::Bang)) => {
                self.pos += 1;
                let value = self.expr(P_UNARY, short_circuit) as i32;
                i64::from(match p {
                    Punct::Plus => value,
                    Punct::Minus => value.wrapping_neg(),
                    Punct::Tilde => !value,
                    _ => i32::from(value == 0),
                })
            }
            _ => i64::from(self.fail(
                &format!("'{}' cannot appear in a preprocessor expression", token.text),
                token.span,
            )),
        }
    }

    fn apply(&mut self, op: Punct, lhs: i64, rhs: i64, short_circuit: bool) -> i64 {
        let (a, b) = (lhs as i32, rhs as i32);
        let value = match op {
            Punct::OrOr => i32::from(a != 0 || b != 0),
            Punct::AndAnd => i32::from(a != 0 && b != 0),
            Punct::Pipe => a | b,
            Punct::Caret => a ^ b,
            Punct::Amp => a & b,
            Punct::EqEq => i32::from(a == b),
            Punct::Ne => i32::from(a != b),
            Punct::Lt => i32::from(a < b),
            Punct::Gt => i32::from(a > b),
            Punct::Le => i32::from(a <= b),
            Punct::Ge => i32::from(a >= b),
            // A shift count outside 0..32 is undefined in C; glslang defines it
            // and so do we, so that no input can trap.
            Punct::Shl => {
                if !(0..32).contains(&b) {
                    0
                } else {
                    ((a as u32) << b) as i32
                }
            }
            Punct::Shr => {
                if !(0..32).contains(&b) {
                    if a < 0 { -1 } else { 0 }
                } else {
                    a >> b
                }
            }
            Punct::Plus => a.wrapping_add(b),
            Punct::Minus => a.wrapping_sub(b),
            Punct::Star => a.wrapping_mul(b),
            Punct::Slash | Punct::Percent => {
                let divisor = if b == 0 {
                    if !short_circuit {
                        self.diagnostics.push(PpDiagnostic::error(
                            PpCode::DivisionByZero,
                            "division by zero in a preprocessor expression",
                            self.span,
                        ));
                    }
                    1
                } else {
                    b
                };
                if op == Punct::Slash {
                    a.wrapping_div(divisor)
                } else {
                    a.wrapping_rem(divisor)
                }
            }
            _ => 0,
        };
        i64::from(value)
    }
}

/// Read an integer literal the way `#if` sees it: any base, any suffix, and
/// truncated to 32 bits because that is the width glslang evaluates in.
fn parse_int(text: &str) -> Option<i64> {
    let digits = text.trim_end_matches(['u', 'U', 'l', 'L']);
    if digits.is_empty() {
        return None;
    }
    let value = if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()?
    } else if digits.len() > 1 && digits.starts_with('0') {
        u64::from_str_radix(&digits[1..], 8).ok()?
    } else {
        digits.parse::<u64>().ok()?
    };
    Some(i64::from(value as u32 as i32))
}
