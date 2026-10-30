//! Recursive-descent parser over the logical lines produced by [`crate::lexer`].
//!
//! The dispatch order mirrors GNU Make's `eval()` in `src/read.c`:
//!
//! 1. A line starting with the recipe prefix (TAB, or `.RECIPEPREFIX`)
//!    while a rule is open is a recipe line — nothing else is checked.
//! 2. Comments are stripped (ignoring `#` inside `$(…)` and `\#`).
//! 3. Variable assignments come next, optionally preceded by any mix of
//!    `export` / `unexport` / `override` / `private`, and including
//!    `define … endef` and `undefine`. This is checked early so that
//!    variables may be named like directives (`ifdef = 1`).
//! 4. Conditionals, then `export` / `unexport` / `vpath` / `include` /
//!    `load`, then rule headers (with target-specific variables).
//!
//! GNU Make's "rule context" — which rule a recipe line belongs to — stays
//! open across blank lines, comments and conditional directives, and is
//! closed by any other statement. Errors are collected as
//! [`MakeDiagnostic`]s rather than aborting.

use crate::ast::{
    AssignOp, Assignment, Conditional, ConditionalKind, Define, Directive, DirectiveKind, File,
    Identifier, Include, Item, Modifier, RecipeLine, Rule,
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
    /// First byte of a recipe line — TAB unless `.RECIPEPREFIX` changes it.
    recipe_prefix: u8,
    /// Conditional nesting depth of the line being parsed.
    depth: usize,
    /// True while recipe lines would be attached to a rule.
    rule_open: bool,
    /// Conditional depth at which the open rule was declared.
    rule_depth: usize,
    /// Bumped every time a new rule context starts, so a parse level can
    /// tell whether the rule it owns is still the open one.
    rule_serial: u32,
    /// Recipe lines found inside a conditional nested under the open
    /// rule, tagged with that rule's serial. The level owning the rule
    /// collects them once the conditional closes.
    nested_recipes: Vec<(u32, RecipeLine)>,
}

