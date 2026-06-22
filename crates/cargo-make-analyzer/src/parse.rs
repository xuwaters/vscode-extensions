//! The parse pipeline — turn a `Makefile.toml` source into a typed
//! [`ParsedFile`].
//!
//! Parsing leans on [`toml_edit`]'s lenient, span-preserving document
//! model. We never mutate the document, so the byte spans it records stay
//! valid and are projected straight onto our [`ast`]. Structural lints are
//! collected alongside.

use std::ops::Range;

use toml_edit::{ImDocument, Item, Value};

use crate::ast::{File, KeyVal, Reference, Task, TaskField};
use crate::diagnostics::{CargoMakeDiagnostic, DiagnosticCode};
use crate::schema;
use crate::spans::{ByteSpan, SpanTable};
use crate::vfs::FileUri;

pub use crate::vfs::ParsedFile;

pub fn parse(uri: FileUri, source: String) -> ParsedFile {
    let spans = SpanTable::new(&source);
    let mut diagnostics: Vec<CargoMakeDiagnostic> = Vec::new();

    let ast = match ImDocument::parse(source.clone()) {
        Ok(doc) => build_file(&doc, &source, &mut diagnostics),
        Err(err) => {
            let span = range_span(err.span());
            diagnostics.push(CargoMakeDiagnostic::error(
                DiagnosticCode::ParseError,
                err.message().to_string(),
                span,
            ));
            File::default()
        }
    };

    ParsedFile { uri, source, ast, spans, diagnostics }
}

fn build_file(
    doc: &ImDocument<String>,
    source: &str,
    diags: &mut Vec<CargoMakeDiagnostic>,
) -> File {
    let mut file = File { parse_ok: true, ..File::default() };
    let root = doc.as_table();

    // [tasks.*]
    if let Some((_, tasks_item)) = root.get_key_value("tasks") {
        if let Some(tasks) = tasks_item.as_table_like() {
            let names: Vec<String> = tasks.iter().map(|(k, _)| k.to_string()).collect();
            for name in names {
                if let Some((key, item)) = tasks.get_key_value(&name) {
                    let task = build_task(&name, range_span(key.span()), item, source, diags);
                    file.tasks.push(task);
                }
            }
        }
    }

    // [env]
    if let Some((_, env_item)) = root.get_key_value("env") {
        if let Some(env) = env_item.as_table_like() {
            let keys: Vec<String> = env.iter().map(|(k, _)| k.to_string()).collect();
            for key in keys {
                if let Some((k, item)) = env.get_key_value(&key) {
                    // Skip `[env.profile]` sub-tables — only direct vars.
                    if item.is_table_like() {
                        continue;
                    }
                    file.env_vars.push(key_val(k.to_string(), range_span(k.span()), item, source));
                }
            }
        }
    }

    // [config]
    if let Some((_, config_item)) = root.get_key_value("config") {
        if let Some(config) = config_item.as_table_like() {
            let keys: Vec<String> = config.iter().map(|(k, _)| k.to_string()).collect();
            for key in keys {
                if let Some((k, item)) = config.get_key_value(&key) {
                    let key_span = range_span(k.span());
                    if !schema::is_config_key(&key) {
                        diags.push(CargoMakeDiagnostic::warning(
                            DiagnosticCode::UnknownConfigKey,
                            format!("`{key}` is not a recognised [config] key"),
                            key_span,
                        ));
                    }
                    file.config_keys.push(key_val(key, key_span, item, source));
                }
            }
        }
    }

    file
}

