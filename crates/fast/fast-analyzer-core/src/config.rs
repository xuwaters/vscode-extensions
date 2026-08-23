//! Rule severities: the `[default, strict]` table from design/rules.md, the
//! per-rule overrides, and the resolution order — override, then strict
//! column, then default column.

use std::collections::HashMap;

use crate::protocol::{Config, Severity};

/// `(rule id, [normal default, strict default])`. `None` is off.
///
/// Rules answered on the TypeScript side (`no-invalid-css`, the discovery
/// rules) are listed too: the plugin asks the engine for resolved severities
/// so the table exists exactly once.
pub const RULE_DEFAULTS: &[(&str, [Option<Severity>; 2])] = &[
    ("no-unknown-tag-name", [None, Some(Severity::Warning)]),
    ("no-missing-import", [None, Some(Severity::Warning)]),
    ("no-unclosed-tag", [Some(Severity::Warning), Some(Severity::Error)]),
    ("no-unknown-attribute", [None, Some(Severity::Warning)]),
    ("no-unknown-property", [None, Some(Severity::Warning)]),
    // Off upstream because lit's event model made it noisy; FAST's `$emit`
    // gives a definite list, and events are the corpus's most common binding.
    ("no-unknown-event", [Some(Severity::Warning), Some(Severity::Warning)]),
    ("no-unknown-slot", [None, Some(Severity::Warning)]),
    ("no-unintended-mixed-binding", [Some(Severity::Warning), Some(Severity::Warning)]),
    ("no-expressionless-property-binding", [Some(Severity::Error), Some(Severity::Error)]),
    ("no-invalid-attribute-name", [Some(Severity::Error), Some(Severity::Error)]),
    ("no-invalid-tag-name", [Some(Severity::Error), Some(Severity::Error)]),
    ("no-missing-element-type-definition", [None, None]),
    ("no-invalid-css", [Some(Severity::Warning), Some(Severity::Error)]),
    ("no-noncallable-event-binding", [Some(Severity::Error), Some(Severity::Error)]),
    ("no-boolean-in-attribute-binding", [Some(Severity::Error), Some(Severity::Error)]),
    ("no-complex-attribute-binding", [Some(Severity::Error), Some(Severity::Error)]),
    ("no-incompatible-type-binding", [Some(Severity::Error), Some(Severity::Error)]),
    ("no-invalid-directive-binding", [Some(Severity::Error), Some(Severity::Error)]),
    ("no-incompatible-attr-config", [Some(Severity::Warning), Some(Severity::Error)]),
    ("no-attr-visibility-mismatch", [None, Some(Severity::Warning)]),
    ("no-non-reactive-binding", [Some(Severity::Warning), Some(Severity::Error)]),
    ("no-invalid-directive-target", [Some(Severity::Error), Some(Severity::Error)]),
    ("no-slot-without-shadow-root", [Some(Severity::Warning), Some(Severity::Warning)]),
    ("no-duplicate-tag-name", [Some(Severity::Error), Some(Severity::Error)]),
    ("no-untyped-template", [None, Some(Severity::Warning)]),
    ("no-implicit-prevent-default", [None, None]),
];

pub fn all_rule_ids() -> impl Iterator<Item = &'static str> {
    RULE_DEFAULTS.iter().map(|(id, _)| *id)
}

pub fn resolve_severity(config: &Config, rule_id: &str) -> Option<Severity> {
    match config.rules.get(rule_id).map(String::as_str) {
        Some("off") => return None,
        Some("warning") => return Some(Severity::Warning),
        Some("error") => return Some(Severity::Error),
        _ => {}
    }
    let defaults = RULE_DEFAULTS
        .iter()
        .find(|(id, _)| *id == rule_id)
        .map(|(_, d)| d)?;
    defaults[if config.strict { 1 } else { 0 }]
}

pub fn resolved_severities(config: &Config) -> HashMap<String, String> {
    all_rule_ids()
        .map(|id| {
            let value = match resolve_severity(config, id) {
                None => "off",
                Some(Severity::Warning) => "warning",
                Some(Severity::Error) => "error",
                Some(Severity::Suggestion) => "suggestion",
            };
            (id.to_string(), value.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_strict() {
        let normal = Config::default();
        let strict = Config {
            strict: true,
            ..Config::default()
        };
        assert_eq!(resolve_severity(&normal, "no-unknown-tag-name"), None);
        assert_eq!(
            resolve_severity(&strict, "no-unknown-tag-name"),
            Some(Severity::Warning)
        );
        assert_eq!(
            resolve_severity(&normal, "no-unclosed-tag"),
            Some(Severity::Warning)
        );
        assert_eq!(
            resolve_severity(&strict, "no-unclosed-tag"),
            Some(Severity::Error)
        );
        // The changed default: events are checked out of the box.
        assert_eq!(
            resolve_severity(&normal, "no-unknown-event"),
            Some(Severity::Warning)
        );
    }

    #[test]
    fn overrides_win_over_strict() {
        let mut config = Config {
            strict: true,
            ..Config::default()
        };
        config
            .rules
            .insert("no-unclosed-tag".into(), "off".into());
        config
            .rules
            .insert("no-unknown-tag-name".into(), "error".into());
        assert_eq!(resolve_severity(&config, "no-unclosed-tag"), None);
        assert_eq!(
            resolve_severity(&config, "no-unknown-tag-name"),
            Some(Severity::Error)
        );
    }

    #[test]
    fn every_rule_has_exactly_one_entry() {
        let mut seen = std::collections::HashSet::new();
        for id in all_rule_ids() {
            assert!(seen.insert(id), "duplicate rule id {id}");
        }
        assert_eq!(seen.len(), 26);
    }
}