/// What [`parse_var_assignment`] found at the start of a statement.
enum VarLine<'t> {
    /// `[mods] NAME op value` — `rest` starts at NAME.
    Assign { modifiers: Vec<Modifier>, rest: &'t str, def: VarDef },
    /// `[mods] define …` — `rest` is the text after `define`.
    Define { modifiers: Vec<Modifier>, rest: &'t str },
    /// `[mods] undefine …` — `rest` is the text after `undefine`.
    Undefine { rest: &'t str, keyword: &'t str },
}

/// Byte offsets of a variable definition within the text it was parsed
/// from (see [`parse_variable_definition`]).
#[derive(Clone, Copy)]
struct VarDef {
    name_end: usize,
    op: AssignOp,
    op_end: usize,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str, lines: &'a [LogicalLine]) -> Self {
        Parser {
            source,
            lines,
            pos: 0,
            diagnostics: Vec::new(),
            phony: Vec::new(),
            recipe_prefix: b'\t',
            depth: 0,
            rule_open: false,
            rule_depth: 0,
            rule_serial: 0,
            nested_recipes: Vec::new(),
        }
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
        let items = self.parse_items(&[]);
        File { items, span: ByteSpan::new(start, end) }
    }

    /// Parse items until EOF or until a line whose first word is one of
    /// `terminators` (used to recurse into conditional branches).
    fn parse_items(&mut self, terminators: &[&str]) -> Vec<Item> {
        let mut out = Vec::new();
        // `(index in out, rule_serial)` of the rule declared at this level.
        let mut own_rule: Option<(usize, u32)> = None;
        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];
            if line.kind == LineKind::Blank {
                self.pos += 1;
                continue;
            }
            if self.rule_open && self.is_recipe_line(line) {
                let rl = self.recipe_line_from(line);
                if self.rule_depth == self.depth {
                    if let Some((idx, serial)) = own_rule {
                        if serial == self.rule_serial {
                            if let Some(Item::Rule(r)) = out.get_mut(idx) {
                                r.span.end = line.span.end;
                                r.recipe_lines.push(rl);
                            }
                        }
                    }
                } else if self.rule_depth < self.depth {
                    self.nested_recipes.push((self.rule_serial, rl));
                }
                self.pos += 1;
                continue;
            }
            if line.kind == LineKind::Comment {
                self.pos += 1;
                continue;
            }

            let text = strip_comment(line.text.trim_start()).trim_end();
            if text.is_empty() {
                self.pos += 1;
                continue;
            }
            if terminators.contains(&first_word(text)) {
                return out;
            }

            match self.parse_statement(line, text) {
                Some(Item::Rule(r)) => {
                    // Track .PHONY targets so subsequent rules can be
                    // marked is_phony in the outline.
                    if r.targets.first().is_some_and(|t| t.name == ".PHONY") {
                        self.phony.extend(r.prerequisites.iter().map(|p| p.name.clone()));
                    }
                    own_rule = Some((out.len(), self.rule_serial));
                    out.push(Item::Rule(r));
                }
                Some(Item::Conditional(c)) => {
                    // Recipe lines inside the conditional belong to the
                    // rule that was open when it started.
                    if let Some((idx, serial)) = own_rule {
                        let mut taken = Vec::new();
                        self.nested_recipes.retain(|(s, rl)| {
                            if *s == serial {
                                taken.push(rl.clone());
                                false
                            } else {
                                true
                            }
                        });
                        if let Some(Item::Rule(r)) = out.get_mut(idx) {
                            if !taken.is_empty() {
                                r.span.end = r.span.end.max(c.span.end);
                                r.recipe_lines.extend(taken);
                            }
                        }
                    }
                    out.push(Item::Conditional(c));
                }
                Some(item) => out.push(item),
                None => {
                    // An unrecognised space-indented line right after a
                    // rule is most likely a recipe typed with spaces.
                    let owned = own_rule.filter(|&(_, s)| {
                        self.rule_open && self.rule_depth == self.depth && s == self.rule_serial
                    });
                    if let (LineKind::RecipeWithSpaces, Some((idx, _))) = (line.kind, owned) {
                        self.diagnostics.push(MakeDiagnostic::error(
                            DiagnosticCode::RecipeUsesSpaces,
                            "recipe line uses spaces where a tab is required",
                            line.span,
                        ));
                        let rl = self.recipe_line_from(line);
                        if let Some(Item::Rule(r)) = out.get_mut(idx) {
                            r.span.end = line.span.end;
                            r.recipe_lines.push(rl);
                        }
                    }
                }
            }
        }
        out
    }

    /// Parse one statement. `text` is the line with leading whitespace
    /// and any trailing comment removed. Always advances `pos`.
    fn parse_statement(&mut self, line: &LogicalLine, text: &str) -> Option<Item> {
        if let Some(var) = parse_var_assignment(text, false) {
            // Any variable statement ends the previous rule.
            self.rule_open = false;
            return Some(self.parse_var_line(line, var));
        }

        if let Some(kind) = match_conditional_open(text) {
            return Some(Item::Conditional(self.parse_conditional(line, text, kind)));
        }

        let word = first_word(text);
        let stray = match word {
            "else" => Some((DiagnosticCode::ElseWithoutIf, "`else` without a matching `if`")),
            "endif" => Some((DiagnosticCode::EndifWithoutIf, "`endif` without a matching `if`")),
            "endef" => Some((
                DiagnosticCode::EndefWithoutDefine,
                "`endef` without a matching `define`",
            )),
            _ => None,
        };
        if let Some((code, msg)) = stray {
            self.diagnostics.push(MakeDiagnostic::error(code, msg, line.span));
            self.pos += 1;
            return None;
        }

        if starts_keyword(text, "include") || starts_keyword(text, "-include")
            || starts_keyword(text, "sinclude")
        {
            self.rule_open = false;
            return Some(Item::Include(self.parse_include(line, text)));
        }
        if let Some(k) = match_directive(text) {
            self.rule_open = false;
            self.pos += 1;
            return Some(Item::Directive(self.parse_directive(line, text, k)));
        }

        // A recipe-prefixed line with no rule open that isn't a variable
        // or directive either.
        if self.is_recipe_line(line) {
            self.diagnostics.push(MakeDiagnostic::error(
                DiagnosticCode::RecipeOutsideRule,
                "recipe line appears outside of any target rule",
                line.span,
            ));
            self.pos += 1;
            return None;
        }

        let item = self.try_parse_rule(line);
        self.pos += 1;
        item
    }

    fn parse_var_line(&mut self, line: &LogicalLine, var: VarLine<'_>) -> Item {
        match var {
            VarLine::Assign { modifiers, rest, def } => {
                let a = self.make_assignment(line, modifiers, Vec::new(), rest, def, None);
                if a.name.name == ".RECIPEPREFIX" {
                    self.recipe_prefix = a.value.bytes().next().unwrap_or(b'\t');
                }
                self.pos += 1;
                Item::Assignment(a)
            }
            VarLine::Define { modifiers, rest } => Item::Define(self.parse_define(line, modifiers, rest)),
            VarLine::Undefine { rest, keyword } => {
                let name_span = self.locate(line, keyword, "undefine");
                self.pos += 1;
                Item::Directive(Directive {
                    kind: DirectiveKind::Undefine,
                    arguments: rest.trim().to_string(),
                    span: line.span,
                    name_span,
                })
            }
        }
    }

    fn make_assignment(
        &mut self,
        line: &LogicalLine,
        modifiers: Vec<Modifier>,
        targets: Vec<Identifier>,
        rest: &str,
        def: VarDef,
        inline_recipe: Option<&str>,
    ) -> Assignment {
        let name = rest[..def.name_end].trim_end();
        if name.is_empty() {
            self.diagnostics.push(MakeDiagnostic::error(
                DiagnosticCode::EmptyVariableName,
                "empty variable name",
                line.span,
            ));
        }
        let name_span = self.locate(line, rest, name);
        let mut value = rest[def.op_end..].trim().to_string();
        // A `;` in a target-specific variable line is part of the value.
        if let Some(r) = inline_recipe {
            value.push(';');
            value.push_str(r);
        }
        Assignment {
            modifiers,
            targets,
            name: Identifier { name: name.to_string(), span: name_span },
            op: def.op,
            value,
            span: line.span,
        }
    }

    fn parse_conditional(&mut self, open: &LogicalLine, text: &str, kind: ConditionalKind) -> Conditional {
        let keyword_len = kind.as_str().len();
        let condition = text[keyword_len..].trim().to_string();
        let name_span = self.locate(open, text, kind.as_str());
        self.check_condition(open, text, kind);
        let mut span = open.span;
        self.pos += 1;
        self.depth += 1;

        let then_branch = self.parse_items(&["else", "endif"]);
        let mut else_branch = Vec::new();
        let mut chained = false;

        if self.pos < self.lines.len() {
            let l = &self.lines[self.pos];
            let t = strip_comment(l.text.trim_start()).trim_end();
            if first_word(t) == "else" {
                self.pos += 1;
                // `else ifeq …` chains into a nested conditional that
                // shares this conditional's `endif`.
                let rest = t["else".len()..].trim_start();
                if let Some(inner) = match_conditional_open(rest) {
                    self.pos -= 1;
                    let nested = self.parse_conditional(l, rest, inner);
                    span.end = nested.span.end;
                    else_branch.push(Item::Conditional(nested));
                    chained = true;
                } else {
                    if !rest.is_empty() {
                        self.extraneous_text(l, "else");
                    }
                    else_branch = self.parse_items(&["else", "endif"]);
                    // A second `else` for the same conditional.
                    while self.pos < self.lines.len() {
                        let l = &self.lines[self.pos];
                        if first_word(strip_comment(l.text.trim_start())) != "else" {
                            break;
                        }
                        self.diagnostics.push(MakeDiagnostic::error(
                            DiagnosticCode::ElseWithoutIf,
                            "only one `else` per conditional",
                            l.span,
                        ));
                        self.pos += 1;
                        else_branch.extend(self.parse_items(&["else", "endif"]));
                    }
                }
            }
        }
        self.depth -= 1;

        if chained {
            // The nested conditional consumed (or diagnosed) the `endif`.
        } else if self.pos < self.lines.len()
            && first_word(strip_comment(self.lines[self.pos].text.trim_start())) == "endif"
        {
            let l = &self.lines[self.pos];
            let t = strip_comment(l.text.trim_start()).trim_end();
            if t.len() > "endif".len() {
                self.extraneous_text(l, "endif");
            }
            span.end = l.span.end;
            self.pos += 1;
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

    /// `[modifiers] define NAME [op]`, body lines, `endef` — after
    /// `do_define()`. `header` is the text following `define`.
    fn parse_define(&mut self, open: &LogicalLine, modifiers: Vec<Modifier>, header: &str) -> Define {
        let header = header.trim();
        let (name_str, op) = match parse_variable_definition(header) {
            Some(def) => {
                if !header[def.op_end..].trim().is_empty() {
                    self.extraneous_text(open, "define");
                }
                (header[..def.name_end].trim_end(), Some(def.op))
            }
            None => (header, None),
        };
        if name_str.is_empty() {
            self.diagnostics.push(MakeDiagnostic::error(
                DiagnosticCode::EmptyVariableName,
                "empty variable name",
                open.span,
            ));
        }
        let name_span = self.locate(open, header, name_str);
        let name = Identifier { name: name_str.to_string(), span: name_span };

        let header_start = open.span.start;
        let mut body = String::new();
        let mut closed_end = open.span.end;
        let mut nlevels = 1;
        let mut first_line = true;
        self.pos += 1;

        while self.pos < self.lines.len() {
            let l = &self.lines[self.pos];
            // Lines starting with the recipe prefix are never directives;
            // otherwise nested `define`s must be matched by their `endef`.
            if !self.starts_with_prefix(l) {
                let t = l.text.trim_start_matches([' ', '\t']);
                if is_word(t, "define") {
                    nlevels += 1;
                } else if is_word(t, "endef") {
                    if !strip_comment(&t["endef".len()..]).trim().is_empty() {
                        self.extraneous_text(l, "endef");
                    }
                    nlevels -= 1;
                    if nlevels == 0 {
                        closed_end = l.span.end;
                        self.pos += 1;
                        return Define {
                            modifiers,
                            name,
                            op,
                            body,
                            span: ByteSpan::new(header_start, closed_end),
                            name_span,
                        };
                    }
                }
            }
            if !first_line {
                body.push('\n');
            }
            first_line = false;
            body.push_str(self.source[l.span.start as usize..l.span.end as usize]
                .trim_end_matches('\n')
                .trim_end_matches('\r'));
            closed_end = l.span.end;
            self.pos += 1;
        }

        // Ran off the end without finding endef.
        self.diagnostics.push(MakeDiagnostic::error(
            DiagnosticCode::DefineNotClosed,
            "`define` block is not closed — expected `endef`",
            open.span,
        ));
        Define {
            modifiers,
            name,
            op,
            body,
            span: ByteSpan::new(header_start, closed_end),
            name_span,
        }
    }

    fn parse_include(&mut self, line: &LogicalLine, text: &str) -> Include {
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

    fn parse_directive(&mut self, line: &LogicalLine, text: &str, kind: DirectiveKind) -> Directive {
        let kw = first_word(text);
        let arguments = text[kw.len()..].trim().to_string();
        let name_span = self.locate(line, text, kw);
        Directive { kind, arguments, span: line.span, name_span }
    }

    /// Parse a rule header, or a target-specific variable assignment.
    /// Does not advance `pos`; recipe lines are attached by
    /// [`Parser::parse_items`].
    fn try_parse_rule(&mut self, line: &LogicalLine) -> Option<Item> {
        let raw = line.text.trim_start();
        // A `;` starts an inline recipe unless a `#` comes first.
        let (header, inline_recipe) = match find_unquoted(raw, b";#") {
            Some(i) if raw.as_bytes()[i] == b';' => (&raw[..i], Some(raw[i + 1..].trim())),
            Some(i) => (&raw[..i], None),
            None => (raw, None),
        };
        let colon = find_toplevel_colon(header)?;
        let is_grouped = colon > 0 && header.as_bytes()[colon - 1] == b'&';
        let targets_str = header[..if is_grouped { colon - 1 } else { colon }].trim();

        // GNU Make accepts and ignores rules without targets, recipe
        // lines included.
        self.start_rule_context();
        if targets_str.is_empty() {
            return None;
        }

        let (after, is_double) = if header[colon + 1..].starts_with(':') {
            (colon + 2, true)
        } else {
            (colon + 1, false)
        };
        let rest = &header[after..];
        let targets = self.identifiers(line, targets_str);

        if let Some(VarLine::Assign { modifiers, rest, def }) = parse_var_assignment(rest, true) {
            self.rule_open = false;
            let a = self.make_assignment(line, modifiers, targets, rest, def, inline_recipe);
            return Some(Item::Assignment(a));
        }

        // Static pattern rule: `targets: target-pattern: prereq-patterns`.
        let (static_pattern, prereqs_str) = match find_toplevel_colon(rest) {
            Some(c) => {
                let pattern = rest[..c].trim();
                let pattern = (!pattern.is_empty()).then(|| Identifier {
                    name: pattern.to_string(),
                    span: self.locate(line, rest, pattern),
                });
                (pattern, &rest[c + 1..])
            }
            None => (None, rest),
        };
        let (normal, order_only) = match find_unquoted(prereqs_str, b"|") {
            Some(i) => (&prereqs_str[..i], &prereqs_str[i + 1..]),
            None => (prereqs_str, &prereqs_str[prereqs_str.len()..]),
        };
        let prerequisites = self.identifiers(line, normal.trim());
        let order_only = self.identifiers(line, order_only.trim());

        let is_pattern = static_pattern.is_some() || targets.iter().any(|t| t.name.contains('%'));
        let is_phony = targets
            .first()
            .map(|t| self.phony.iter().any(|p| p == &t.name))
            .unwrap_or(false);

        let name_span = targets
            .first()
            .map(|t| t.span)
            .unwrap_or_else(|| ByteSpan::new(line.content_start, line.content_start));

        Some(Item::Rule(Rule {
            targets,
            prerequisites,
            is_double_colon: is_double,
            is_pattern,
            is_phony,
            is_grouped,
            static_pattern,
            order_only,
            inline_recipe: inline_recipe.map(str::to_string),
            recipe_lines: Vec::new(),
            span: line.span,
            name_span,
        }))
    }

    fn start_rule_context(&mut self) {
        self.rule_open = true;
        self.rule_depth = self.depth;
        self.rule_serial += 1;
    }

    fn starts_with_prefix(&self, line: &LogicalLine) -> bool {
        self.source.as_bytes().get(line.span.start as usize) == Some(&self.recipe_prefix)
    }

    fn is_recipe_line(&self, line: &LogicalLine) -> bool {
        line.kind != LineKind::Blank && self.starts_with_prefix(line)
    }

    fn recipe_line_from(&self, line: &LogicalLine) -> RecipeLine {
        RecipeLine { text: line.text.clone(), span: line.span }
    }

    /// Validate a conditional's arguments the way `conditional_line()`
    /// does; `text` starts at the keyword.
    fn check_condition(&mut self, line: &LogicalLine, text: &str, kind: ConditionalKind) {
        let kw = kind.as_str();
        if !matches!(text.as_bytes().get(kw.len()), None | Some(b' ' | b'\t')) {
            self.diagnostics.push(MakeDiagnostic::error(
                DiagnosticCode::InvalidConditional,
                format!("`{kw}` must be followed by whitespace"),
                line.span,
            ));
        }
        let args = text[kw.len()..].trim();
        let valid = match kind {
            // Exactly one variable name; `$(…)` may contain spaces.
            ConditionalKind::Ifdef | ConditionalKind::Ifndef => {
                find_unquoted(args, b" \t").is_none()
            }
            ConditionalKind::Ifeq | ConditionalKind::Ifneq => match parse_ifeq_args(args) {
                Some(extra) => {
                    if extra {
                        self.extraneous_text(line, kw);
                    }
                    true
                }
                None => false,
            },
        };
        if !valid {
            self.diagnostics.push(MakeDiagnostic::error(
                DiagnosticCode::InvalidConditional,
                format!("invalid syntax in `{kw}` conditional"),
                line.span,
            ));
        }
    }

    fn extraneous_text(&mut self, line: &LogicalLine, directive: &str) {
        self.diagnostics.push(MakeDiagnostic::error(
            DiagnosticCode::ExtraneousText,
            format!("extraneous text after `{directive}` directive"),
            line.span,
        ));
    }

    /// Source span of `needle`, searched for at or after the position of
    /// `within` (a sub-slice of `line.text`). Joining continuation lines
    /// only ever shrinks the text, so a text offset never overshoots the
    /// matching source offset.
    fn locate(&self, line: &LogicalLine, within: &str, needle: &str) -> ByteSpan {
        let text_off = (within.as_ptr() as usize)
            .checked_sub(line.text.as_ptr() as usize)
            .filter(|&o| o <= line.text.len())
            .unwrap_or(0);
        let end = line.span.end as usize;
        let mut from = (line.span.start as usize + text_off).min(end);
        while !self.source.is_char_boundary(from) {
            from -= 1;
        }
        let start = self.source[from..end]
            .find(needle)
            .map(|o| from + o)
            .unwrap_or(from);
        ByteSpan::from_usize(start, start + needle.len())
    }

    fn identifiers(&self, line: &LogicalLine, list: &str) -> Vec<Identifier> {
        let base = self.locate(line, list, "").start as usize;
        split_into_identifiers(self.source, base, list)
    }
}

/// After `parse_var_assignment()`: skip any `export` / `unexport` /
/// `override` / `private` modifiers, then expect a variable definition,
/// or (unless `targvar`) `define` / `undefine`.
fn parse_var_assignment(text: &str, targvar: bool) -> Option<VarLine<'_>> {
    let mut modifiers = Vec::new();
    let mut p = text.trim_start();
    if p.is_empty() {
        return None;
    }
    loop {
        if let Some(def) = parse_variable_definition(p) {
            return Some(VarLine::Assign { modifiers, rest: p, def });
        }
        let word = first_word(p);
        let after = &p[word.len()..];
        if let Some(m) = Modifier::from_word(word) {
            modifiers.push(m);
        } else if !targvar && word == "define" {
            return Some(VarLine::Define { modifiers, rest: after });
        } else if !targvar && word == "undefine" {
            return Some(VarLine::Undefine { rest: after, keyword: p });
        } else {
            return None;
        }
        p = after.trim_start();
        if p.is_empty() {
            return None;
        }
    }
}

/// After `parse_variable_definition()` in `src/variable.c`: `text` (no
/// leading whitespace) is a definition if a single word — `$(…)`
/// references may contain spaces — is followed by an assignment
/// operator.
fn parse_variable_definition(text: &str) -> Option<VarDef> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut end: Option<usize> = None;
    loop {
        let c = *b.get(i)?;
        if c == b'#' {
            return None;
        }
        if c == b' ' || c == b'\t' {
            // Variable names can't contain spaces.
            if end.is_some() {
                return None;
            }
            end = Some(i);
            while matches!(b.get(i), Some(b' ' | b'\t')) {
                i += 1;
            }
            continue;
        }
        let start = i;
        i += 1;
        let def = |op, op_end| Some(VarDef { name_end: end.unwrap_or(start), op, op_end });
        if c == b'?' && b.get(i) == Some(&b'=') {
            return def(AssignOp::Conditional, i + 1);
        }
        if c == b'=' {
            return def(AssignOp::Recursive, i);
        }
        if c == b':' {
            let n = &b[i..];
            return if n.starts_with(b"=") {
                def(AssignOp::Simple, i + 1)
            } else if n.starts_with(b":=") {
                def(AssignOp::Immediate, i + 2)
            } else if n.starts_with(b"::=") {
                def(AssignOp::ImmediateExpand, i + 3)
            } else {
                None
            };
        }
        if b.get(i) == Some(&b'=') {
            match c {
                b'+' => return def(AssignOp::Append, i + 1),
                b'!' => return def(AssignOp::Shell, i + 1),
                _ => {}
            }
        }
        if end.is_some() {
            return None;
        }
        if c == b'$' {
            i = skip_reference(b, i);
        }
    }
}

