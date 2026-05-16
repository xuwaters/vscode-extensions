//! Completion provider — surfaces keys defined in the same file when
//! the cursor is inside a variable-reference position (`$|`, `${|}`).
//!
//! Strategy:
//!
//! * If the cursor sits inside a value and the prefix to the cursor ends
//!   in `$NAME_PREFIX` or `${NAME_PREFIX`, emit completions for every key
//!   defined earlier (or anywhere) in the file matching that prefix.
//! * Otherwise, return nothing — completing key *names* at the start of
//!   a line is not useful (users always type new keys there).

use crate::ast::{Entry, File};
use crate::parse::ParsedFile;
use crate::spans::LineCol;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub kind: CompletionKind,
    pub insert_text: String,
    pub replace_length: u32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum CompletionKind {
    Variable,
}

pub fn completions(parsed: &ParsedFile, pos: LineCol) -> Vec<CompletionItem> {
    let cursor = parsed.spans.line_col_to_offset(&parsed.source, pos) as usize;
    let bytes = parsed.source.as_bytes();
    if cursor > bytes.len() {
        return Vec::new();
    }

    // Are we inside a value (any assignment's value_span containing the cursor)?
    let inside_value = is_inside_any_value(&parsed.ast, cursor as u32);
    if !inside_value {
        return Vec::new();
    }

    // Look back from the cursor for the trigger: `$NAME_PREFIX` or `${NAME_PREFIX`.
    let Some((prefix_len, name_prefix, braced)) = scan_back_for_trigger(bytes, cursor) else {
        return Vec::new();
    };

    let keys = collect_keys(&parsed.ast);
    let mut items: Vec<CompletionItem> = keys
        .iter()
        .filter(|k| k.starts_with(name_prefix))
        .map(|k| {
            let insert = if braced {
                k.to_string()
            } else {
                k.to_string()
            };
            CompletionItem {
                label: k.clone(),
                detail: Some("dotenv key".to_string()),
                kind: CompletionKind::Variable,
                insert_text: insert,
                replace_length: prefix_len as u32,
            }
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup_by(|a, b| a.label == b.label);
    items
}

fn collect_keys(file: &File) -> Vec<String> {
    let mut out = Vec::new();
    for entry in &file.entries {
        if let Entry::Assignment(a) = entry {
            if !a.name.name.is_empty() {
                out.push(a.name.name.clone());
            }
        }
    }
    out
}

fn is_inside_any_value(file: &File, cursor: u32) -> bool {
    for entry in &file.entries {
        if let Entry::Assignment(a) = entry {
            if cursor >= a.value_span.start && cursor <= a.value_span.end {
                return true;
            }
            // Also accept cursor sitting one past the `=` even when the
            // value is empty — value_span.start == value_span.end there.
            if cursor > a.equals_span.start && cursor <= a.span.end {
                return true;
            }
        }
    }
    false
}

/// Walk back from `cursor` over identifier chars; if we land on `$` or
/// `${`, return `(chars_to_replace, prefix, braced)`. Returns None
/// otherwise.
fn scan_back_for_trigger(bytes: &[u8], cursor: usize) -> Option<(usize, &str, bool)> {
    let mut i = cursor;
    while i > 0 && is_ident_byte(bytes[i - 1]) {
        i -= 1;
    }
    let name_start = i;
    // Now check what immediately precedes name_start.
    if name_start == 0 {
        return None;
    }
    let prev = bytes[name_start - 1];
    if prev == b'$' {
        let prefix = std::str::from_utf8(&bytes[name_start..cursor]).ok()?;
        // `cursor - (name_start - 1)` would also overwrite the `$`. We
        // only replace the prefix chars (excluding the `$`).
        return Some((cursor - name_start, prefix, false));
    }
    if prev == b'{' && name_start >= 2 && bytes[name_start - 2] == b'$' {
        let prefix = std::str::from_utf8(&bytes[name_start..cursor]).ok()?;
        return Some((cursor - name_start, prefix, true));
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    fn complete_at(src: &str, line: u32, col: u32) -> Vec<String> {
        let pf = parse(FileUri::new("t"), src.to_string());
        completions(&pf, LineCol { line, col })
            .into_iter()
            .map(|i| i.label)
            .collect()
    }

    #[test]
    fn completes_after_dollar() {
        // Line 1, cursor right after `$` (col 3 on `B=$`).
        let labels = complete_at("FOO=1\nB=$\n", 1, 3);
        assert_eq!(labels, vec!["B", "FOO"]);
    }

    #[test]
    fn completes_with_prefix() {
        let labels = complete_at("FOO=1\nBAR=2\nB=$F\n", 2, 4);
        assert_eq!(labels, vec!["FOO"]);
    }

    #[test]
    fn completes_inside_braces() {
        // Source line 1: `OUT="${F"` (cols: O=0 U=1 T=2 ==3 "=4 $=5 {=6 F=7 "=8).
        // Cursor right after `F` is col 8.
        let labels = complete_at("FOO=1\nOUT=\"${F\"\n", 1, 8);
        assert_eq!(labels, vec!["FOO"]);
    }

    #[test]
    fn no_completion_outside_value() {
        let labels = complete_at("FOO=1\n\n", 0, 1);
        assert!(labels.is_empty());
    }
}
