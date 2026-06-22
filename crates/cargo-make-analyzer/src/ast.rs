//! The typed model the analyzer works with, projected out of the lenient
//! `toml_edit` document during [`crate::parse`].
//!
//! It is intentionally lossy: only the pieces the feature providers need
//! (tasks, env vars, config keys, and their byte spans) are retained.

use crate::spans::ByteSpan;

#[derive(Debug, Clone, Default)]
pub struct File {
    /// Every `[tasks.NAME]` definition, in document order.
    pub tasks: Vec<Task>,
    /// Variables declared under `[env]` (top-level, non-profile).
    pub env_vars: Vec<KeyVal>,
    /// Keys declared under `[config]`.
    pub config_keys: Vec<KeyVal>,
    /// Whether the document parsed without a fatal TOML error.
    pub parse_ok: bool,
}

impl File {
    pub fn task(&self, name: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.name == name)
    }
}

/// A `[tasks.NAME]` definition.
#[derive(Debug, Clone)]
pub struct Task {
    pub name: String,
    /// The task name token in the `[tasks.NAME]` header.
    pub name_span: ByteSpan,
    /// The whole table, from header to the end of its body.
    pub span: ByteSpan,
    /// Optional platform suffix when this is a `[tasks.NAME.linux]` override.
    pub platform: Option<String>,
    /// All key/value fields directly inside the table.
    pub fields: Vec<TaskField>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub dependencies: Vec<Reference>,
    /// Task names referenced from `run_task` / `alias` / platform aliases.
    pub references: Vec<Reference>,
    pub has_command: bool,
    pub has_script: bool,
    pub has_run_task: bool,
}

/// One key inside a task table (used for hover, completion context, lints).
#[derive(Debug, Clone)]
pub struct TaskField {
    pub key: String,
    pub key_span: ByteSpan,
    pub value_span: ByteSpan,
    /// Keys of a `condition` sub-table, when this field is `condition`.
    pub condition_keys: Vec<KeyVal>,
}

/// A textual reference to another task (dependency / run_task / alias).
#[derive(Debug, Clone)]
pub struct Reference {
    pub name: String,
    pub span: ByteSpan,
}

/// A generic key/value pair with spans and a short value preview.
#[derive(Debug, Clone)]
pub struct KeyVal {
    pub key: String,
    pub key_span: ByteSpan,
    pub value_span: ByteSpan,
    pub value_preview: String,
}