fn build_task(
    name: &str,
    name_span: ByteSpan,
    item: &Item,
    source: &str,
    diags: &mut Vec<CargoMakeDiagnostic>,
) -> Task {
    let mut task = Task {
        name: name.to_string(),
        name_span,
        span: item_span(item),
        platform: None,
        fields: Vec::new(),
        description: None,
        category: None,
        dependencies: Vec::new(),
        references: Vec::new(),
        has_command: false,
        has_script: false,
        has_run_task: false,
    };

    if let Some(tbl) = item.as_table_like() {
        let keys: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).collect();
        for key in keys {
            let Some((k, fitem)) = tbl.get_key_value(&key) else { continue };
            let key_span = range_span(k.span());
            let value_span = item_span(fitem);

            if !schema::is_task_field(&key) {
                diags.push(CargoMakeDiagnostic::warning(
                    DiagnosticCode::UnknownTaskField,
                    format!("`{key}` is not a recognised task field"),
                    key_span,
                ));
            }

            let mut condition_keys = Vec::new();
            match key.as_str() {
                "command" => task.has_command = true,
                "script" => task.has_script = true,
                "run_task" => {
                    task.has_run_task = true;
                    collect_run_task_refs(fitem, &mut task.references);
                }
                "dependencies" => collect_dependencies(fitem, &mut task.dependencies),
                "description" => task.description = fitem.as_str().map(String::from),
                "category" => task.category = fitem.as_str().map(String::from),
                "alias" | "linux_alias" | "windows_alias" | "mac_alias" => {
                    if let Some(s) = fitem.as_str() {
                        task.references.push(Reference { name: s.to_string(), span: value_span });
                    }
                }
                "condition" => {
                    condition_keys = collect_condition(fitem, source, diags);
                }
                "linux" | "windows" | "mac" => lint_platform_override(fitem, diags),
                _ => {}
            }

            task.fields.push(TaskField { key, key_span, value_span, condition_keys });
        }
    }

    // Exactly one of command / script / run_task is allowed.
    let actions = [task.has_command, task.has_script, task.has_run_task]
        .iter()
        .filter(|b| **b)
        .count();
    if actions > 1 {
        diags.push(CargoMakeDiagnostic::warning(
            DiagnosticCode::ConflictingAction,
            format!(
                "task `{}` declares multiple actions; only one of `command`, `script`, or `run_task` is used",
                task.name
            ),
            task.name_span,
        ));
    }

    // A task may not depend on itself.
    for dep in &task.dependencies {
        if dep.name == task.name {
            diags.push(CargoMakeDiagnostic::warning(
                DiagnosticCode::SelfDependency,
                format!("task `{}` depends on itself", task.name),
                dep.span,
            ));
        }
    }

    task
}

/// Collect the keys of a task `condition` table and lint unknown criteria.
fn collect_condition(
    item: &Item,
    _source: &str,
    diags: &mut Vec<CargoMakeDiagnostic>,
) -> Vec<KeyVal> {
    let mut out = Vec::new();
    if let Some(tbl) = item.as_table_like() {
        let keys: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).collect();
        for key in keys {
            let Some((k, citem)) = tbl.get_key_value(&key) else { continue };
            let key_span = range_span(k.span());
            if !schema::is_condition_key(&key) {
                diags.push(CargoMakeDiagnostic::warning(
                    DiagnosticCode::UnknownConditionKey,
                    format!("`{key}` is not a recognised condition criterion"),
                    key_span,
                ));
            }
            out.push(KeyVal {
                key,
                key_span,
                value_span: item_span(citem),
                value_preview: String::new(),
            });
        }
    }
    out
}

/// Lint the inner keys of a `[tasks.X.linux]`-style platform override.
fn lint_platform_override(item: &Item, diags: &mut Vec<CargoMakeDiagnostic>) {
    if let Some(tbl) = item.as_table_like() {
        let keys: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).collect();
        for key in keys {
            let Some((k, _)) = tbl.get_key_value(&key) else { continue };
            if !schema::is_task_field(&key) {
                diags.push(CargoMakeDiagnostic::warning(
                    DiagnosticCode::UnknownTaskField,
                    format!("`{key}` is not a recognised task field"),
                    range_span(k.span()),
                ));
            }
        }
    }
}

/// Pull task names out of a `dependencies` array: each element is either a
/// string or an inline `{ name = "...", path = "..." }` table.
fn collect_dependencies(item: &Item, out: &mut Vec<Reference>) {
    let Some(array) = item.as_array() else { return };
    for value in array.iter() {
        match value {
            Value::String(s) => {
                out.push(Reference { name: s.value().to_string(), span: value_span(value) });
            }
            Value::InlineTable(t) => {
                if let Some(name) = t.get("name").and_then(Value::as_str) {
                    out.push(Reference { name: name.to_string(), span: value_span(value) });
                }
            }
            _ => {}
        }
    }
}

/// Pull task names referenced by a `run_task` value, which may be a string,
/// an array of names, or a `{ name = ... }` table whose `name` is itself a
/// string or an array.
fn collect_run_task_refs(item: &Item, out: &mut Vec<Reference>) {
    if let Some(s) = item.as_str() {
        out.push(Reference { name: s.to_string(), span: item_span(item) });
        return;
    }
    if let Some(value) = item.as_value() {
        collect_run_task_value(value, out);
    }
}

