//! Prefix and subcommand-path matching helpers used by the iterator.
//!
//! Kept in its own module so it can be unit-tested without touching the
//! decoder.

/// Does `label` match the user-typed `prefix`?
///
/// This is plain byte-prefix matching — case-sensitive. The consuming
/// editor (VSCode) is expected to do its own substring / fuzzy filtering
/// on the returned `label`s, so this layer only filters out clearly
/// non-matching items.
pub fn matches_prefix(label: &str, prefix: &str) -> bool {
    label.as_bytes().starts_with(prefix.as_bytes())
}

/// Does `entry_path` apply to a query at `query_subpath`?
///
/// `entry_path` is the subcommand chain attached to the entry (e.g.
/// `["remote", "add"]` for an option that fish gates on
/// `__fish_git_using_command remote add`).
///
/// `query_subpath` is the remainder of the user's command line after the
/// top-level command (e.g. for cursor on `git remote add --m|`,
/// query_subpath is `["remote", "add"]`).
///
/// The entry applies iff `entry_path` is a (possibly empty) prefix of
/// `query_subpath`. Empty `entry_path` means "applies anywhere".
pub fn entry_path_applies(entry_path: &[&str], query_subpath: &[&str]) -> bool {
    if entry_path.len() > query_subpath.len() {
        return false;
    }
    entry_path
        .iter()
        .zip(query_subpath.iter())
        .all(|(a, b)| a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_basic() {
        assert!(matches_prefix("--anyauth", "--an"));
        assert!(matches_prefix("--anyauth", ""));
        assert!(matches_prefix("--anyauth", "--anyauth"));
        assert!(!matches_prefix("--anyauth", "--xx"));
        assert!(!matches_prefix("--an", "--anyauth"));
    }

    #[test]
    fn prefix_case_sensitive() {
        assert!(!matches_prefix("--Verbose", "--v"));
    }

    #[test]
    fn path_empty_applies_anywhere() {
        assert!(entry_path_applies(&[], &[]));
        assert!(entry_path_applies(&[], &["remote"]));
        assert!(entry_path_applies(&[], &["remote", "add"]));
    }

    #[test]
    fn path_exact_and_prefix() {
        assert!(entry_path_applies(&["remote"], &["remote"]));
        assert!(entry_path_applies(&["remote"], &["remote", "add"]));
        assert!(entry_path_applies(&["remote", "add"], &["remote", "add"]));
        assert!(!entry_path_applies(&["remote", "add"], &["remote"]));
        assert!(!entry_path_applies(&["remote"], &["rebase"]));
    }
}
