//! Recursive-descent parser over the logical lines produced by [`crate::lexer`].
//!
//! The parser's job is to classify each [`LogicalLine`] into an [`ast::Item`]
//! while tracking three structural bits of state: which rule a recipe line
//! belongs to, whether we're inside a `define ... endef` block, and how
//! many conditionals are open. Errors are collected as
//! [`MakeDiagnostic`]s rather than aborting.

use crate::ast::{
    AssignOp, Assignment, Conditional, ConditionalKind, Define, Directive, DirectiveKind, File,
    Identifier, Include, Item, RecipeLine, Rule,
};
use crate::diagnostics::{DiagnosticCode, MakeDiagnostic};
use crate::lexer::{LineKind, LogicalLine};
use crate::spans::ByteSpan;

pub struct Parser<'a> {
    source: &'a str,
    lines: &'a [LogicalLine],
    pos: usize,
    diagnostics: Vec<MakeDiagnostic>,
    /// Set of target names previously declared phony via `.PHONY: foo bar`.
    phony: Vec<String>,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str, lines: &'a [LogicalLine]) -> Self {
        Parser { source, lines, pos: 0, diagnostics: Vec::new(), phony: Vec::new() }
    }

    pub fn into_diagnostics(self) -> Vec<MakeDiagnostic> {
        self.diagnostics
    }

    pub fn parse_file(&mut self) -> File {
        let start = self
            .lines
            .first()
            .map(|l| l.span.start)
            .unwrap_or(0);
        let end = self
            .lines
            .last()
            .map(|l| l.span.end)
            .unwrap_or(0);
        let items = self.parse_items(None);
        File { items, span: ByteSpan::new(start, end) }
    }

    /// Parse items until EOF or until the line text starts with one of
    /// the terminators (used to recurse into conditional branches).
    fn parse_items(&mut self, terminators: Option<&[&str]>) -> Vec<Item> {
        let mut out = Vec::new();
        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];
            match line.kind {
                LineKind::Blank | LineKind::Comment => {
                    self.pos += 1;
                    continue;
                }
                LineKind::Recipe => {
                    // Recipe line outside of a rule — try to attach it to
                    // the previous rule; if there isn't one, emit MAKE002.
                    if let Some(Item::Rule(r)) = out.last_mut() {
                        let rl = self.recipe_line_from(line);
                        r.recipe_lines.push(rl);
                        r.span.end = line.span.end;
                        self.pos += 1;
                        continue;
                    }
                    self.diagnostics.push(MakeDiagnostic::error(
                        DiagnosticCode::RecipeOutsideRule,
                        "recipe line appears outside of any target rule",
                        line.span,
                    ));
                    self.pos += 1;
                    continue;
                }
                LineKind::RecipeWithSpaces => {
                    // If the previous item was a rule, this looks like a
                    // recipe line that used spaces instead of a tab.
                    // Otherwise treat it as a statement.
                    if let Some(Item::Rule(_)) = out.last() {
                        self.diagnostics.push(MakeDiagnostic::error(
                            DiagnosticCode::RecipeUsesSpaces,
                            "recipe line uses spaces where a tab is required",
                            line.span,
                        ));
                        if let Some(Item::Rule(r)) = out.last_mut() {
                            let rl = self.recipe_line_from(line);
                            r.recipe_lines.push(rl);
                            r.span.end = line.span.end;
                        }
                        self.pos += 1;
                        continue;
                    }
                    // Fall through to Statement handling.
                }
                LineKind::Statement => {}
            }

            let text = line.text.trim_start();
            if let Some(terms) = terminators {
                if terms.iter().any(|t| starts_keyword(text, t)) {
                    return out;
                }
            }

            if let Some(item) = self.parse_statement(line) {
                // Track .PHONY targets so subsequent rules can be
                // marked is_phony in the outline.
                if let Item::Rule(r) = &item {
                    if let Some(t) = r.targets.first() {
                        if t.name == ".PHONY" {
                            for p in &r.prerequisites {
                                self.phony.push(p.name.clone());
                            }
                        }
                    }
                }
                out.push(item);
            } else {
                // parse_statement always advances; this branch is for
                // statements that produced nothing (e.g. stray tokens).
                self.pos += 1;
            }
        }
        out
    }

    fn parse_statement(&mut self, line: &LogicalLine) -> Option<Item> {
        let text = line.text.trim_start();

        // Directives first.
        if let Some(kind) = match_conditional_open(text) {
            return Some(Item::Conditional(self.parse_conditional(line, kind)));
        }
        if starts_keyword(text, "define") {
            return Some(Item::Define(self.parse_define(line)));
        }
        if starts_keyword(text, "include") || starts_keyword(text, "-include")
            || starts_keyword(text, "sinclude")
        {
            return Some(Item::Include(self.parse_include(line)));
        }
        if let Some(k) = match_directive(text) {
            self.pos += 1;
            return Some(Item::Directive(self.parse_directive(line, k)));
        }

        // Assignment vs rule: find the first unquoted `:`, `=`, `:=`,
        // `::=`, `?=`, `+=`, `!=` at the top level (ignoring `$(...)`
        // and `${...}`).
        if let Some(assign) = self.try_parse_assignment(line) {
            self.pos += 1;
            return Some(Item::Assignment(assign));
        }
        if let Some(rule) = self.try_parse_rule(line) {
            return Some(Item::Rule(rule));
        }

        // Unrecognised — skip one line and keep going.
        self.pos += 1;
        None
    }

    fn parse_conditional(&mut self, open: &LogicalLine, kind: ConditionalKind) -> Conditional {
        let text = open.text.trim_start();
        let keyword_len = kind.as_str().len();
        let condition = text[keyword_len..].trim().to_string();
        let name_span = keyword_span(self.source, open.content_start, kind.as_str());
        let mut span = open.span;
        self.pos += 1;

        let then_branch = self.parse_items(Some(&["else", "endif"]));
        let mut else_branch = Vec::new();

        if self.pos < self.lines.len() {
            let l = &self.lines[self.pos];
            let t = l.text.trim_start();
            if starts_keyword(t, "else") {
                self.pos += 1;
                // `else ifeq ...` chains into a nested conditional.
                let rest = t["else".len()..].trim_start();
                if let Some(inner) = match_conditional_open(rest) {
                    // Re-synthesize a LogicalLine covering the same span
                    // but with just the inner `ifeq …` text; we parse it
                    // as a nested conditional then skip to its `endif`.
                    let synthetic = LogicalLine {
                        text: rest.to_string(),
                        span: l.span,
                        content_start: l.content_start + ("else".len() as u32) + 1,
                        kind: LineKind::Statement,
                    };
                    let nested = self.parse_conditional(&synthetic, inner);
                    else_branch.push(Item::Conditional(nested));
                } else {
                    else_branch = self.parse_items(Some(&["endif"]));
                }
            }
        }

        // Consume the terminating endif, if any.
        if self.pos < self.lines.len() {
            let l = &self.lines[self.pos];
            if starts_keyword(l.text.trim_start(), "endif") {
                span.end = l.span.end;
                self.pos += 1;
            } else {
                self.diagnostics.push(MakeDiagnostic::error(
                    DiagnosticCode::ConditionalNotClosed,
                    format!("`{}` is not closed — expected `endif`", kind.as_str()),
                    open.span,
                ));
            }
        } else {
            self.diagnostics.push(MakeDiagnostic::error(
                DiagnosticCode::ConditionalNotClosed,
                format!("`{}` is not closed — expected `endif`", kind.as_str()),
                open.span,
            ));
        }

        Conditional {
            kind,
            condition,
            then_branch,
            else_branch,
            span,
            name_span,
        }
    }

    fn parse_define(&mut self, open: &LogicalLine) -> Define {
        let text = open.text.trim_start();
        // Form: `define NAME [op]\n body lines \n endef`
        let after_kw = text["define".len()..].trim_start();
        // Detect trailing assignment operator.
        let (name_str, op) = split_define_header(after_kw);
        let name_offset = self.source[open.content_start as usize..]
            .find(&name_str)
            .map(|o| open.content_start as usize + o)
            .unwrap_or(open.content_start as usize);
        let name_span = ByteSpan::from_usize(name_offset, name_offset + name_str.len());
        let name = Identifier { name: name_str, span: name_span };

        let header_start = open.span.start;
        let mut body = String::new();
        let mut closed_end = open.span.end;
        self.pos += 1;

        while self.pos < self.lines.len() {
            let l = &self.lines[self.pos];
            let t = l.text.trim_start();
            if starts_keyword(t, "endef") {
                closed_end = l.span.end;
                self.pos += 1;
                return Define {
                    name,
                    op,
                    body,
                    span: ByteSpan::new(header_start, closed_end),
                    name_span,
                };
            }
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&self.source[l.span.start as usize..l.span.end as usize]
                .trim_end_matches('\n')
                .trim_end_matches('\r'));
            self.pos += 1;
        }

        // Ran off the end without finding endef.
        self.diagnostics.push(MakeDiagnostic::error(
            DiagnosticCode::DefineNotClosed,
            "`define` block is not closed — expected `endef`",
            open.span,
        ));
        Define {
            name,
            op,
            body,
            span: ByteSpan::new(header_start, closed_end),
            name_span,
        }
    }

    fn parse_include(&mut self, line: &LogicalLine) -> Include {
        let text = line.text.trim_start();
        let (kw_len, optional) = if starts_keyword(text, "-include") {
            (8, true)
        } else if starts_keyword(text, "sinclude") {
            (8, true)
        } else {
            (7, false)
        };
        let rest = text[kw_len..].trim();
        let paths: Vec<String> = rest
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        self.pos += 1;
        Include { paths, optional, span: line.span }
    }

    fn parse_directive(&mut self, line: &LogicalLine, kind: DirectiveKind) -> Directive {
        let text = line.text.trim_start();
        let kw = kind.as_str();
        let arguments = text[kw.len()..].trim().to_string();
        let name_span = keyword_span(self.source, line.content_start, kw);
        Directive { kind, arguments, span: line.span, name_span }
    }

    fn try_parse_assignment(&mut self, line: &LogicalLine) -> Option<Assignment> {
        let text = line.text.trim_start();
        let (op_idx, op_len, op) = find_assignment_operator(text)?;

        // Disambiguate `=`, `:=`, etc. from a rule header: if a `:`
        // appears *before* the `=` and is not part of `:=` / `::=`, this
        // is a rule.
        if let Some(colon) = find_toplevel_colon(text) {
            if colon < op_idx && !is_assign_colon(text, colon) {
                return None;
            }
        }

        let name = text[..op_idx].trim_end();
        if name.is_empty() {
            return None;
        }
        // Validate that the name does not contain whitespace — a true
        // rule like `foo bar: baz` has a space in the "LHS" but no `=`
        // operator, and we've already filtered those above. But an
        // accidental `foo bar = baz` is legal in GNU Make and produces
        // a warning elsewhere; we still parse it as an assignment.
        let name_offset = self.source[line.content_start as usize..]
            .find(name)
            .map(|o| line.content_start as usize + o)
            .unwrap_or(line.content_start as usize);
        let name_span = ByteSpan::from_usize(name_offset, name_offset + name.len());

        let value = text[op_idx + op_len..].trim().to_string();

        Some(Assignment {
            name: Identifier { name: name.to_string(), span: name_span },
            op,
            value,
            span: line.span,
        })
    }

    fn try_parse_rule(&mut self, line: &LogicalLine) -> Option<Rule> {
        let text = line.text.trim_start();
        let colon = find_toplevel_colon(text)?;
        // Double-colon rule?
        let (colon_end, is_double) = if text[colon + 1..].starts_with(':') {
            (colon + 2, true)
        } else {
            (colon + 1, false)
        };

        let targets_str = text[..colon].trim();
        let prereqs_str = text[colon_end..].trim();

        if targets_str.is_empty() {
            return None;
        }

        let targets: Vec<Identifier> =
            split_into_identifiers(self.source, line.content_start as usize, targets_str);
        let prereqs_start = line.content_start as usize
            + text[..colon_end].len();
        let prerequisites: Vec<Identifier> =
            split_into_identifiers(self.source, prereqs_start, prereqs_str);

        let is_pattern = targets.iter().any(|t| t.name.contains('%'));
        let is_phony = targets
            .first()
            .map(|t| self.phony.iter().any(|p| p == &t.name))
            .unwrap_or(false);

        let name_span = targets
            .first()
            .map(|t| t.span)
            .unwrap_or_else(|| ByteSpan::new(line.content_start, line.content_start));

        let mut rule = Rule {
            targets,
            prerequisites,
            is_double_colon: is_double,
            is_pattern,
            is_phony,
            recipe_lines: Vec::new(),
            span: line.span,
            name_span,
        };

        // Collect recipe lines from subsequent lines.
        self.pos += 1;
        while self.pos < self.lines.len() {
            let l = &self.lines[self.pos];
            match l.kind {
                LineKind::Recipe => {
                    rule.recipe_lines.push(self.recipe_line_from(l));
                    rule.span.end = l.span.end;
                    self.pos += 1;
                }
                LineKind::Blank | LineKind::Comment => {
                    // Blank / comment lines between recipe lines are
                    // allowed; they don't belong to the rule but they
                    // also don't terminate it — as long as the next
                    // non-blank line is a recipe.
                    let mut lookahead = self.pos + 1;
                    let mut found_recipe = false;
                    while lookahead < self.lines.len() {
                        match self.lines[lookahead].kind {
                            LineKind::Blank | LineKind::Comment => lookahead += 1,
                            LineKind::Recipe => {
                                found_recipe = true;
                                break;
                            }
                            _ => break,
                        }
                    }
                    if found_recipe {
                        // Skip over the blank/comment lines and continue
                        // gathering recipes.
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }

        Some(rule)
    }

    fn recipe_line_from(&self, line: &LogicalLine) -> RecipeLine {
        RecipeLine { text: line.text.clone(), span: line.span }
    }
}

fn match_conditional_open(text: &str) -> Option<ConditionalKind> {
    if starts_keyword(text, "ifeq") {
        Some(ConditionalKind::Ifeq)
    } else if starts_keyword(text, "ifneq") {
        Some(ConditionalKind::Ifneq)
    } else if starts_keyword(text, "ifdef") {
        Some(ConditionalKind::Ifdef)
    } else if starts_keyword(text, "ifndef") {
        Some(ConditionalKind::Ifndef)
    } else {
        None
    }
}

fn match_directive(text: &str) -> Option<DirectiveKind> {
    if starts_keyword(text, "export") {
        Some(DirectiveKind::Export)
    } else if starts_keyword(text, "unexport") {
        Some(DirectiveKind::Unexport)
    } else if starts_keyword(text, "override") {
        Some(DirectiveKind::Override)
    } else if starts_keyword(text, "private") {
        Some(DirectiveKind::Private)
    } else if starts_keyword(text, "undefine") {
        Some(DirectiveKind::Undefine)
    } else if starts_keyword(text, "vpath") {
        Some(DirectiveKind::VPath)
    } else {
        None
    }
}

fn starts_keyword(text: &str, kw: &str) -> bool {
    if !text.starts_with(kw) {
        return false;
    }
    match text[kw.len()..].chars().next() {
        None => true,
        Some(c) => c.is_whitespace() || c == '(' || c == ':',
    }
}

fn keyword_span(source: &str, content_start: u32, kw: &str) -> ByteSpan {
    let from = content_start as usize;
    let end = (from + kw.len()).min(source.len());
    ByteSpan::from_usize(from, end)
}

fn split_define_header(rest: &str) -> (String, Option<AssignOp>) {
    // `NAME` or `NAME =` / `NAME :=` / etc.
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }
    // Find the first whitespace.
    let (name, tail) = match trimmed.find(char::is_whitespace) {
        Some(i) => (&trimmed[..i], trimmed[i..].trim_start()),
        None => (trimmed, ""),
    };
    let op = if tail.is_empty() {
        None
    } else if tail == "=" {
        Some(AssignOp::Recursive)
    } else if tail == ":=" {
        Some(AssignOp::Simple)
    } else if tail == "::=" {
        Some(AssignOp::Immediate)
    } else if tail == "?=" {
        Some(AssignOp::Conditional)
    } else if tail == "+=" {
        Some(AssignOp::Append)
    } else if tail == "!=" {
        Some(AssignOp::Shell)
    } else {
        None
    };
    (name.to_string(), op)
}

/// Return `(index_of_operator_start, operator_length, op_kind)` for the
/// first top-level assignment operator in `text`, skipping `$( … )` and
/// `${ … }` function calls. Returns `None` if no operator is found at
/// the top level. A colon is only treated as an assignment when it is
/// part of `:=` or `::=`.
fn find_assignment_operator(text: &str) -> Option<(usize, usize, AssignOp)> {
    let bytes = text.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'(' | b'{' => {
                // Only start a group if preceded by a `$`.
                if i > 0 && bytes[i - 1] == b'$' {
                    depth += 1;
                }
            }
            b')' | b'}' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ if depth == 0 => {
                // Try operators in longest-first order.
                if bytes[i..].starts_with(b"::=") {
                    return Some((i, 3, AssignOp::Immediate));
                }
                if bytes[i..].starts_with(b":=") {
                    return Some((i, 2, AssignOp::Simple));
                }
                if bytes[i..].starts_with(b"?=") {
                    return Some((i, 2, AssignOp::Conditional));
                }
                if bytes[i..].starts_with(b"+=") {
                    return Some((i, 2, AssignOp::Append));
                }
                if bytes[i..].starts_with(b"!=") {
                    return Some((i, 2, AssignOp::Shell));
                }
                if b == b'=' {
                    return Some((i, 1, AssignOp::Recursive));
                }
                // A bare `:` is not an assignment — leave it for the
                // rule-parser.
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_toplevel_colon(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'(' | b'{' if i > 0 && bytes[i - 1] == b'$' => depth += 1,
            b')' | b'}' if depth > 0 => depth -= 1,
            b':' if depth == 0 => {
                // Skip `:=` / `::=` which are assignment operators.
                if bytes.get(i + 1) == Some(&b'=') {
                    // :=
                    i += 2;
                    continue;
                }
                if bytes.get(i + 1) == Some(&b':') && bytes.get(i + 2) == Some(&b'=') {
                    // ::=
                    i += 3;
                    continue;
                }
                // Windows-style drive letters like `C:\path` inside
                // target names are rare but should be tolerated. Skip
                // a colon that is immediately followed by `\` or `/`
                // *and* preceded by a single ASCII letter at the start
                // of the current token.
                if let Some(next) = bytes.get(i + 1) {
                    if (*next == b'\\' || *next == b'/') && i > 0 {
                        let prev = bytes[i - 1];
                        if prev.is_ascii_alphabetic() {
                            // Only treat as drive letter if the letter
                            // is at the start of the token.
                            let token_start = bytes[..i - 1]
                                .iter()
                                .rposition(|c| c.is_ascii_whitespace())
                                .map(|p| p + 1)
                                .unwrap_or(0);
                            if token_start == i - 1 {
                                i += 1;
                                continue;
                            }
                        }
                    }
                }
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn is_assign_colon(text: &str, colon: usize) -> bool {
    let bytes = text.as_bytes();
    // `::=` or `:=`.
    if bytes.get(colon + 1) == Some(&b'=') {
        return true;
    }
    if bytes.get(colon + 1) == Some(&b':') && bytes.get(colon + 2) == Some(&b'=') {
        return true;
    }
    false
}

/// Split a whitespace-separated target / prereq list into identifiers
/// with spans anchored at the absolute byte offset of each identifier
/// within the original source.
fn split_into_identifiers(source: &str, base: usize, list: &str) -> Vec<Identifier> {
    let mut out = Vec::new();
    if list.is_empty() {
        return out;
    }
    // Locate `list` within the source starting at or after `base`.
    let anchor = source[base..]
        .find(list)
        .map(|o| base + o)
        .unwrap_or(base);
    let bytes = list.as_bytes();
    let mut i = 0;
    let mut depth: i32 = 0;
    let mut token_start: Option<usize> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'(' | b'{' if i > 0 && bytes[i - 1] == b'$' => {
                depth += 1;
                if token_start.is_none() {
                    token_start = Some(i.saturating_sub(1));
                }
            }
            b')' | b'}' if depth > 0 => depth -= 1,
            c if c.is_ascii_whitespace() && depth == 0 => {
                if let Some(ts) = token_start.take() {
                    let name = list[ts..i].to_string();
                    out.push(Identifier {
                        name,
                        span: ByteSpan::from_usize(anchor + ts, anchor + i),
                    });
                }
            }
            _ => {
                if token_start.is_none() {
                    token_start = Some(i);
                }
            }
        }
        i += 1;
    }
    if let Some(ts) = token_start {
        out.push(Identifier {
            name: list[ts..].to_string(),
            span: ByteSpan::from_usize(anchor + ts, anchor + list.len()),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse(source: &str) -> (File, Vec<MakeDiagnostic>) {
        let lines = lex(source);
        let mut p = Parser::new(source, &lines);
        let f = p.parse_file();
        (f, p.into_diagnostics())
    }

    #[test]
    fn parses_simple_rule() {
        let src = "all: main.o util.o\n\tgcc -o all main.o util.o\n";
        let (file, diags) = parse(src);
        assert!(diags.is_empty(), "diagnostics: {:?}", diags);
        assert_eq!(file.items.len(), 1);
        let Item::Rule(r) = &file.items[0] else { panic!() };
        assert_eq!(r.targets[0].name, "all");
        assert_eq!(r.prerequisites.len(), 2);
        assert_eq!(r.recipe_lines.len(), 1);
    }

    #[test]
    fn parses_simple_assignment() {
        let src = "CFLAGS = -O2 -Wall\n";
        let (file, diags) = parse(src);
        assert!(diags.is_empty());
        assert_eq!(file.items.len(), 1);
        let Item::Assignment(a) = &file.items[0] else { panic!() };
        assert_eq!(a.name.name, "CFLAGS");
        assert_eq!(a.op, AssignOp::Recursive);
        assert_eq!(a.value, "-O2 -Wall");
    }

    #[test]
    fn parses_all_assign_ops() {
        let src = "A = 1\nB := 2\nC ::= 3\nD ?= 4\nE += 5\nF != date\n";
        let (file, _) = parse(src);
        assert_eq!(file.items.len(), 6);
        let ops: Vec<_> = file
            .items
            .iter()
            .map(|i| match i {
                Item::Assignment(a) => a.op,
                _ => panic!("not an assignment"),
            })
            .collect();
        assert_eq!(
            ops,
            vec![
                AssignOp::Recursive,
                AssignOp::Simple,
                AssignOp::Immediate,
                AssignOp::Conditional,
                AssignOp::Append,
                AssignOp::Shell,
            ]
        );
    }

    #[test]
    fn parses_pattern_rule() {
        let src = "%.o: %.c\n\tgcc -c $< -o $@\n";
        let (file, _) = parse(src);
        let Item::Rule(r) = &file.items[0] else { panic!() };
        assert!(r.is_pattern);
    }

    #[test]
    fn parses_phony_from_prior_declaration() {
        let src = ".PHONY: clean\nclean:\n\trm -rf build\n";
        let (file, _) = parse(src);
        // Two rules.
        assert_eq!(file.items.len(), 2);
        let Item::Rule(r2) = &file.items[1] else { panic!() };
        assert!(r2.is_phony);
    }

    #[test]
    fn parses_define_block() {
        let src = "define GREETING\nhello\nworld\nendef\n";
        let (file, diags) = parse(src);
        assert!(diags.is_empty());
        let Item::Define(d) = &file.items[0] else { panic!() };
        assert_eq!(d.name.name, "GREETING");
        assert!(d.body.contains("hello"));
        assert!(d.body.contains("world"));
    }

    #[test]
    fn unterminated_define_is_diagnosed() {
        let src = "define X\nfoo\n";
        let (_file, diags) = parse(src);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::DefineNotClosed);
    }

    #[test]
    fn parses_conditional_with_else() {
        let src = "ifeq ($(OS),Linux)\nA = 1\nelse\nA = 2\nendif\n";
        let (file, diags) = parse(src);
        assert!(diags.is_empty());
        let Item::Conditional(c) = &file.items[0] else { panic!() };
        assert_eq!(c.kind, ConditionalKind::Ifeq);
        assert_eq!(c.then_branch.len(), 1);
        assert_eq!(c.else_branch.len(), 1);
    }

    #[test]
    fn unclosed_conditional_is_diagnosed() {
        let src = "ifeq ($(OS),Linux)\nA = 1\n";
        let (_, diags) = parse(src);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::ConditionalNotClosed));
    }

    #[test]
    fn parses_include_directive() {
        let src = "include config.mk deps.mk\n";
        let (file, _) = parse(src);
        let Item::Include(i) = &file.items[0] else { panic!() };
        assert_eq!(i.paths, vec!["config.mk".to_string(), "deps.mk".to_string()]);
        assert!(!i.optional);
    }

    #[test]
    fn parses_dash_include_as_optional() {
        let src = "-include deps.mk\n";
        let (file, _) = parse(src);
        let Item::Include(i) = &file.items[0] else { panic!() };
        assert!(i.optional);
    }

    #[test]
    fn recipe_with_spaces_is_diagnosed() {
        let src = "all:\n  echo hi\n";
        let (file, diags) = parse(src);
        assert_eq!(file.items.len(), 1);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::RecipeUsesSpaces));
    }

    #[test]
    fn recipe_without_rule_is_diagnosed() {
        let src = "\techo hi\n";
        let (_, diags) = parse(src);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::RecipeOutsideRule));
    }

    #[test]
    fn value_with_dollar_parens_does_not_confuse_assignment_finder() {
        let src = "SOURCES = $(wildcard *.c) foo.c\n";
        let (file, _) = parse(src);
        let Item::Assignment(a) = &file.items[0] else { panic!() };
        assert_eq!(a.name.name, "SOURCES");
        assert_eq!(a.value, "$(wildcard *.c) foo.c");
    }
}