fn collect_run_task_value(value: &Value, out: &mut Vec<Reference>) {
    match value {
        Value::String(s) => {
            out.push(Reference { name: s.value().to_string(), span: value_span(value) });
        }
        Value::Array(arr) => {
            for v in arr.iter() {
                collect_run_task_value(v, out);
            }
        }
        Value::InlineTable(t) => {
            if let Some((_, name_item)) = t.get_key_value("name") {
                if let Some(name) = name_item.as_str() {
                    out.push(Reference {
                        name: name.to_string(),
                        span: range_span(name_item.span()),
                    });
                } else if let Some(arr) = name_item.as_array() {
                    for v in arr.iter() {
                        collect_run_task_value(v, out);
                    }
                }
            }
        }
        _ => {}
    }
}

fn key_val(key: String, key_span: ByteSpan, item: &Item, source: &str) -> KeyVal {
    let value_span = item_span(item);
    KeyVal { key, key_span, value_span, value_preview: preview(value_span, source) }
}

fn preview(span: ByteSpan, source: &str) -> String {
    let slice = source.get(span.start as usize..span.end as usize).unwrap_or("");
    let collapsed: String = slice.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 60;
    if collapsed.chars().count() <= MAX {
        collapsed
    } else {
        let mut out: String = collapsed.chars().take(MAX).collect();
        out.push('…');
        out
    }
}

fn item_span(item: &Item) -> ByteSpan {
    range_span(item.span())
}

fn value_span(value: &Value) -> ByteSpan {
    range_span(value.span())
}

fn range_span(range: Option<Range<usize>>) -> ByteSpan {
    match range {
        Some(r) => ByteSpan::from_usize(r.start, r.end),
        None => ByteSpan::EMPTY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_src(src: &str) -> ParsedFile {
        parse(FileUri::new("file:///Makefile.toml"), src.to_string())
    }

    #[test]
    fn collects_tasks_with_description() {
        let src = "[tasks.build]\ndescription = \"Build it\"\ncommand = \"cargo\"\nargs = [\"build\"]\n";
        let pf = parse_src(src);
        assert_eq!(pf.ast.tasks.len(), 1);
        let task = &pf.ast.tasks[0];
        assert_eq!(task.name, "build");
        assert_eq!(task.description.as_deref(), Some("Build it"));
        assert!(task.has_command);
        assert!(pf.diagnostics.is_empty(), "{:?}", pf.diagnostics);
    }

    #[test]
    fn task_name_span_points_at_name() {
        let src = "[tasks.build]\ncommand = \"cargo\"\n";
        let pf = parse_src(src);
        let task = &pf.ast.tasks[0];
        let name = &src[task.name_span.start as usize..task.name_span.end as usize];
        assert_eq!(name, "build");
    }

    #[test]
    fn flags_conflicting_actions() {
        let src = "[tasks.x]\ncommand = \"cargo\"\nscript = \"echo hi\"\n";
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ConflictingAction));
    }

    #[test]
    fn flags_unknown_task_field() {
        let src = "[tasks.x]\ncommnd = \"cargo\"\n";
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownTaskField));
    }

    #[test]
    fn flags_unknown_config_key() {
        let src = "[config]\nskip_core_task = true\n";
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownConfigKey));
    }

    #[test]
    fn flags_self_dependency() {
        let src = "[tasks.x]\ndependencies = [\"x\"]\n";
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::SelfDependency));
    }

    #[test]
    fn collects_dependencies_as_references() {
        let src = "[tasks.x]\ndependencies = [\"a\", { name = \"b\", path = \"sub\" }]\n";
        let pf = parse_src(src);
        let names: Vec<&str> =
            pf.ast.tasks[0].dependencies.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn parse_error_is_reported() {
        let src = "[tasks.x]\ncommand = \n";
        let pf = parse_src(src);
        assert!(!pf.ast.parse_ok);
        assert!(pf.diagnostics.iter().any(|d| d.code == DiagnosticCode::ParseError));
    }

    #[test]
    fn unknown_condition_key_flagged() {
        let src = "[tasks.x]\ncommand = \"cargo\"\n[tasks.x.condition]\nplatforms = [\"linux\"]\nbogus = 1\n";
        let pf = parse_src(src);
        assert!(pf
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownConditionKey));
    }
}
