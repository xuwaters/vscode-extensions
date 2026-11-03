//! Conditional translation: which of a file's `@if`, `@elif` and `@else`
//! branches survive a given set of features.
//!
//! WESL allows the attributes on nearly anything — a declaration, a `{ … }`
//! group of them, a statement, a struct member, a parameter — so this works on
//! the token stream rather than on the declarations [`wgsl_syntax`] records.
//! An attributed element runs from its first attribute to whatever ends it in
//! context: a `,` among members and parameters, a `;` or a closing block among
//! statements and declarations.
//!
//! The answer is two lists of spans. **Removed** spans are elements whose
//! condition is false, attributes and all. **Blanked** spans are what is left
//! of WESL in a surviving element: the condition attribute itself, and the
//! braces of a module-scope group, which WGSL has no use for.

use std::collections::{BTreeMap, HashMap};

use analyzer_core::spans::ByteSpan;
use wgsl_syntax::Parsed;
use wgsl_syntax::lexer::{Token, TokenKind};

/// What conditional translation does to one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Translation {
    pub removed: Vec<ByteSpan>,
    pub blanked: Vec<ByteSpan>,
}

impl Translation {
    /// Whether `span` is gone, inside an element whose condition is false.
    pub fn is_removed(&self, span: ByteSpan) -> bool {
        self.removed.iter().any(|removed| removed.start <= span.start && span.end <= removed.end)
    }
}

/// Why a file could not be translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable;

/// Translate a file under `features`; one not listed is false.
///
/// `Err` when a condition is not an expression this can read — the element
/// cannot then be kept or dropped with any confidence.
pub fn translate(
    parsed: &Parsed,
    source: &str,
    features: &BTreeMap<String, bool>,
) -> Result<Translation, Unreadable> {
    let toks: Vec<&Token> = parsed.tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
    let text = |i: usize| parsed.text(source, toks[i].span);
    let matching = match_brackets(&toks, source, parsed);
    let enclosing = enclosing_opens(&toks, &matching, source, parsed);

    let mut translation = Translation::default();
    // Per enclosing bracket — `None` is module scope — whether a branch of the
    // `@if` chain open there has been taken yet.
    let mut chains: HashMap<Option<usize>, bool> = HashMap::new();
    let mut i = 0;
    while i < toks.len() {
        let attribute = text(i);
        if toks[i].kind != TokenKind::Attribute || !matches!(attribute, "@if" | "@elif" | "@else")
        {
            i += 1;
            continue;
        }
        let run_start = attribute_run_start(&toks, &matching, source, parsed, i);
        let condition_end = match toks.get(i + 1) {
            Some(_) if text(i + 1) == "(" => matching[i + 1].ok_or(Unreadable)?,
            _ if attribute == "@else" => i,
            _ => return Err(Unreadable),
        };
        let element = attribute_run_end(&toks, &matching, source, parsed, condition_end + 1);
        let scope = enclosing[run_start];
        let end = element_end(&toks, &matching, source, parsed, element, scope);

        let taken = chains.get(&scope).copied();
        let keep = match attribute {
            "@if" => evaluate(&toks, source, parsed, i + 2, condition_end, features)?,
            "@elif" => {
                taken == Some(false)
                    && evaluate(&toks, source, parsed, i + 2, condition_end, features)?
            }
            _ => taken == Some(false),
        };
        match attribute {
            "@else" => chains.remove(&scope),
            _ => chains.insert(scope, taken == Some(true) || keep),
        };

        let last = end.saturating_sub(1).max(element);
        if keep {
            let condition = ByteSpan::new(toks[i].span.start, toks[condition_end].span.end);
            translation.blanked.push(condition);
            // A module-scope group's braces are WESL's, not WGSL's.
            if scope.is_none() && text(element) == "{" {
                if let Some(close) = matching[element] {
                    translation.blanked.push(toks[element].span);
                    translation.blanked.push(toks[close].span);
                }
            }
            i = condition_end + 1;
        } else {
            let span = ByteSpan::new(toks[run_start].span.start, toks[last].span.end);
            translation.removed.push(span);
            i = end.max(i + 1);
        }
    }
    Ok(translation)
}

