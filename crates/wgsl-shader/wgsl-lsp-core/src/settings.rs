//! The `wgsl.*` and `glsl.*` settings, as the client sends them.
//!
//! Both languages carry the same shape, which is why they share one struct.
//! Server *capabilities* are not per-language, so where a setting decides
//! whether a request is advertised at all, the capability takes the union and
//! the handler checks the document's own language — see
//! [`Settings::either`].
//!
//! Everything is `#[serde(default)]`, so a client that sends a partial object,
//! or none, gets the documented defaults rather than a deserialisation error.

use serde::{Deserialize, Serialize};
use wgsl_syntax::Language;


/// Everything the server reads out of the client's configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub wgsl: LanguageSettings,
    pub glsl: LanguageSettings,
}

impl Settings {
    /// The settings for one language.
    pub fn for_language(&self, language: Language) -> &LanguageSettings {
        match language {
            Language::Wgsl => &self.wgsl,
            Language::Glsl => &self.glsl,
        }
    }

    /// Whether a predicate holds for either language.
    ///
    /// A capability is advertised when *any* language wants it; the handler
    /// then declines for a document whose language does not.
    pub fn either(&self, predicate: impl Fn(&LanguageSettings) -> bool) -> bool {
        predicate(&self.wgsl) || predicate(&self.glsl)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LanguageSettings {
    pub validate: Validate,
    /// The `#version` to analyse a file that declares none as, written the way
    /// the directive is: `450`, `330 core`, `300 es`. Empty follows the spec,
    /// which says GLSL 1.10.
    ///
    /// GLSL only, and carried on the shared struct like everything else —
    /// WGSL has one version and simply never reads it. This is what replaced
    /// `glsl.validate.dialect` when naga stopped answering for GLSL
    /// (decision 0008): the question is no longer "should this be checked",
    /// it is "which GLSL is this".
    pub default_version: String,
    pub completion: Toggle,
    pub semantic_tokens: bool,
    pub inlay_hints: InlayHints,
    pub format: Format,
    pub embedded: Embedded,
}

impl Default for LanguageSettings {
    fn default() -> Self {
        Self {
            validate: Validate::default(),
            default_version: String::new(),
            completion: Toggle { enabled: true },
            semantic_tokens: true,
            inlay_hints: InlayHints::default(),
            format: Format::default(),
            embedded: Embedded::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Validate {
    pub on_save: bool,
    pub on_type: bool,
}

impl Default for Validate {
    fn default() -> Self {
        Self { on_save: true, on_type: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Toggle {
    pub enabled: bool,
}

impl Default for Toggle {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Inlay hints default to off, matching every other extension in this repo:
/// they are the kind of thing a reader wants sometimes and never by surprise.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InlayHints {
    pub enabled: bool,
    /// Inferred types on bindings that declare none — WGSL's `let x = …`.
    pub types: bool,
    /// Parameter names at call sites: `mix(e1: a, e2: b, e3: t)`.
    pub parameter_names: bool,
}

/// The formatter is a re-indenter, not a pretty-printer. It is off by default
/// because a shader written to a house style should not be silently restyled
/// by "format on save".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Format {
    pub enable: bool,
    pub indent_width: u32,
}

impl Default for Format {
    fn default() -> Self {
        Self { enable: false, indent_width: 4 }
    }
}

/// Shaders embedded in Rust and TS/JS string literals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Embedded {
    pub enabled: bool,
    /// A fragment inside a string is usually not a standalone module, so it
    /// gets the language features but no squiggles unless asked for.
    pub diagnostics: bool,
}

impl Default for Embedded {
    fn default() -> Self {
        Self { enabled: true, diagnostics: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_object_yields_the_documented_defaults() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.wgsl.completion.enabled);
        assert!(settings.wgsl.validate.on_save);
        assert!(!settings.wgsl.validate.on_type);
        assert!(settings.wgsl.semantic_tokens);
        assert!(!settings.wgsl.inlay_hints.enabled);
        assert!(!settings.glsl.format.enable);
        assert!(settings.glsl.embedded.enabled);
        assert!(!settings.glsl.embedded.diagnostics);
    }

    /// A client that sends one key must not lose the defaults for the rest.
    #[test]
    fn a_partial_object_keeps_the_other_defaults() {
        let settings: Settings =
            serde_json::from_value(serde_json::json!({ "wgsl": { "validate": { "onType": true } } }))
                .unwrap();
        assert!(settings.wgsl.validate.on_type);
        assert!(settings.wgsl.validate.on_save);
        assert!(settings.wgsl.completion.enabled);
        assert_eq!(settings.glsl, LanguageSettings::default());
    }

    #[test]
    fn a_capability_is_advertised_when_either_language_wants_it() {
        let mut settings = Settings::default();
        settings.wgsl.semantic_tokens = false;
        assert!(settings.either(|l| l.semantic_tokens));
        settings.glsl.semantic_tokens = false;
        assert!(!settings.either(|l| l.semantic_tokens));
    }

    /// `glsl.validate.dialect` was removed with naga's GLSL front end
    /// (decision 0008). A client that still sends it must not be rejected —
    /// every settings struct is `deny_unknown_fields`-free for exactly this.
    #[test]
    fn a_retired_setting_is_ignored_rather_than_fatal() {
        let settings: Settings = serde_json::from_value(
            serde_json::json!({ "glsl": { "validate": { "dialect": "opengl" } } }),
        )
        .unwrap();
        assert!(settings.glsl.validate.on_save);
        assert!(!settings.glsl.validate.on_type);
    }

    #[test]
    fn settings_are_selected_per_language() {
        let mut settings = Settings::default();
        settings.glsl.completion.enabled = false;
        assert!(settings.for_language(Language::Wgsl).completion.enabled);
        assert!(!settings.for_language(Language::Glsl).completion.enabled);
    }
}
