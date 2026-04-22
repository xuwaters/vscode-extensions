//! Extract `# proto-file:` / `# proto-message:` / `# proto-import:` /
//! `# proto-syntax:` header annotations from the leading `#` comments of a
//! textproto document. Mirrors the convention used by `txtpbfmt` and the
//! public specs so the analyzer can bind a document to its schema.

use super::ast::{HeaderAnnotation, HeaderHints};
use super::lexer::Comment;
use crate::diagnostics::{DiagnosticCode, ProtoDiagnostic, Severity};
use crate::spans::ByteSpan;
use smol_str::SmolStr;

pub struct HeaderOutcome {
    pub hints: HeaderHints,
    pub diagnostics: Vec<ProtoDiagnostic>,
}

const KNOWN_KEYS: &[&str] = &["proto-file", "proto-message", "proto-import", "proto-syntax"];

/// Extract header annotations. Only consecutive `#` comments starting from the
/// top of the file are considered part of the header — once we see a comment
/// whose body has no `proto-*` prefix, we still keep scanning for any later
/// leading comments until a non-comment token, so the author can interleave
/// human comments with schema hints.
pub fn extract(source: &str, comments: &[Comment]) -> HeaderOutcome {
    let mut hints = HeaderHints::default();
    let mut diagnostics = Vec::new();

    for c in comments {
        // Only inspect leading comments — we consider any `#` comment at or
        // before the first token. Since our parser gathers all comments into
        // a single list and textproto is data (no functions), the cheapest
        // correct policy is: any `# proto-*: …` line anywhere in the file
        // participates in the header. The spec never attaches proto-* hints
        // to interior positions.
        let Some(ann) = try_parse_annotation(source, c) else { continue };
        match ann.key.as_str() {
            "proto-file" => assign_unique(&mut hints.proto_file, ann, &mut diagnostics),
            "proto-message" => assign_unique(&mut hints.proto_message, ann, &mut diagnostics),
            "proto-import" => hints.proto_import.push(ann),
            "proto-syntax" => assign_unique(&mut hints.proto_syntax, ann, &mut diagnostics),
            _ => {
                // Matches `proto-` prefix but unknown key.
                diagnostics.push(ProtoDiagnostic::new(
                    DiagnosticCode::TextprotoHeaderUnknown,
                    Severity::Warning,
                    format!("Unknown header annotation `# {}:`", ann.key),
                    ann.key_span,
                ));
                hints.unknown.push(ann);
            }
        }
    }

    HeaderOutcome { hints, diagnostics }
}

fn assign_unique(
    slot: &mut Option<HeaderAnnotation>,
    ann: HeaderAnnotation,
    diagnostics: &mut Vec<ProtoDiagnostic>,
) {
    if slot.is_some() {
        diagnostics.push(ProtoDiagnostic::new(
            DiagnosticCode::TextprotoHeaderDuplicate,
            Severity::Warning,
            format!("Duplicate header annotation `# {}:` — earlier value wins", ann.key),
            ann.key_span,
        ));
        return;
    }
    *slot = Some(ann);
}

/// Recognise `# <key>: <value>` where `<key>` starts with `proto-`. Returns
/// `None` for plain `#` comments so human prose isn't treated as an error.
fn try_parse_annotation(source: &str, c: &Comment) -> Option<HeaderAnnotation> {
    let body_start = c.span.start as usize + 1; // skip `#`
    let body_end = c.span.end as usize;
    let body = &source[body_start..body_end];
    // Trim leading whitespace within the comment body.
    let trimmed = body.trim_start();
    let leading_ws = body.len() - trimmed.len();
    let key_start = body_start + leading_ws;

    // A header annotation is any `proto-*: value` — we report an unknown-key
    // warning for `proto-*` that isn't recognised so typos don't go silent.
    let (key, rest) = trimmed.split_once(':')?;
    let key = key.trim_end();
    if !key.starts_with("proto-") {
        return None;
    }
    let key_end = key_start + key.len();

    // `rest` is what appears after the first `:` (exclusive). Trim whitespace
    // off its front to find where the value text actually begins.
    let value_body = rest.trim_start();
    let value_leading_ws = rest.len() - value_body.len();
    let value_start = key_end + 1 /* the `:` */ + value_leading_ws;
    let value = value_body.trim_end();
    let value_end = value_start + value.len();

    let _ = KNOWN_KEYS; // reserved for future "did you mean" suggestions
    Some(HeaderAnnotation {
        key: SmolStr::new(key),
        key_span: ByteSpan::new(key_start as u32, key_end as u32),
        value: value.to_string(),
        value_span: ByteSpan::new(value_start as u32, value_end as u32),
        span: c.span,
    })
}

#[cfg(test)]
mod tests {
    use super::super::lexer::lex;
    use super::*;

    #[test]
    fn extracts_proto_file_and_message() {
        let src = "# proto-file: foo/bar.proto\n# proto-message: pkg.Msg\nname: \"a\"";
        let lex_out = lex(src);
        let out = extract(src, &lex_out.comments);
        assert!(out.diagnostics.is_empty());
        let pf = out.hints.proto_file.unwrap();
        assert_eq!(pf.value, "foo/bar.proto");
        let pm = out.hints.proto_message.unwrap();
        assert_eq!(pm.value, "pkg.Msg");
    }

    #[test]
    fn value_span_points_at_value_text() {
        let src = "# proto-file: schema.proto";
        let lex_out = lex(src);
        let out = extract(src, &lex_out.comments);
        let pf = out.hints.proto_file.unwrap();
        let slice = &src[pf.value_span.start as usize..pf.value_span.end as usize];
        assert_eq!(slice, "schema.proto");
    }

    #[test]
    fn duplicate_is_warning_and_first_wins() {
        let src = "# proto-file: a.proto\n# proto-file: b.proto";
        let lex_out = lex(src);
        let out = extract(src, &lex_out.comments);
        assert_eq!(out.diagnostics.len(), 1);
        assert_eq!(out.hints.proto_file.unwrap().value, "a.proto");
    }

    #[test]
    fn unknown_proto_dash_key_is_warning() {
        let src = "# proto-banana: 42";
        let lex_out = lex(src);
        let out = extract(src, &lex_out.comments);
        assert_eq!(out.diagnostics.len(), 1);
        assert_eq!(out.diagnostics[0].code, DiagnosticCode::TextprotoHeaderUnknown);
    }

    #[test]
    fn plain_comments_are_ignored() {
        let src = "# just a note\nname: \"a\"";
        let lex_out = lex(src);
        let out = extract(src, &lex_out.comments);
        assert!(out.diagnostics.is_empty());
        assert!(out.hints.proto_file.is_none());
    }
}