/// For every bracket, the index of its partner. `<` and `>` are not brackets
/// here: they are as often comparisons as template lists.
fn match_brackets(toks: &[&Token], source: &str, parsed: &Parsed) -> Vec<Option<usize>> {
    let mut matching = vec![None; toks.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (i, token) in toks.iter().enumerate() {
        match parsed.text(source, token.span) {
            "(" | "[" | "{" => stack.push(i),
            ")" | "]" | "}" => {
                if let Some(open) = stack.pop() {
                    matching[open] = Some(i);
                    matching[i] = Some(open);
                }
            }
            _ => {}
        }
    }
    matching
}

/// For every token, the bracket it sits directly inside, or `None` at module
/// scope.
fn enclosing_opens(
    toks: &[&Token],
    matching: &[Option<usize>],
    source: &str,
    parsed: &Parsed,
) -> Vec<Option<usize>> {
    let mut enclosing = Vec::with_capacity(toks.len());
    let mut stack: Vec<usize> = Vec::new();
    for (i, token) in toks.iter().enumerate() {
        let text = parsed.text(source, token.span);
        if matches!(text, ")" | "]" | "}") && matching[i].is_some() {
            stack.pop();
        }
        enclosing.push(stack.last().copied());
        if matches!(text, "(" | "[" | "{") && matching[i].is_some() {
            stack.push(i);
        }
    }
    enclosing
}

/// The first attribute of the run the attribute at `i` belongs to:
/// `@group(0) @if(x) @binding(0) var …` starts at `@group`.
fn attribute_run_start(
    toks: &[&Token],
    matching: &[Option<usize>],
    source: &str,
    parsed: &Parsed,
    i: usize,
) -> usize {
    let mut start = i;
    while let Some(previous) = start.checked_sub(1) {
        let attribute = if parsed.text(source, toks[previous].span) == ")" {
            match matching[previous] {
                Some(open) if open >= 1 => open - 1,
                _ => break,
            }
        } else {
            previous
        };
        if toks[attribute].kind != TokenKind::Attribute {
            break;
        }
        start = attribute;
    }
    start
}

/// The first token after the run of attributes continuing at `i`.
fn attribute_run_end(
    toks: &[&Token],
    matching: &[Option<usize>],
    source: &str,
    parsed: &Parsed,
    mut i: usize,
) -> usize {
    while i < toks.len() && toks[i].kind == TokenKind::Attribute {
        i += 1;
        if i < toks.len() && parsed.text(source, toks[i].span) == "(" {
            i = matching[i].map_or(toks.len(), |close| close + 1);
        }
    }
    i
}

/// One past the last token of the element starting at `first`, whose
/// enclosing bracket is `scope`.
fn element_end(
    toks: &[&Token],
    matching: &[Option<usize>],
    source: &str,
    parsed: &Parsed,
    first: usize,
    scope: Option<usize>,
) -> usize {
    let text = |i: usize| parsed.text(source, toks[i].span);
    // Members and parameters are separated by commas, and a comma inside one
    // can only sit in a template list: `array<f32, 4>`.
    let listed = scope.is_some_and(|open| match text(open) {
        "(" => true,
        "{" => open >= 2 && text(open - 2) == "struct",
        _ => false,
    });
    let opener = if first < toks.len() { text(first) } else { "" };
    let block = matches!(
        opener,
        "fn" | "struct" | "if" | "for" | "while" | "loop" | "switch" | "{" | "continuing"
            | "case" | "default"
    );

    let mut template = 0usize;
    let mut i = first;
    while i < toks.len() {
        match text(i) {
            ")" | "]" | "}" => return i,
            ";" => return i + 1,
            "," if listed && template == 0 => return i + 1,
            "<" if listed
                && i > first
                && matches!(toks[i - 1].kind, TokenKind::Ident | TokenKind::Type) =>
            {
                template += 1;
            }
            ">" if template > 0 => template -= 1,
            "(" | "[" | "{" => {
                let Some(close) = matching[i] else { return toks.len() };
                if text(i) == "{" && block && !listed {
                    // `if … {} else if … {} else {}` is one statement.
                    let chained =
                        opener == "if" && close + 1 < toks.len() && text(close + 1) == "else";
                    if !chained {
                        return close + 1;
                    }
                }
                i = close;
            }
            _ => {}
        }
        i += 1;
    }
    toks.len()
}

/// Evaluate the condition between tokens `from` and `to` (exclusive): feature
/// names, `true`, `false`, `!`, `&&`, `||` and parentheses.
fn evaluate(
    toks: &[&Token],
    source: &str,
    parsed: &Parsed,
    from: usize,
    to: usize,
    features: &BTreeMap<String, bool>,
) -> Result<bool, Unreadable> {
    let words: Vec<&str> = toks[from..to].iter().map(|t| parsed.text(source, t.span)).collect();
    let mut parser = Condition { words: &words, at: 0, features };
    let value = parser.or()?;
    // A trailing comma is allowed in an attribute's arguments.
    if parser.peek() == Some(",") {
        parser.at += 1;
    }
    if parser.at != words.len() {
        return Err(Unreadable);
    }
    Ok(value)
}

/// Recursive descent over a condition's tokens. `&&` and `||` arrive as two
/// one-character tokens apiece.
struct Condition<'a> {
    words: &'a [&'a str],
    at: usize,
    features: &'a BTreeMap<String, bool>,
}

