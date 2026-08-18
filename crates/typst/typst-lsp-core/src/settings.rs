//! The `typstUltra.*` settings, as the server sees them.
//!
//! The host owns the ones that need a runtime — timers, the network, platform
//! font directories — and passes the rest through `initializationOptions` and
//! `workspace/didChangeConfiguration`. Everything here has a default, so a
//! partial settings object from an older client still deserializes.

use serde::{Deserialize, Serialize};

/// When to recompile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompileWhen {
    /// After every edit, debounced.
    #[default]
    OnType,
    /// Only on save.
    OnSave,
    /// Never automatically.
    Never,
}

/// Whether to serve semantic tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SemanticTokensMode {
    /// Colour from the real parser. The default, and the primary colouring
    /// mechanism — the TextMate grammar is deliberately minimal.
    #[default]
    Enable,
    /// Fall back to the grammar's approximate colouring.
    Disable,
}

/// Which formatter to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormatterMode {
    /// `typstyle-core`, pinned to the compiler's version.
    #[default]
    Typstyle,
    /// Do not offer formatting.
    Off,
}

/// Compile scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompileSettings {
    /// The trigger.
    pub when: CompileWhen,
    /// Milliseconds of quiet before compiling. Enforced host-side, since the
    /// timer needs a runtime.
    pub debounce: u64,
}

impl Default for CompileSettings {
    fn default() -> Self {
        Self { when: CompileWhen::default(), debounce: 150 }
    }
}

/// Formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FormatterSettings {
    /// `typstyle` or `off`.
    pub mode: FormatterMode,
    /// Target line width.
    pub print_width: usize,
    /// Spaces per indent level.
    pub indent_size: usize,
}

impl Default for FormatterSettings {
    fn default() -> Self {
        Self { mode: FormatterMode::default(), print_width: 80, indent_size: 2 }
    }
}

/// Memory knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MemorySettings {
    /// comemo cache age. Lower is both faster and smaller here — measured, and
    /// counter-intuitive enough that decision 0005 exists to explain it. This
    /// deviates from typst-cli's `10` on purpose.
    pub evict_age: usize,
    /// Offer a server restart above this heap size; `0` disables.
    pub restart_threshold_mb: u64,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self { evict_age: 1, restart_threshold_mb: 1024 }
    }
}

/// A toggle with a default of on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Enabled {
    /// Whether the feature is on.
    pub enabled: bool,
}

impl Default for Enabled {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// A toggle with a default of off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Disabled {
    /// Whether the feature is on.
    pub enabled: bool,
}

/// Everything the server reads from `typstUltra.*`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Compile trigger and debounce.
    pub compile: CompileSettings,
    /// Whether to publish diagnostics at all.
    pub diagnostics: Enabled,
    /// Semantic token mode.
    pub semantic_tokens: SemanticTokensMode,
    /// Formatter configuration.
    pub formatter: FormatterSettings,
    /// Parameter-name inlay hints. Off by default: in markup-heavy files they
    /// are noise.
    pub inlay_hints: Disabled,
    /// comemo and restart thresholds.
    pub memory: MemorySettings,
}

impl Settings {
    /// Whether an edit should schedule a compile.
    pub fn compiles_on_type(&self) -> bool {
        self.compile.when == CompileWhen::OnType
    }

    /// Whether a save should schedule a compile.
    pub fn compiles_on_save(&self) -> bool {
        matches!(self.compile.when, CompileWhen::OnType | CompileWhen::OnSave)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_settings_object_takes_every_default() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.memory.evict_age, 1, "0005: not typst-cli's 10");
        assert_eq!(settings.compile.debounce, 150);
        assert_eq!(settings.formatter.print_width, 80);
        assert!(settings.diagnostics.enabled);
        assert!(!settings.inlay_hints.enabled);
    }

    #[test]
    fn a_partial_settings_object_keeps_the_other_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"formatter":{"printWidth":100}}"#).unwrap();
        assert_eq!(settings.formatter.print_width, 100);
        assert_eq!(settings.formatter.indent_size, 2);
        assert_eq!(settings.formatter.mode, FormatterMode::Typstyle);
    }

    #[test]
    fn compile_triggers_follow_the_setting() {
        let mut settings = Settings::default();
        assert!(settings.compiles_on_type() && settings.compiles_on_save());

        settings.compile.when = CompileWhen::OnSave;
        assert!(!settings.compiles_on_type() && settings.compiles_on_save());

        settings.compile.when = CompileWhen::Never;
        assert!(!settings.compiles_on_type() && !settings.compiles_on_save());
    }
}
