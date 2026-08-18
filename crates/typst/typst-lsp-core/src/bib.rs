//! A BibTeX reader, for the `.bib` files typst's `bibliography()` loads.
//!
//! Typst compiles bibliographies through `biblatex`/`hayagriva`, neither of
//! which is built for an editor: they answer "does this file parse", not "what
//! is at byte 412, and where does the entry under the cursor start and end".
//! Every IDE feature here needs the second question answered, so the file gets
//! its own forgiving parser.
//!
//! Three properties matter, and they are what a compiler-grade parser will not
//! give you:
//!
//! * **Ranges for everything.** The entry type, the citation key, each field
//!   name, each value — all carry byte ranges, because symbols, folding, rename,
//!   and semantic tokens are all just ranges in the end.
//! * **Error recovery.** A file is half-typed most of the time it is parsed. An
//!   unclosed entry stops at the next `@` that starts a line rather than
//!   swallowing the rest of the file, so the entries below it keep working.
//! * **Every token, including the gaps.** Text outside an entry is a comment in
//!   BibTeX, and colouring it as one is how a reader sees that their stray
//!   paragraph is being ignored.

use std::collections::HashMap;
use std::ops::Range;

/// A parsed `.bib` file.
#[derive(Debug, Clone, Default)]
pub struct Bib {
    /// Every entry, in document order.
    pub entries: Vec<Entry>,
    /// Syntax errors and lint warnings, in document order.
    pub problems: Vec<Problem>,
    /// The whole file as non-overlapping coloured spans, in document order.
    pub tokens: Vec<Token>,
}

/// What kind of `@…` block an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A reference: `@article`, `@book`, and the rest.
    Reference,
    /// `@string{ key = "value" }`, an abbreviation other values may use.
    String,
    /// `@preamble{ "…" }`, LaTeX passed straight through.
    Preamble,
    /// `@comment{ … }`, ignored by every consumer.
    Comment,
}

/// One `@…{…}` block.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Which of the four kinds this is.
    pub kind: EntryKind,
    /// The type name, lowercased: `article`, `inproceedings`, `string`.
    pub type_name: String,
    /// The type's range, `@` included, so `@article` highlights as one token.
    pub type_range: Range<usize>,
    /// The citation key. `None` for `@string`, `@preamble`, `@comment`, and for
    /// a reference that has not been given one yet.
    pub key: Option<String>,
    /// Where the key is, or where it would go if it is missing — an empty range
    /// just after the opening brace, so a diagnostic still has somewhere to sit.
    pub key_range: Range<usize>,
    /// The fields, in document order. Duplicated names are kept as written; the
    /// lint reports them rather than the parser hiding them.
    pub fields: Vec<Field>,
    /// The whole block, from `@` to the closing delimiter.
    pub range: Range<usize>,
}

/// One `name = value` pair inside an entry.
#[derive(Debug, Clone)]
pub struct Field {
    /// The field name, lowercased. BibTeX field names are case-insensitive.
    pub name: String,
    /// The name as written, for edits that must not change the file's spelling.
    pub written: String,
    /// The name's range.
    pub name_range: Range<usize>,
    /// The value, absent when the field is still being typed.
    pub value: Option<Value>,
}

/// A field's value: one or more parts joined by `#`.
#[derive(Debug, Clone)]
pub struct Value {
    /// The full range, delimiters and `#` joins included.
    pub range: Range<usize>,
    /// The value with delimiters removed, `@string` abbreviations expanded, and
    /// whitespace collapsed — what a reader wants to see in a tooltip.
    pub text: String,
    /// The parts, kept so abbreviations can be expanded once the whole file has
    /// been read: a `@string` may be defined below the entry that uses it.
    parts: Vec<Part>,
}

/// One piece of a concatenated value.
#[derive(Debug, Clone)]
enum Part {
    /// A braced, quoted, or numeric literal, already cleaned.
    Literal(String),
    /// A bare identifier: a reference to an `@string` abbreviation.
    Macro(String),
}

/// A syntax error or a lint warning.
#[derive(Debug, Clone)]
pub struct Problem {
    /// Where to underline.
    pub range: Range<usize>,
    /// What to say.
    pub message: String,
    /// How loudly to say it.
    pub severity: Severity,
    /// A second place worth pointing at — the first of two duplicate keys.
    pub related: Option<(Range<usize>, String)>,
}

/// How serious a [`Problem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The file does not parse, or says something contradictory.
    Error,
    /// The file parses, but something is probably wrong.
    Warning,
}

/// A coloured span. Non-overlapping, in document order.
#[derive(Debug, Clone)]
pub struct Token {
    /// The span.
    pub range: Range<usize>,
    /// What it is.
    pub kind: TokenKind,
}

/// What a [`Token`] is, in the vocabulary the semantic-token legend speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// `@article` — the `@` and the type name together.
    EntryType,
    /// A citation key.
    Key,
    /// A field name.
    FieldName,
    /// A braced or quoted value.
    Value,
    /// A bare number value.
    Number,
    /// A bare identifier value: a reference to an `@string` abbreviation.
    Macro,
    /// `{`, `}`, `(`, `)`, `=`, `,`, `#`.
    Punct,
    /// Text outside any entry, and the body of `@comment`. BibTeX ignores both.
    Comment,
}

impl Bib {
    /// Parse a `.bib` file. Never fails: what does not parse becomes a
    /// [`Problem`] and the parser moves on.
    pub fn parse(text: &str) -> Self {
        let mut bib = Parser::new(text).run();
        bib.expand_abbreviations();
        bib.lint();
        bib
    }

