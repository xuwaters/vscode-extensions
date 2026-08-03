//! Hand-written `.env` parser.
//!
//! `.env` is line-oriented but with one wrinkle: a quoted value may span
//! multiple physical lines (`KEY="line1\nline2"`). The parser consumes
//! byte-by-byte, joining quoted continuations into a single
//! [`ast::Assignment`].

use crate::ast::{Assignment, Blank, Comment, Entry, File, Identifier, Invalid, ValueKind, VarRef};
use crate::diagnostics::{DiagnosticCode, DotenvDiagnostic};
use crate::spans::ByteSpan;
use std::collections::HashMap;

pub struct Parser<'a> {
    source: &'a [u8],
    pos: usize,
    diagnostics: Vec<DotenvDiagnostic>,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str) -> Self {
        Parser { source: source.as_bytes(), pos: 0, diagnostics: Vec::new() }
    }

    pub fn into_diagnostics(self) -> Vec<DotenvDiagnostic> {
        self.diagnostics
    }

    pub fn diagnostics(&self) -> &[DotenvDiagnostic] {
        &self.diagnostics
    }

    pub fn parse_file(&mut self) -> File {
        let mut entries = Vec::new();
        while self.pos < self.source.len() {
            let line_start = self.pos;
            let trimmed_start = self.skip_inline_whitespace(line_start);

            // Blank line (possibly with whitespace only).
            if self.is_eol_at(trimmed_start) {
                let end = self.consume_to_eol();
                entries.push(Entry::Blank(Blank {
                    span: ByteSpan::from_usize(line_start, end),
                }));
                continue;
            }

            // Comment line.
            if self.source[trimmed_start] == b'#' {
                let end = self.consume_to_eol();
                let text = std::str::from_utf8(&self.source[trimmed_start..end_excl_newline(end, self.source)])
                    .unwrap_or("")
                    .to_string();
                entries.push(Entry::Comment(Comment {
                    span: ByteSpan::from_usize(line_start, end),
                    text,
                }));
                continue;
            }

            // Otherwise: assignment (or malformed).
            entries.push(self.parse_assignment(line_start, trimmed_start));
        }
        let file = File { entries };
        self.check_duplicates(&file);
        self.check_var_refs(&file);
        file
    }

    fn parse_assignment(&mut self, line_start: usize, content_start: usize) -> Entry {
        let mut cursor = content_start;

        // Optional `export ` prefix.
        let mut export = false;
        if self.source[cursor..].starts_with(b"export") {
            let after = cursor + 6;
            if after < self.source.len() && matches!(self.source[after], b' ' | b'\t') {
                export = true;
                cursor = self.skip_inline_whitespace(after + 1);
            }
        }

        let key_start = cursor;
        while cursor < self.source.len() {
            let c = self.source[cursor];
            if is_key_char(c) {
                cursor += 1;
            } else {
                break;
            }
        }
        let key_end = cursor;
        let after_key_ws = self.skip_inline_whitespace(cursor);

        // Find the `=`. Anything else after the key (besides whitespace) is invalid.
        if after_key_ws >= self.source.len() || self.source[after_key_ws] != b'=' {
            // No `=` on this line — emit MissingEquals and keep the line
            // as an opaque entry so it survives a round-trip.
            let line_end = self.consume_to_eol();
            self.diagnostics.push(DotenvDiagnostic::warning(
                DiagnosticCode::MissingEquals,
                "line is not blank, a comment, or an assignment",
                ByteSpan::from_usize(line_start, end_excl_newline(line_end, self.source)),
            ));
            return Entry::Invalid(Invalid {
                span: ByteSpan::from_usize(line_start, line_end),
            });
        }
        let equals_pos = after_key_ws;

        // Empty / invalid key?
        if key_end == key_start {
            self.diagnostics.push(DotenvDiagnostic::error(
                DiagnosticCode::EmptyKey,
                "assignment has no key",
                ByteSpan::from_usize(key_start, equals_pos),
            ));
        } else if !is_valid_key(&self.source[key_start..key_end]) {
            // is_key_char only allows valid chars, so this fires only if
            // the first char is a digit.
            self.diagnostics.push(DotenvDiagnostic::warning(
                DiagnosticCode::InvalidKey,
                "key should start with a letter or underscore",
                ByteSpan::from_usize(key_start, key_end),
            ));
        }

        // Detect spaces around `=`.
        let space_before = key_end != after_key_ws;
        let after_eq = equals_pos + 1;
        let space_after = after_eq < self.source.len()
            && matches!(self.source[after_eq], b' ' | b'\t');
        if space_before || space_after {
            // Span just the `=` and any surrounding whitespace.
            let span_start = key_end;
            let mut span_end = after_eq;
            while span_end < self.source.len()
                && matches!(self.source[span_end], b' ' | b'\t')
            {
                span_end += 1;
            }
            self.diagnostics.push(DotenvDiagnostic::warning(
                DiagnosticCode::SpacesAroundEquals,
                "spaces around `=` may not be accepted by some dotenv loaders",
                ByteSpan::from_usize(span_start, span_end),
            ));
        }

        // Parse the value.
        let value_start = self.skip_inline_whitespace(equals_pos + 1);
        let (value_end, value_kind, references, line_end) =
            self.parse_value(value_start);
        self.pos = line_end;

        let name = std::str::from_utf8(&self.source[key_start..key_end])
            .unwrap_or("")
            .to_string();
        Entry::Assignment(Assignment {
            span: ByteSpan::from_usize(line_start, line_end),
            export,
            name: Identifier {
                name,
                span: ByteSpan::from_usize(key_start, key_end),
            },
            equals_span: ByteSpan::from_usize(equals_pos, equals_pos + 1),
            value_span: ByteSpan::from_usize(value_start, value_end),
            value_kind,
            references,
        })
    }

    /// Returns `(value_end, kind, refs, line_end_including_newline)`.
    fn parse_value(
        &mut self,
        start: usize,
    ) -> (usize, ValueKind, Vec<VarRef>, usize) {
        if start >= self.source.len() || self.is_eol_at(start) {
            let line_end = consume_to_eol_from(self.source, start);
            return (start, ValueKind::Empty, Vec::new(), line_end);
        }

        let first = self.source[start];
        if first == b'"' {
            self.parse_double_quoted(start)
        } else if first == b'\'' {
            self.parse_single_quoted(start)
        } else {
            self.parse_unquoted(start)
        }
    }

    fn parse_unquoted(
        &mut self,
        start: usize,
    ) -> (usize, ValueKind, Vec<VarRef>, usize) {
        let mut i = start;
        let mut last_non_ws = start;
        let mut refs = Vec::new();
        while i < self.source.len() {
            let c = self.source[i];
            if c == b'\n' || c == b'\r' {
                break;
            }
            // Inline comment: ` #...` (space then hash) terminates the value.
            if c == b'#' && i > start && matches!(self.source[i - 1], b' ' | b'\t') {
                break;
            }
            if c == b'$' {
                if let Some(r) = self.try_parse_var_ref(i) {
                    let end = r.span.end as usize;
                    refs.push(r);
                    last_non_ws = end;
                    i = end;
                    continue;
                }
            }
            if c != b' ' && c != b'\t' {
                last_non_ws = i + 1;
            }
            i += 1;
        }
        let line_end = consume_to_eol_from(self.source, i);
        (last_non_ws, ValueKind::Unquoted, refs, line_end)
    }

    fn parse_single_quoted(
        &mut self,
        start: usize,
    ) -> (usize, ValueKind, Vec<VarRef>, usize) {
        // Skip opening quote.
        let mut i = start + 1;
        while i < self.source.len() {
            if self.source[i] == b'\'' {
                let end = i + 1;
                let line_end = consume_to_eol_from(self.source, end);
                return (end, ValueKind::SingleQuoted, Vec::new(), line_end);
            }
            i += 1;
        }
        // Unclosed.
        self.diagnostics.push(DotenvDiagnostic::error(
            DiagnosticCode::UnclosedQuote,
            "unclosed single quote",
            ByteSpan::from_usize(start, i),
        ));
        (i, ValueKind::UnclosedSingle, Vec::new(), i)
    }

    fn parse_double_quoted(
        &mut self,
        start: usize,
    ) -> (usize, ValueKind, Vec<VarRef>, usize) {
        let mut i = start + 1;
        let mut refs = Vec::new();
        while i < self.source.len() {
            let c = self.source[i];
            if c == b'\\' && i + 1 < self.source.len() {
                i += 2;
                continue;
            }
            if c == b'"' {
                let end = i + 1;
                let line_end = consume_to_eol_from(self.source, end);
                return (end, ValueKind::DoubleQuoted, refs, line_end);
            }
            if c == b'$' {
                if let Some(r) = self.try_parse_var_ref(i) {
                    let end = r.span.end as usize;
                    refs.push(r);
                    i = end;
                    continue;
                }
            }
            i += 1;
        }
        self.diagnostics.push(DotenvDiagnostic::error(
            DiagnosticCode::UnclosedQuote,
            "unclosed double quote",
            ByteSpan::from_usize(start, i),
        ));
        (i, ValueKind::UnclosedDouble, refs, i)
    }

    /// Try to parse `$NAME` or `${NAME}` starting at the `$`. Returns
    /// `None` if no valid reference is present.
    fn try_parse_var_ref(&self, dollar: usize) -> Option<VarRef> {
        let after = dollar + 1;
        if after >= self.source.len() {
            return None;
        }
        if self.source[after] == b'{' {
            // ${NAME} — accept any non-`}` inside, but only report the
            // bare identifier portion for diagnostics.
            let name_start = after + 1;
            let mut i = name_start;
            while i < self.source.len() && self.source[i] != b'}' && self.source[i] != b'\n' {
                i += 1;
            }
            if i >= self.source.len() || self.source[i] != b'}' {
                return None;
            }
            let name_end = first_non_ident(&self.source[name_start..i]) + name_start;
            let name = std::str::from_utf8(&self.source[name_start..name_end])
                .unwrap_or("")
                .to_string();
            Some(VarRef {
                name,
                span: ByteSpan::from_usize(dollar, i + 1),
                name_span: ByteSpan::from_usize(name_start, name_end),
                braced: true,
            })
        } else {
            // $NAME — must start with letter/underscore.
            let c = self.source[after];
            if !is_key_start(c) {
                return None;
            }
            let mut i = after + 1;
            while i < self.source.len() && is_key_char(self.source[i]) {
                i += 1;
            }
            let name = std::str::from_utf8(&self.source[after..i])
                .unwrap_or("")
                .to_string();
            Some(VarRef {
                name,
                span: ByteSpan::from_usize(dollar, i),
                name_span: ByteSpan::from_usize(after, i),
                braced: false,
            })
        }
    }

    fn check_duplicates(&mut self, file: &File) {
        let mut seen: HashMap<&str, ByteSpan> = HashMap::new();
        for entry in &file.entries {
            let Entry::Assignment(a) = entry else { continue };
            if a.name.name.is_empty() {
                continue;
            }
            if let Some(_prev) = seen.get(a.name.name.as_str()) {
                self.diagnostics.push(DotenvDiagnostic::warning(
                    DiagnosticCode::DuplicateKey,
                    format!("duplicate key `{}`", a.name.name),
                    a.name.span,
                ));
            } else {
                seen.insert(a.name.name.as_str(), a.name.span);
            }
        }
    }

    fn check_var_refs(&mut self, file: &File) {
        let defined: std::collections::HashSet<&str> = file
            .entries
            .iter()
            .filter_map(|e| match e {
                Entry::Assignment(a) if !a.name.name.is_empty() => Some(a.name.name.as_str()),
                _ => None,
            })
            .collect();
        for entry in &file.entries {
            let Entry::Assignment(a) = entry else { continue };
            for r in &a.references {
                if r.name.is_empty() {
                    continue;
                }
                if !defined.contains(r.name.as_str()) {
                    self.diagnostics.push(DotenvDiagnostic::info(
                        DiagnosticCode::UnknownVariableRef,
                        format!("`{}` is not defined in this file", r.name),
                        r.name_span,
                    ));
                }
            }
        }
    }

    fn skip_inline_whitespace(&self, mut i: usize) -> usize {
        while i < self.source.len() && matches!(self.source[i], b' ' | b'\t') {
            i += 1;
        }
        i
    }

    fn is_eol_at(&self, i: usize) -> bool {
        i >= self.source.len() || matches!(self.source[i], b'\n' | b'\r')
    }

    fn consume_to_eol(&mut self) -> usize {
        let end = consume_to_eol_from(self.source, self.pos);
        self.pos = end;
        end
    }
}

