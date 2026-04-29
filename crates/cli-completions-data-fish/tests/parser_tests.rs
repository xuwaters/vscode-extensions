//! End-to-end tests of the fish extractor against tiny synthetic .fish
//! fixtures. The unit tests inside `src/extractor/*` cover the lexer,
//! predicate handler, and parser in isolation; this file exercises the
//! full pipeline from text → directive list.

use cli_completions_data_fish::extractor::fish_parser::parse;

#[test]
fn realistic_curl_block() {
    let text = r#"
# vim: set filetype=fish:
complete -c curl -l abstract-unix-socket -d '(HTTP) Connect through an abstract Unix domain socket'
complete -c curl -l anyauth -d '(HTTP) Use most secure authentication method automatically'
complete -c curl -s a -l append -d '(FTP SFTP) Upload: append to the target file'
"#;
    let (directives, stats) = parse(text, "curl");
    assert_eq!(stats.kept, 3);
    assert_eq!(stats.dropped, 0);
    assert_eq!(directives.len(), 3);

    let names: Vec<_> = directives.iter().map(|d| d.longs.join(",")).collect();
    assert_eq!(names, vec!["abstract-unix-socket", "anyauth", "append"]);
    assert_eq!(directives[2].shorts, vec![b'a']);
}

#[test]
fn git_predicate_block() {
    let text = "\
complete git -f -n __fish_git_needs_command -l version -s v -d 'display git version'
complete -c git -n '__fish_git_using_command remote' -l verbose
complete -c git -n '__fish_git_using_command log show diff-tree' -l pretty
";
    let (directives, _) = parse(text, "git");
    assert_eq!(directives.len(), 3);

    // First entry: top-level only.
    assert!(directives[0]
        .flags
        .contains(cli_completions::EntryFlags::TOP_LEVEL_ONLY));
    assert!(directives[0].subcommand_paths.is_empty());

    // Second entry: under `remote`.
    assert_eq!(
        directives[1].subcommand_paths,
        vec![vec!["remote".to_owned()]]
    );

    // Third entry: alternatives across log/show/diff-tree.
    assert_eq!(
        directives[2].subcommand_paths,
        vec![
            vec!["log".to_owned()],
            vec!["show".to_owned()],
            vec!["diff-tree".to_owned()],
        ]
    );
}

#[test]
fn dropped_predicate_does_not_blow_up_neighbours() {
    let text = "\
complete -c foo -n 'string match -qr bar' -l cant
complete -c foo -l fine -d 'still here'
";
    let (directives, stats) = parse(text, "foo");
    assert_eq!(directives.len(), 1);
    assert_eq!(directives[0].longs, vec!["fine"]);
    assert_eq!(stats.dropped, 1);
    assert_eq!(stats.kept, 1);
}

#[test]
fn multi_line_continuation_is_one_directive() {
    let text = "complete -c curl \\\n    -l verbose \\\n    -d 'be loud about it'\n";
    let (directives, stats) = parse(text, "curl");
    assert_eq!(stats.kept, 1);
    assert_eq!(directives[0].longs, vec!["verbose"]);
    assert_eq!(directives[0].description.as_deref(), Some("be loud about it"));
}

#[test]
fn comment_lines_skipped() {
    let text = "\
# this is a comment
   # indented comment
complete -c foo -l real
";
    let (directives, _) = parse(text, "");
    assert_eq!(directives.len(), 1);
    assert_eq!(directives[0].command, "foo");
}

#[test]
fn arg_values_dropped_when_dynamic() {
    let text = "complete -c foo -l branch -a '(__fish_git_branches)'\n";
    let (directives, _) = parse(text, "");
    assert_eq!(directives.len(), 1);
    assert!(directives[0].arg_values.is_empty());
    assert_eq!(directives[0].longs, vec!["branch"]);
}

#[test]
fn arg_values_kept_when_static() {
    let text = "complete -c tar -l format -x -a 'gnu pax ustar oldgnu posix'\n";
    let (directives, _) = parse(text, "");
    assert_eq!(directives.len(), 1);
    assert_eq!(
        directives[0].arg_values,
        vec!["gnu", "pax", "ustar", "oldgnu", "posix"]
    );
}