    /// The reference entries, skipping `@string`, `@preamble`, and `@comment`.
    pub fn references(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| entry.kind == EntryKind::Reference)
    }

    /// The entry with a given citation key.
    pub fn find(&self, key: &str) -> Option<&Entry> {
        self.references().find(|entry| entry.key.as_deref() == Some(key))
    }

    /// The `@string` abbreviation with a given name, and its defining field.
    pub fn abbreviation(&self, name: &str) -> Option<(&Entry, &Field)> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::String)
            .find_map(|entry| {
                let field = entry.fields.iter().find(|field| field.name == name)?;
                Some((entry, field))
            })
    }

    /// The innermost entry containing an offset.
    pub fn entry_at(&self, offset: usize) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.range.contains(&offset))
    }

    /// Whether the file parsed without syntax errors. Warnings do not count —
    /// the formatter refuses on errors alone.
    pub fn is_valid(&self) -> bool {
        !self.problems.iter().any(|problem| problem.severity == Severity::Error)
    }

    /// Substitute `@string` abbreviations into the values that name them.
    ///
    /// A second pass because a file may use an abbreviation above the line that
    /// defines it, and because only the display text changes — the ranges, and
    /// so every edit built from them, still describe the file as written.
    fn expand_abbreviations(&mut self) {
        let definitions: HashMap<String, String> = self
            .entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::String)
            .flat_map(|entry| entry.fields.iter())
            // The *uncollapsed* text: `@string{acm = "ACM "}` ends in a space
            // that `acm # {Press}` is relying on.
            .filter_map(|field| Some((field.name.clone(), field.value.as_ref()?.raw())))
            .collect();

        if definitions.is_empty() {
            return;
        }

        for entry in &mut self.entries {
            for field in &mut entry.fields {
                let Some(value) = field.value.as_mut() else { continue };
                let names_one = value.parts.iter().any(|part| match part {
                    Part::Macro(name) => definitions.contains_key(name),
                    Part::Literal(_) => false,
                });
                if !names_one {
                    continue;
                }

                let expanded: String = value
                    .parts
                    .iter()
                    .map(|part| match part {
                        Part::Literal(text) => text.as_str(),
                        // An abbreviation we cannot resolve stays as written,
                        // which is both honest and what the file says.
                        Part::Macro(name) => {
                            definitions.get(name).map(String::as_str).unwrap_or(name)
                        }
                    })
                    .collect();

                value.text = collapse(&expanded);
            }
        }
    }

    /// Duplicate keys, missing required fields, unknown entry types.
    fn lint(&mut self) {
        let mut seen: HashMap<&str, Range<usize>> = HashMap::new();
        let mut found = Vec::new();

        for entry in self.references() {
            let Some(key) = entry.key.as_deref() else { continue };

            match seen.get(key) {
                Some(first) => found.push(Problem {
                    range: entry.key_range.clone(),
                    message: format!("duplicate citation key `{key}`"),
                    severity: Severity::Error,
                    related: Some((first.clone(), "first defined here".into())),
                }),
                None => {
                    seen.insert(key, entry.key_range.clone());
                }
            }

            if !ENTRY_TYPES.iter().any(|(name, _)| *name == entry.type_name) {
                found.push(Problem {
                    range: entry.type_range.clone(),
                    message: format!("unknown entry type `{}`", entry.type_name),
                    severity: Severity::Warning,
                    related: None,
                });
                // Required fields are meaningless for a type we do not know.
                continue;
            }

            for alternatives in required_fields(&entry.type_name) {
                if alternatives.iter().any(|name| entry.field(name).is_some()) {
                    continue;
                }
                found.push(Problem {
                    range: entry.key_range.clone(),
                    message: format!(
                        "`@{}` is missing {}",
                        entry.type_name,
                        or_list(alternatives)
                    ),
                    severity: Severity::Warning,
                    related: None,
                });
            }

            let mut names: HashMap<&str, Range<usize>> = HashMap::new();
            for field in &entry.fields {
                match names.get(field.name.as_str()) {
                    Some(first) => found.push(Problem {
                        range: field.name_range.clone(),
                        message: format!("duplicate field `{}`", field.name),
                        severity: Severity::Warning,
                        related: Some((first.clone(), "first set here".into())),
                    }),
                    None => {
                        names.insert(field.name.as_str(), field.name_range.clone());
                    }
                }
            }
        }

        self.problems.extend(found);
        self.problems.sort_by_key(|problem| problem.range.start);
    }
}

impl Value {
    /// The parts joined with no whitespace normalisation, which is what an
    /// abbreviation's definition has to keep: its trailing space is deliberate.
    fn raw(&self) -> String {
        self.parts
            .iter()
            .map(|part| match part {
                Part::Literal(text) | Part::Macro(text) => text.as_str(),
            })
            .collect()
    }
}

