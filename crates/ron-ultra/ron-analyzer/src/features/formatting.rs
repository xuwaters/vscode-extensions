//! Comment-preserving RON pretty-printer.
//!
//! Scalars are reproduced byte-for-byte (number notation, quoting, escapes,
//! raw strings all survive), and every comment is kept beside the entry it
//! annotates. What gets normalized is shape:
//!
//! * a container goes on one line — `(x: 1, y: 2)` — when it holds no
//!   comments, fits within [`FormatOptions::print_width`], and the author
//!   did not break the line right after its opening bracket;
//! * otherwise it is expanded, one entry per line, indented from the
//!   options, every entry followed by a comma;
//! * spacing is `field: value`, `a, b`, `Name(...)` with no space;
//! * runs of blank lines between entries collapse to one, and blank lines
//!   just inside brackets and at the ends of the file disappear;
//! * attributes are kept verbatim, one per line.
//!
//! The "newline after the opening bracket" rule is the author's switch: put
//! one there and the container stays expanded however short it is; take it
//! out and the formatter collapses it again whenever it fits.
//!
//! Files with a syntax error are refused (`None`): format-on-save fires
//! while you are typing, and rewriting a half-typed document around a hole
//! is worse than leaving it alone.

use crate::ast::{Comment, CommentKind, Container, Entry, EntryKey, File, Value, ValueKind};
use crate::workspace::ParsedFile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FormatOptions {
    pub tab_size: u32,
    pub insert_spaces: bool,
    /// Widest line a collapsed container may produce. `0` expands every
    /// non-empty container.
    pub print_width: u32,
    pub insert_final_newline: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        FormatOptions { tab_size: 4, insert_spaces: true, print_width: 80, insert_final_newline: true }
    }
}

/// Format the whole file. `None` means no edit: either the file has a
/// blocking error, or it is already formatted.
pub fn format_file(pf: &ParsedFile, opts: &FormatOptions) -> Option<String> {
    if pf.diagnostics.iter().any(|d| d.code.blocks_formatting()) {
        return None;
    }
    let mut p = Printer { src: &pf.source, opts, out: String::with_capacity(pf.source.len()) };
    p.file(&pf.ast);
    let mut out = p.out;
    out.truncate(out.trim_end().len());
    if opts.insert_final_newline && !out.is_empty() {
        out.push('\n');
    }
    // Strings and block comments carry their own line endings; settle on
    // the file's, judged by its first line break.
    out = out.replace("\r\n", "\n");
    if pf.source.find('\n').is_some_and(|i| pf.source[..i].ends_with('\r')) {
        out = out.replace('\n', "\r\n");
    }
    (out != pf.source).then_some(out)
}

struct Printer<'a> {
    src: &'a str,
    opts: &'a FormatOptions,
    out: String,
}

impl<'a> Printer<'a> {
    // ---- output primitives -------------------------------------------------

    fn push(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn indent(&mut self, level: usize) {
        for _ in 0..level {
            if self.opts.insert_spaces {
                for _ in 0..self.opts.tab_size.max(1) {
                    self.out.push(' ');
                }
            } else {
                self.out.push('\t');
            }
        }
    }

    /// Start a new line at `level`, after a blank one if `blank`. At the
    /// very start of the output there is nothing to break.
    fn line(&mut self, level: usize, blank: bool) {
        if self.out.is_empty() {
            return;
        }
        self.out.truncate(self.out.trim_end_matches([' ', '\t']).len());
        self.out.push('\n');
        if blank {
            self.out.push('\n');
        }
        self.indent(level);
    }

    /// Display width of the current line so far.
    fn column(&self) -> usize {
        let line = self.out.rsplit('\n').next().unwrap_or("");
        line.chars().map(|c| if c == '\t' { self.opts.tab_size as usize } else { 1 }).sum()
    }

    fn text(&self, span: crate::spans::ByteSpan) -> &'a str {
        &self.src[span.start as usize..span.end as usize]
    }

    fn comment(&mut self, c: &Comment) {
        let text = c.text(self.src);
        self.push(text.trim_end());
    }

    /// Comments that each start a line, except one that shared its line
    /// with the previous comment, which stays beside it. `fresh` is true
    /// when nothing has been written at this level yet, so no blank line
    /// belongs before the first.
    fn comment_lines(&mut self, comments: &[Comment], level: usize, fresh: &mut bool) {
        for (i, c) in comments.iter().enumerate() {
            if i > 0 && !c.own_line {
                self.push(" ");
            } else {
                self.line(level, c.blank_before && !*fresh);
            }
            self.comment(c);
            *fresh = false;
        }
    }

