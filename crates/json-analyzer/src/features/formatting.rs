//! Comment-preserving pretty-printer with optional recursive key sort.
//!
//! Scalars are reproduced byte-for-byte from the source (quote style,
//! escapes, number notation all survive); only *shape* is normalized:
//! one member or element per line, indentation from the options,
//! trailing commas dropped, a single space after `:`. Comments ride
//! along with the entry they annotate, which is what lets `sort_keys`
//! reorder an object without orphaning its documentation.
//!
//! JSON Lines gets the compact layout instead: one record per line,
//! blank lines dropped, `, ` and `: ` spacing normalized.
//!
//! Files whose parse produced structural errors (holes, unterminated
//! literals) are refused — `None` — so a half-typed document is never
//! rewritten into something worse.

use crate::ast::{Ast, Comment, Member, Value, ValueKind};
use crate::diagnostics::DiagnosticCode;
use crate::workspace::ParsedFile;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FormatOptions {
    pub tab_size: u32,
    pub insert_spaces: bool,
    /// Sort object keys recursively (see [`compare_keys`], stable).
    pub sort_keys: bool,
    pub insert_final_newline: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        FormatOptions {
            tab_size: 2,
            insert_spaces: true,
            sort_keys: false,
            insert_final_newline: true,
        }
    }
}

/// Codes that mean the AST has holes or untrustworthy raw slices.
const BLOCKING: [DiagnosticCode; 5] = [
    DiagnosticCode::SyntaxError,
    DiagnosticCode::UnterminatedString,
    DiagnosticCode::UnterminatedComment,
    DiagnosticCode::MultipleTopLevelValues,
    DiagnosticCode::InvalidNumber,
];

fn refuses(pf: &ParsedFile) -> bool {
    let blocked = pf.diagnostics.iter().any(|d| BLOCKING.contains(&d.code));
    if blocked {
        return true;
    }
    // A JSON Lines record cannot carry its comments onto one line
    // without either dropping them or breaking the format; refuse.
    matches!(pf.ast, Ast::Lines(_))
        && pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::CommentNotAllowed)
}

/// Format the whole file. `None` means "no edits": the file is already
/// formatted, or the formatter refused to touch a broken parse.
pub fn format_file(pf: &ParsedFile, opts: &FormatOptions) -> Option<String> {
    if refuses(pf) {
        return None;
    }
    if let Ast::Single(root) = &pf.ast {
        // A file with no value and no comments is whitespace; leave it be.
        if root.value.is_none() && root.leading.is_empty() && root.trailing.is_empty() {
            return None;
        }
    }
    let mut out = String::with_capacity(pf.source.len() + pf.source.len() / 8);
    match &pf.ast {
        Ast::Single(root) => {
            let renderer = Renderer { source: &pf.source, opts };
            for comment in &root.leading {
                renderer.comment_line(&mut out, comment, 0);
            }
            if let Some(value) = &root.value {
                renderer.pretty(&mut out, value, 0);
                out.push('\n');
            }
            for comment in &root.trailing {
                renderer.comment_line(&mut out, comment, 0);
            }
        }
        Ast::Lines(records) => {
            let renderer = Renderer { source: &pf.source, opts };
            for record in records {
                renderer.compact(&mut out, &record.value, usize::MAX);
                out.push('\n');
            }
        }
    }
    if !opts.insert_final_newline {
        while out.ends_with('\n') {
            out.pop();
        }
    }
    if out == pf.source {
        return None;
    }
    Some(out)
}

pub struct Renderer<'a> {
    pub source: &'a str,
    pub opts: &'a FormatOptions,
}

impl<'a> Renderer<'a> {
    fn indent(&self, out: &mut String, depth: u32) {
        if self.opts.insert_spaces {
            for _ in 0..depth * self.opts.tab_size {
                out.push(' ');
            }
        } else {
            for _ in 0..depth {
                out.push('\t');
            }
        }
    }

    /// Raw slice with CRLF collapsed to LF, so the host-side newline
    /// translation never manufactures `\r\r\n`.
    fn push_raw(&self, out: &mut String, value: &Value) {
        push_normalized(out, value.raw(self.source));
    }