impl Entry {
    /// A field by name, matched case-insensitively as BibTeX does.
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }

    /// A field's cleaned value.
    pub fn value(&self, name: &str) -> Option<&str> {
        Some(self.field(name)?.value.as_ref()?.text.as_str())
    }

    /// The first of several fields that is present — `year` or `date`, the
    /// pairs where biblatex and BibTeX disagree.
    fn either(&self, names: &[&str]) -> Option<&str> {
        names.iter().find_map(|name| self.value(name))
    }

    /// A one-line description, for a completion item or a symbol's detail.
    ///
    /// `Knuth — Literate Programming (1984)`, degrading gracefully as fields go
    /// missing, because half-written entries are the common case in an editor.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        if let Some(author) = self.either(&["author", "editor"]) {
            parts.push(short_authors(author));
        }
        if let Some(title) = self.either(&["title", "booktitle"]) {
            parts.push(title.to_string());
        }
        if let Some(year) = self.either(&["year", "date"]) {
            parts.push(format!("({})", year_of(year)));
        }

        if parts.is_empty() {
            return format!("@{}", self.type_name);
        }
        parts.join(" — ")
    }

    /// The tooltip: a heading, a citation-shaped line, then every field.
    pub fn markdown(&self) -> String {
        let mut out = String::new();

        match self.key.as_deref() {
            Some(key) => out.push_str(&format!("**`@{key}`** · `{}`\n\n", self.type_name)),
            None => out.push_str(&format!("**`@{}`**\n\n", self.type_name)),
        }

        if let Some(title) = self.value("title") {
            out.push_str(&format!("**{title}**\n\n"));
        }
        if let Some(reference) = self.reference_line() {
            out.push_str(&reference);
            out.push_str("\n\n");
        }

        for field in &self.fields {
            let Some(value) = field.value.as_ref() else { continue };
            if field.name == "title" {
                continue;
            }
            out.push_str(&format!("- `{}`: {}\n", field.name, truncate(&value.text, 200)));
        }

        out
    }

    /// The prose line under the title: authors, container, year.
    fn reference_line(&self) -> Option<String> {
        let mut parts = Vec::new();

        if let Some(author) = self.value("author") {
            parts.push(author.replace(" and ", "; "));
        } else if let Some(editor) = self.value("editor") {
            parts.push(format!("{} (ed.)", editor.replace(" and ", "; ")));
        }

        if let Some(container) =
            self.either(&["journal", "journaltitle", "booktitle", "publisher", "school"])
        {
            parts.push(container.to_string());
        }
        if let Some(year) = self.either(&["year", "date"]) {
            parts.push(year.to_string());
        }

        (!parts.is_empty()).then(|| parts.join(". "))
    }
}

/// `Knuth, Donald E. and Lamport, Leslie` → `Knuth & Lamport`.
///
/// Surnames only, and never more than two of them: a completion list is read at
/// a glance, and a full author list is three lines of noise in it.
fn short_authors(authors: &str) -> String {
    let names: Vec<&str> = authors.split(" and ").map(str::trim).collect();
    let surname = |name: &str| -> String {
        match name.split_once(',') {
            // `Knuth, Donald E.` — already surname first.
            Some((last, _)) => last.trim().to_string(),
            // `Donald E. Knuth` — the last word is the surname.
            None => name.rsplit(' ').next().unwrap_or(name).to_string(),
        }
    };

    match names.as_slice() {
        [] => String::new(),
        [one] => surname(one),
        [one, two] => format!("{} & {}", surname(one), surname(two)),
        [one, ..] => format!("{} et al.", surname(one)),
    }
}

/// The year inside a biblatex `date` field: `2022-06-01` → `2022`.
fn year_of(date: &str) -> &str {
    date.split(['-', '/']).next().unwrap_or(date).trim()
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept}…")
}

/// `author`, or `author` or `editor` — the phrasing a diagnostic needs.
fn or_list(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
    match quoted.as_slice() {
        [] => String::new(),
        [one] => format!("a {one} field"),
        _ => format!("a {} field", quoted.join(" or ")),
    }
}

// ── The parser ───────────────────────────────────────────────────────────────