    // ---- file ----------------------------------------------------------------

    fn file(&mut self, f: &File) {
        let mut fresh = true;
        for attr in &f.attributes {
            self.comment_lines(&attr.leading, 0, &mut fresh);
            self.line(0, attr.blank_before && !fresh);
            self.push(self.text(attr.span));
            for c in &attr.trailing {
                self.push(" ");
                self.comment(c);
            }
            fresh = false;
        }
        self.comment_lines(&f.leading, 0, &mut fresh);
        if let Some(value) = &f.value {
            self.line(0, f.value_blank_before && !fresh);
            self.value(value, 0, 0);
            for c in &f.value_trailing {
                self.push(" ");
                self.comment(c);
            }
            fresh = false;
        }
        self.comment_lines(&f.trailing, 0, &mut fresh);
    }

    // ---- values ----------------------------------------------------------------

    /// Write `v` at the current position. `suffix` is how many characters
    /// will follow it on this line (a comma), for the width check.
    fn value(&mut self, v: &Value, level: usize, suffix: usize) {
        let mut level = level;
        for c in &v.prefix {
            self.comment(c);
            if c.kind == CommentKind::Line {
                level += 1;
                self.line(level, false);
            } else {
                self.push(" ");
            }
        }
        let container = v.container();
        if let Some(flat) = self.flat(v) {
            let fits = self.column() + flat.chars().count() + suffix <= self.opts.print_width as usize;
            // An empty container has nothing to put on separate lines.
            if fits || container.is_none_or(|c| c.entries.is_empty()) {
                self.push(&flat);
                return;
            }
        }
        match (&v.kind, container) {
            (ValueKind::Named { name, .. }, Some(c)) => {
                self.push(self.text(name.span));
                self.expanded(c, level);
            }
            (_, Some(c)) => self.expanded(c, level),
            // Scalars always flatten; a multi-line string is written as is.
            _ => self.push(v.raw(self.src)),
        }
    }

    /// The one-line rendering of `v`, ignoring its own prefix comments
    /// (already written). `None` when anything inside forbids one line.
    fn flat(&self, v: &Value) -> Option<String> {
        match &v.kind {
            ValueKind::Named { name, body: Some(c) } => {
                Some(format!("{}{}", self.text(name.span), self.flat_container(c)?))
            }
            ValueKind::Container(c) => self.flat_container(c),
            _ => Some(v.raw(self.src).to_string()),
        }
    }

    fn flat_container(&self, c: &Container) -> Option<String> {
        if !c.dangling.is_empty() {
            return None;
        }
        if !c.entries.is_empty() && (c.broken || self.opts.print_width == 0) {
            return None;
        }
        let mut parts = Vec::with_capacity(c.entries.len());
        for e in &c.entries {
            if !e.leading.is_empty() || !e.trailing.is_empty() {
                return None;
            }
            let value = self.flat_child(&e.value)?;
            parts.push(match &e.key {
                Some(EntryKey::Field(id)) => format!("{}: {value}", self.text(id.span)),
                Some(EntryKey::Map(k)) => format!("{}: {value}", self.flat_child(k)?),
                None => value,
            });
        }
        Some(format!("{}{}{}", c.kind.open(), parts.join(", "), c.kind.close()))
    }

    /// A value nested in a one-line container: no comments of its own, and
    /// no line breaks hidden in a string.
    fn flat_child(&self, v: &Value) -> Option<String> {
        if !v.prefix.is_empty() {
            return None;
        }
        let flat = self.flat(v)?;
        (!flat.contains('\n')).then_some(flat)
    }

    fn expanded(&mut self, c: &Container, level: usize) {
        self.push(c.kind.open());
        let inner = level + 1;
        let mut fresh = true;
        // Comments sharing the opening bracket's line stay there.
        let head = match c.entries.first() {
            Some(e) => &e.leading[..],
            None => &c.dangling[..],
        };
        let sticky = head.iter().take_while(|c| !c.own_line).count();
        for comment in &head[..sticky] {
            self.push(" ");
            self.comment(comment);
        }
        for (i, e) in c.entries.iter().enumerate() {
            let leading = if i == 0 { &e.leading[sticky..] } else { &e.leading[..] };
            self.entry(e, leading, inner, &mut fresh);
        }
        let dangling = if c.entries.is_empty() { &c.dangling[sticky..] } else { &c.dangling[..] };
        self.comment_lines(dangling, inner, &mut fresh);
        self.line(level, false);
        self.push(c.kind.close());
    }

