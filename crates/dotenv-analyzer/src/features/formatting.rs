//! Opinionated `.env` formatter.
//!
//! The formatter normalises the *shape* of a file and never rewrites the
//! bytes of a value: quoting style, escapes, and multi-line string
//! contents survive verbatim. What it does change:
//!
//! * leading indentation on assignments and comments is dropped;
//! * `export` is separated from the key by exactly one space;
//! * whitespace around `=` disappears (`KEY = v` → `KEY=v`) — the same
//!   thing `ENV003` warns about, since many loaders reject it;
//! * trailing whitespace goes away, and an inline comment is separated
//!   from the value by exactly one space;
//! * runs of blank lines collapse to [`FormatOptions::max_blank_lines`],
//!   with blank lines at the top and bottom of the file removed.
//!
//! Keys are never reordered — `.env` values may reference keys defined
//! above them (`URL=$HOST/api`), so sorting could change what a loader
//! resolves.
//!
//! Files with error-severity diagnostics (unclosed quote, missing key)
//! are refused outright: the parse is untrustworthy there and rewriting
//! could destroy data. VSCode surfaces the refusal as "no formatting
//! edits", leaving the underlying diagnostic visible.

use crate::ast::{Assignment, Entry};
use crate::diagnostics::Severity;
use crate::parse::ParsedFile;
use crate::spans::{ByteSpan, LineCol};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FormatOptions {
    /// Upper bound on consecutive blank lines. `0` removes them all.
    pub max_blank_lines: u32,
    /// Terminate the formatted text with exactly one newline.
    pub insert_final_newline: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        FormatOptions { max_blank_lines: 1, insert_final_newline: true }
    }
}

/// Format the whole file. `None` means "no edits" — either the parse hit
/// an error the formatter refuses to work around, or the file is already
/// formatted.
pub fn format_file(pf: &ParsedFile, opts: &FormatOptions) -> Option<String> {
    if has_blocking_error(pf) {
        return None;
    }
    let mut out = render(&pf.ast.entries, &pf.source, opts, Edges::Trim);
    if !opts.insert_final_newline && out.ends_with('\n') {
        out.pop();
    }
    if out == pf.source {
        return None;
    }
    Some(out)
}

/// Format the entries overlapping the inclusive line range
/// `[start_line, end_line]`, returning the span to replace and its
/// replacement. Blank lines at the edges of the selection are kept
/// (capped) rather than trimmed — they belong to the surrounding file,
/// not to the selection.
pub fn format_range(
    pf: &ParsedFile,
    start_line: u32,
    end_line: u32,
    opts: &FormatOptions,
) -> Option<(ByteSpan, String)> {
    if has_blocking_error(pf) {
        return None;
    }
    let start_off = pf
        .spans
        .line_col_to_offset(&pf.source, LineCol { line: start_line, col: 0 });
    let end_off = pf.spans.line_col_to_offset(
        &pf.source,
        LineCol { line: end_line.saturating_add(1), col: 0 },
    );

    let selected: Vec<&Entry> = pf
        .ast
        .entries
        .iter()
        .filter(|e| overlaps(e.span(), start_off, end_off))
        .collect();
    let span = ByteSpan::new(selected.first()?.span().start, selected.last()?.span().end);

    let mut out = render(selected, &pf.source, opts, Edges::Keep);
    // The replaced slice only ends in a newline when the last selected
    // line does; re-emitting one would merge with the following line.
    if !slice(&pf.source, span).ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    if out == slice(&pf.source, span) {
        return None;
    }
    Some((span, out))
}

/// How blank lines at the start and end of the rendered run are treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edges {
    /// Drop them — used for a whole file.
    Trim,
    /// Keep them, still capped by `max_blank_lines`.
    Keep,
}

fn render<'a>(
    entries: impl IntoIterator<Item = &'a Entry>,
    source: &str,
    opts: &FormatOptions,
    edges: Edges,
) -> String {
    let mut out = String::with_capacity(source.len());
    let mut pending_blanks = 0u32;
    let mut wrote_line = false;
    for entry in entries {
        if matches!(entry, Entry::Blank(_)) {
            pending_blanks += 1;
            continue;
        }
        if wrote_line || edges == Edges::Keep {
            push_blanks(&mut out, pending_blanks, opts);
        }
        pending_blanks = 0;
        render_entry(&mut out, entry, source);
        out.push('\n');
        wrote_line = true;
    }
    if edges == Edges::Keep {
        push_blanks(&mut out, pending_blanks, opts);
    }
    out
}