struct Parser<'a> {
    text: &'a str,
    pos: usize,
    out: Bib,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, pos: 0, out: Bib::default() }
    }

    fn run(mut self) -> Bib {
        loop {
            // Everything up to the next `@` is a comment, per BibTeX's rule that
            // text outside an entry is ignored.
            let start = self.pos;
            while let Some(character) = self.peek() {
                if character == '@' {
                    break;
                }
                self.advance();
            }
            self.push_token(start..self.pos, TokenKind::Comment);

            if self.peek().is_none() {
                break;
            }
            self.entry();
        }

        self.out
    }

    /// One `@…` block, starting at the `@`.
    fn entry(&mut self) {
        let start = self.pos;
        self.advance(); // `@`

        let name_start = self.pos;
        self.eat_while(|character| character.is_ascii_alphabetic());
        let type_name = self.text[name_start..self.pos].to_ascii_lowercase();
        let type_range = start..self.pos;
        self.push_token(type_range.clone(), TokenKind::EntryType);

        if type_name.is_empty() {
            self.error(type_range.clone(), "expected an entry type after `@`");
            return;
        }

        let kind = match type_name.as_str() {
            "string" => EntryKind::String,
            "preamble" => EntryKind::Preamble,
            "comment" => EntryKind::Comment,
            _ => EntryKind::Reference,
        };

        self.skip_whitespace();
        let Some(open) = self.peek().filter(|character| matches!(character, '{' | '(')) else {
            self.error(
                type_range.clone(),
                format!("expected `{{` after `@{type_name}`"),
            );
            return;
        };
        let close = if open == '{' { '}' } else { ')' };
        self.punct();

        let mut entry = Entry {
            kind,
            type_name,
            type_range,
            key: None,
            // Where a missing key would go, so a diagnostic has a home.
            key_range: self.pos..self.pos,
            fields: Vec::new(),
            range: start..self.pos,
        };

        match kind {
            // `@comment{…}` is free text; colour it and skip to the close.
            EntryKind::Comment => {
                let body = self.pos;
                self.skip_balanced(open, close);
                let end = self.text[..self.pos].trim_end_matches(close).len();
                self.push_token(body..end, TokenKind::Comment);
            }
            // `@preamble{"…"}` is one value with no name.
            EntryKind::Preamble => {
                self.skip_whitespace();
                self.value();
                self.expect_close(close, &entry);
            }
            // `@string` has fields but no key; a reference has both.
            EntryKind::String => {
                self.fields(close, &mut entry);
            }
            EntryKind::Reference => {
                self.key(close, &mut entry);
                self.fields(close, &mut entry);
            }
        }

        entry.range = start..self.pos;
        self.out.entries.push(entry);
    }

    /// The citation key, up to the first `,` or the closing delimiter.
    fn key(&mut self, close: char, entry: &mut Entry) {
        self.skip_whitespace();
        let start = self.pos;
        while let Some(character) = self.peek() {
            if character == ',' || character == close || character.is_whitespace() {
                break;
            }
            self.advance();
        }

        let key = &self.text[start..self.pos];
        entry.key_range = start..self.pos;

        if key.is_empty() {
            self.error(start..start, "expected a citation key");
            return;
        }

        self.push_token(start..self.pos, TokenKind::Key);
        entry.key = Some(key.to_string());
    }

    /// The `name = value` list, up to the closing delimiter.
    fn fields(&mut self, close: char, entry: &mut Entry) {
        loop {
            self.skip_whitespace();

            match self.peek() {
                None => {
                    self.unclosed(entry);
                    return;
                }
                Some(character) if character == close => {
                    self.punct();
                    return;
                }
                Some(',') => {
                    self.punct();
                    continue;
                }
                // A new entry starting at column 0 means the one above it was
                // never closed. Stopping here keeps the rest of the file working.
                Some('@') if self.at_line_start() => {
                    self.unclosed(entry);
                    return;
                }
                Some(_) => {}
            }

            let start = self.pos;
            self.eat_while(is_name_char);
            if self.pos == start {
                // Nothing consumable: report it once and step over it, or the
                // loop would spin on the same byte forever.
                let range = start..self.next_boundary();
                self.error(range, "unexpected character");
                self.advance();
                continue;
            }

            let name_range = start..self.pos;
            let written = self.text[name_range.clone()].to_string();
            self.push_token(name_range.clone(), TokenKind::FieldName);

            self.skip_whitespace();
            if self.peek() != Some('=') {
                self.error(name_range.clone(), format!("expected `=` after `{written}`"));
                entry.fields.push(Field {
                    name: written.to_ascii_lowercase(),
                    written,
                    name_range,
                    value: None,
                });
                continue;
            }
            self.punct();

            self.skip_whitespace();
            let value = self.value();

            entry.fields.push(Field {
                name: written.to_ascii_lowercase(),
                written,
                name_range,
                value,
            });
        }
    }

    /// One value expression: parts joined by `#`.
    fn value(&mut self) -> Option<Value> {
        let start = self.pos;
        let mut parts = Vec::new();

        loop {
            self.skip_whitespace();
            let Some(character) = self.peek() else { break };

            match character {
                '{' | '"' => {
                    let range = self.delimited(character);
                    self.push_token(range.clone(), TokenKind::Value);
                    parts.push(Part::Literal(clean(&self.text[range])));
                }
                character if is_name_char(character) => {
                    let part = self.pos;
                    self.eat_while(is_name_char);
                    let text = &self.text[part..self.pos];

                    // A bare run is a number if it is all digits — `year = 1984`
                    // — and an abbreviation otherwise — `month = jan`.
                    if text.chars().all(|character| character.is_ascii_digit()) {
                        self.push_token(part..self.pos, TokenKind::Number);
                        parts.push(Part::Literal(text.to_string()));
                    } else {
                        self.push_token(part..self.pos, TokenKind::Macro);
                        parts.push(Part::Macro(text.to_string()));
                    }
                }
                _ => break,
            }

            self.skip_whitespace();
            // A `#` means another part follows; anything else ends the value.
            if self.peek() == Some('#') {
                self.punct();
                continue;
            }
            break;
        }

        if self.pos == start {
            self.error(start..self.next_boundary(), "expected a value");
            return None;
        }

        let text: String = parts
            .iter()
            .map(|part| match part {
                Part::Literal(text) | Part::Macro(text) => text.as_str(),
            })
            .collect();

        Some(Value {
            range: start..self.pos,
            text: collapse(&text),
            parts,
        })
    }

    /// A `{…}` or `"…"` run, returning its full range including delimiters.
    fn delimited(&mut self, open: char) -> Range<usize> {
        let start = self.pos;
        let close = if open == '{' { '}' } else { '"' };
        self.advance();

        let mut depth = 1usize;
        while let Some(character) = self.peek() {
            match character {
                // A backslash escapes the next character, `\{` included.
                '\\' => {
                    self.advance();
                    if self.peek().is_some() {
                        self.advance();
                    }
                    continue;
                }
                // Braces nest inside both forms; quotes only close at depth 1.
                '{' => depth += 1,
                '}' if close == '}' => {
                    depth -= 1;
                    if depth == 0 {
                        self.advance();
                        return start..self.pos;
                    }
                }
                '}' => depth = depth.saturating_sub(1),
                '"' if close == '"' && depth == 1 => {
                    self.advance();
                    return start..self.pos;
                }
                _ => {}
            }
            self.advance();
        }

        self.error(start..self.pos, "unterminated value");
        start..self.pos
    }

    /// Skip a balanced `{…}` run without recording anything inside it.
    fn skip_balanced(&mut self, open: char, close: char) {
        let mut depth = 1usize;
        while let Some(character) = self.peek() {
            if character == open {
                depth += 1;
            } else if character == close {
                depth -= 1;
                if depth == 0 {
                    self.advance();
                    return;
                }
            }
            self.advance();
        }
    }

    fn expect_close(&mut self, close: char, entry: &Entry) {
        self.skip_whitespace();
        match self.peek() {
            Some(character) if character == close => self.punct(),
            _ => self.unclosed(entry),
        }
    }

    fn unclosed(&mut self, entry: &Entry) {
        self.error(
            entry.type_range.clone(),
            format!("unclosed `@{}` entry", entry.type_name),
        );
    }

    // ── Cursor primitives ────────────────────────────────────────────────────

    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn advance(&mut self) {
        if let Some(character) = self.peek() {
            self.pos += character.len_utf8();
        }
    }

    /// The offset one character on, without moving — for a range that must not
    /// be empty when it lands at the end of the file.
    fn next_boundary(&self) -> usize {
        match self.peek() {
            Some(character) => self.pos + character.len_utf8(),
            None => self.pos,
        }
    }

    fn eat_while(&mut self, mut allowed: impl FnMut(char) -> bool) {
        while let Some(character) = self.peek() {
            if !allowed(character) {
                break;
            }
            self.advance();
        }
    }

    fn skip_whitespace(&mut self) {
        self.eat_while(char::is_whitespace);
    }

    fn at_line_start(&self) -> bool {
        self.text[..self.pos].ends_with('\n') || self.pos == 0
    }

    /// Consume one punctuation character and colour it.
    fn punct(&mut self) {
        let start = self.pos;
        self.advance();
        self.push_token(start..self.pos, TokenKind::Punct);
    }

    fn push_token(&mut self, range: Range<usize>, kind: TokenKind) {
        if range.is_empty() {
            return;
        }
        self.out.tokens.push(Token { range, kind });
    }

    fn error(&mut self, range: Range<usize>, message: impl Into<String>) {
        self.out.problems.push(Problem {
            range,
            message: message.into(),
            severity: Severity::Error,
            related: None,
        });
    }
}