    fn comment_text(&self, comment: &Comment) -> &'a str {
        &self.source[comment.span.start as usize..comment.span.end as usize]
    }

    fn comment_line(&self, out: &mut String, comment: &Comment, depth: u32) {
        self.indent(out, depth);
        push_normalized(out, self.comment_text(comment));
        out.push('\n');
    }

    fn trailing_comment(&self, out: &mut String, comment: &Comment) {
        out.push(' ');
        push_normalized(out, self.comment_text(comment));
    }

    fn members<'m>(&self, members: &'m [Member]) -> Vec<&'m Member> {
        let mut refs: Vec<&Member> = members.iter().collect();
        if self.opts.sort_keys {
            refs.sort_by(|a, b| compare_keys(&a.key.name, &b.key.name));
        }
        refs
    }

    pub fn pretty(&self, out: &mut String, value: &Value, depth: u32) {
        match &value.kind {
            ValueKind::Object(object) => {
                if object.members.is_empty() && object.dangling.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                out.push('\n');
                let members = self.members(&object.members);
                let last = members.len().saturating_sub(1);
                for (i, member) in members.iter().enumerate() {
                    for comment in &member.leading {
                        self.comment_line(out, comment, depth + 1);
                    }
                    self.indent(out, depth + 1);
                    push_normalized(
                        out,
                        &self.source[member.key.span.start as usize..member.key.span.end as usize],
                    );
                    out.push_str(": ");
                    self.pretty(out, &member.value, depth + 1);
                    if i != last {
                        out.push(',');
                    }
                    if let Some(comment) = &member.trailing {
                        self.trailing_comment(out, comment);
                    }
                    out.push('\n');
                }
                for comment in &object.dangling {
                    self.comment_line(out, comment, depth + 1);
                }
                self.indent(out, depth);
                out.push('}');
            }
            ValueKind::Array(array) => {
                if array.elements.is_empty() && array.dangling.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                out.push('\n');
                let last = array.elements.len().saturating_sub(1);
                for (i, element) in array.elements.iter().enumerate() {
                    for comment in &element.leading {
                        self.comment_line(out, comment, depth + 1);
                    }
                    self.indent(out, depth + 1);
                    self.pretty(out, &element.value, depth + 1);
                    if i != last {
                        out.push(',');
                    }
                    if let Some(comment) = &element.trailing {
                        self.trailing_comment(out, comment);
                    }
                    out.push('\n');
                }
                for comment in &array.dangling {
                    self.comment_line(out, comment, depth + 1);
                }
                self.indent(out, depth);
                out.push(']');
            }
            _ => self.push_raw(out, value),
        }
    }

    /// Single-line rendering, used for JSON Lines records and table
    /// cells. `budget` caps the output length in bytes (the cap lands
    /// on a char boundary, with an ellipsis).
    pub fn compact(&self, out: &mut String, value: &Value, budget: usize) {
        let start = out.len();
        self.compact_inner(out, value);
        if out.len() - start > budget {
            let mut cut = start + budget;
            while !out.is_char_boundary(cut) {
                cut -= 1;
            }
            out.truncate(cut);
            out.push('…');
        }
    }

    fn compact_inner(&self, out: &mut String, value: &Value) {
        match &value.kind {
            ValueKind::Object(object) => {
                out.push('{');
                let members = self.members(&object.members);
                let last = members.len().saturating_sub(1);
                for (i, member) in members.iter().enumerate() {
                    push_normalized(
                        out,
                        &self.source[member.key.span.start as usize..member.key.span.end as usize],
                    );
                    out.push_str(": ");
                    self.compact_inner(out, &member.value);
                    if i != last {
                        out.push_str(", ");
                    }
                }
                out.push('}');
            }
            ValueKind::Array(array) => {
                out.push('[');
                let last = array.elements.len().saturating_sub(1);
                for (i, element) in array.elements.iter().enumerate() {
                    self.compact_inner(out, &element.value);
                    if i != last {
                        out.push_str(", ");
                    }
                }
                out.push(']');
            }
            _ => self.push_raw(out, value),
        }
    }
}