fn push_blanks(out: &mut String, count: u32, opts: &FormatOptions) {
    for _ in 0..count.min(opts.max_blank_lines) {
        out.push('\n');
    }
}

/// Renders one entry without its line terminator.
fn render_entry(out: &mut String, entry: &Entry, source: &str) {
    match entry {
        Entry::Assignment(a) => render_assignment(out, a, source),
        Entry::Comment(c) => out.push_str(c.text.trim_end()),
        // An unparseable line means nothing to the formatter, so it is
        // reproduced as written apart from surrounding whitespace.
        Entry::Invalid(i) => out.push_str(slice(source, i.span).trim()),
        Entry::Blank(_) => {}
    }
}

fn render_assignment(out: &mut String, a: &Assignment, source: &str) {
    let value = slice(source, a.value_span);
    // A value starting with `#` is ambiguous — loaders disagree on
    // whether `KEY=#x` holds the literal `#x` or nothing at all. Closing
    // up the whitespace around `=` could flip that reading, so the line
    // is reproduced as written.
    if value.starts_with('#') {
        out.push_str(slice(source, a.span).trim());
        return;
    }
    if a.export {
        out.push_str("export ");
    }
    out.push_str(&a.name.name);
    out.push('=');
    out.push_str(value);
    // Whatever follows the value on its last physical line: an inline
    // comment, or trailing junk after a closing quote.
    let trailing = source[a.value_span.end as usize..a.span.end as usize].trim();
    if !trailing.is_empty() {
        out.push(' ');
        out.push_str(trailing);
    }
}

fn has_blocking_error(pf: &ParsedFile) -> bool {
    pf.diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error))
}

fn overlaps(span: ByteSpan, start: u32, end: u32) -> bool {
    if span.is_empty() {
        return span.start >= start && span.start < end;
    }
    span.start < end && span.end > start
}

