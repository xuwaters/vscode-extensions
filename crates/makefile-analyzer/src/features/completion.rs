//! Completion provider for Makefile recipe lines.
//!
//! Workflow at the cursor:
//!
//! 1. Locate the recipe line containing the cursor (via the AST). If
//!    the cursor is on a target header, an assignment, a directive, or
//!    a comment, return no completions.
//! 2. Slice the recipe text from the start of the (joined) recipe line
//!    up to the cursor.
//! 3. Trim leading recipe prefixes (`@`, `-`, `+`, leading TAB).
//! 4. Split on shell-level command separators (`;`, `|`, `&&`, `||`,
//!    backticks `` ` ``) while respecting single/double quotes; keep
//!    the *last* segment — that's where the cursor is.
//! 5. Tokenise the last segment. The first token is the command, all
//!    tokens up to (but not including) the cursor token are treated as
//!    subcommand-path components, and the cursor token is the
//!    "active prefix" handed to [`CompletionsDb::query`].
//!
//! The shell tokenizer is intentionally naive: it does not expand make
//! variables (`$(VAR)` is treated as opaque) and does not understand
//! redirection or process substitution. That's fine for v1 — unusual
//! shell shapes just stop completing.

use cli_completions::{CompletionsDb, MatchKind};
use serde::Serialize;

use crate::ast::{File, Item, RecipeLine, Rule};
use crate::parse::ParsedFile;
use crate::spans::LineCol;

/// One completion item, ready for serialisation to the editor.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CompletionItem {
    /// Editor-visible label (e.g. `--verbose`, `-v`, `add`, `gnu`).
    pub label: String,
    /// One-line description, if any.
    pub detail: Option<String>,
    /// Coarse kind so the editor can pick an icon.
    pub kind: CompletionKind,
    /// Text inserted on accept. For now identical to [`label`], but kept
    /// distinct so future versions can append `=` for `REQUIRES_ARG`
    /// flags or trailing space for subcommands.
    pub insert_text: String,
    /// Number of UTF-16 code units immediately before the cursor that
    /// the editor should replace when accepting this item. VSCode's
    /// default word-pattern range excludes leading dashes, so without
    /// this hint accepting `--head` after typing `--hea` would replace
    /// only `hea` and produce `----head`.
    pub replace_length: u32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum CompletionKind {
    Long,
    Short,
    ArgValue,
    Subcommand,
}

impl From<MatchKind> for CompletionKind {
    fn from(value: MatchKind) -> Self {
        match value {
            MatchKind::Long => CompletionKind::Long,
            MatchKind::Short => CompletionKind::Short,
            MatchKind::ArgValue => CompletionKind::ArgValue,
            MatchKind::Subcommand => CompletionKind::Subcommand,
        }
    }
}

/// Soft cap on the number of completions returned to the editor.
/// VSCode happily filters thousands client-side, but emitting tens of
/// thousands per keystroke (e.g. `gcc.fish` has 800+ entries) wastes
/// JSON encoding time for no perceptible gain.
const MAX_RESULTS: usize = 250;