impl<'a> Condition<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.words.get(self.at).copied()
    }

    fn pair(&mut self, symbol: &str) -> bool {
        let found = self.peek() == Some(symbol) && self.words.get(self.at + 1) == Some(&symbol);
        if found {
            self.at += 2;
        }
        found
    }

    fn or(&mut self) -> Result<bool, Unreadable> {
        let mut value = self.and()?;
        while self.pair("|") {
            value |= self.and()?;
        }
        Ok(value)
    }

    fn and(&mut self) -> Result<bool, Unreadable> {
        let mut value = self.unary()?;
        while self.pair("&") {
            value &= self.unary()?;
        }
        Ok(value)
    }

    fn unary(&mut self) -> Result<bool, Unreadable> {
        let word = self.peek().ok_or(Unreadable)?;
        self.at += 1;
        match word {
            "!" => Ok(!self.unary()?),
            "(" => {
                let value = self.or()?;
                if self.peek() != Some(")") {
                    return Err(Unreadable);
                }
                self.at += 1;
                Ok(value)
            }
            "true" => Ok(true),
            "false" => Ok(false),
            name if name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') => {
                Ok(self.features.get(name).copied().unwrap_or(false))
            }
            _ => Err(Unreadable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgsl_syntax::Language;

    /// The source with every removed span cut and every blanked span
    /// replaced by spaces, whitespace runs then folded for comparison.
    fn translated(source: &str, features: &[(&str, bool)]) -> String {
        let parsed = wgsl_syntax::parse(source, Language::Wesl);
        let features = features.iter().map(|(name, on)| (name.to_string(), *on)).collect();
        let translation = translate(&parsed, source, &features).expect("readable");
        let mut out = String::new();
        for (i, c) in source.char_indices() {
            let at = ByteSpan::new(i as u32, i as u32 + 1);
            if translation.is_removed(at) {
                continue;
            }
            if translation.blanked.iter().any(|b| b.start <= at.start && at.end <= b.end) {
                out.push(' ');
            } else {
                out.push(c);
            }
        }
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn an_if_else_chain_keeps_exactly_one_branch() {
        let source = "@if(a) const N = 1;\n@elif(b) const N = 2;\n@else const N = 3;\n";
        assert_eq!(translated(source, &[("a", true), ("b", true)]), "const N = 1;");
        assert_eq!(translated(source, &[("b", true)]), "const N = 2;");
        assert_eq!(translated(source, &[]), "const N = 3;");
    }

    #[test]
    fn conditions_combine_with_not_and_or() {
        let source = "@if(!a && (b || c)) fn f() {}\n";
        assert_eq!(translated(source, &[("c", true)]), "fn f() {}");
        assert_eq!(translated(source, &[("a", true), ("c", true)]), "");
    }

    #[test]
    fn other_attributes_go_with_the_element() {
        let source = "@group(0) @if(debug) @binding(1) var<storage, read_write> log: array<u32>;\n";
        assert_eq!(translated(source, &[]), "");
        assert_eq!(
            translated(source, &[("debug", true)]),
            "@group(0) @binding(1) var<storage, read_write> log: array<u32>;"
        );
    }

    #[test]
    fn a_module_scope_group_loses_its_braces() {
        let source = "@if(a) {\n    const X = 1;\n    fn g() {}\n}\nfn h() {}\n";
        assert_eq!(translated(source, &[("a", true)]), "const X = 1; fn g() {} fn h() {}");
        assert_eq!(translated(source, &[]), "fn h() {}");
    }

    #[test]
    fn statements_members_and_parameters_are_elements_too() {
        let source = "struct S {\n    a: f32,\n    @if(x) b: array<f32, 4>,\n    c: f32,\n}\n\
                      fn f(@if(x) p: f32, q: f32) {\n    @if(x) { let y = 1; }\n    \
                      @if(x) if q > 0.0 { return; } else { return; }\n    let z = 2;\n}\n";
        assert_eq!(
            translated(source, &[]),
            "struct S { a: f32, c: f32, } fn f( q: f32) { let z = 2; }"
        );
    }

    #[test]
    fn an_unreadable_condition_is_an_error() {
        let parsed = wgsl_syntax::parse("@if(a + 1) fn f() {}\n", Language::Wesl);
        let source = "@if(a + 1) fn f() {}\n";
        assert_eq!(translate(&parsed, source, &BTreeMap::new()), Err(Unreadable));
    }
}
