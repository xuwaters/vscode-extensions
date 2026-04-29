//! End-to-end tests: build a synthetic blob via `Builder`, decode it
//! via `CompletionsDb`, and assert the round trip is faithful.

use cli_completions::{Builder, CompletionsDb, DirectiveInput, EntryFlags, FormatError, MatchKind};

fn directive<'a>(
    command: &'a str,
    short: Option<u8>,
    long: Option<&'a str>,
    description: Option<&'a str>,
    subcommand_path: &'a [&'a str],
) -> DirectiveInput<'a> {
    DirectiveInput {
        command,
        short,
        long,
        description,
        flags: EntryFlags::default(),
        subcommand_path,
        arg_values: &[],
    }
}

#[test]
fn empty_builder_produces_valid_blob() {
    let blob = Builder::new().build();
    let db = CompletionsDb::from_bytes(&blob).expect("empty blob decodes");
    assert_eq!(db.command_count(), 0);
    assert_eq!(db.entry_count(), 0);
    assert!(!db.has_command("git"));
}

#[test]
fn rejects_blob_shorter_than_header() {
    let res = CompletionsDb::from_bytes(&[0u8; 4]);
    assert_eq!(res.unwrap_err(), FormatError::TooShort);
}

#[test]
fn rejects_bad_magic() {
    let mut blob = Builder::new().build();
    blob[0] = b'X';
    let err = CompletionsDb::from_bytes(&blob).unwrap_err();
    assert_eq!(err, FormatError::BadMagic);
}

#[test]
fn rejects_unsupported_version() {
    let mut blob = Builder::new().build();
    // Version field is at byte 4 (LE u32). Bump it to 999.
    blob[4..8].copy_from_slice(&999u32.to_le_bytes());
    let err = CompletionsDb::from_bytes(&blob).unwrap_err();
    assert_eq!(err, FormatError::UnsupportedVersion(999));
}

#[test]
fn single_long_option_round_trip() {
    let mut b = Builder::new();
    b.add(directive(
        "curl",
        None,
        Some("anyauth"),
        Some("(HTTP) Use most secure auth method automatically"),
        &[],
    ));
    let blob = b.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    assert!(db.has_command("curl"));
    assert!(!db.has_command("xcurl"));
    assert_eq!(db.command_count(), 1);
    assert_eq!(db.entry_count(), 1);

    let matches: Vec<_> = db.query(&["curl"], "--an").collect();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].label.as_ref(), "--anyauth");
    assert_eq!(matches[0].kind, MatchKind::Long);
    assert_eq!(
        matches[0].description,
        Some("(HTTP) Use most secure auth method automatically")
    );
}

#[test]
fn multiple_commands_binary_search() {
    let mut b = Builder::new();
    for cmd in ["curl", "git", "make", "tar", "docker"] {
        b.add(directive(cmd, None, Some("help"), Some("Show help"), &[]));
    }
    let blob = b.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();
    assert_eq!(db.command_count(), 5);
    for cmd in ["curl", "git", "make", "tar", "docker"] {
        assert!(db.has_command(cmd), "missing: {cmd}");
    }
    assert!(!db.has_command("nope"));
    assert!(!db.has_command(""));
}

#[test]
fn prefix_filter_rejects_non_matches() {
    let mut b = Builder::new();
    b.add(directive("curl", None, Some("anyauth"), None, &[]));
    b.add(directive("curl", None, Some("verbose"), None, &[]));
    b.add(directive("curl", None, Some("version"), None, &[]));
    let blob = b.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    let labels: Vec<_> = db
        .query(&["curl"], "--ver")
        .map(|m| m.label.into_owned())
        .collect();
    assert_eq!(labels, vec!["--verbose", "--version"]);
}

#[test]
fn empty_prefix_yields_all_options_in_sorted_order() {
    let mut b = Builder::new();
    b.add(directive("curl", None, Some("verbose"), None, &[]));
    b.add(directive("curl", None, Some("anyauth"), None, &[]));
    b.add(directive("curl", None, Some("cacert"), None, &[]));
    let blob = b.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    let labels: Vec<_> = db
        .query(&["curl"], "")
        .map(|m| m.label.into_owned())
        .collect();
    assert_eq!(labels, vec!["--anyauth", "--cacert", "--verbose"]);
}

#[test]
fn unknown_command_query_is_empty() {
    let blob = Builder::new().build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();
    let matches: Vec<_> = db.query(&["does-not-exist"], "--").collect();
    assert!(matches.is_empty());
}

