//! Macro expansion, with the provenance rules of decision 0003.
//!
//! The algorithm is the C one (Prosser's), which is what §3.3 defers to: a work
//! list of pending tokens, and a *hide set* per token naming the macros it must
//! never be re-expanded for. Hide sets rather than glslang's "this macro is
//! busy" flag, because a work list has no call stack to hang a busy flag on —
//! the observable behaviour is the same, and the corner cases (`#define A B` /
//! `#define B A`, a macro name arriving from a nested expansion) come out right
//! by construction.
//!
//! Provenance, which is the part this crate exists for:
//!
//! - a token substituted from a macro **body** is attributed to the whole
//!   invocation — the name token for an object-like macro, name through `)` for
//!   a function-like one. A hover or a rename lands on text the user can see,
//!   never inside a `#define` they are not looking at.
//! - a token that came through a macro **argument** keeps the span it was
//!   written at, so `FOO(myVar)` still finds `myVar`.
//! - a token synthesised by `##` or `#` belongs to no source text at all, and is
//!   attributed to the invocation like a body token.
//!
//! Nothing here can panic on any input. Runaway expansion is bounded by a step
//! budget and argument pre-expansion by a depth cap, both of which report a
//! diagnostic and pass the remaining tokens through unexpanded.

use std::collections::VecDeque;

use analyzer_core::spans::{ByteSpan, SpanTable};

use super::macros::{MacroKind, MacroTable};
use super::{Origin, PpToken};
use crate::diagnostics::{PpCode, PpDiagnostic};
use crate::lexer::{self, Punct, TokenKind};

/// Expansions allowed per segment before we call it a runaway.
const STEP_BUDGET: u32 = 20_000;

/// How deeply argument pre-expansion may nest before we call it a runaway.
const DEPTH_LIMIT: usize = 64;

/// What the dynamic predefines need in order to have a value.
pub(crate) struct ExpandCtx<'a> {
    pub source: &'a str,
    pub lines: &'a SpanTable,
    /// Added to a 1-based physical line number to get the logical one that
    /// `#line` asked for.
    pub line_offset: i64,
    /// The value of `__FILE__`.
    pub file_number: i64,
    /// The value of `__VERSION__`.
    pub version: i64,
}

impl ExpandCtx<'_> {
    /// The logical (post-`#line`) 1-based line the byte at `offset` sits on.
    fn line_at(&self, offset: u32) -> i64 {
        let physical = self.lines.offset_to_line_col(self.source, offset).line as i64 + 1;
        physical + self.line_offset
    }

    /// The integer a dynamic predefine stands for, if `name` is one.
    fn dynamic(&self, name: &str, span: ByteSpan) -> Option<i64> {
        match name {
            "__LINE__" => Some(self.line_at(span.start)),
            "__FILE__" => Some(self.file_number),
            "__VERSION__" => Some(self.version),
            _ => None,
        }
    }
}

/// A token on the work list: the token itself plus the macros it is painted
/// against.
#[derive(Debug, Clone)]
struct ExpToken {
    tok: PpToken,
    /// Macro ids this token may never be re-expanded for. Empty for almost
    /// every token, and an empty `Vec` costs no allocation.
    hide: Vec<u32>,
}

impl ExpToken {
    fn plain(tok: PpToken) -> Self {
        ExpToken { tok, hide: Vec::new() }
    }

    fn hidden_by(&self, id: u32) -> bool {
        self.hide.contains(&id)
    }
}

/// Expand a run of source tokens. The macro table is fixed for the whole run,
/// which is why the driver flushes a segment before every directive.
pub(crate) fn expand(
    macros: &MacroTable,
    ctx: &ExpandCtx<'_>,
    input: Vec<PpToken>,
    diagnostics: &mut Vec<PpDiagnostic>,
) -> Vec<PpToken> {
    let work: VecDeque<ExpToken> = input.into_iter().map(ExpToken::plain).collect();
    let mut budget = STEP_BUDGET;
    let out = run(macros, ctx, work, diagnostics, &mut budget, 0);
    out.into_iter().map(|t| t.tok).collect()
}

