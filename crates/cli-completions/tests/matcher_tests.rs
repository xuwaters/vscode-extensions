//! Behavioural tests for the small matcher helpers. The unit tests in
//! `src/matcher.rs` cover the basic cases; this file pins the more
//! interesting interactions exposed via the public iterator.

use cli_completions::{Builder, CompletionsDb, DirectiveInput, EntryFlags, MatchKind};

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
fn non_dash_prefix_yields_subcommand_matches() {
    // Per RFC §13 phase 6, a non-`-` prefix surfaces subcommand-name
    // completions, deduplicated and sorted.
    let blob = build(&[
        ("git", Some("verbose"), &["remote"]),
        ("git", Some("mirror"), &["remote"]),
        ("git", Some("force"), &["rebase"]),
    ]);
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    let matches: Vec<_> = db.query(&["git"], "re").collect();
    let labels: Vec<_> = matches.iter().map(|m| m.label.as_ref()).collect();
    assert_eq!(labels, vec!["rebase", "remote"]);
    assert!(matches.iter().all(|m| m.kind == MatchKind::Subcommand));
}

#[test]
fn empty_prefix_emits_subcommands_then_options() {
    // Empty prefix returns the union — subcommands first, options after.
    let blob = build(&[
        ("git", Some("verbose"), &[]),
        ("git", Some("mirror"), &["remote"]),
    ]);
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    let matches: Vec<_> = db.query(&["git"], "").collect();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].kind, MatchKind::Subcommand);
    assert_eq!(matches[0].label, "remote");
    assert_eq!(matches[1].kind, MatchKind::Long);
    assert_eq!(matches[1].label, "--verbose");
}