/// Key order for `sort_keys`, the way a reader expects to find names in
/// a settings file rather than the way bytes happen to be numbered.
///
/// Raw code-point order drops `[` (U+005B) between the uppercase and
/// lowercase letters, so `"C_pp"` sorts before `"[astro]"` sorts before
/// `"astro"` — the same name in three neighbourhoods. Folding case
/// first keeps every punctuation-led key (`[astro]`, `$schema`) in one
/// group ahead of the words, and digit runs compare as numbers so
/// `item2` precedes `item10`. Exact code points break ties last, which
/// keeps the order total and deterministic (`A` before `a`).
fn compare_keys(a: &str, b: &str) -> Ordering {
    natural_cmp(a, b).then_with(|| a.cmp(b))
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a, b);
    loop {
        let (ca, cb) = match (x.chars().next(), y.chars().next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => (ca, cb),
        };
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let (da, resta) = split_digits(x);
            let (db, restb) = split_digits(y);
            match number_cmp(da, db) {
                Ordering::Equal => {
                    x = resta;
                    y = restb;
                }
                ord => return ord,
            }
        } else {
            match fold(ca).cmp(&fold(cb)) {
                Ordering::Equal => {
                    x = &x[ca.len_utf8()..];
                    y = &y[cb.len_utf8()..];
                }
                ord => return ord,
            }
        }
    }
}

/// Leading run of ASCII digits, and whatever follows it.
fn split_digits(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    s.split_at(end)
}