/// The work-list loop. Returns tokens that no longer expand.
fn run(
    macros: &MacroTable,
    ctx: &ExpandCtx<'_>,
    mut work: VecDeque<ExpToken>,
    diagnostics: &mut Vec<PpDiagnostic>,
    budget: &mut u32,
    depth: usize,
) -> Vec<ExpToken> {
    let mut out: Vec<ExpToken> = Vec::with_capacity(work.len());
    while let Some(current) = work.pop_front() {
        if current.tok.kind != TokenKind::Ident {
            out.push(current);
            continue;
        }
        // The three dynamic predefines win over any user definition, matching
        // glslang, which switches on them before it consults its macro table.
        if let Some(value) = ctx.dynamic(&current.tok.text, current.tok.span) {
            out.push(ExpToken {
                tok: PpToken {
                    kind: TokenKind::Int,
                    span: current.tok.span,
                    origin: Origin::MacroBody,
                    leading_space: current.tok.leading_space,
                    text: value.to_string(),
                },
                hide: current.hide,
            });
            continue;
        }
        let Some(def) = macros.lookup(&current.tok.text) else {
            out.push(current);
            continue;
        };
        if current.hidden_by(def.id) || def.kind == MacroKind::Dynamic {
            out.push(current);
            continue;
        }
        if *budget == 0 {
            // Report once, then drain: the remaining tokens are still valid
            // output, they are simply no longer expanded.
            if !diagnostics.iter().any(|d| d.code == PpCode::ExpansionLimit) {
                diagnostics.push(PpDiagnostic::error(
                    PpCode::ExpansionLimit,
                    "macro expansion did not terminate; the rest of this region is \
                     left unexpanded",
                    current.tok.span,
                ));
            }
            out.push(current);
            out.extend(work.drain(..));
            break;
        }

        match def.kind {
            // Already handled by `ctx.dynamic`; belt and braces, because a
            // panic here would take the whole server down.
            MacroKind::Dynamic => out.push(current),
            MacroKind::Object => {
                *budget -= 1;
                let site = current.tok.span;
                let mut hide = current.hide.clone();
                push_hide(&mut hide, def.id);
                let mut body =
                    substitute(def, &[], &[], site, current.tok.leading_space, diagnostics);
                for token in &mut body {
                    merge_hide(&mut token.hide, &hide);
                }
                push_front(&mut work, body);
            }
            MacroKind::Function => {
                if !matches!(
                    work.front().map(|t| t.tok.kind),
                    Some(TokenKind::Punct(Punct::LParen))
                ) {
                    // A function-like macro name that is not called is just a
                    // name. This is the rule that keeps `foo = MAX;` alone.
                    out.push(current);
                    continue;
                }
                *budget -= 1;
                let (args, close) = match collect_args(&mut work) {
                    Ok(pair) => pair,
                    Err(consumed) => {
                        diagnostics.push(PpDiagnostic::error(
                            PpCode::UnterminatedMacroInvocation,
                            format!("no closing ')' for the call to '{}'", def.name),
                            current.tok.span,
                        ));
                        out.push(current);
                        push_front(&mut work, consumed);
                        continue;
                    }
                };
                let args = fit_arity(args, def.params.len(), &def.name, &current, diagnostics);
                let site = current.tok.span.join(close.tok.span);
                // Prosser: the result is painted with what the name and the
                // closing paren agree on, plus this macro.
                let mut hide = intersect_hide(&current.hide, &close.hide);
                push_hide(&mut hide, def.id);
                let expanded_args = pre_expand_args(macros, ctx, &args, diagnostics, budget, depth);
                let mut body = substitute(
                    def,
                    &args,
                    &expanded_args,
                    site,
                    current.tok.leading_space,
                    diagnostics,
                );
                for token in &mut body {
                    merge_hide(&mut token.hide, &hide);
                }
                push_front(&mut work, body);
            }
        }
    }
    out
}

