//! Static knowledge of the cargo-make `Makefile.toml` schema.
//!
//! The lists below mirror the `Task`, `ConfigSection`, and `TaskCondition`
//! structs in cargo-make's `src/lib/types.rs`. They drive three things:
//!
//! - completion (offer the known keys / values),
//! - hover (one-line documentation per key),
//! - lints (flag keys that are not in the list as "unknown").

/// A schema key paired with a short, editor-friendly description.
#[derive(Debug, Clone, Copy)]
pub struct KeyDoc {
    pub key: &'static str,
    pub doc: &'static str,
}

/// Top-level sections of a `Makefile.toml`.
pub const TOP_LEVEL_KEYS: &[KeyDoc] = &[
    KeyDoc { key: "extend", doc: "Path(s) to other Makefile.toml files to extend from." },
    KeyDoc { key: "config", doc: "Global cargo-make configuration." },
    KeyDoc { key: "env_files", doc: "List of files to load environment variables from." },
    KeyDoc { key: "env", doc: "Global environment variables." },
    KeyDoc { key: "env_scripts", doc: "Scripts executed to set up environment variables." },
    KeyDoc { key: "tasks", doc: "Task definitions (`[tasks.NAME]`)." },
    KeyDoc { key: "plugins", doc: "Plugin aliases and implementations." },
];

/// Fields a `[tasks.NAME]` table accepts.
pub const TASK_FIELDS: &[KeyDoc] = &[
    KeyDoc { key: "clear", doc: "When extending, ignore all data from the base task." },
    KeyDoc { key: "description", doc: "Human-readable description shown in `cargo make --list-all-steps`." },
    KeyDoc { key: "category", doc: "Category used to group the task in the task list." },
    KeyDoc { key: "disabled", doc: "If true, the task is not invoked (dependencies still run)." },
    KeyDoc { key: "private", doc: "If true, hide the task from the list and forbid direct invocation." },
    KeyDoc { key: "deprecated", doc: "Mark the task as deprecated (bool or message string)." },
    KeyDoc { key: "extend", doc: "Name of another task to inherit fields from." },
    KeyDoc { key: "workspace", doc: "Set to false to disable per-member workspace execution." },
    KeyDoc { key: "plugin", doc: "Name of the plugin used to run this task." },
    KeyDoc { key: "watch", doc: "Enable file watching (bool or watch options table)." },
    KeyDoc { key: "condition", doc: "Conditions that must hold for the task to run." },
    KeyDoc { key: "condition_script", doc: "Script whose exit code decides whether the task runs (0 = run)." },
    KeyDoc { key: "condition_script_runner_args", doc: "Runner args inserted before the condition script path." },
    KeyDoc { key: "ignore_errors", doc: "If true, task errors do not fail the flow." },
    KeyDoc { key: "force", doc: "Deprecated — use `ignore_errors`." },
    KeyDoc { key: "env_files", doc: "Env files loaded before this task runs." },
    KeyDoc { key: "env", doc: "Environment variables scoped to this task." },
    KeyDoc { key: "cwd", doc: "Working directory for the task." },
    KeyDoc { key: "alias", doc: "Redirect to another task; all other fields are ignored." },
    KeyDoc { key: "linux_alias", doc: "Alias used only on Linux." },
    KeyDoc { key: "windows_alias", doc: "Alias used only on Windows." },
    KeyDoc { key: "mac_alias", doc: "Alias used only on macOS." },
    KeyDoc { key: "install_crate", doc: "Crate to install before running (name, table, or bool)." },
    KeyDoc { key: "install_crate_args", doc: "Extra arguments passed to `cargo install`." },
    KeyDoc { key: "install_script", doc: "Script run to install dependencies before the task." },
    KeyDoc { key: "command", doc: "Executable to run." },
    KeyDoc { key: "args", doc: "Arguments passed to `command`." },
    KeyDoc { key: "script", doc: "Inline script to execute (string or array of lines)." },
    KeyDoc { key: "script_runner", doc: "Interpreter for `script` (e.g. bash, python, @duckscript, @rust, @shell)." },
    KeyDoc { key: "script_runner_args", doc: "Runner args inserted before the script path." },
    KeyDoc { key: "script_extension", doc: "File extension used when writing the script to disk." },
    KeyDoc { key: "run_task", doc: "Another task (or tasks) to invoke from this task." },
    KeyDoc { key: "dependencies", doc: "Tasks that run before this task." },
    KeyDoc { key: "toolchain", doc: "Rust toolchain to run the task under (e.g. \"nightly\")." },
    KeyDoc { key: "linux", doc: "Task field overrides applied on Linux." },
    KeyDoc { key: "windows", doc: "Task field overrides applied on Windows." },
    KeyDoc { key: "mac", doc: "Task field overrides applied on macOS." },
];