/// Characters a field name or an `@string` reference may contain.
fn is_name_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '+' | ':')
}

/// Strip a value's delimiters and the braces that only protect capitalisation.
///
/// `{The {ACM} Symposium}` → `The ACM Symposium`. The braces carry meaning for
/// the *typesetter*; for a tooltip they are noise.
fn clean(raw: &str) -> String {
    let inner = match raw.chars().next() {
        Some('{') => raw.strip_prefix('{').and_then(|r| r.strip_suffix('}')).unwrap_or(raw),
        Some('"') => raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(raw),
        _ => raw,
    };

    let mut out = String::with_capacity(inner.len());
    let mut characters = inner.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            // `\&` is an escaped literal; `\LaTeX` is a command we cannot render,
            // so its name is the best available approximation.
            '\\' => match characters.peek() {
                Some(next) if !next.is_alphanumeric() => {
                    out.push(*next);
                    characters.next();
                }
                _ => {}
            },
            '{' | '}' => {}
            // The BibTeX dash conventions, which every reader recognises.
            '-' if characters.peek() == Some(&'-') => {
                characters.next();
                if characters.peek() == Some(&'-') {
                    characters.next();
                    out.push('—');
                } else {
                    out.push('–');
                }
            }
            '~' => out.push(' '),
            _ => out.push(character),
        }
    }

    out
}

/// Collapse runs of whitespace, including the newlines a wrapped value carries.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Vocabulary ───────────────────────────────────────────────────────────────

/// Every entry type BibTeX and biblatex define, with a one-line description.
///
/// Both dialects, because typst reads `.bib` files through `biblatex` and a file
/// written for either compiles.
pub const ENTRY_TYPES: &[(&str, &str)] = &[
    ("article", "An article in a journal, magazine, or newspaper"),
    ("book", "A book with a publisher"),
    ("mvbook", "A multi-volume book"),
    ("inbook", "A part of a book with its own title"),
    ("bookinbook", "A book published as part of another book"),
    ("suppbook", "Supplemental material in a book"),
    ("booklet", "A printed work with no publisher"),
    ("collection", "A book of contributions by different authors"),
    ("mvcollection", "A multi-volume collection"),
    ("incollection", "A contribution to a collection"),
    ("suppcollection", "Supplemental material in a collection"),
    ("dataset", "A data set or similar collection of data"),
    ("manual", "Technical documentation"),
    ("misc", "Anything that fits no other type"),
    ("online", "An online resource"),
    ("patent", "A patent or patent request"),
    ("periodical", "A complete issue of a periodical"),
    ("suppperiodical", "Supplemental material in a periodical"),
    ("proceedings", "The published proceedings of a conference"),
    ("mvproceedings", "Multi-volume conference proceedings"),
    ("inproceedings", "A paper in conference proceedings"),
    ("reference", "A single-volume work of reference"),
    ("mvreference", "A multi-volume work of reference"),
    ("inreference", "An entry in a work of reference"),
    ("report", "A report issued by an institution"),
    ("set", "An entry set"),
    ("software", "A computer program"),
    ("thesis", "A thesis of any kind"),
    ("unpublished", "A work that has not been formally published"),
    // BibTeX's own types, still ubiquitous in the wild.
    ("conference", "A paper in conference proceedings (legacy `inproceedings`)"),
    ("electronic", "An online resource (legacy `online`)"),
    ("mastersthesis", "A master's thesis"),
    ("phdthesis", "A PhD thesis"),
    ("techreport", "A technical report (legacy `report`)"),
    ("www", "An online resource (legacy `online`)"),
];

