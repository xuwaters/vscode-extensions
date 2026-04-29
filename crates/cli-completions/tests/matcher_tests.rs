//! Behavioural tests for the small matcher helpers. The unit tests in
//! `src/matcher.rs` cover the basic cases; this file pins the more
//! interesting interactions exposed via the public iterator.

use cli_completions::{Builder, CompletionsDb, DirectiveInput, EntryFlags};

fn build(directives: &[(&str, Option<&str>, &[&str])]) -> Vec<u8> {
    let mut b = Builder::new();
    for (command, long, path) in directives {
        b.add(DirectiveInput {
            command,
            short: None,
            long: *long,
            description: None,
            flags: EntryFlags::default(),
            subcommand_path: path,
            arg_values: &[],
        });
    }
    b.build()
}

#[test]
fn unrelated_subcommand_path_does_not_match() {
    // Entry under "remote" shouldn't surface under "rebase".
    let blob = build(&[
        ("git", Some("verbose"), &["remote"]),
        ("git", Some("interactive"), &["rebase"]),
    ]);
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    let labels: Vec<_> = db
        .query(&["git", "rebase"], "--")
        .map(|m| m.label.into_owned())
        .collect();
    assert_eq!(labels, vec!["--interactive"]);
}

#[test]
fn deeper_query_includes_shallower_entries() {
    // An entry at depth 1 ("remote") should still apply for queries at
    // depth 2 ("remote add").
    let blob = build(&[
        ("git", Some("verbose"), &["remote"]),
        ("git", Some("fetch"), &["remote", "add"]),
    ]);
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    let mut labels: Vec<_> = db
        .query(&["git", "remote", "add"], "--")
        .map(|m| m.label.into_owned())
        .collect();
    labels.sort();
    assert_eq!(labels, vec!["--fetch", "--verbose"]);
}

#[test]
fn shallow_query_excludes_deeper_entries() {
    // Querying at "git remote" must not surface entries gated on
    // "remote add".
    let blob = build(&[
        ("git", Some("verbose"), &["remote"]),
        ("git", Some("fetch"), &["remote", "add"]),
    ]);
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    let labels: Vec<_> = db
        .query(&["git", "remote"], "--")
        .map(|m| m.label.into_owned())
        .collect();
    assert_eq!(labels, vec!["--verbose"]);
}

#[test]
fn prefix_without_dash_yields_nothing_in_v1() {
    // RFC §13 phase 6 will add subcommand and arg-value matches; for
    // phase 1 a non-dash prefix returns nothing even if subcommand
    // entries are present.
    let blob = build(&[("git", Some("verbose"), &["remote"])]);
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    let matches: Vec<_> = db.query(&["git"], "re").collect();
    assert!(matches.is_empty());
}