/// Keys recognised inside `[config]`.
pub const CONFIG_KEYS: &[KeyDoc] = &[
    KeyDoc { key: "skip_core_tasks", doc: "If true, do not load the predefined core tasks." },
    KeyDoc { key: "modify_core_tasks", doc: "Namespace / privatise the predefined core tasks." },
    KeyDoc { key: "init_task", doc: "Task invoked at the start of every flow." },
    KeyDoc { key: "end_task", doc: "Task invoked at the end of every flow." },
    KeyDoc { key: "on_error_task", doc: "Task invoked if any task in the flow errors." },
    KeyDoc { key: "legacy_migration_task", doc: "Task used for legacy migration flows." },
    KeyDoc { key: "additional_profiles", doc: "Extra profile names to load." },
    KeyDoc { key: "min_version", doc: "Minimum required cargo-make version." },
    KeyDoc { key: "default_to_workspace", doc: "Default workspace-execution behaviour for tasks." },
    KeyDoc { key: "skip_git_env_info", doc: "Skip collecting git environment info (faster startup)." },
    KeyDoc { key: "skip_rust_env_info", doc: "Skip collecting rust environment info (faster startup)." },
    KeyDoc { key: "skip_crate_env_info", doc: "Skip collecting current-crate environment info." },
    KeyDoc { key: "reduce_output", doc: "Reduce console output outside of CI." },
    KeyDoc { key: "time_summary", doc: "Print a per-task time summary at the end of the flow." },
    KeyDoc { key: "load_cargo_aliases", doc: "Automatically expose cargo aliases as cargo-make tasks." },
    KeyDoc { key: "disable_install", doc: "Disable all automatic crate installation." },
    KeyDoc { key: "main_project_member", doc: "Workspace member treated as the main project." },
    KeyDoc { key: "load_script", doc: "Script executed while loading the descriptor." },
    KeyDoc { key: "linux_load_script", doc: "`load_script` override for Linux." },
    KeyDoc { key: "windows_load_script", doc: "`load_script` override for Windows." },
    KeyDoc { key: "mac_load_script", doc: "`load_script` override for macOS." },
    KeyDoc { key: "unstable_features", doc: "Enable unstable cargo-make features." },
];

/// Keys recognised inside a task `condition` table.
pub const CONDITION_KEYS: &[KeyDoc] = &[
    KeyDoc { key: "condition_type", doc: "How criteria combine: \"and\", \"or\", or \"group_or\"." },
    KeyDoc { key: "fail_message", doc: "Message printed when the condition is not met." },
    KeyDoc { key: "profiles", doc: "Profile names that must be active." },
    KeyDoc { key: "os", doc: "Target OS names (from `cfg`), e.g. \"linux\", \"macos\"." },
    KeyDoc { key: "platforms", doc: "Platform names: \"linux\", \"windows\", \"mac\"." },
    KeyDoc { key: "channels", doc: "Rust channels: \"stable\", \"beta\", \"nightly\"." },
    KeyDoc { key: "env_set", doc: "Environment variables that must be defined." },
    KeyDoc { key: "env_not_set", doc: "Environment variables that must not be defined." },
    KeyDoc { key: "env", doc: "Environment variables that must equal the given values." },
    KeyDoc { key: "env_not", doc: "Environment variables that must not equal the given values." },
    KeyDoc { key: "env_true", doc: "Environment variables that must be truthy." },
    KeyDoc { key: "env_false", doc: "Environment variables that must be falsy." },
    KeyDoc { key: "env_contains", doc: "Environment variables that must contain the given substrings." },
    KeyDoc { key: "rust_version", doc: "Rust version constraints (min / max / equal)." },
    KeyDoc { key: "files_exist", doc: "Paths that must exist." },
    KeyDoc { key: "files_not_exist", doc: "Paths that must not exist." },
    KeyDoc { key: "files_modified", doc: "Run only when input files are newer than output files." },
];

/// Built-in script runners recognised by cargo-make. Custom runners (any
/// other string, e.g. `python`, `node`, `perl`) are also valid.
pub const SCRIPT_RUNNERS: &[KeyDoc] = &[
    KeyDoc { key: "@duckscript", doc: "Run the script with the embedded DuckScript runner." },
    KeyDoc { key: "@rust", doc: "Compile and run the script as Rust source." },
    KeyDoc { key: "@shell", doc: "Cross-platform shell (sh on Unix, converted on Windows)." },
];

/// Task field names whose value is a script that should be highlighted /
/// treated as embedded shell.
pub const SCRIPT_FIELD_KEYS: &[&str] =
    &["script", "install_script", "condition_script", "pre", "main", "post"];

fn lookup<'a>(keys: &'a [KeyDoc], key: &str) -> Option<&'a KeyDoc> {
    keys.iter().find(|k| k.key == key)
}

pub fn is_task_field(key: &str) -> bool {
    lookup(TASK_FIELDS, key).is_some()
}

pub fn is_config_key(key: &str) -> bool {
    lookup(CONFIG_KEYS, key).is_some()
}

pub fn is_condition_key(key: &str) -> bool {
    lookup(CONDITION_KEYS, key).is_some()
}

pub fn task_field_doc(key: &str) -> Option<&'static str> {
    lookup(TASK_FIELDS, key).map(|k| k.doc)
}

pub fn config_key_doc(key: &str) -> Option<&'static str> {
    lookup(CONFIG_KEYS, key).map(|k| k.doc)
}

pub fn condition_key_doc(key: &str) -> Option<&'static str> {
    lookup(CONDITION_KEYS, key).map(|k| k.doc)
}