/// The field names worth completing, with a one-line description.
pub const FIELDS: &[(&str, &str)] = &[
    ("address", "Publisher's address (legacy `location`)"),
    ("annote", "An annotation, for annotated bibliography styles"),
    ("author", "The authors, joined by ` and `"),
    ("booktitle", "The title of the book a contribution appears in"),
    ("chapter", "A chapter or section number"),
    ("crossref", "The key of another entry this one inherits from"),
    ("date", "The publication date, `YYYY-MM-DD` (biblatex)"),
    ("doi", "The Digital Object Identifier"),
    ("edition", "The edition, as a number or an ordinal"),
    ("editor", "The editors, joined by ` and `"),
    ("eprint", "The preprint identifier"),
    ("eprinttype", "The preprint archive, such as `arXiv`"),
    ("howpublished", "How an unusual work was published"),
    ("institution", "The issuing institution of a report"),
    ("isbn", "The International Standard Book Number"),
    ("issn", "The International Standard Serial Number"),
    ("issue", "The issue of a periodical"),
    ("journal", "The journal name (legacy `journaltitle`)"),
    ("journaltitle", "The journal name (biblatex)"),
    ("key", "A sort key, when there is no author"),
    ("language", "The work's language"),
    ("location", "The place of publication"),
    ("month", "The publication month"),
    ("note", "Anything the reader should know"),
    ("number", "The issue or report number"),
    ("organization", "The organisation behind a manual or conference"),
    ("pages", "The page range, as `97--111`"),
    ("publisher", "The publisher"),
    ("school", "The institution a thesis was written at"),
    ("series", "The series a book belongs to"),
    ("title", "The title"),
    ("translator", "The translators"),
    ("type", "The specific type of a report or thesis"),
    ("url", "The address the work can be read at"),
    ("urldate", "When the URL was last checked"),
    ("volume", "The volume of a journal or multi-volume book"),
    ("year", "The publication year (legacy `date`)"),
];

/// The fields an entry type is expected to carry.
///
/// Each inner slice is a set of alternatives: `year` **or** `date` satisfies the
/// same requirement, which is how a biblatex file and a BibTeX file can both be
/// complete. Types not listed here are never warned about.
pub fn required_fields(type_name: &str) -> &'static [&'static [&'static str]] {
    const AUTHOR: &[&str] = &["author"];
    const EDITOR: &[&str] = &["author", "editor"];
    const TITLE: &[&str] = &["title"];
    const BOOKTITLE: &[&str] = &["booktitle"];
    const YEAR: &[&str] = &["year", "date"];
    const JOURNAL: &[&str] = &["journal", "journaltitle"];
    const PUBLISHER: &[&str] = &["publisher"];
    const INSTITUTION: &[&str] = &["institution", "school"];
    const SCHOOL: &[&str] = &["school", "institution"];
    const NOTE: &[&str] = &["note"];

    match type_name {
        "article" => &[AUTHOR, TITLE, JOURNAL, YEAR],
        "book" | "mvbook" => &[EDITOR, TITLE, PUBLISHER, YEAR],
        "inbook" | "incollection" => &[AUTHOR, TITLE, BOOKTITLE, YEAR],
        "inproceedings" | "conference" => &[AUTHOR, TITLE, BOOKTITLE, YEAR],
        "proceedings" | "mvproceedings" => &[TITLE, YEAR],
        "report" | "techreport" => &[AUTHOR, TITLE, INSTITUTION, YEAR],
        "thesis" | "phdthesis" | "mastersthesis" => &[AUTHOR, TITLE, SCHOOL, YEAR],
        "unpublished" => &[AUTHOR, TITLE, NOTE],
        "manual" | "booklet" => &[TITLE],
        "online" | "electronic" | "www" => &[TITLE],
        _ => &[],
    }
}

// ── Formatting ───────────────────────────────────────────────────────────────