    fn entry(&mut self, e: &Entry, leading: &[Comment], level: usize, fresh: &mut bool) {
        self.comment_lines(leading, level, fresh);
        self.line(level, e.blank_before && !*fresh);
        *fresh = false;
        match &e.key {
            Some(EntryKey::Field(id)) => {
                self.push(self.text(id.span));
                self.push(": ");
            }
            Some(EntryKey::Map(k)) => {
                self.value(k, level, 2);
                self.push(": ");
            }
            None => {}
        }
        self.value(&e.value, level, 1);
        self.push(",");
        for c in &e.trailing {
            self.push(" ");
            self.comment(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{parse, FileUri};
    use pretty_assertions::assert_eq;

    fn format_with(src: &str, opts: &FormatOptions) -> Option<String> {
        format_file(&parse(FileUri::new("t.ron"), src.to_string()), opts)
    }

    /// What the editor ends up with: the input when there is no edit.
    fn fmt(src: &str) -> String {
        fmt_with(src, &FormatOptions::default())
    }

    fn fmt_with(src: &str, opts: &FormatOptions) -> String {
        let out = format_with(src, opts).unwrap_or_else(|| src.to_string());
        let again = format_with(&out, opts).unwrap_or_else(|| out.clone());
        assert_eq!(again, out, "not idempotent for {src:?}");
        out
    }

    #[test]
    fn short_containers_collapse() {
        assert_eq!(fmt("(  x:1,y : 2 ,)"), "(x: 1, y: 2)\n");
        assert_eq!(fmt("[ 1,2,3, ]"), "[1, 2, 3]\n");
        assert_eq!(fmt("{ \"a\" :1 }"), "{\"a\": 1}\n");
        assert_eq!(fmt("Point ( x: 1 )"), "Point(x: 1)\n");
        assert_eq!(fmt("Some( 5 )"), "Some(5)\n");
        assert_eq!(fmt("( )"), "()\n");
        assert_eq!(fmt("[\n]"), "[]\n");
    }

    #[test]
    fn newline_after_open_bracket_keeps_it_expanded() {
        assert_eq!(fmt("Point(\nx: 1, y: 2)"), "Point(\n    x: 1,\n    y: 2,\n)\n");
        assert_eq!(fmt("[\n1]"), "[\n    1,\n]\n");
    }

    #[test]
    fn wide_containers_expand() {
        let opts = FormatOptions { print_width: 20, ..FormatOptions::default() };
        assert_eq!(
            fmt_with("(name: \"a long name\", id: 7)", &opts),
            "(\n    name: \"a long name\",\n    id: 7,\n)\n"
        );
        // Only the container that overflows expands.
        assert_eq!(fmt_with("[(1, 2), (3, 4), (5, 6)]", &opts), "[\n    (1, 2),\n    (3, 4),\n    (5, 6),\n]\n");
    }

    #[test]
    fn width_counts_the_trailing_comma() {
        // `    a: [1, 2, 3],` is 17 wide: fits at 17, not at 16.
        let src = "(\n a: [1, 2, 3])";
        let at = |w| FormatOptions { print_width: w, ..FormatOptions::default() };
        assert_eq!(fmt_with(src, &at(17)), "(\n    a: [1, 2, 3],\n)\n");
        assert_eq!(fmt_with(src, &at(16)), "(\n    a: [\n        1,\n        2,\n        3,\n    ],\n)\n");
    }

    #[test]
    fn print_width_zero_expands_everything() {
        let opts = FormatOptions { print_width: 0, ..FormatOptions::default() };
        assert_eq!(fmt_with("(a: [1], b: ())", &opts), "(\n    a: [\n        1,\n    ],\n    b: (),\n)\n");
    }

    #[test]
    fn scalars_are_verbatim() {
        let src = "[0xFF_u8, 1_000, 1.e5, -inf, r#\"raw \"q\"\"#, b\"\\x00\", '\\u{1F980}', \"esc\\t\", r#type]";
        let wide = FormatOptions { print_width: 200, ..FormatOptions::default() };
        assert_eq!(fmt_with(src, &wide), format!("{src}\n"));
    }

    #[test]
    fn indentation_follows_the_options() {
        let src = "(\na: (\nb: 1))";
        let two = FormatOptions { tab_size: 2, ..FormatOptions::default() };
        assert_eq!(fmt_with(src, &two), "(\n  a: (\n    b: 1,\n  ),\n)\n");
        let tabs = FormatOptions { insert_spaces: false, ..FormatOptions::default() };
        assert_eq!(fmt_with(src, &tabs), "(\n\ta: (\n\t\tb: 1,\n\t),\n)\n");
    }

    #[test]
    fn comments_force_expansion_and_stay_put() {
        let src = "(a: 1, // one\nb: 2)";
        assert_eq!(fmt(src), "(\n    a: 1, // one\n    b: 2,\n)\n");
    }

    #[test]
    fn comment_positions() {
        let src = "\
// file header

#![enable(implicit_some)] // why

Config( // root
    // leading
    a: 1,   /* trailing */


    b: /* inline */ 2,
    c: // explains
        3,
    // dangling
) // after
// footer
";
        let expected = "\
// file header

#![enable(implicit_some)] // why

Config( // root
    // leading
    a: 1, /* trailing */

    b: /* inline */ 2,
    c: // explains
        3,
    // dangling
) // after
// footer
";
        assert_eq!(fmt(src), expected);
    }

    #[test]
    fn comment_in_an_empty_container() {
        assert_eq!(fmt("[ // nothing\n]"), "[ // nothing\n]\n");
        assert_eq!(fmt("[\n    // nothing\n]"), "[\n    // nothing\n]\n");
    }

    #[test]
    fn blank_lines_inside_brackets_and_at_edges_go() {
        assert_eq!(fmt("\n\n(\n\n    a: 1,\n\n)\n\n\n"), "(\n    a: 1,\n)\n");
    }

    #[test]
    fn blank_line_between_comment_and_entry_is_kept() {
        let src = "(\n    a: 1,\n    // section\n\n    b: 2,\n)\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn multiline_strings_never_join_a_line() {
        let src = "(\n    text: \"one\ntwo\",\n)\n";
        assert_eq!(fmt(src), src);
        // A string with a newline cannot sit in a collapsed list.
        assert_eq!(fmt("[\"a\nb\", 1]"), "[\n    \"a\nb\",\n    1,\n]\n");
    }

    #[test]
    fn map_keys_can_be_anything() {
        assert_eq!(fmt("{ (1,2) : [ 3 ], Red: 4 }"), "{(1, 2): [3], Red: 4}\n");
    }

    #[test]
    fn realistic_document() {
        let src = "\
#![enable(implicit_some)]
GameConfig(window_size:(800,600),
  window_title:\"PAC-MAN\",
     fullscreen:false,
  key_bindings:{
    \"up\":Up,\"down\":Down,
  },
)";
        let expected = "\
#![enable(implicit_some)]
GameConfig(
    window_size: (800, 600),
    window_title: \"PAC-MAN\",
    fullscreen: false,
    key_bindings: {
        \"up\": Up,
        \"down\": Down,
    },
)
";
        // The root is wider than 80 columns on one line, so it expands even
        // though `(` is followed by `window_size` on the same line.
        assert_eq!(fmt(src), expected);
    }

    #[test]
    fn refuses_broken_files() {
        assert_eq!(format_with("(a: 1", &FormatOptions::default()), None);
        assert_eq!(format_with("(a: , b: 2)", &FormatOptions::default()), None);
        assert_eq!(format_with("\"open", &FormatOptions::default()), None);
        assert_eq!(format_with("(a: 1, 2)", &FormatOptions::default()), None);
    }

    #[test]
    fn duplicates_do_not_block() {
        assert_eq!(fmt("( a:1, a:2 )"), "(a: 1, a: 2)\n");
    }

    #[test]
    fn already_formatted_is_no_edit() {
        assert_eq!(format_with("(a: 1)\n", &FormatOptions::default()), None);
    }

    #[test]
    fn final_newline_option() {
        let opts = FormatOptions { insert_final_newline: false, ..FormatOptions::default() };
        assert_eq!(fmt_with("(a:1)\n\n", &opts), "(a: 1)");
    }

    #[test]
    fn empty_and_comment_only_files() {
        assert_eq!(format_with("", &FormatOptions::default()), None);
        assert_eq!(fmt("  // todo  \n\n"), "// todo\n");
    }

    #[test]
    fn crlf_is_kept() {
        assert_eq!(fmt("( a:1, // c\r\nb:2 )\r\n"), "(\r\n    a: 1, // c\r\n    b: 2,\r\n)\r\n");
    }
}