fn consume_to_eol_from(src: &[u8], mut i: usize) -> usize {
    while i < src.len() && src[i] != b'\n' {
        i += 1;
    }
    if i < src.len() && src[i] == b'\n' {
        i += 1;
    }
    i
}

fn end_excl_newline(end: usize, src: &[u8]) -> usize {
    let mut e = end;
    while e > 0 && matches!(src[e - 1], b'\n' | b'\r') {
        e -= 1;
    }
    e
}

fn is_key_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_key_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'.'
}

fn is_valid_key(bytes: &[u8]) -> bool {
    bytes.first().is_some_and(|c| is_key_start(*c))
}

fn first_non_ident(bytes: &[u8]) -> usize {
    let mut i = 0;
    if i < bytes.len() && is_key_start(bytes[i]) {
        i += 1;
        while i < bytes.len() && is_key_char(bytes[i]) {
            i += 1;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> (File, Vec<DotenvDiagnostic>) {
        let mut p = Parser::new(src);
        let f = p.parse_file();
        (f, p.into_diagnostics())
    }

    #[test]
    fn simple_assignment() {
        let (f, diags) = parse("FOO=bar\n");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(f.entries.len(), 1);
        let Entry::Assignment(a) = &f.entries[0] else { panic!() };
        assert_eq!(a.name.name, "FOO");
        assert_eq!(a.value_kind, ValueKind::Unquoted);
    }

    #[test]
    fn export_prefix() {
        let (f, diags) = parse("export PATH=/usr/bin\n");
        assert!(diags.is_empty());
        let Entry::Assignment(a) = &f.entries[0] else { panic!() };
        assert!(a.export);
        assert_eq!(a.name.name, "PATH");
    }

    #[test]
    fn double_quoted_value() {
        let (f, diags) = parse("MSG=\"hello world\"\n");
        assert!(diags.is_empty());
        let Entry::Assignment(a) = &f.entries[0] else { panic!() };
        assert_eq!(a.value_kind, ValueKind::DoubleQuoted);
    }

    #[test]
    fn single_quoted_no_interpolation() {
        let (f, diags) = parse("LITERAL='$NOT_A_REF'\n");
        assert!(diags.is_empty());
        let Entry::Assignment(a) = &f.entries[0] else { panic!() };
        assert_eq!(a.value_kind, ValueKind::SingleQuoted);
        assert!(a.references.is_empty());
    }

    #[test]
    fn variable_reference() {
        let (f, _diags) = parse("A=1\nB=$A-${A}\n");
        let Entry::Assignment(b) = &f.entries[1] else { panic!() };
        assert_eq!(b.references.len(), 2);
        assert_eq!(b.references[0].name, "A");
        assert!(!b.references[0].braced);
        assert_eq!(b.references[1].name, "A");
        assert!(b.references[1].braced);
    }

    #[test]
    fn unknown_var_ref_emits_info() {
        let (_f, diags) = parse("X=$UNDEFINED\n");
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::UnknownVariableRef),
            "diags: {diags:?}"
        );
    }

    #[test]
    fn duplicate_key_warns() {
        let (_f, diags) = parse("FOO=1\nFOO=2\n");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::DuplicateKey));
    }

    #[test]
    fn spaces_around_equals_warns() {
        let (_f, diags) = parse("FOO = bar\n");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::SpacesAroundEquals));
    }

    #[test]
    fn missing_equals_warns() {
        let (_f, diags) = parse("just_text\n");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::MissingEquals));
    }

    #[test]
    fn missing_equals_line_is_kept_verbatim() {
        let src = "just_text\nA=1\n";
        let (f, _diags) = parse(src);
        assert_eq!(f.entries.len(), 2);
        let Entry::Invalid(i) = &f.entries[0] else { panic!() };
        assert_eq!(&src[i.span.start as usize..i.span.end as usize], "just_text\n");
    }

    #[test]
    fn unclosed_double_quote() {
        let (_f, diags) = parse("X=\"oops\n");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnclosedQuote));
    }

    #[test]
    fn comment_and_blank() {
        let (f, diags) = parse("# top\n\nFOO=1\n");
        assert!(diags.is_empty());
        assert!(matches!(f.entries[0], Entry::Comment(_)));
        assert!(matches!(f.entries[1], Entry::Blank(_)));
        assert!(matches!(f.entries[2], Entry::Assignment(_)));
    }

    #[test]
    fn inline_comment_terminates_unquoted_value() {
        let (f, diags) = parse("FOO=bar # trailing\n");
        assert!(diags.is_empty());
        let Entry::Assignment(a) = &f.entries[0] else { panic!() };
        let value = &"FOO=bar # trailing\n"[a.value_span.start as usize..a.value_span.end as usize];
        assert_eq!(value, "bar");
    }

    #[test]
    fn multiline_double_quoted_value() {
        let src = "MSG=\"line1\nline2\"\nNEXT=ok\n";
        let (f, diags) = parse(src);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(f.entries.len(), 2);
    }

    #[test]
    fn key_starting_with_digit_warns() {
        let (_f, diags) = parse("1FOO=bar\n");
        assert!(diags.iter().any(|d| d.code == DiagnosticCode::InvalidKey));
    }
}