fn slice(source: &str, span: ByteSpan) -> &str {
    &source[span.start as usize..span.end as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    fn format(src: &str) -> Option<String> {
        format_with(src, &FormatOptions::default())
    }

    fn format_with(src: &str, opts: &FormatOptions) -> Option<String> {
        let pf = parse(FileUri::new("t"), src.to_string());
        format_file(&pf, opts)
    }

    /// Formatted output, falling back to the input when there is nothing
    /// to change — mirrors what the editor ends up with.
    fn formatted(src: &str) -> String {
        format(src).unwrap_or_else(|| src.to_string())
    }

    #[test]
    fn closes_up_spaces_around_equals() {
        assert_eq!(formatted("FOO = bar\n"), "FOO=bar\n");
        assert_eq!(formatted("FOO= bar\n"), "FOO=bar\n");
        assert_eq!(formatted("FOO =bar\n"), "FOO=bar\n");
    }

    #[test]
    fn strips_indentation_and_trailing_whitespace() {
        assert_eq!(formatted("   FOO=bar   \n    # note  \n"), "FOO=bar\n# note\n");
    }

    #[test]
    fn normalizes_export_prefix() {
        assert_eq!(formatted("export    FOO = bar\n"), "export FOO=bar\n");
    }

    #[test]
    fn keeps_inline_comment_one_space_from_value() {
        assert_eq!(formatted("FOO=bar      # note\n"), "FOO=bar # note\n");
        assert_eq!(formatted("FOO=\"bar\"  # note\n"), "FOO=\"bar\" # note\n");
    }

    #[test]
    fn preserves_value_bytes_verbatim() {
        // Quotes, escapes, inner runs of spaces, and an unresolved `$REF`
        // (info-level only, so it must not block) all survive untouched.
        let src = "  A='  keep  me '\n  B = \"esc \\\" and $REF\"\n  C=has spaces inside\n";
        assert_eq!(
            formatted(src),
            "A='  keep  me '\nB=\"esc \\\" and $REF\"\nC=has spaces inside\n"
        );
    }

    #[test]
    fn preserves_multiline_quoted_value() {
        let src = "  MSG = \"line1\n  line2  \nline3\"  # tail\nNEXT=ok\n";
        assert_eq!(formatted(src), "MSG=\"line1\n  line2  \nline3\" # tail\nNEXT=ok\n");
    }

    #[test]
    fn collapses_blank_runs_and_trims_file_edges() {
        assert_eq!(formatted("\n\n\nA=1\n\n\n\nB=2\n\n\n"), "A=1\n\nB=2\n");
    }

    #[test]
    fn max_blank_lines_zero_removes_all_blanks() {
        let opts = FormatOptions { max_blank_lines: 0, ..FormatOptions::default() };
        assert_eq!(format_with("A=1\n\n\nB=2\n", &opts).unwrap(), "A=1\nB=2\n");
    }

    #[test]
    fn max_blank_lines_two_keeps_two() {
        let opts = FormatOptions { max_blank_lines: 2, ..FormatOptions::default() };
        assert_eq!(format_with("A=1\n\n\n\n\nB=2\n", &opts).unwrap(), "A=1\n\n\nB=2\n");
    }

    #[test]
    fn adds_missing_final_newline() {
        assert_eq!(formatted("A=1"), "A=1\n");
    }

    #[test]
    fn insert_final_newline_disabled_strips_it() {
        let opts = FormatOptions { insert_final_newline: false, ..FormatOptions::default() };
        assert_eq!(format_with("A=1\n", &opts).unwrap(), "A=1");
    }

    #[test]
    fn keeps_unparseable_lines() {
        assert_eq!(formatted("  garbage line  \nA=1\n"), "garbage line\nA=1\n");
    }

    #[test]
    fn leaves_hash_leading_values_alone() {
        assert_eq!(formatted("A= # note\nB=#note\n"), "A= # note\nB=#note\n");
    }

    #[test]
    fn empty_value_keeps_bare_equals() {
        assert_eq!(formatted("A =   \n"), "A=\n");
    }

    #[test]
    fn refuses_when_a_quote_is_unclosed() {
        assert_eq!(format("A = \"oops\n"), None);
    }

    #[test]
    fn refuses_when_a_key_is_missing() {
        assert_eq!(format(" = value\n"), None);
    }

    #[test]
    fn already_formatted_file_produces_no_edit() {
        assert_eq!(format("A=1\n\nB=2 # note\n"), None);
    }

    #[test]
    fn is_idempotent() {
        let src = "\n\n  export A = 1   # one\n\n\n\nB='  raw  '\ngarbage\nC=\"multi\nline\"\n\n";
        let once = formatted(src);
        assert_eq!(formatted(&once), once);
    }

    #[test]
    fn crlf_line_endings_are_normalized_to_lf() {
        assert_eq!(formatted("A = 1\r\nB=2\r\n"), "A=1\nB=2\n");
    }

    fn format_lines(src: &str, start: u32, end: u32) -> Option<(ByteSpan, String)> {
        let pf = parse(FileUri::new("t"), src.to_string());
        format_range(&pf, start, end, &FormatOptions::default())
    }

    #[test]
    fn range_touches_only_selected_lines() {
        let src = "A = 1\nB = 2\nC = 3\n";
        let (span, text) = format_lines(src, 1, 1).unwrap();
        assert_eq!(slice(src, span), "B = 2\n");
        assert_eq!(text, "B=2\n");
    }

    #[test]
    fn range_covering_a_multiline_value_takes_the_whole_entry() {
        let src = "A=1\nMSG = \"one\ntwo\"\nB=2\n";
        let (span, text) = format_lines(src, 1, 1).unwrap();
        assert_eq!(slice(src, span), "MSG = \"one\ntwo\"\n");
        assert_eq!(text, "MSG=\"one\ntwo\"\n");
    }

    #[test]
    fn range_keeps_blank_lines_at_its_edges() {
        let src = "A=1\n\nB = 2\n\nC=3\n";
        let (span, text) = format_lines(src, 1, 3).unwrap();
        assert_eq!(slice(src, span), "\nB = 2\n\n");
        assert_eq!(text, "\nB=2\n\n");
    }

    #[test]
    fn range_without_a_trailing_newline_does_not_grow_one() {
        let src = "A=1\nB = 2";
        let (span, text) = format_lines(src, 1, 1).unwrap();
        assert_eq!(slice(src, span), "B = 2");
        assert_eq!(text, "B=2");
    }

    #[test]
    fn range_over_formatted_lines_produces_no_edit() {
        assert_eq!(format_lines("A=1\nB=2\n", 0, 0), None);
    }
}