/// Build the replacement list for one invocation.
///
/// `raw` holds the arguments as written; `expanded` holds them macro-expanded.
/// §3.3 wants the raw form next to `#` and `##` and the expanded form
/// everywhere else, which is the only reason both exist.
fn substitute(
    def: &super::macros::MacroDef,
    raw: &[Vec<ExpToken>],
    expanded: &[Vec<ExpToken>],
    site: ByteSpan,
    leading_space: bool,
    diagnostics: &mut Vec<PpDiagnostic>,
) -> Vec<ExpToken> {
    let param = |token: &PpToken| -> Option<usize> {
        if token.kind != TokenKind::Ident {
            return None;
        }
        def.params.iter().position(|p| *p == token.text)
    };

    let mut result: Vec<ExpToken> = Vec::with_capacity(def.body.len());
    let mut i = 0;
    while i < def.body.len() {
        let token = &def.body[i];
        // `# param` — stringify. Only function-like macros have parameters, so
        // only they can stringify.
        if token.kind == TokenKind::Punct(Punct::Hash) && def.kind == MacroKind::Function {
            match def.body.get(i + 1).and_then(param) {
                Some(index) => {
                    result.push(ExpToken::plain(stringify(
                        raw.get(index).map(Vec::as_slice).unwrap_or(&[]),
                        site,
                        token.leading_space,
                    )));
                    i += 2;
                    continue;
                }
                None => {
                    diagnostics.push(PpDiagnostic::error(
                        PpCode::BadStringify,
                        format!(
                            "'#' in the body of '{}' is not followed by one of its \
                             parameters",
                            def.name
                        ),
                        def.span,
                    ));
                }
            }
        }
        // `##` — paste what we last produced onto what comes next.
        if token.kind == TokenKind::Punct(Punct::HashHash) {
            let Some(next) = def.body.get(i + 1) else {
                diagnostics.push(PpDiagnostic::error(
                    PpCode::BadTokenPaste,
                    format!("'##' ends the body of '{}' with nothing to paste", def.name),
                    def.span,
                ));
                i += 1;
                continue;
            };
            let mut rhs: Vec<ExpToken> = match param(next) {
                Some(index) => raw
                    .get(index)
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                    .iter()
                    .map(|t| ExpToken { tok: from_argument(&t.tok), hide: t.hide.clone() })
                    .collect(),
                None => vec![ExpToken::plain(from_body(next, site))],
            };
            match (result.pop(), rhs.is_empty()) {
                (None, _) => {
                    diagnostics.push(PpDiagnostic::error(
                        PpCode::BadTokenPaste,
                        format!("'##' starts the body of '{}' with nothing to paste", def.name),
                        def.span,
                    ));
                    result.append(&mut rhs);
                }
                // Pasting onto an empty argument leaves the left side alone.
                (Some(left), true) => result.push(left),
                (Some(left), false) => {
                    let right = rhs.remove(0);
                    match paste(&left.tok, &right.tok, site) {
                        Some(pasted) => result.push(ExpToken {
                            tok: pasted,
                            hide: intersect_hide(&left.hide, &right.hide),
                        }),
                        None => {
                            diagnostics.push(PpDiagnostic::warning(
                                PpCode::BadTokenPaste,
                                format!(
                                    "'{}{}' is not a single token; '##' left both in place",
                                    left.tok.text, right.tok.text
                                ),
                                site,
                            ));
                            result.push(left);
                            result.push(right);
                        }
                    }
                    result.append(&mut rhs);
                }
            }
            i += 2;
            continue;
        }
        // A parameter. Next to `##` it goes in raw; otherwise pre-expanded.
        if let Some(index) = param(token) {
            let adjacent_paste = def
                .body
                .get(i + 1)
                .is_some_and(|t| t.kind == TokenKind::Punct(Punct::HashHash));
            let source = if adjacent_paste { raw } else { expanded };
            let arg = source.get(index).map(Vec::as_slice).unwrap_or(&[]);
            let start = result.len();
            result.extend(arg.iter().map(|t| ExpToken {
                tok: from_argument(&t.tok),
                hide: t.hide.clone(),
            }));
            // The body decides the spacing at the seam, not the call site.
            if let Some(first) = result.get_mut(start) {
                first.tok.leading_space = token.leading_space;
            }
            i += 1;
            continue;
        }
        result.push(ExpToken::plain(from_body(token, site)));
        i += 1;
    }
    // The invocation's own leading space survives its replacement.
    if let Some(first) = result.first_mut() {
        first.tok.leading_space = leading_space;
    }
    result
}

/// Macro-expand each argument once, before it is substituted.
fn pre_expand_args(
    macros: &MacroTable,
    ctx: &ExpandCtx<'_>,
    args: &[Vec<ExpToken>],
    diagnostics: &mut Vec<PpDiagnostic>,
    budget: &mut u32,
    depth: usize,
) -> Vec<Vec<ExpToken>> {
    if depth >= DEPTH_LIMIT {
        // Deep enough that we stop pre-expanding rather than risk the stack.
        // The step budget has almost certainly fired already.
        return args.to_vec();
    }
    args.iter()
        .map(|arg| {
            let work: VecDeque<ExpToken> = arg.iter().cloned().collect();
            run(macros, ctx, work, diagnostics, budget, depth + 1)
        })
        .collect()
}

/// Take `( a, b )` off the front of the work list. `Err` carries back
/// everything consumed so the caller can put it where it was.
type Args = (Vec<Vec<ExpToken>>, ExpToken);