#[test]
fn empty_path_query_is_empty() {
    let mut b = Builder::new();
    b.add(directive("curl", None, Some("anyauth"), None, &[]));
    let blob = b.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();
    let matches: Vec<_> = db.query(&[], "--").collect();
    assert!(matches.is_empty());
}

#[test]
fn short_option_yields_short_match() {
    let mut b = Builder::new();
    b.add(directive("curl", Some(b'v'), None, Some("Verbose"), &[]));
    let blob = b.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    let matches: Vec<_> = db.query(&["curl"], "-v").collect();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].label.as_ref(), "-v");
    assert_eq!(matches[0].kind, MatchKind::Short);
    assert_eq!(matches[0].description, Some("Verbose"));
}

#[test]
fn long_only_matched_by_double_dash_prefix() {
    let mut b = Builder::new();
    b.add(directive("curl", Some(b'v'), Some("verbose"), None, &[]));
    let blob = b.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    // "--" prefix should only see the long form.
    let labels: Vec<_> = db
        .query(&["curl"], "--")
        .map(|m| m.label.into_owned())
        .collect();
    assert_eq!(labels, vec!["--verbose"]);
}

#[test]
fn subcommand_path_filters() {
    let mut b = Builder::new();
    // Top-level "git --version".
    b.add(directive("git", None, Some("version"), None, &[]));
    // "git remote --verbose".
    b.add(directive(
        "git",
        None,
        Some("verbose"),
        None,
        &["remote"],
    ));
    // "git remote add --fetch".
    b.add(directive(
        "git",
        None,
        Some("fetch"),
        None,
        &["remote", "add"],
    ));
    let blob = b.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    // At the top level, only --version is offered.
    let mut labels: Vec<_> = db
        .query(&["git"], "--")
        .map(|m| m.label.into_owned())
        .collect();
    labels.sort();
    assert_eq!(labels, vec!["--version"]);

    // At "git remote", we see top-level + remote-scoped (entry path is
    // a prefix of query path).
    let mut labels: Vec<_> = db
        .query(&["git", "remote"], "--")
        .map(|m| m.label.into_owned())
        .collect();
    labels.sort();
    assert_eq!(labels, vec!["--verbose", "--version"]);

    // At "git remote add", all three apply (top-level + remote + remote add).
    let mut labels: Vec<_> = db
        .query(&["git", "remote", "add"], "--")
        .map(|m| m.label.into_owned())
        .collect();
    labels.sort();
    assert_eq!(labels, vec!["--fetch", "--verbose", "--version"]);
}

#[test]
fn top_level_only_flag_suppresses_under_subcommand() {
    let mut b = Builder::new();
    b.add(DirectiveInput {
        command: "git",
        short: None,
        long: Some("html-path"),
        description: None,
        flags: EntryFlags::from_bits(EntryFlags::TOP_LEVEL_ONLY),
        subcommand_path: &[],
        arg_values: &[],
    });
    b.add(directive("git", None, Some("version"), None, &[]));
    let blob = b.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();

    let mut labels: Vec<_> = db
        .query(&["git"], "--")
        .map(|m| m.label.into_owned())
        .collect();
    labels.sort();
    assert_eq!(labels, vec!["--html-path", "--version"]);

    // Under a subcommand, the TOP_LEVEL_ONLY entry vanishes.
    let labels: Vec<_> = db
        .query(&["git", "remote"], "--")
        .map(|m| m.label.into_owned())
        .collect();
    assert_eq!(labels, vec!["--version"]);
}

#[test]
fn build_is_deterministic() {
    let mk = || {
        let mut b = Builder::new();
        b.add(directive("curl", None, Some("verbose"), None, &[]));
        b.add(directive("curl", None, Some("anyauth"), None, &[]));
        b.add(directive("git", None, Some("version"), None, &[]));
        b.build()
    };
    assert_eq!(mk(), mk());
}

#[test]
fn directive_with_no_useful_data_is_dropped() {
    let mut b = Builder::new();
    b.add(directive("curl", None, None, Some("orphan desc"), &[]));
    let blob = b.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();
    assert_eq!(db.command_count(), 0);
    assert_eq!(db.entry_count(), 0);
}

#[test]
fn truncated_blob_after_header_rejected() {
    let mut b = Builder::new();
    b.add(directive("curl", None, Some("anyauth"), None, &[]));
    let blob = b.build();
    // Truncate so the cmd table is no longer fully present.
    let truncated = &blob[..blob.len() - 1];
    let err = CompletionsDb::from_bytes(truncated).unwrap_err();
    matches!(err, FormatError::OutOfBounds { .. });
}
