//! Interpret fish `-n PREDICATE` strings into subcommand-path filters.
//!
//! See RFC 005 §5.2 for the supported pattern table. Anything we don't
//! recognise causes [`interpret`] to return [`PredicateOutcome::Drop`] —
//! the caller drops the directive entirely, since the predicate would
//! otherwise gate the entry on state we cannot evaluate.

/// Result of interpreting a fish predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PredicateOutcome {
    /// No `-n` was present (or the predicate matched a recognised
    /// "always true" pattern). Entry applies anywhere.
    Always,
    /// Entry only applies at the top level (no subcommand entered).
    /// Encoded as `EntryFlags::TOP_LEVEL_ONLY`.
    TopLevel,
    /// Entry applies under one of the listed subcommand paths.
    /// Each path is a `Vec<String>` (single element for `seen_subcommand_from x`,
    /// multi for `__fish_<cmd>_using_command remote add`).
    Paths(Vec<Vec<String>>),
    /// Predicate is unsupported; drop the directive.
    Drop,
}

/// Interpret one predicate string.
pub fn interpret(predicate: &str) -> PredicateOutcome {
    let p = predicate.trim();
    if p.is_empty() {
        return PredicateOutcome::Always;
    }

    // `__fish_use_subcommand` and `__fish_<cmd>_needs_command` both mean
    // "no subcommand has been entered yet".
    if p == "__fish_use_subcommand" {
        return PredicateOutcome::TopLevel;
    }
    if let Some(rest) = p.strip_prefix("__fish_") {
        if let Some(cmd) = rest.strip_suffix("_needs_command") {
            if !cmd.is_empty() {
                return PredicateOutcome::TopLevel;
            }
        }
    }

    // `__fish_seen_subcommand_from X [Y …]` — each arg is an alternative
    // single-segment subcommand.
    if let Some(args) = p.strip_prefix("__fish_seen_subcommand_from ") {
        return parse_alternative_subcommands(args);
    }
    if let Some(args) = p.strip_prefix("__fish_seen_subcommand_from\t") {
        return parse_alternative_subcommands(args);
    }

    // `not __fish_seen_subcommand_from …` — too imprecise (it means "any
    // command except these"), drop. We could accept and emit `Always`
    // since dropping risks losing genuinely-useful global flags, but
    // RFC §5.2 explicitly maps this to "Drop" because the caller can't
    // surface "anywhere except X" with our current path model.
    if p.starts_with("not __fish_seen_subcommand_from") {
        return PredicateOutcome::Drop;
    }

    // `__fish_<cmd>_using_command X [Y …]` — each arg may be a single-word
    // subcommand or a multi-word path. Per RFC §5.2 we treat the whole
    // arg list as alternatives, each split on whitespace. This produces
    // some false positives (e.g. `__fish_git_using_command remote add`
    // becomes alternatives `[remote]` and `[add]`) but never silently
    // drops a useful entry.
    if let Some(rest) = p.strip_prefix("__fish_") {
        if let Some(rest_after_cmd) = strip_until(rest, "_using_command") {
            let args = rest_after_cmd
                .strip_prefix(' ')
                .or_else(|| rest_after_cmd.strip_prefix('\t'))
                .unwrap_or("");
            return parse_alternative_subcommands(args);
        }
    }

    // Anything else — `string match`, `commandline -ct …`, `test`,
    // `__fish_seen_argument`, complex boolean expressions — we cannot
    // statically evaluate.
    PredicateOutcome::Drop
}

/// Strip leading text up to and including `marker`, returning the
/// remainder. Returns `None` if `marker` is absent.
fn strip_until<'a>(s: &'a str, marker: &str) -> Option<&'a str> {
    s.find(marker).map(|idx| &s[idx + marker.len()..])
}

/// Parse a whitespace-separated arg list into alternative subcommand
/// paths.
fn parse_alternative_subcommands(args: &str) -> PredicateOutcome {
    let alternatives: Vec<Vec<String>> = args
        .split_whitespace()
        .filter(|w| is_plain_word(w))
        .map(|w| vec![w.to_owned()])
        .collect();
    if alternatives.is_empty() {
        PredicateOutcome::Drop
    } else {
        PredicateOutcome::Paths(alternatives)
    }
}

/// A "plain" subcommand word — letters, digits, dash, underscore, dot.
/// Anything else (variables, parens, quotes) means we can't trust the
/// extracted name.
fn is_plain_word(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_always() {
        assert_eq!(interpret(""), PredicateOutcome::Always);
        assert_eq!(interpret("   "), PredicateOutcome::Always);
    }

    #[test]
    fn use_subcommand_is_top_level() {
        assert_eq!(interpret("__fish_use_subcommand"), PredicateOutcome::TopLevel);
        assert_eq!(interpret("__fish_git_needs_command"), PredicateOutcome::TopLevel);
        assert_eq!(interpret("__fish_docker_needs_command"), PredicateOutcome::TopLevel);
    }

    #[test]
    fn seen_subcommand_from_alternatives() {
        assert_eq!(
            interpret("__fish_seen_subcommand_from add commit push"),
            PredicateOutcome::Paths(vec![
                vec!["add".to_owned()],
                vec!["commit".to_owned()],
                vec!["push".to_owned()],
            ])
        );
    }

    #[test]
    fn using_command_alternatives() {
        assert_eq!(
            interpret("__fish_git_using_command log show diff-tree rev-list"),
            PredicateOutcome::Paths(vec![
                vec!["log".to_owned()],
                vec!["show".to_owned()],
                vec!["diff-tree".to_owned()],
                vec!["rev-list".to_owned()],
            ])
        );
    }

    #[test]
    fn negated_seen_drops() {
        assert_eq!(
            interpret("not __fish_seen_subcommand_from foo"),
            PredicateOutcome::Drop
        );
    }

    #[test]
    fn unsupported_drops() {
        assert_eq!(interpret("string match -qr foo"), PredicateOutcome::Drop);
        assert_eq!(interpret("__fish_seen_argument -l flag"), PredicateOutcome::Drop);
        assert_eq!(
            interpret("commandline -ct"),
            PredicateOutcome::Drop
        );
    }

    #[test]
    fn empty_args_drop() {
        assert_eq!(
            interpret("__fish_seen_subcommand_from "),
            PredicateOutcome::Drop
        );
    }

    #[test]
    fn quoted_or_dynamic_arg_skipped() {
        // `(foo)` is a substitution, not a literal subcommand — filter
        // it out, but keep the literal one.
        assert_eq!(
            interpret("__fish_seen_subcommand_from add (foo)"),
            PredicateOutcome::Paths(vec![vec!["add".to_owned()]])
        );
    }
}