fn collect_args(work: &mut VecDeque<ExpToken>) -> Result<Args, Vec<ExpToken>> {
    let open = work.pop_front().expect("caller checked for '('");
    let mut consumed = vec![open];
    let mut args: Vec<Vec<ExpToken>> = vec![Vec::new()];
    let mut depth = 0usize;
    loop {
        let Some(token) = work.pop_front() else {
            return Err(consumed);
        };
        consumed.push(token.clone());
        match token.tok.kind {
            TokenKind::Punct(Punct::LParen) => {
                depth += 1;
                args.last_mut().expect("never empty").push(token);
            }
            TokenKind::Punct(Punct::RParen) if depth == 0 => return Ok((args, token)),
            TokenKind::Punct(Punct::RParen) => {
                depth -= 1;
                args.last_mut().expect("never empty").push(token);
            }
            TokenKind::Punct(Punct::Comma) if depth == 0 => args.push(Vec::new()),
            _ => args.last_mut().expect("never empty").push(token),
        }
    }
}

/// Reconcile what was passed with what the macro declares, reporting the
/// mismatch and then padding or truncating so substitution cannot index out of
/// range.
fn fit_arity(
    mut args: Vec<Vec<ExpToken>>,
    wanted: usize,
    name: &str,
    call: &ExpToken,
    diagnostics: &mut Vec<PpDiagnostic>,
) -> Vec<Vec<ExpToken>> {
    // `F()` calling a macro that takes nothing is zero arguments, not one empty
    // one. `F()` calling a macro that takes one is one empty argument.
    if wanted == 0 && args.len() == 1 && args[0].is_empty() {
        args.clear();
    }
    if args.len() != wanted {
        diagnostics.push(PpDiagnostic::error(
            PpCode::MacroArgumentCount,
            format!(
                "'{name}' takes {wanted} argument{}, but {} {} given",
                if wanted == 1 { "" } else { "s" },
                args.len(),
                if args.len() == 1 { "was" } else { "were" }
            ),
            call.tok.span,
        ));
        args.resize(wanted, Vec::new());
    }
    args
}

/// A token substituted from a macro body: attributed to the invocation.
fn from_body(token: &PpToken, site: ByteSpan) -> PpToken {
    PpToken {
        kind: token.kind,
        span: site,
        origin: Origin::MacroBody,
        leading_space: token.leading_space,
        text: token.text.clone(),
    }
}

/// A token that arrived through an argument: keeps the span it was written at.
fn from_argument(token: &PpToken) -> PpToken {
    PpToken {
        origin: match token.origin {
            Origin::Source => Origin::MacroArg,
            other => other,
        },
        ..token.clone()
    }
}

/// `# param`: the argument's spelling, as a string literal.
fn stringify(arg: &[ExpToken], site: ByteSpan, leading_space: bool) -> PpToken {
    let mut text = String::from("\"");
    for (i, token) in arg.iter().enumerate() {
        if i > 0 && token.tok.leading_space {
            text.push(' ');
        }
        for c in token.tok.text.chars() {
            if c == '"' || c == '\\' {
                text.push('\\');
            }
            text.push(c);
        }
    }
    text.push('"');
    PpToken { kind: TokenKind::Str, span: site, origin: Origin::MacroBody, leading_space, text }
}

/// `a ## b`: concatenate the spellings and re-lex. `None` when the result is
/// not exactly one token, which §3.3 leaves undefined and we merely report.
fn paste(left: &PpToken, right: &PpToken, site: ByteSpan) -> Option<PpToken> {
    let text = format!("{}{}", left.text, right.text);
    let mut significant = lexer::tokenize(&text).into_iter().filter(|t| !t.kind.is_trivia());
    let first = significant.next()?;
    if significant.next().is_some() || first.span.end as usize != text.len() {
        return None;
    }
    Some(PpToken {
        kind: first.kind,
        span: site,
        origin: Origin::MacroBody,
        leading_space: left.leading_space,
        text,
    })
}

fn push_front(work: &mut VecDeque<ExpToken>, tokens: Vec<ExpToken>) {
    for token in tokens.into_iter().rev() {
        work.push_front(token);
    }
}

fn push_hide(hide: &mut Vec<u32>, id: u32) {
    if !hide.contains(&id) {
        hide.push(id);
    }
}

fn merge_hide(hide: &mut Vec<u32>, extra: &[u32]) {
    for id in extra {
        push_hide(hide, *id);
    }
}

fn intersect_hide(a: &[u32], b: &[u32]) -> Vec<u32> {
    a.iter().copied().filter(|id| b.contains(id)).collect()
}