/// Two digit runs by value: leading zeros are noise, then longer wins,
/// then the digits themselves.
fn number_cmp(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn push_normalized(out: &mut String, raw: &str) {
    if !raw.contains('\r') {
        out.push_str(raw);
        return;
    }
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' && chars.peek() == Some(&'\n') {
            continue;
        }
        out.push(ch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flavor::Flavor;
    use crate::workspace::{parse, FileUri};
    use pretty_assertions::assert_eq;

    fn fmt(src: &str, flavor: Flavor) -> Option<String> {
        fmt_with(src, flavor, &FormatOptions::default())
    }

    fn fmt_with(src: &str, flavor: Flavor, opts: &FormatOptions) -> Option<String> {
        let pf = parse(FileUri::new("t"), src.to_string(), flavor);
        format_file(&pf, opts)
    }

    fn formatted(src: &str, flavor: Flavor) -> String {
        fmt(src, flavor).unwrap_or_else(|| src.to_string())
    }

    #[test]
    fn expands_nested_containers() {
        assert_eq!(
            formatted(r#"{"b":{"c":[1,2]},"a":true}"#, Flavor::Json),
            "{\n  \"b\": {\n    \"c\": [\n      1,\n      2\n    ]\n  },\n  \"a\": true\n}\n"
        );
    }

    #[test]
    fn empty_containers_stay_inline() {
        assert_eq!(formatted(r#"{"a":{},"b":[]}"#, Flavor::Json), "{\n  \"a\": {},\n  \"b\": []\n}\n");
    }

    #[test]
    fn preserves_scalar_bytes() {
        let src = "{\"n\":1e-7,\"s\":\"\\u00e9 \\n x\"}";
        assert_eq!(
            formatted(src, Flavor::Json),
            "{\n  \"n\": 1e-7,\n  \"s\": \"\\u00e9 \\n x\"\n}\n"
        );
    }

    #[test]
    fn keeps_comments_in_place() {
        let src = "// top\n{\n  // about a\n  \"a\": 1, // trailing\n  \"b\": 2\n  /* last */\n}\n";
        assert_eq!(formatted(src, Flavor::Jsonc), src);
    }

    #[test]
    fn drops_trailing_commas() {
        assert_eq!(formatted("{\n  \"a\": 1,\n}\n", Flavor::Jsonc), "{\n  \"a\": 1\n}\n");
    }

    #[test]
    fn sort_keys_recursively_with_comments_attached() {
        let src = "{\n  // about b\n  \"b\": {\n    \"z\": 1,\n    \"a\": 2\n  }, // b tail\n  \"a\": 1\n}\n";
        let opts = FormatOptions { sort_keys: true, ..FormatOptions::default() };
        assert_eq!(
            fmt_with(src, Flavor::Jsonc, &opts).unwrap(),
            "{\n  \"a\": 1,\n  // about b\n  \"b\": {\n    \"a\": 2,\n    \"z\": 1\n  } // b tail\n}\n"
        );
    }

    #[test]
    fn sort_is_stable_for_duplicate_keys() {
        let opts = FormatOptions { sort_keys: true, ..FormatOptions::default() };
        assert_eq!(
            fmt_with("{\"a\": 1, \"a\": 2}", Flavor::Json, &opts).unwrap(),
            "{\n  \"a\": 1,\n  \"a\": 2\n}\n"
        );
    }

    /// Keys of a sorted object, in order — the shape most sort tests want.
    fn sorted_keys(src: &str, flavor: Flavor) -> Vec<String> {
        let opts = FormatOptions { sort_keys: true, ..FormatOptions::default() };
        let out = fmt_with(src, flavor, &opts).unwrap_or_else(|| src.to_string());
        out.lines()
            .filter_map(|line| line.trim().split_once(": "))
            .map(|(key, _)| key.trim_matches(|c| c == '"' || c == '\'').to_string())
            .collect()
    }

    #[test]
    fn sort_groups_bracketed_keys_before_words() {
        // The reported case: `[` (U+005B) sits between the upper and
        // lower alphabets, so code-point order scatters these three.
        assert_eq!(
            sorted_keys(r#"{"astro": 1, "C_pp": 2, "[astro]": 3}"#, Flavor::Json),
            ["[astro]", "astro", "C_pp"]
        );
    }

    #[test]
    fn sort_is_case_insensitive() {
        assert_eq!(
            sorted_keys(r#"{"Zebra": 1, "apple": 2, "Apricot": 3, "banana": 4}"#, Flavor::Json),
            ["apple", "Apricot", "banana", "Zebra"]
        );
    }

    #[test]
    fn sort_falls_back_to_code_points_for_case_only_ties() {
        // Deterministic and total: same letters, uppercase first.
        assert_eq!(
            sorted_keys(r#"{"ab": 1, "AB": 2, "Ab": 3, "aB": 4}"#, Flavor::Json),
            ["AB", "Ab", "aB", "ab"]
        );
    }

    #[test]
    fn sort_compares_digit_runs_as_numbers() {
        assert_eq!(
            sorted_keys(r#"{"item10": 1, "item9": 2, "item100": 3, "item2": 4}"#, Flavor::Json),
            ["item2", "item9", "item10", "item100"]
        );
    }

    #[test]
    fn sort_ignores_leading_zeros_in_digit_runs() {
        assert_eq!(
            sorted_keys(r#"{"v007": 1, "v7a": 2, "v10": 3}"#, Flavor::Json),
            ["v007", "v7a", "v10"]
        );
    }

    #[test]
    fn sort_orders_dotted_setting_names_by_segment() {
        assert_eq!(
            sorted_keys(
                r#"{"editor.fontSize": 1, "$schema": 2, "editor.formatOnSave": 3, "[json]": 4, "editorBracket": 5}"#,
                Flavor::Json
            ),
            ["$schema", "[json]", "editor.fontSize", "editor.formatOnSave", "editorBracket"]
        );
    }

    #[test]
    fn sort_handles_non_ascii_keys() {
        // Case folds (`É` with `é`), but this is not a full Unicode
        // collation: accented letters keep their code points, so they
        // land after the ASCII alphabet rather than beside `e`.
        assert_eq!(
            sorted_keys(r#"{"Éclair": 1, "eclair": 2, "école": 3, "zebra": 4}"#, Flavor::Json),
            ["eclair", "zebra", "Éclair", "école"]
        );
    }

    #[test]
    fn sort_places_a_prefix_before_its_extensions() {
        assert_eq!(
            sorted_keys(r#"{"abc": 1, "ab": 2, "": 3, "a": 4}"#, Flavor::Json),
            ["", "a", "ab", "abc"]
        );
    }

    #[test]
    fn sort_uses_decoded_key_text_not_the_escapes() {
        // "\u0061pple" is `apple`, and must sort as one.
        assert_eq!(
            sorted_keys("{\"banana\": 1, \"\\u0061pple\": 2}", Flavor::Json),
            ["\\u0061pple", "banana"]
        );
    }

    #[test]
    fn sort_orders_unquoted_json5_keys_with_the_quoted_ones() {
        assert_eq!(
            sorted_keys("{zed: 1, 'apple': 2, beta: 3}", Flavor::Json5),
            ["apple", "beta", "zed"]
        );
    }

    #[test]
    fn sort_is_idempotent() {
        let opts = FormatOptions { sort_keys: true, ..FormatOptions::default() };
        let src = r#"{"[astro]": 1, "C_pp": {"z9": 1, "z10": 2}, "astro": 3, "Astro": 4}"#;
        let once = fmt_with(src, Flavor::Json, &opts).unwrap();
        assert_eq!(fmt_with(&once, Flavor::Json, &opts), None, "second sort still edits");
    }

    #[test]
    fn json5_scalars_survive_untouched() {
        let src = "{unquoted:'single',hex:0xFF,half:.5,inf:-Infinity}";
        assert_eq!(
            formatted(src, Flavor::Json5),
            "{\n  unquoted: 'single',\n  hex: 0xFF,\n  half: .5,\n  inf: -Infinity\n}\n"
        );
    }

    #[test]
    fn tabs_when_asked() {
        let opts = FormatOptions { insert_spaces: false, ..FormatOptions::default() };
        assert_eq!(fmt_with("{\"a\":1}", Flavor::Json, &opts).unwrap(), "{\n\t\"a\": 1\n}\n");
    }

    #[test]
    fn wide_indent_when_asked() {
        let opts = FormatOptions { tab_size: 4, ..FormatOptions::default() };
        assert_eq!(fmt_with("{\"a\":1}", Flavor::Json, &opts).unwrap(), "{\n    \"a\": 1\n}\n");
    }

    #[test]
    fn already_formatted_yields_none() {
        assert_eq!(fmt("{\n  \"a\": 1\n}\n", Flavor::Json), None);
    }

    #[test]
    fn refuses_broken_files() {
        assert_eq!(fmt("{\"a\": }", Flavor::Json), None);
        assert_eq!(fmt("{\"a\": \"unterminated", Flavor::Json), None);
        assert_eq!(fmt("{} {}", Flavor::Json), None);
    }

    #[test]
    fn formats_scalar_root() {
        assert_eq!(formatted("  42  ", Flavor::Json), "42\n");
    }

    #[test]
    fn empty_file_produces_no_edit() {
        assert_eq!(fmt("", Flavor::Json), None);
        assert_eq!(fmt("\n", Flavor::Json), None);
    }

    #[test]
    fn is_idempotent() {
        let sources = [
            ("// a\n{\"b\":[1,{\"c\":2},],\"a\":'x'} // t", Flavor::Json5),
            ("{\"a\":{},\"b\":[[]],\"c\":\"multi \\n line\"}", Flavor::Json),
        ];
        for (src, flavor) in sources {
            let once = formatted(src, flavor);
            assert_eq!(formatted(&once, flavor), once, "not idempotent for {src:?}");
        }
    }

    #[test]
    fn crlf_input_comes_out_lf() {
        assert_eq!(formatted("{\r\n  \"a\": 1\r\n}\r\n", Flavor::Json), "{\n  \"a\": 1\n}\n");
    }

    #[test]
    fn jsonl_compacts_each_record() {
        let input = "{\"b\": 1, \"a\": {\"y\": 2}}\n\n[1,2]\n\"text\"\n";
        assert_eq!(
            formatted(input, Flavor::Jsonl),
            "{\"b\": 1, \"a\": {\"y\": 2}}\n[1, 2]\n\"text\"\n"
        );
    }

    #[test]
    fn jsonl_sorts_each_record() {
        let opts = FormatOptions { sort_keys: true, ..FormatOptions::default() };
        assert_eq!(
            fmt_with("{\"b\": 1, \"a\": 2}\n", Flavor::Jsonl, &opts).unwrap(),
            "{\"a\": 2, \"b\": 1}\n"
        );
    }

    #[test]
    fn jsonl_refuses_broken_lines() {
        assert_eq!(fmt("{\"a\": 1}\nnot json\n", Flavor::Jsonl), None);
    }

    #[test]
    fn final_newline_can_be_disabled() {
        let opts = FormatOptions { insert_final_newline: false, ..FormatOptions::default() };
        assert_eq!(fmt_with("{\"a\":1}", Flavor::Json, &opts).unwrap(), "{\n  \"a\": 1\n}");
    }
}
