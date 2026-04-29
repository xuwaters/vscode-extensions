//! End-to-end queries against the *real* embedded blob.
//!
//! These tests rely on the `data/fish-snapshot/` directory being
//! populated; if it's empty the embedded blob is empty and the tests
//! degrade to the trivial "blob is well-formed" check.

use cli_completions_data_fish as fish;

#[test]
fn embedded_blob_is_decodable() {
    let db = fish::embedded();
    // Even with an empty snapshot, the header must round-trip.
    assert!(db.entry_count() <= u32::MAX as usize);
}

#[test]
fn curl_anyauth_present_when_snapshot_populated() {
    let db = fish::embedded();
    if !db.has_command("curl") {
        eprintln!("skipping: snapshot does not include curl.fish");
        return;
    }
    let labels: Vec<String> = db
        .query(&["curl"], "--anyauth")
        .map(|m| m.label.into_owned())
        .collect();
    assert!(
        labels.contains(&"--anyauth".to_owned()),
        "expected --anyauth in {labels:?}"
    );
}

#[test]
fn curl_prefix_matches_at_least_one() {
    let db = fish::embedded();
    if !db.has_command("curl") {
        return;
    }
    let count = db.query(&["curl"], "--c").count();
    assert!(count > 0, "expected curl to have at least one --c* option");
}

#[test]
fn unknown_command_is_empty() {
    let db = fish::embedded();
    let count = db.query(&["this-command-definitely-does-not-exist-anywhere"], "").count();
    assert_eq!(count, 0);
}

#[test]
fn git_subcommand_filtering() {
    let db = fish::embedded();
    if !db.has_command("git") {
        return;
    }
    // At top-level we should see git's `--version` (gated on
    // `__fish_git_needs_command`).
    let top_versions: Vec<_> = db.query(&["git"], "--ver").collect();
    let version_label = top_versions.iter().find(|m| m.label == "--version");
    assert!(
        version_label.is_some(),
        "expected --version at git top-level, got {:?}",
        top_versions.iter().map(|m| m.label.as_ref()).collect::<Vec<_>>()
    );

    // Inside a subcommand (e.g. `git remote`), the top-level-only
    // `--version` should be filtered out.
    let in_remote: Vec<_> = db.query(&["git", "remote"], "--ver").collect();
    let still_top = in_remote.iter().any(|m| m.label == "--version");
    assert!(
        !still_top,
        "top-level-only --version should not surface inside `git remote`"
    );
}