/// After `skip_reference()` in `src/misc.c`: `i` is just past a `$`.
/// Returns the offset just past the variable reference.
fn skip_reference(b: &[u8], i: usize) -> usize {
    let (open, close) = match b.get(i) {
        Some(b'(') => (b'(', b')'),
        Some(b'{') => (b'{', b'}'),
        Some(_) => return i + 1,
        None => return i,
    };
    let mut count = 0;
    for (j, &c) in b.iter().enumerate().skip(i) {
        if c == open {
            count += 1;
        } else if c == close {
            count -= 1;
            if count == 0 {
                return j + 1;
            }
        }
    }
    b.len()
}

/// After `find_map_unquote()` with `MAP_VARIABLE`: the first byte in
/// `stops` that is outside any `$(…)` reference and not escaped by an
/// odd number of backslashes.
fn find_unquoted(text: &str, stops: &[u8]) -> Option<usize> {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'$' {
            i = skip_reference(b, i + 1);
            continue;
        }
        if stops.contains(&c) {
            let backslashes = b[..i].iter().rev().take_while(|&&c| c == b'\\').count();
            if backslashes % 2 == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// After `remove_comments()`: cut the line at the first unquoted `#`.
fn strip_comment(text: &str) -> &str {
    match find_unquoted(text, b"#") {
        Some(i) => &text[..i],
        None => text,
    }
}

/// The first blank-delimited word, like GNU Make's `end_of_token()`.
fn first_word(text: &str) -> &str {
    let end = text.find([' ', '\t']).unwrap_or(text.len());
    &text[..end]
}

/// True when `text` is `kw` alone or followed by a blank — the test
/// `do_define()` uses for nested `define` / `endef`.
fn is_word(text: &str, kw: &str) -> bool {
    text.starts_with(kw) && matches!(text.as_bytes().get(kw.len()), None | Some(b' ' | b'\t'))
}

/// GNU Make only recognises a conditional keyword as a whole
/// blank-delimited word. `ifeq(…)` / `ifneq"…"` are accepted too so the
/// block structure survives; [`Parser::check_condition`] reports them.
fn match_conditional_open(text: &str) -> Option<ConditionalKind> {
    let kind = match first_word(text) {
        "ifeq" => return Some(ConditionalKind::Ifeq),
        "ifneq" => return Some(ConditionalKind::Ifneq),
        "ifdef" => return Some(ConditionalKind::Ifdef),
        "ifndef" => return Some(ConditionalKind::Ifndef),
        _ if text.starts_with("ifeq") => ConditionalKind::Ifeq,
        _ if text.starts_with("ifneq") => ConditionalKind::Ifneq,
        _ => return None,
    };
    let next = text.as_bytes()[kind.as_str().len()];
    matches!(next, b'(' | b'"' | b'\'').then_some(kind)
}

/// The argument forms `conditional_line()` accepts for `ifeq` / `ifneq`:
/// `(a,b)`, or two strings each quoted with `"` or `'`. Returns whether
/// text follows the arguments, or `None` if they are malformed.
fn parse_ifeq_args(args: &str) -> Option<bool> {
    let b = args.as_bytes();
    let termin = match *b.first()? {
        b'(' => b',',
        c @ (b'"' | b'\'') => c,
        _ => return None,
    };
    let mut i = 1;
    while i < b.len() && b[i] != termin {
        if b[i] == b'$' {
            i = skip_reference(b, i + 1);
        } else {
            i += 1;
        }
    }
    if i >= b.len() {
        return None;
    }
    i += 1;
    let termin = if termin == b',' {
        b')'
    } else {
        while matches!(b.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        let c = *b.get(i)?;
        if c != b'"' && c != b'\'' {
            return None;
        }
        i += 1;
        c
    };
    if termin == b')' {
        // Parentheses in the second argument must balance.
        let mut count = 0;
        while i < b.len() {
            match b[i] {
                b'(' => count += 1,
                b')' if count <= 0 => break,
                b')' => count -= 1,
                _ => {}
            }
            i += 1;
        }
    } else {
        while i < b.len() && b[i] != termin {
            i += 1;
        }
    }
    if i >= b.len() {
        return None;
    }
    Some(!args[i + 1..].trim().is_empty())
}

fn match_directive(text: &str) -> Option<DirectiveKind> {
    let (kw, kind) = if starts_keyword(text, "export") {
        ("export", DirectiveKind::Export)
    } else if starts_keyword(text, "unexport") {
        ("unexport", DirectiveKind::Unexport)
    } else if starts_keyword(text, "override") {
        ("override", DirectiveKind::Override)
    } else if starts_keyword(text, "private") {
        ("private", DirectiveKind::Private)
    } else if starts_keyword(text, "vpath") {
        ("vpath", DirectiveKind::VPath)
    } else if starts_keyword(text, "load") {
        ("load", DirectiveKind::Load)
    } else if starts_keyword(text, "-load") {
        ("-load", DirectiveKind::Load)
    } else {
        return None;
    };
    // `export:` / `export : deps` is a rule whose target happens to be
    // named like a directive, not the directive itself.
    let rest = text[kw.len()..].trim_start();
    if rest.starts_with(':') && !is_assign_colon(rest, 0) {
        return None;
    }
    Some(kind)
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

/// The first `:` outside any `$(…)` reference, skipping DOS drive
/// letters such as `C:\path`.
fn find_toplevel_colon(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'$' => {
                i = skip_reference(bytes, i + 1);
                continue;
            }
            b':' => {
                // Only treat `X:\` / `X:/` as a drive letter if the
                // letter is at the start of the current token.
                if let Some(next) = bytes.get(i + 1) {
                    if (*next == b'\\' || *next == b'/') && i > 0 {
                        let prev = bytes[i - 1];
                        if prev.is_ascii_alphabetic() {
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
    fn directive_named_target_is_a_rule() {
        let src = ".PHONY: export\nexport:\n\t@echo export\nvpath : x\n\techo vpath\n";
        let (file, diags) = parse(src);
        assert!(diags.is_empty(), "diagnostics: {:?}", diags);
        let Item::Rule(r) = &file.items[1] else { panic!() };
        assert_eq!(r.targets[0].name, "export");
        assert_eq!(r.recipe_lines.len(), 1);
        let Item::Rule(r) = &file.items[2] else { panic!() };
        assert_eq!(r.targets[0].name, "vpath");
    }

    #[test]
    fn export_with_assignment_is_still_a_directive() {
        let src = "export FOO := 1\nexport\n";
        let (file, _) = parse(src);
        assert!(file.items.iter().all(|i| !matches!(i, Item::Rule(_))));
    }

    #[test]
    fn value_with_dollar_parens_does_not_confuse_assignment_finder() {
        let src = "SOURCES = $(wildcard *.c) foo.c\n";
        let (file, _) = parse(src);
        let Item::Assignment(a) = &file.items[0] else { panic!() };
        assert_eq!(a.name.name, "SOURCES");
        assert_eq!(a.value, "$(wildcard *.c) foo.c");
    }

    fn assert_clean(diags: &[MakeDiagnostic]) {
        assert!(diags.is_empty(), "diagnostics: {:?}", diags);
    }

    #[test]
    fn define_with_modifiers_swallows_body() {
        let src = "override export define CMD :=\nall: dep\n\techo $@\nifeq (a,b)\nendef\nX = 1\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        assert_eq!(file.items.len(), 2);
        let Item::Define(d) = &file.items[0] else { panic!("{:?}", file.items[0]) };
        assert_eq!(d.name.name, "CMD");
        assert_eq!(&src[d.name_span.start as usize..d.name_span.end as usize], "CMD");
        assert_eq!(d.op, Some(AssignOp::Simple));
        assert_eq!(d.modifiers, vec![Modifier::Override, Modifier::Export]);
        assert_eq!(d.body, "all: dep\n\techo $@\nifeq (a,b)");
        assert!(matches!(&file.items[1], Item::Assignment(a) if a.name.name == "X"));
    }

    #[test]
    fn private_and_export_define() {
        for kw in ["export", "private", "unexport"] {
            let src = format!("{kw} define FOO\nfoo: bar\nendef\n");
            let (file, diags) = parse(&src);
            assert_clean(&diags);
            assert_eq!(file.items.len(), 1);
            assert!(matches!(&file.items[0], Item::Define(d) if d.name.name == "FOO"));
        }
    }

    #[test]
    fn nested_define_needs_matching_endef() {
        let src = "define OUTER\ndefine INNER\nx\nendef\nendef\nY = 2\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        assert_eq!(file.items.len(), 2);
        let Item::Define(d) = &file.items[0] else { panic!() };
        assert_eq!(d.body, "define INNER\nx\nendef");
    }

    #[test]
    fn define_operator_without_space() {
        let src = "define A=\n1\nendef\ndefine B :::=\n2\nendef\ndefine $(call n, x) +=\n3\nendef\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        let ops: Vec<_> = file
            .items
            .iter()
            .map(|i| match i {
                Item::Define(d) => (d.name.name.clone(), d.op),
                _ => panic!("not a define"),
            })
            .collect();
        assert_eq!(
            ops,
            vec![
                ("A".to_string(), Some(AssignOp::Recursive)),
                ("B".to_string(), Some(AssignOp::ImmediateExpand)),
                ("$(call n, x)".to_string(), Some(AssignOp::Append)),
            ]
        );
    }

    #[test]
    fn define_body_keeps_blank_lines_and_tab_endef() {
        let src = "define T\n\n\tendef\n  endef # done\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        assert_eq!(file.items.len(), 1);
        let Item::Define(d) = &file.items[0] else { panic!() };
        assert_eq!(d.body, "\n\tendef");
        assert_eq!(d.span.end as usize, src.len());
    }

    #[test]
    fn define_extraneous_text_is_diagnosed() {
        let src = "define A = junk\nx\nendef junk\n";
        let (_, diags) = parse(src);
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::ExtraneousText));
    }

    #[test]
    fn define_ends_rule_context() {
        let src = "all:\n\techo a\ndefine X\nendef\n\techo b\n";
        let (file, diags) = parse(src);
        let Item::Rule(r) = &file.items[0] else { panic!() };
        assert_eq!(r.recipe_lines.len(), 1);
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::RecipeOutsideRule));
    }

    #[test]
    fn define_inside_conditional() {
        let src = "ifdef X\ndefine Y\nelse\nendif\nendef\nendif\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        let Item::Conditional(c) = &file.items[0] else { panic!() };
        assert_eq!(c.then_branch.len(), 1);
        assert!(c.else_branch.is_empty());
    }

    #[test]
    fn stray_endef_and_endif_are_diagnosed() {
        let (_, diags) = parse("endef\nendif\nelse\n");
        let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
        assert_eq!(
            codes,
            vec![
                DiagnosticCode::EndefWithoutDefine,
                DiagnosticCode::EndifWithoutIf,
                DiagnosticCode::ElseWithoutIf,
            ]
        );
    }

    #[test]
    fn modifiers_produce_assignments() {
        let src = "export CC := gcc\noverride private CFLAGS += -g\nexport A B\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        let Item::Assignment(a) = &file.items[0] else { panic!() };
        assert_eq!(a.name.name, "CC");
        assert_eq!(a.modifiers, vec![Modifier::Export]);
        let Item::Assignment(a) = &file.items[1] else { panic!() };
        assert_eq!(a.name.name, "CFLAGS");
        assert_eq!(a.op, AssignOp::Append);
        assert_eq!(a.modifiers, vec![Modifier::Override, Modifier::Private]);
        let Item::Directive(d) = &file.items[2] else { panic!() };
        assert_eq!(d.kind, DirectiveKind::Export);
        assert_eq!(d.arguments, "A B");
    }

    #[test]
    fn variables_named_like_directives() {
        let src = "ifdef = 1\ninclude := x\ndefine := y\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        assert!(file.items.iter().all(|i| matches!(i, Item::Assignment(_))));
    }

    #[test]
    fn undefine_with_modifier() {
        let (file, diags) = parse("override undefine FOO\n");
        assert_clean(&diags);
        let Item::Directive(d) = &file.items[0] else { panic!() };
        assert_eq!(d.kind, DirectiveKind::Undefine);
        assert_eq!(d.arguments, "FOO");
    }

    #[test]
    fn trailing_comments_are_stripped() {
        let src = "A = 1 # one\nall: dep # comment\nB = $(subst #,x,y)\nC = a\\#b\n";
        let (file, _) = parse(src);
        let Item::Assignment(a) = &file.items[0] else { panic!() };
        assert_eq!(a.value, "1");
        let Item::Rule(r) = &file.items[1] else { panic!() };
        assert_eq!(r.prerequisites.len(), 1);
        let Item::Assignment(a) = &file.items[2] else { panic!() };
        assert_eq!(a.value, "$(subst #,x,y)");
        let Item::Assignment(a) = &file.items[3] else { panic!() };
        assert_eq!(a.value, "a\\#b");
    }

    #[test]
    fn rule_header_variants() {
        let src = "all: a b | dir ; @echo hi # shell\n\
                   $(OBJS): %.o: %.c\n\
                   x.tab.c x.tab.h &: x.y\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        let Item::Rule(r) = &file.items[0] else { panic!() };
        let names = |ids: &[Identifier]| ids.iter().map(|i| i.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&r.prerequisites), vec!["a", "b"]);
        assert_eq!(names(&r.order_only), vec!["dir"]);
        assert_eq!(r.inline_recipe.as_deref(), Some("@echo hi # shell"));
        let Item::Rule(r) = &file.items[1] else { panic!() };
        assert_eq!(r.targets[0].name, "$(OBJS)");
        assert_eq!(r.static_pattern.as_ref().unwrap().name, "%.o");
        assert_eq!(names(&r.prerequisites), vec!["%.c"]);
        assert!(r.is_pattern);
        let Item::Rule(r) = &file.items[2] else { panic!() };
        assert!(r.is_grouped);
        assert_eq!(names(&r.targets), vec!["x.tab.c", "x.tab.h"]);
    }

    #[test]
    fn target_specific_variable() {
        let src = "debug: CFLAGS += -g\nprog: export override V := 1\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        let Item::Assignment(a) = &file.items[0] else { panic!() };
        assert_eq!(a.targets[0].name, "debug");
        assert_eq!(a.name.name, "CFLAGS");
        assert_eq!(a.op, AssignOp::Append);
        assert_eq!(a.value, "-g");
        let Item::Assignment(a) = &file.items[1] else { panic!() };
        assert_eq!(a.modifiers, vec![Modifier::Export, Modifier::Override]);
    }

    #[test]
    fn conditional_inside_recipe() {
        let src = "all:\n\techo start\nifeq ($(X),1)\n\techo one\nelse\n\techo other\nendif\n\techo end\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        let Item::Rule(r) = &file.items[0] else { panic!() };
        assert_eq!(r.recipe_lines.len(), 4);
        assert_eq!(r.span.end as usize, src.len());
    }

    #[test]
    fn tab_indented_statements_without_rule() {
        let src = "ifeq ($(OS),Windows_NT)\n\tEXE = .exe\n\t# comment\nendif\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        let Item::Conditional(c) = &file.items[0] else { panic!() };
        assert!(matches!(&c.then_branch[0], Item::Assignment(a) if a.name.name == "EXE"));
    }

    #[test]
    fn else_if_chain_is_closed_once() {
        let src = "ifeq (a,b)\nA = 1\nelse ifdef B\nA = 2\nelse\nA = 3\nendif\nC = 4\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        assert_eq!(file.items.len(), 2);
        let Item::Conditional(c) = &file.items[0] else { panic!() };
        let Item::Conditional(inner) = &c.else_branch[0] else { panic!() };
        assert_eq!(inner.kind, ConditionalKind::Ifdef);
        assert_eq!(inner.else_branch.len(), 1);
    }

    #[test]
    fn recipe_prefix_can_be_changed() {
        let src = ".RECIPEPREFIX = >\nall:\n> echo hi\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        let Item::Rule(r) = &file.items[1] else { panic!() };
        assert_eq!(r.recipe_lines.len(), 1);
    }

    #[test]
    fn space_indented_assignment_after_rule_is_not_a_recipe() {
        let src = "all:\n\techo\n  FOO = 1\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        assert!(matches!(&file.items[1], Item::Assignment(_)));
    }

    #[test]
    fn conditional_argument_forms() {
        let src = "ifeq ($(A),$(call f,x,(y)))\nendif\n\
                   ifneq \"a\" 'b'\nendif\n\
                   ifeq '$(A)' \"\"\nendif\n\
                   ifdef $(call name, x)\nendif\n\
                   ifndef X # comment\nendif\n";
        let (file, diags) = parse(src);
        assert_clean(&diags);
        assert_eq!(file.items.len(), 5);
    }

    #[test]
    fn invalid_conditionals_are_diagnosed() {
        for src in [
            "ifeq a b\nendif\n",
            "ifeq (a,b\nendif\n",
            "ifneq \"a\" b\nendif\n",
            "ifeq\nendif\n",
            "ifdef A B\nendif\n",
            "ifeq(a,b)\nendif\n",
        ] {
            let (file, diags) = parse(src);
            assert!(matches!(&file.items[0], Item::Conditional(_)), "{src:?}");
            let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
            assert_eq!(codes, vec![DiagnosticCode::InvalidConditional], "{src:?}");
        }
        let (_, diags) = parse("ifeq (a,b) junk\nendif\n");
        assert_eq!(diags[0].code, DiagnosticCode::ExtraneousText);
    }

    #[test]
    fn conditional_keywords_need_a_word_boundary() {
        let (file, diags) = parse("ifdef: x\n\techo\nifeqx = 1\n");
        assert_clean(&diags);
        assert!(matches!(&file.items[0], Item::Rule(r) if r.targets[0].name == "ifdef"));
        assert!(matches!(&file.items[1], Item::Assignment(_)));
    }

    #[test]
    fn else_must_chain_a_valid_conditional() {
        let (_, diags) = parse("ifdef A\nelse endif\nendif\n");
        assert_eq!(diags[0].code, DiagnosticCode::ExtraneousText);
        let (_, diags) = parse("ifdef A\nelse ifeq x\nendif\n");
        assert_eq!(diags[0].code, DiagnosticCode::InvalidConditional);
    }

    #[test]
    fn load_directive() {
        let (file, diags) = parse("-load mk_temp.so\nload: x\n");
        assert_clean(&diags);
        assert!(matches!(&file.items[0], Item::Directive(d) if d.kind == DirectiveKind::Load));
        assert!(matches!(&file.items[1], Item::Rule(_)));
    }
}