/// Top-level entry point — used by the wasm bridge.
pub fn completions(
    parsed: &ParsedFile,
    pos: LineCol,
    db: &CompletionsDb<'_>,
) -> Vec<CompletionItem> {
    let cursor_offset = parsed.spans.line_col_to_offset(&parsed.source, pos);

    let Some(recipe_line) = find_recipe_at(&parsed.ast, cursor_offset) else {
        return Vec::new();
    };

    // Source slice from the start of the (joined) recipe line up to the
    // cursor. We use the original source — recipe_line.text is identical
    // for non-continuation lines, and the joined version replaces the
    // backslash continuation with a literal space which is fine here.
    let start = recipe_line.span.start as usize;
    let end = (cursor_offset as usize).min(parsed.source.len());
    if start >= end {
        return Vec::new();
    }
    let prefix_text = recipe_text_for_completion(&parsed.source[start..end]);

    let segment = last_command_segment(&prefix_text);
    let tokens = shell_tokenise(segment);

    let (path, active) = command_context(&tokens);
    if path.is_empty() {
        return Vec::new();
    }

    let path_refs: Vec<&str> = path.iter().map(String::as_str).collect();

    // `--name=val` shape: the active token contains an `=`. Switch to
    // arg-value completion against the named option.
    if let Some((option_label, value_prefix)) = split_option_value(&active) {
        let replace_length = utf16_len(value_prefix);
        return db
            .query_arg_values(&path_refs, option_label, value_prefix)
            .into_iter()
            .take(MAX_RESULTS)
            .map(|m| CompletionItem {
                label: m.label.clone().into_owned(),
                detail: m.description.map(trim_description),
                kind: m.kind.into(),
                insert_text: m.label.into_owned(),
                replace_length,
            })
            .collect();
    }

    let replace_length = utf16_len(&active);
    db.query(&path_refs, &active)
        .take(MAX_RESULTS)
        .map(|m| CompletionItem {
            label: m.label.clone().into_owned(),
            detail: m.description.map(trim_description),
            kind: m.kind.into(),
            insert_text: m.label.into_owned(),
            replace_length,
        })
        .collect()
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

/// Walk the AST and return a recipe-line whose source span contains
/// `cursor_offset`. Returns `None` if the cursor is anywhere else.
fn find_recipe_at(file: &File, cursor_offset: u32) -> Option<&RecipeLine> {
    fn walk_items<'a>(items: &'a [Item], cursor: u32) -> Option<&'a RecipeLine> {
        for item in items {
            match item {
                Item::Rule(rule) => {
                    if let Some(line) = walk_rule(rule, cursor) {
                        return Some(line);
                    }
                }
                Item::Conditional(c) => {
                    if let Some(line) = walk_items(&c.then_branch, cursor) {
                        return Some(line);
                    }
                    if let Some(line) = walk_items(&c.else_branch, cursor) {
                        return Some(line);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn walk_rule(rule: &Rule, cursor: u32) -> Option<&RecipeLine> {
        rule.recipe_lines
            .iter()
            .find(|line| span_contains_inclusive(line.span.start, line.span.end, cursor))
    }

    walk_items(&file.items, cursor_offset)
}

/// Recipe line spans include the trailing newline. We use an inclusive
/// upper bound so the cursor "just past the last character" still
/// counts as inside the line.
fn span_contains_inclusive(start: u32, end: u32, cursor: u32) -> bool {
    cursor >= start && cursor <= end
}

/// Strip the leading TAB and any GNU Make recipe prefix bytes (`@-+`)
/// from the start of a recipe text. Returns a `String` because we may
/// also need to collapse `\<newline>` continuations into spaces.
fn recipe_text_for_completion(raw: &str) -> String {
    // Drop leading TABs / spaces.
    let trimmed = raw.trim_start_matches(|c| c == '\t' || c == ' ');
    // Strip GNU recipe modifiers — they can appear in any order.
    let mut s = trimmed;
    loop {
        match s.as_bytes().first() {
            Some(b'@') | Some(b'-') | Some(b'+') => s = &s[1..],
            _ => break,
        }
    }
    // Collapse `\<newline>` continuations. Real recipe-line text from
    // the source still contains them; we replace each with a single
    // space so the tokenizer doesn't see embedded newlines.
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            i += 2;
            // Skip a leading TAB on the continuation line — recipe
            // lines start with a tab even when continued.
            while i < bytes.len() && (bytes[i] == b'\t' || bytes[i] == b' ') {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Slice the right-most "command segment" from the prefix text.
///
/// A new segment starts after any of `;`, `|`, `&&`, `||`, or a
/// backtick — but only when those characters appear outside quotes.
fn last_command_segment(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut last_split = 0;
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < bytes.len() {
        let c = bytes[i];
        if in_single {
            if c == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b';' | b'`' => last_split = i + 1,
            b'|' => {
                last_split = if i + 1 < bytes.len() && bytes[i + 1] == b'|' {
                    i + 2
                } else {
                    i + 1
                };
                if last_split > i + 1 {
                    i += 1;
                }
            }
            b'&' if i + 1 < bytes.len() && bytes[i + 1] == b'&' => {
                last_split = i + 2;
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    text[last_split..].trim_start()
}

/// Naive shell tokeniser for the *prefix* of a command line. The last
/// element of the returned vec is the "current word" — possibly empty
/// if the cursor sits right after whitespace.
fn shell_tokenise(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut had_word_chars = false;

    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_single {
            if c == b'\'' {
                in_single = false;
                i += 1;
                continue;
            }
            cur.push(c as char);
            had_word_chars = true;
            i += 1;
            continue;
        }
        if in_double {
            if c == b'\\' && i + 1 < bytes.len() {
                cur.push(bytes[i + 1] as char);
                had_word_chars = true;
                i += 2;
                continue;
            }
            if c == b'"' {
                in_double = false;
                i += 1;
                continue;
            }
            cur.push(c as char);
            had_word_chars = true;
            i += 1;
            continue;
        }
        if c == b' ' || c == b'\t' {
            if had_word_chars || !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                had_word_chars = false;
            }
            i += 1;
            continue;
        }
        if c == b'\'' {
            in_single = true;
            had_word_chars = true;
            i += 1;
            continue;
        }
        if c == b'"' {
            in_double = true;
            had_word_chars = true;
            i += 1;
            continue;
        }
        if c == b'\\' && i + 1 < bytes.len() {
            cur.push(bytes[i + 1] as char);
            had_word_chars = true;
            i += 2;
            continue;
        }
        cur.push(c as char);
        had_word_chars = true;
        i += 1;
    }
    // Always push the final token, even if empty — it represents the
    // cursor position inside (or just after) a word.
    out.push(cur);
    out
}

/// From a token list, derive `(path, active_prefix)` for the database
/// query. `path[0]` is the command name; `path[1..]` are subcommand
/// path components. The last token is the active prefix the user has
/// typed so far.
fn command_context(tokens: &[String]) -> (Vec<String>, String) {
    if tokens.is_empty() {
        return (Vec::new(), String::new());
    }
    if tokens.len() == 1 {
        // The user is still typing the command name itself — nothing
        // for us to complete here. Return an empty path so the caller
        // bails out.
        return (Vec::new(), tokens[0].clone());
    }

    let active = tokens.last().cloned().unwrap_or_default();
    let middle = &tokens[..tokens.len() - 1];

    let command = strip_command_name(&middle[0]);
    if command.is_empty() {
        return (Vec::new(), active);
    }

    // Subcommand-path heuristic: bareword tokens between the command
    // and the cursor that don't start with `-` and don't carry `=` are
    // candidate subcommand names. Anything starting with `-` resets
    // the path so we don't mistake `git -C dir status` for a path.
    let mut path = vec![command];
    for tok in &middle[1..] {
        if tok.is_empty() || tok.starts_with('-') || tok.contains('=') {
            continue;
        }
        if !is_plain_subcommand(tok) {
            continue;
        }
        path.push(tok.clone());
    }

    (path, active)
}

/// Trim a command path / quoting to the bare program name.
///
/// `usr/bin/curl` → `curl`; `"curl"` → `curl`; `$(CC)` → `` (drop).
fn strip_command_name(raw: &str) -> String {
    if raw.starts_with('$') || raw.contains('`') {
        return String::new();
    }
    let unquoted = raw.trim_matches(|c| c == '"' || c == '\'');
    let last = unquoted
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(unquoted);
    last.to_owned()
}

fn is_plain_subcommand(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

/// Detect the `--name=value-prefix` shape of an active token.
///
/// Returns `(option_label, value_prefix)` if the token starts with a
/// dash and contains an `=`; otherwise `None`. Both pieces are slices
/// of the input.
fn split_option_value(active: &str) -> Option<(&str, &str)> {
    if !active.starts_with('-') {
        return None;
    }
    let (head, tail) = active.split_once('=')?;
    Some((head, tail))
}

/// Cap a description for editor display: keep the first sentence (cut
/// at `". "`) and trim to a hard limit. Fish descriptions are usually
/// already terse, but a few are verbose enough to overflow the
/// CompletionItem detail line in VSCode.
const DESCRIPTION_HARD_LIMIT: usize = 120;

fn trim_description(raw: &str) -> String {
    let raw = raw.trim();
    let first_sentence = match raw.find(". ") {
        Some(i) => &raw[..i + 1],
        None => raw,
    };
    if first_sentence.len() <= DESCRIPTION_HARD_LIMIT {
        return first_sentence.to_owned();
    }
    // Try to break on the last word boundary inside the limit.
    let cut = first_sentence
        .char_indices()
        .take_while(|(i, _)| *i <= DESCRIPTION_HARD_LIMIT)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
    let cut = first_sentence[..cut]
        .rfind(' ')
        .map(|i| i)
        .unwrap_or(cut);
    let mut out = first_sentence[..cut].trim_end().to_owned();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::vfs::FileUri;
    use cli_completions::{Builder, DirectiveInput, EntryFlags};

    fn fixture_db() -> Vec<u8> {
        let mut b = Builder::new();
        for (long, desc) in [
            ("verbose", "be loud"),
            ("anyauth", "use most secure auth"),
            ("user-agent", "set User-Agent"),
        ] {
            b.add(DirectiveInput {
                command: "curl",
                short: None,
                long: Some(long),
                description: Some(desc),
                flags: EntryFlags::default(),
                subcommand_path: &[],
                arg_values: &[],
            });
        }
        b.add(DirectiveInput {
            command: "git",
            short: Some(b'v'),
            long: Some("version"),
            description: Some("display git version"),
            flags: EntryFlags::from_bits(EntryFlags::TOP_LEVEL_ONLY),
            subcommand_path: &[],
            arg_values: &[],
        });
        b.add(DirectiveInput {
            command: "git",
            short: None,
            long: Some("mirror"),
            description: Some("mirror remote"),
            flags: EntryFlags::default(),
            subcommand_path: &["remote"],
            arg_values: &[],
        });
        // git: `remote` and `rebase` subcommand markers (entries under
        // those subcommands) so subcommand-name completion has data.
        b.add(DirectiveInput {
            command: "git",
            short: None,
            long: Some("interactive"),
            description: None,
            flags: EntryFlags::default(),
            subcommand_path: &["rebase"],
            arg_values: &[],
        });
        // tar: --format with static enum values.
        b.add(DirectiveInput {
            command: "tar",
            short: None,
            long: Some("format"),
            description: Some("set the archive format"),
            flags: EntryFlags::from_bits(EntryFlags::REQUIRES_ARG | EntryFlags::NO_FILES),
            subcommand_path: &[],
            arg_values: &["gnu", "pax", "ustar", "oldgnu", "posix"],
        });
        b.build()
    }

    fn parse_fixture(src: &str) -> parse::ParsedFile {
        parse::parse(FileUri::new("file:///t.mk"), src.to_owned())
    }

    fn complete_at(src: &str, line: u32, col: u32) -> Vec<String> {
        complete_items(src, line, col)
            .into_iter()
            .map(|i| i.label)
            .collect()
    }

    fn complete_items(src: &str, line: u32, col: u32) -> Vec<CompletionItem> {
        let blob = fixture_db();
        let db = CompletionsDb::from_bytes(&blob).unwrap();
        let parsed = parse_fixture(src);
        completions(&parsed, LineCol { line, col }, &db)
    }

    #[test]
    fn completes_curl_long_options() {
        let src = "build:\n\tcurl --\n";
        // Cursor right after `--`: line 1, col is utf16 length of "\tcurl --" = 8.
        let labels = complete_at(src, 1, 8);
        assert_eq!(labels, vec!["--anyauth", "--user-agent", "--verbose"]);
    }

    #[test]
    fn prefix_filters_results() {
        let src = "build:\n\tcurl --an\n";
        // Cursor after "--an" (col 10).
        let labels = complete_at(src, 1, 10);
        assert_eq!(labels, vec!["--anyauth"]);
    }

    #[test]
    fn cursor_outside_recipe_returns_empty() {
        let src = "VAR = 1\n";
        let labels = complete_at(src, 0, 5);
        assert!(labels.is_empty());
    }

    #[test]
    fn target_header_returns_empty() {
        let src = "build:\n\techo hi\n";
        let labels = complete_at(src, 0, 3);
        assert!(labels.is_empty());
    }

    #[test]
    fn recipe_with_at_prefix_still_completes() {
        let src = "build:\n\t@curl --\n";
        // Tab + '@' + "curl --" — cursor at col 9 sits after "--".
        let labels = complete_at(src, 1, 9);
        assert_eq!(labels, vec!["--anyauth", "--user-agent", "--verbose"]);
    }

    #[test]
    fn pipe_resets_command_context() {
        let src = "build:\n\techo hi | curl --\n";
        // Cursor right after the trailing "--"; col matches `\techo hi | curl --` length = 18.
        let labels = complete_at(src, 1, 18);
        assert!(labels.contains(&"--verbose".to_owned()));
    }

    #[test]
    fn subcommand_path_filters_top_level_only() {
        // `git remote --m` should NOT include `--version` (top-level).
        let src = "build:\n\tgit remote --m\n";
        let labels = complete_at(src, 1, 14);
        assert_eq!(labels, vec!["--mirror"]);
    }

    #[test]
    fn unknown_command_returns_empty() {
        let src = "build:\n\tnope --\n";
        let labels = complete_at(src, 1, 8);
        assert!(labels.is_empty());
    }

    #[test]
    fn short_segment_helpers() {
        assert_eq!(last_command_segment("echo hi | curl --"), "curl --");
        assert_eq!(last_command_segment("curl --"), "curl --");
        assert_eq!(last_command_segment("a && b ; c"), "c");
        assert_eq!(last_command_segment("echo 'a;b' c"), "echo 'a;b' c");
    }

    #[test]
    fn shell_tokeniser_trailing_empty() {
        let toks = shell_tokenise("curl ");
        assert_eq!(toks, vec!["curl".to_owned(), String::new()]);
    }

    #[test]
    fn shell_tokeniser_quotes() {
        let toks = shell_tokenise("curl 'a b' --x");
        assert_eq!(toks, vec!["curl".to_owned(), "a b".to_owned(), "--x".to_owned()]);
    }

    #[test]
    fn subcommand_name_completion() {
        // `git re|` should surface subcommand names `rebase` and `remote`.
        let src = "build:\n\tgit re\n";
        let labels = complete_at(src, 1, 6);
        assert!(labels.contains(&"rebase".to_owned()));
        assert!(labels.contains(&"remote".to_owned()));
    }

    #[test]
    fn arg_value_completion_via_equals() {
        // `tar --format=g|` should complete `gnu`.
        let src = "build:\n\ttar --format=g\n";
        // line 1: "\ttar --format=g" — 15 chars; cursor at end (col=15).
        let labels = complete_at(src, 1, 15);
        assert_eq!(labels, vec!["gnu".to_owned()]);
    }

    #[test]
    fn arg_value_completion_empty_value_returns_all() {
        let src = "build:\n\ttar --format=\n";
        // line 1: "\ttar --format=" — 14 chars; cursor at end (col=14).
        let labels = complete_at(src, 1, 14);
        assert_eq!(labels, vec!["gnu", "oldgnu", "pax", "posix", "ustar"]);
    }

    #[test]
    fn replace_length_covers_typed_long_option_prefix() {
        // Regression: `curl --any<TAB>` used to expand to `----anyauth`
        // because VSCode's default replacement range excludes the
        // leading dashes — it would replace only the word `any`, leaving
        // the original `--` in place. The analyzer must report a
        // replace_length covering the whole `--any` so the editor
        // overwrites it.
        let src = "build:\n\tcurl --any\n";
        // "\tcurl --any" — cursor at end (col=11).
        let items = complete_items(src, 1, 11);
        let anyauth = items.iter().find(|i| i.label == "--anyauth");
        assert!(anyauth.is_some(), "expected --anyauth in completions");
        // `--any` is 5 UTF-16 code units long.
        assert_eq!(anyauth.unwrap().replace_length, 5);
        // And the empty-prefix case still reports zero so freshly typed
        // tokens aren't accidentally chewed into.
        let empty = complete_items("build:\n\tcurl \n", 1, 6);
        assert!(!empty.is_empty());
        assert!(empty.iter().all(|i| i.replace_length == 0));
    }

    #[test]
    fn replace_length_for_arg_value_skips_option_label() {
        // `tar --format=g|`: replace_length should cover only the value
        // prefix `g` (1), not the whole `--format=g` token, since we're
        // replacing the value with `gnu` — not rewriting the option.
        let src = "build:\n\ttar --format=g\n";
        let items = complete_items(src, 1, 15);
        assert!(!items.is_empty());
        assert!(items.iter().all(|i| i.replace_length == 1));
    }

    #[test]
    fn split_option_value_helpers() {
        assert_eq!(split_option_value("--format="), Some(("--format", "")));
        assert_eq!(split_option_value("--format=g"), Some(("--format", "g")));
        assert_eq!(split_option_value("-O=2"), Some(("-O", "2")));
        assert_eq!(split_option_value("--ver"), None);
        assert_eq!(split_option_value("plain=foo"), None);
    }

    #[test]
    fn description_trim_keeps_first_sentence() {
        let s = trim_description("Be loud. Lots of detail follows here.");
        assert_eq!(s, "Be loud.");
    }

    #[test]
    fn description_trim_caps_long_text() {
        let raw = "a ".repeat(80);
        let s = trim_description(&raw);
        assert!(s.ends_with('…'));
        assert!(s.chars().count() < raw.chars().count());
    }

    #[test]
    fn command_context_skips_dashed_tokens() {
        // `-C` is dropped (starts with `-`); `dir` is currently still
        // treated as a subcommand candidate. v1 doesn't model "this
        // short option takes a value", so the heuristic here is
        // intentionally lenient — the worst case is a few extra
        // candidate paths whose entries don't exist in the database.
        let toks = vec!["git".into(), "-C".into(), "dir".into(), "--ver".into()];
        let (path, active) = command_context(&toks);
        assert_eq!(path, vec!["git".to_owned(), "dir".to_owned()]);
        assert_eq!(active, "--ver");
    }
}
