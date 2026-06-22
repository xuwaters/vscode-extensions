//! Completion for `Makefile.toml`.
//!
//! Completion context is derived heuristically from the source rather than
//! a full re-parse: we find the nearest `[table]` header above the cursor
//! to know which schema applies, then decide between *key* completion
//! (left of `=`) and *value* completion (right of `=`).
//!
//! - Inside `[tasks.NAME]` → task field keys (or task names for
//!   `dependencies` / `run_task` / `alias` values).
//! - Inside a task `condition` table → condition criteria.
//! - Inside `[config]` → config keys.
//! - At the document root → top-level section keys.
//! - `script_runner = ` → the built-in runner names.

use serde::Serialize;

use crate::ast::File;
use crate::parse::{self, ParsedFile};
use crate::schema::{self, KeyDoc};
use crate::spans::LineCol;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub kind: CompletionKind,
    pub insert_text: String,
    /// UTF-16 code units before the cursor the editor should overwrite.
    pub replace_length: u32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum CompletionKind {
    Field,
    EnumValue,
    Task,
    Section,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CtxKind {
    Root,
    Task,
    TaskCondition,
    Config,
    EnvOrOther,
}

struct TableCtx {
    kind: CtxKind,
    task_name: Option<String>,
}

pub fn completions(parsed: &ParsedFile, pos: LineCol) -> Vec<CompletionItem> {
    let source = &parsed.source;
    let offset = parsed.spans.line_col_to_offset(source, pos) as usize;
    let offset = offset.min(source.len());

    let line_start = source[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_prefix = &source[line_start..offset];

    // Don't complete while the cursor is on the header line itself.
    if line_prefix.trim_start().starts_with('[') {
        return Vec::new();
    }

    let word = trailing_word(line_prefix);
    let replace_length = utf16_len(word);
    let ctx = current_table(source, line_start);

    // While the user types, the current line is frequently incomplete and
    // would make the whole TOML document fail to parse. Re-parse a repaired
    // copy with the current line removed so task names and already-present
    // keys are still available for completion.
    let line_end = source[offset..].find('\n').map(|i| offset + i).unwrap_or(source.len());
    let repaired = format!("{}{}", &source[..line_start], &source[line_end..]);
    let ast = parse::parse(parsed.uri.clone(), repaired).ast;

    if let Some(eq) = line_prefix.find('=') {
        let key = line_prefix[..eq].trim();
        return value_completions(key, &ast, word, replace_length);
    }

    key_completions(&ctx, &ast, word, replace_length)
}

fn key_completions(
    ctx: &TableCtx,
    ast: &File,
    word: &str,
    replace_length: u32,
) -> Vec<CompletionItem> {
    let (keys, kind): (&[KeyDoc], CompletionKind) = match ctx.kind {
        CtxKind::Root => (schema::TOP_LEVEL_KEYS, CompletionKind::Section),
        CtxKind::Task => (schema::TASK_FIELDS, CompletionKind::Field),
        CtxKind::TaskCondition => (schema::CONDITION_KEYS, CompletionKind::Field),
        CtxKind::Config => (schema::CONFIG_KEYS, CompletionKind::Field),
        CtxKind::EnvOrOther => return Vec::new(),
    };

    let existing = existing_keys(ctx, ast);

    keys.iter()
        .filter(|k| matches_prefix(k.key, word))
        .filter(|k| !existing.iter().any(|e| e == k.key))
        .map(|k| CompletionItem {
            label: k.key.to_string(),
            detail: Some(k.doc.to_string()),
            kind,
            insert_text: k.key.to_string(),
            replace_length,
        })
        .collect()
}

fn value_completions(
    key: &str,
    ast: &File,
    word: &str,
    replace_length: u32,
) -> Vec<CompletionItem> {
    match key {
        "script_runner" => schema::SCRIPT_RUNNERS
            .iter()
            .filter(|k| matches_prefix(k.key, word))
            .map(|k| CompletionItem {
                label: k.key.to_string(),
                detail: Some(k.doc.to_string()),
                kind: CompletionKind::EnumValue,
                insert_text: k.key.to_string(),
                replace_length,
            })
            .collect(),
        "dependencies" | "run_task" | "alias" | "linux_alias" | "windows_alias"
        | "mac_alias" => ast
            .tasks
            .iter()
            .filter(|t| matches_prefix(&t.name, word))
            .map(|t| CompletionItem {
                label: t.name.clone(),
                detail: t.description.clone(),
                kind: CompletionKind::Task,
                insert_text: t.name.clone(),
                replace_length,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn existing_keys(ctx: &TableCtx, ast: &File) -> Vec<String> {
    match ctx.kind {
        CtxKind::Task => ctx
            .task_name
            .as_deref()
            .and_then(|n| ast.task(n))
            .map(|t| t.fields.iter().map(|f| f.key.clone()).collect())
            .unwrap_or_default(),
        CtxKind::Config => ast.config_keys.iter().map(|k| k.key.clone()).collect(),
        _ => Vec::new(),
    }
}

/// Find the nearest table header above `line_start` and classify it.
fn current_table(source: &str, line_start: usize) -> TableCtx {
    let before = &source[..line_start];
    for line in before.lines().rev() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            return classify_header(trimmed);
        }
    }
    TableCtx { kind: CtxKind::Root, task_name: None }
}

fn classify_header(header: &str) -> TableCtx {
    let inner = header
        .trim_start_matches('[')
        .trim_end()
        .trim_end_matches(']')
        .trim_start_matches('[')
        .trim_end_matches(']');
    let segments = split_dotted(inner);

    let first = segments.first().map(String::as_str);
    let last = segments.last().map(String::as_str);

    match first {
        Some("tasks") => {
            let task_name = segments.get(1).cloned();
            let kind = match last {
                Some("condition") if segments.len() >= 3 => CtxKind::TaskCondition,
                Some("env") if segments.len() >= 3 => CtxKind::EnvOrOther,
                _ => CtxKind::Task,
            };
            TableCtx { kind, task_name }
        }
        Some("config") => TableCtx { kind: CtxKind::Config, task_name: None },
        Some("env") => TableCtx { kind: CtxKind::EnvOrOther, task_name: None },
        _ => TableCtx { kind: CtxKind::EnvOrOther, task_name: None },
    }
}

/// Split a dotted table path into segments, honouring quoted keys.
fn split_dotted(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in inner.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '.' => {
                    out.push(cur.trim().to_string());
                    cur.clear();
                }
                _ => cur.push(c),
            },
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Trailing run of identifier-ish characters immediately before the cursor.
fn trailing_word(prefix: &str) -> &str {
    let bytes = prefix.as_bytes();
    let mut start = bytes.len();
    while start > 0 {
        let c = bytes[start - 1];
        let ok = c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'@' | b'.' | b'/');
        if !ok {
            break;
        }
        start -= 1;
    }
    &prefix[start..]
}

fn matches_prefix(candidate: &str, word: &str) -> bool {
    if word.is_empty() {
        return true;
    }
    candidate.to_ascii_lowercase().starts_with(&word.to_ascii_lowercase())
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    fn complete(src: &str, line: u32, col: u32) -> Vec<String> {
        let pf = parse(FileUri::new("t"), src.to_string());
        completions(&pf, LineCol { line, col })
            .into_iter()
            .map(|c| c.label)
            .collect()
    }

    #[test]
    fn completes_task_fields() {
        // After `[tasks.build]` on a fresh line typing `desc`.
        let src = "[tasks.build]\ndesc";
        let labels = complete(src, 1, 4);
        assert!(labels.contains(&"description".to_string()));
    }

    #[test]
    fn excludes_existing_task_fields() {
        let src = "[tasks.build]\ncommand = \"cargo\"\nc";
        let labels = complete(src, 2, 1);
        assert!(!labels.contains(&"command".to_string()));
        assert!(labels.contains(&"category".to_string()));
    }

    #[test]
    fn completes_config_keys() {
        let src = "[config]\nskip";
        let labels = complete(src, 1, 4);
        assert!(labels.iter().any(|l| l.starts_with("skip_")));
    }

    #[test]
    fn completes_script_runner_values() {
        let src = "[tasks.x]\nscript_runner = \"@d";
        let labels = complete(src, 1, 19);
        assert!(labels.contains(&"@duckscript".to_string()));
    }

    #[test]
    fn completes_dependency_task_names() {
        let src = "[tasks.a]\ncommand = \"x\"\n[tasks.b]\ndependencies = [\"a";
        let labels = complete(src, 3, 18);
        assert!(labels.contains(&"a".to_string()));
    }

    #[test]
    fn completes_condition_keys() {
        let src = "[tasks.x.condition]\nplat";
        let labels = complete(src, 1, 4);
        assert!(labels.contains(&"platforms".to_string()));
    }

    #[test]
    fn no_completion_on_header_line() {
        let src = "[tasks.build]";
        let labels = complete(src, 0, 5);
        assert!(labels.is_empty());
    }

    #[test]
    fn top_level_section_completion() {
        let src = "ext";
        let labels = complete(src, 0, 3);
        assert!(labels.contains(&"extend".to_string()));
    }
}
