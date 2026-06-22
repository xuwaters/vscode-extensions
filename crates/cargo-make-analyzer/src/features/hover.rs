//! Hover tooltips. Resolves the byte offset under the cursor to one of:
//! a task field key (→ schema doc), a condition criterion (→ schema doc),
//! a config key (→ schema doc), a task reference (→ target description),
//! or a task header name (→ task summary).

use crate::ast::{File, Task};
use crate::schema;
use crate::spans::ByteSpan;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Hover {
    pub markdown: String,
    pub span: ByteSpan,
}

pub fn hover(file: &File, offset: u32) -> Option<Hover> {
    for task in &file.tasks {
        // A reference to another task wins over the generic field doc so
        // that the cursor on a dependency name jumps to that task's blurb.
        for r in task.dependencies.iter().chain(task.references.iter()) {
            if r.span.contains(offset) {
                if let Some(target) = file.task(&r.name) {
                    return Some(Hover {
                        markdown: task_markdown(target),
                        span: r.span,
                    });
                }
            }
        }

        if task.name_span.contains(offset) {
            return Some(Hover { markdown: task_markdown(task), span: task.name_span });
        }

        for field in &task.fields {
            if field.key == "condition" {
                for ck in &field.condition_keys {
                    if ck.key_span.contains(offset) {
                        return Some(key_hover(&ck.key, schema::condition_key_doc(&ck.key), ck.key_span));
                    }
                }
            }
            if field.key_span.contains(offset) {
                return Some(key_hover(&field.key, schema::task_field_doc(&field.key), field.key_span));
            }
        }
    }

    for kv in &file.config_keys {
        if kv.key_span.contains(offset) {
            return Some(key_hover(&kv.key, schema::config_key_doc(&kv.key), kv.key_span));
        }
    }

    for kv in &file.env_vars {
        if kv.key_span.contains(offset) {
            return Some(Hover {
                markdown: format!("**{}** — environment variable", kv.key),
                span: kv.key_span,
            });
        }
    }

    None
}

fn key_hover(key: &str, doc: Option<&str>, span: ByteSpan) -> Hover {
    let markdown = match doc {
        Some(doc) => format!("**`{key}`**\n\n{doc}"),
        None => format!("**`{key}`**"),
    };
    Hover { markdown, span }
}

fn task_markdown(task: &Task) -> String {
    let mut md = format!("**task** `{}`", task.name);
    if let Some(desc) = &task.description {
        if !desc.is_empty() {
            md.push_str("\n\n");
            md.push_str(desc);
        }
    }
    if let Some(cat) = &task.category {
        md.push_str(&format!("\n\n*category:* {cat}"));
    }
    if !task.dependencies.is_empty() {
        let deps: Vec<&str> = task.dependencies.iter().map(|d| d.name.as_str()).collect();
        md.push_str(&format!("\n\n*dependencies:* {}", deps.join(", ")));
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    fn hover_at(src: &str, needle: &str) -> Option<Hover> {
        let pf = parse(FileUri::new("t"), src.to_string());
        let offset = src.find(needle).unwrap() as u32 + 1;
        hover(&pf.ast, offset)
    }

    #[test]
    fn hover_on_task_field_key() {
        let h = hover_at("[tasks.x]\ndescription = \"hi\"\n", "description").unwrap();
        assert!(h.markdown.contains("description"));
    }

    #[test]
    fn hover_on_dependency_shows_target() {
        let src = "[tasks.a]\ndescription = \"the A task\"\ncommand = \"x\"\n[tasks.b]\ndependencies = [\"a\"]\n";
        let pf = parse(FileUri::new("t"), src.to_string());
        // Offset inside the "a" inside dependencies (the last occurrence).
        let offset = src.rfind("\"a\"").unwrap() as u32 + 1;
        let h = hover(&pf.ast, offset).unwrap();
        assert!(h.markdown.contains("the A task"), "{}", h.markdown);
    }

    #[test]
    fn hover_on_config_key() {
        let h = hover_at("[config]\nskip_core_tasks = true\n", "skip_core_tasks").unwrap();
        assert!(h.markdown.contains("core tasks"));
    }
}
