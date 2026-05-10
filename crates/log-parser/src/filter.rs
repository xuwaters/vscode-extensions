use regex::{Regex, RegexBuilder};
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Rule {
    pub pattern: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(rename = "caseSensitive", default)]
    pub case_sensitive: bool,
    /// Disabled rules are skipped during matching but kept in the input
    /// list so the per-line match index aligns with the rule index.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

pub enum Matcher {
    /// `enabled = false` — never matches; preserves rule index slot.
    Disabled,
    Substring {
        needle: String,
        case_sensitive: bool,
    },
    Regex(Regex),
}

impl Matcher {
    pub fn is_match(&self, text: &str) -> bool {
        match self {
            Matcher::Disabled => false,
            Matcher::Substring {
                needle,
                case_sensitive,
            } => {
                if *case_sensitive {
                    text.contains(needle.as_str())
                } else {
                    // Lowercase comparison without allocating per-call where possible.
                    // For correctness with non-ASCII, fall back to to_lowercase.
                    if needle.is_ascii() && text.is_ascii() {
                        text.as_bytes()
                            .windows(needle.len())
                            .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
                    } else {
                        text.to_lowercase().contains(&needle.to_lowercase())
                    }
                }
            }
            Matcher::Regex(re) => re.is_match(text),
        }
    }
}

pub fn build(rule: &Rule) -> Result<Matcher, String> {
    if !rule.enabled {
        return Ok(Matcher::Disabled);
    }
    if rule.regex {
        let re = RegexBuilder::new(&rule.pattern)
            .case_insensitive(!rule.case_sensitive)
            .build()
            .map_err(|e| format!("invalid regex /{}/: {}", rule.pattern, e))?;
        Ok(Matcher::Regex(re))
    } else {
        Ok(Matcher::Substring {
            needle: rule.pattern.clone(),
            case_sensitive: rule.case_sensitive,
        })
    }
}

pub fn compile(rules: &[Rule]) -> Result<Vec<Matcher>, String> {
    rules.iter().map(build).collect()
}

/// Build a single ad-hoc matcher (used by the search box).
pub fn build_search(query: &str, regex: bool, case_sensitive: bool) -> Result<Matcher, String> {
    let rule = Rule {
        pattern: query.to_string(),
        regex,
        case_sensitive,
        enabled: true,
    };
    build(&rule)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, regex: bool, case_sensitive: bool) -> Rule {
        Rule {
            pattern: pattern.to_string(),
            regex,
            case_sensitive,
            enabled: true,
        }
    }

    #[test]
    fn substring_case_insensitive_default() {
        let m = build(&rule("error", false, false)).unwrap();
        assert!(m.is_match("ERROR: thing"));
        assert!(m.is_match("error: thing"));
        assert!(!m.is_match("nothing here"));
    }

    #[test]
    fn substring_case_sensitive() {
        let m = build(&rule("Error", false, true)).unwrap();
        assert!(m.is_match("Error: thing"));
        assert!(!m.is_match("error: thing"));
    }

    #[test]
    fn regex_case_insensitive_default() {
        let m = build(&rule("ERROR|FATAL", true, false)).unwrap();
        assert!(m.is_match("Got fatal exception"));
        assert!(m.is_match("got error"));
        assert!(!m.is_match("plain"));
    }

    #[test]
    fn regex_invalid_returns_error() {
        let err = build(&rule("(unclosed", true, false))
            .err()
            .expect("expected an error");
        assert!(err.contains("invalid regex"));
    }

    #[test]
    fn disabled_never_matches() {
        let mut r = rule("anything", false, false);
        r.enabled = false;
        let m = build(&r).unwrap();
        assert!(!m.is_match("anything goes here"));
    }
}
