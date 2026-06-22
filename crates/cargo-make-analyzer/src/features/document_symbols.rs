//! Hierarchical document-symbol tree for the outline view: tasks at the
//! top level, plus `env` and `config` groups.

use crate::ast::{File, Task};
use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    pub detail: String,
    pub kind: SymbolKind,
    pub range: ByteSpan,
    pub selection_range: ByteSpan,
    pub children: Vec<DocumentSymbol>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Task,
    EnvGroup,
    EnvVar,
    ConfigGroup,
    ConfigKey,
}

pub fn document_symbols(file: &File) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();

    for task in &file.tasks {
        out.push(task_symbol(task));
    }

    if !file.env_vars.is_empty() {
        let children: Vec<DocumentSymbol> = file
            .env_vars
            .iter()
            .map(|v| DocumentSymbol {
                name: v.key.clone(),
                detail: v.value_preview.clone(),
                kind: SymbolKind::EnvVar,
                range: v.key_span.join(v.value_span),
                selection_range: v.key_span,
                children: Vec::new(),
            })
            .collect();
        out.push(group_symbol("env", SymbolKind::EnvGroup, children));
    }

    if !file.config_keys.is_empty() {
        let children: Vec<DocumentSymbol> = file
            .config_keys
            .iter()
            .map(|v| DocumentSymbol {
                name: v.key.clone(),
                detail: v.value_preview.clone(),
                kind: SymbolKind::ConfigKey,
                range: v.key_span.join(v.value_span),
                selection_range: v.key_span,
                children: Vec::new(),
            })
            .collect();
        out.push(group_symbol("config", SymbolKind::ConfigGroup, children));
    }

    out
}

fn task_symbol(task: &Task) -> DocumentSymbol {
    let detail = match &task.description {
        Some(d) if !d.is_empty() => d.clone(),
        _ => action_summary(task),
    };
    DocumentSymbol {
        name: task.name.clone(),
        detail,
        kind: SymbolKind::Task,
        range: task.span,
        selection_range: task.name_span,
        children: Vec::new(),
    }
}

fn action_summary(task: &Task) -> String {
    if task.has_command {
        "command".to_string()
    } else if task.has_script {
        "script".to_string()
    } else if task.has_run_task {
        "run_task".to_string()
    } else if !task.dependencies.is_empty() {
        format!("{} dependencies", task.dependencies.len())
    } else {
        String::new()
    }
}

fn group_symbol(name: &str, kind: SymbolKind, children: Vec<DocumentSymbol>) -> DocumentSymbol {
    let range = children
        .iter()
        .map(|c| c.range)
        .reduce(|a, b| a.join(b))
        .unwrap_or(ByteSpan::EMPTY);
    DocumentSymbol {
        name: name.to_string(),
        detail: format!("{} entries", children.len()),
        kind,
        range,
        selection_range: range,
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    fn syms(src: &str) -> Vec<DocumentSymbol> {
        let pf = parse(FileUri::new("t"), src.to_string());
        document_symbols(&pf.ast)
    }

    #[test]
    fn lists_tasks_and_groups() {
        let src = "[config]\nskip_core_tasks = true\n[env]\nFOO = \"bar\"\n[tasks.build]\ndescription = \"Build\"\ncommand = \"cargo\"\n";
        let s = syms(src);
        assert!(s.iter().any(|x| x.kind == SymbolKind::Task && x.name == "build"));
        assert!(s.iter().any(|x| x.kind == SymbolKind::EnvGroup));
        assert!(s.iter().any(|x| x.kind == SymbolKind::ConfigGroup));
    }

    #[test]
    fn task_detail_prefers_description() {
        let s = syms("[tasks.t]\ndescription = \"Hi\"\ncommand = \"x\"\n");
        let task = s.iter().find(|x| x.name == "t").unwrap();
        assert_eq!(task.detail, "Hi");
    }
}