/// Rewrite a file in the canonical layout: one field per line, aligned by
/// indentation, a trailing comma on every field, one blank line between entries.
///
/// Returns `None` when the file has syntax errors — the same rule the typst
/// formatter follows, and for the same reason: reformatting a half-typed file
/// into something else is how a formatter gets turned off.
///
/// Values are copied **verbatim**, delimiters and all. Normalising them would
/// mean deciding what `{\"o}` should become, and a formatter that edits content
/// is not a formatter.
pub fn format(text: &str, indent: usize) -> Option<String> {
    let bib = Bib::parse(text);
    if !bib.is_valid() {
        return None;
    }

    let pad = " ".repeat(indent);
    let mut out = String::with_capacity(text.len());

    // Text outside entries — real comments — is kept, so a file with a licence
    // header at the top still has one afterwards.
    let mut cursor = 0;
    for entry in &bib.entries {
        let between = text[cursor..entry.range.start].trim();
        if !between.is_empty() {
            for line in between.lines() {
                out.push_str(line.trim_end());
                out.push('\n');
            }
            out.push('\n');
        }
        cursor = entry.range.end;

        // Only references get the canonical layout. `@string`, `@preamble`, and
        // `@comment` are copied exactly as written: `@string` does not take the
        // trailing comma a reference does, and the other two hold text whose
        // shape is the author's business.
        if entry.kind != EntryKind::Reference {
            out.push_str(text[entry.range.clone()].trim_end());
            out.push_str("\n\n");
            continue;
        }

        match entry.key.as_deref() {
            Some(key) => out.push_str(&format!("@{}{{{},\n", entry.type_name, key)),
            None => out.push_str(&format!("@{}{{\n", entry.type_name)),
        }

        for field in &entry.fields {
            let Some(value) = field.value.as_ref() else { continue };
            out.push_str(&format!(
                "{pad}{} = {},\n",
                field.name,
                collapse_value(&text[value.range.clone()])
            ));
        }

        out.push_str("}\n\n");
    }

    let trailing = text[cursor..].trim();
    if !trailing.is_empty() {
        for line in trailing.lines() {
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out.push('\n');
    }

    // Exactly one trailing newline, whatever the input had.
    Some(format!("{}\n", out.trim_end()))
}

/// Join a value's wrapped lines, keeping its delimiters and its content.
fn collapse_value(raw: &str) -> String {
    if !raw.contains('\n') {
        return raw.to_string();
    }
    raw.lines().map(str::trim).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"@article{knuth1984,
  author = {Knuth, Donald E.},
  title = {Literate Programming},
  journal = {The Computer Journal},
  volume = {27},
  number = {2},
  pages = {97--111},
  year = {1984},
}

@book{tufte2001,
  author = {Tufte, Edward R.},
  title = {The Visual Display of Quantitative Information},
  publisher = {Graphics Press},
  year = {2001},
}
"#;

    #[test]
    fn entries_carry_their_keys_types_and_fields() {
        let bib = Bib::parse(SAMPLE);
        assert_eq!(bib.entries.len(), 2);

        let entry = &bib.entries[0];
        assert_eq!(entry.type_name, "article");
        assert_eq!(entry.key.as_deref(), Some("knuth1984"));
        assert_eq!(entry.fields.len(), 7);
        assert_eq!(entry.value("title"), Some("Literate Programming"));
        assert_eq!(entry.value("pages"), Some("97–111"));
        assert_eq!(&SAMPLE[entry.type_range.clone()], "@article");
        assert_eq!(&SAMPLE[entry.key_range.clone()], "knuth1984");
    }

    #[test]
    fn a_clean_file_reports_nothing() {
        let bib = Bib::parse(SAMPLE);
        assert!(bib.problems.is_empty(), "{:#?}", bib.problems);
        assert!(bib.is_valid());
    }

    #[test]
    fn an_entry_range_covers_the_whole_block() {
        let bib = Bib::parse(SAMPLE);
        let text = &SAMPLE[bib.entries[0].range.clone()];
        assert!(text.starts_with("@article{"));
        assert!(text.ends_with('}'));
    }

    #[test]
    fn quoted_and_bare_values_parse_alongside_braced_ones() {
        let bib = Bib::parse(
            "@article{k, title = \"Quoted {Title}\", year = 1984, month = jan }",
        );
        let entry = &bib.entries[0];
        assert_eq!(entry.value("title"), Some("Quoted Title"));
        assert_eq!(entry.value("year"), Some("1984"));
        assert_eq!(entry.value("month"), Some("jan"));
    }

    #[test]
    fn concatenated_values_join_with_the_abbreviation_expanded() {
        let bib = Bib::parse("@string{acm = \"ACM \"}\n@article{k, publisher = acm # {Press}}");
        assert_eq!(bib.entries[1].value("publisher"), Some("ACM Press"));
        assert!(bib.abbreviation("acm").is_some());
    }

    /// The abbreviation may be defined below the entry that uses it, which is
    /// why expansion is a second pass rather than something the parser does.
    #[test]
    fn an_abbreviation_defined_later_still_expands() {
        let bib = Bib::parse("@article{k, journal = tcj}\n@string{tcj = {The Computer Journal}}");
        assert_eq!(bib.entries[0].value("journal"), Some("The Computer Journal"));
    }

    #[test]
    fn an_unresolved_abbreviation_stays_as_written() {
        let bib = Bib::parse("@article{k, month = jan}");
        assert_eq!(bib.entries[0].value("month"), Some("jan"));
    }

    #[test]
    fn nested_braces_survive_and_protection_braces_are_stripped() {
        let bib = Bib::parse("@article{k, title = {The {ACM} Symposium}}");
        assert_eq!(bib.entries[0].value("title"), Some("The ACM Symposium"));
    }

    #[test]
    fn a_wrapped_value_collapses_to_one_line() {
        let bib = Bib::parse("@article{k, title = {One\n    two\n    three}}");
        assert_eq!(bib.entries[0].value("title"), Some("One two three"));
    }

    #[test]
    fn parentheses_delimit_an_entry_too() {
        let bib = Bib::parse("@article(k, title = {T})");
        assert_eq!(bib.entries[0].key.as_deref(), Some("k"));
        assert!(bib.is_valid(), "{:#?}", bib.problems);
    }

    /// The editor case: the file is being typed and the entry is not closed yet.
    /// The entries below it must keep parsing.
    #[test]
    fn an_unclosed_entry_stops_at_the_next_one() {
        let bib = Bib::parse("@article{first,\n  title = {T},\n\n@book{second,\n}\n");

        assert_eq!(bib.entries.len(), 2);
        assert_eq!(bib.entries[1].key.as_deref(), Some("second"));
        assert!(
            bib.problems.iter().any(|problem| problem.message.contains("unclosed")),
            "{:#?}",
            bib.problems
        );
    }

    #[test]
    fn a_missing_key_is_reported_where_it_would_go() {
        let bib = Bib::parse("@article{,\n  title = {T},\n}\n");
        let problem = bib
            .problems
            .iter()
            .find(|problem| problem.message.contains("citation key"))
            .expect("expected a missing-key error");
        assert_eq!(problem.range.start, "@article{".len());
    }

    #[test]
    fn duplicate_keys_are_an_error_that_points_at_the_first() {
        let bib = Bib::parse("@misc{same, title={A}}\n@misc{same, title={B}}\n");
        let problem = bib
            .problems
            .iter()
            .find(|problem| problem.message.contains("duplicate citation key"))
            .expect("expected a duplicate-key error");

        assert_eq!(problem.severity, Severity::Error);
        assert!(problem.related.is_some(), "must point at the first definition");
        assert!(!bib.is_valid());
    }

    #[test]
    fn a_missing_required_field_warns_without_blocking() {
        let bib = Bib::parse("@article{k, title = {T}}\n");
        let messages: Vec<&str> =
            bib.problems.iter().map(|problem| problem.message.as_str()).collect();

        assert!(
            messages.iter().any(|message| message.contains("`journal`")),
            "{messages:?}"
        );
        assert!(bib.problems.iter().all(|p| p.severity == Severity::Warning));
        assert!(bib.is_valid(), "a warning must not block the formatter");
    }

    #[test]
    fn either_year_or_date_satisfies_the_requirement() {
        let complete = "@article{k, author={A}, title={T}, journal={J}, date={2024-01-02}}";
        let bib = Bib::parse(complete);
        assert!(bib.problems.is_empty(), "{:#?}", bib.problems);
    }

    #[test]
    fn an_unknown_entry_type_warns() {
        let bib = Bib::parse("@nonsense{k, title = {T}}\n");
        assert!(
            bib.problems.iter().any(|p| p.message.contains("unknown entry type")),
            "{:#?}",
            bib.problems
        );
    }

    #[test]
    fn tokens_cover_the_file_in_order_without_overlapping() {
        let bib = Bib::parse(SAMPLE);
        for pair in bib.tokens.windows(2) {
            assert!(
                pair[0].range.end <= pair[1].range.start,
                "{:?} overlaps {:?}",
                pair[0],
                pair[1]
            );
        }

        let types: Vec<TokenKind> = bib.tokens.iter().map(|token| token.kind).collect();
        assert!(types.contains(&TokenKind::EntryType));
        assert!(types.contains(&TokenKind::Key));
        assert!(types.contains(&TokenKind::FieldName));
        assert!(types.contains(&TokenKind::Value));
    }

    #[test]
    fn text_outside_an_entry_is_a_comment() {
        let bib = Bib::parse("A stray note.\n\n@misc{k, title={T}}\n");
        let first = &bib.tokens[0];
        assert_eq!(first.kind, TokenKind::Comment);
        assert!(bib.is_valid(), "{:#?}", bib.problems);
    }

    #[test]
    fn a_summary_degrades_as_fields_go_missing() {
        let full = Bib::parse("@article{k, author={Knuth, Donald E.}, title={LP}, year={1984}}");
        assert_eq!(full.entries[0].summary(), "Knuth — LP — (1984)");

        let bare = Bib::parse("@article{k,}");
        assert_eq!(bare.entries[0].summary(), "@article");
    }

    #[test]
    fn author_lists_shorten_to_surnames() {
        assert_eq!(short_authors("Knuth, Donald E."), "Knuth");
        assert_eq!(short_authors("Donald E. Knuth"), "Knuth");
        assert_eq!(
            short_authors("Madje, Laurenz and Haug, Martin"),
            "Madje & Haug"
        );
        assert_eq!(short_authors("A, One and B, Two and C, Three"), "A et al.");
    }

    #[test]
    fn formatting_is_canonical_and_idempotent() {
        let messy = "@ARTICLE{ knuth1984 ,Author={Knuth, Donald E.},TITLE={Literate Programming},journal={The Computer Journal},year={1984}}";
        let once = format(messy, 2).expect("a valid file formats");

        assert_eq!(
            once,
            "@article{knuth1984,\n  author = {Knuth, Donald E.},\n  \
             title = {Literate Programming},\n  journal = {The Computer Journal},\n  \
             year = {1984},\n}\n"
        );
        assert_eq!(format(&once, 2).as_deref(), Some(once.as_str()), "not idempotent");
    }

    #[test]
    fn formatting_leaves_a_broken_file_alone() {
        assert!(format("@article{k, title = {unclosed\n", 2).is_none());
    }

    #[test]
    fn formatting_keeps_comments_and_string_definitions() {
        let text = "% a licence header\n\n@string{acm = {ACM}}\n\n@misc{k, title = {T}}\n";
        let formatted = format(text, 2).expect("valid");
        assert!(formatted.contains("% a licence header"), "{formatted}");
        assert!(formatted.contains("@string{"), "{formatted}");
    }

    /// The file as it is being typed. Every prefix of a real bibliography has
    /// to parse without panicking, keep every range inside the text and on a
    /// character boundary, and — when the formatter accepts it — produce
    /// something that parses in turn.
    #[test]
    fn every_prefix_of_a_file_holds_together() {
        for end in 0..=SAMPLE.len() {
            if !SAMPLE.is_char_boundary(end) {
                continue;
            }
            let text = &SAMPLE[..end];
            let bib = Bib::parse(text);

            for token in &bib.tokens {
                assert!(token.range.end <= text.len(), "token past the end at {end}");
                assert!(text.is_char_boundary(token.range.start), "at {end}");
                assert!(text.is_char_boundary(token.range.end), "at {end}");
            }
            for problem in &bib.problems {
                assert!(problem.range.end <= text.len(), "problem past the end at {end}");
            }

            if let Some(formatted) = format(text, 2) {
                assert!(
                    Bib::parse(&formatted).is_valid(),
                    "formatting the first {end} bytes produced a file that does not \
                     parse:\n{formatted}"
                );
            }
        }
    }

    #[test]
    fn parsing_a_file_of_nothing_but_junk_terminates() {
        for text in ["@", "@@@@", "{}{}", "@article", "@article{", "@article{k", "  "] {
            let bib = Bib::parse(text);
            // The point is that it returns at all; the contents are secondary.
            assert!(bib.entries.len() <= 1, "{text:?}");
        }
    }

    #[test]
    fn multibyte_text_keeps_ranges_on_character_boundaries() {
        let text = "@article{müller2020,\n  title = {Über Bäume — 中文},\n}\n";
        let bib = Bib::parse(text);

        assert_eq!(bib.entries[0].key.as_deref(), Some("müller2020"));
        assert_eq!(bib.entries[0].value("title"), Some("Über Bäume — 中文"));
        for token in &bib.tokens {
            assert!(text.is_char_boundary(token.range.start));
            assert!(text.is_char_boundary(token.range.end));
        }
    }
}
