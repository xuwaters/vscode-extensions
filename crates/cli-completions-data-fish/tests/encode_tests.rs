//! Encode / decode round-trip via the runtime crate's `Builder`, fed
//! by the extractor's `Directive` output. This catches regressions in
//! the wire format <-> directive shape mapping.

use cli_completions::{Builder, CompletionsDb, DirectiveInput};
use cli_completions_data_fish::extractor::fish_parser::parse;

#[test]
fn small_fixture_roundtrip() {
    let text = "\
complete -c curl -l anyauth -d 'use most secure auth'
complete -c curl -s v -l verbose -d 'be loud'
complete -c curl -l user-agent -x -d 'set the User-Agent'
";
    let (directives, _) = parse(text, "curl");

    let mut builder = Builder::new();
    for d in &directives {
        // The directive can have multiple shorts/longs; flatten the
        // simple case (1:1 or 0:1) the same way the build pipeline does.
        let arg_values: Vec<&str> = d.arg_values.iter().map(String::as_str).collect();
        let path: Vec<&str> = Vec::new();
        let short = d.shorts.first().copied();
        let long = d.longs.first().map(String::as_str);
        builder.add(DirectiveInput {
            command: &d.command,
            short,
            long,
            description: d.description.as_deref(),
            flags: d.flags,
            subcommand_path: &path,
            arg_values: &arg_values,
        });
    }

    let blob = builder.build();
    let db = CompletionsDb::from_bytes(&blob).unwrap();
    assert_eq!(db.command_count(), 1);

    let labels: Vec<String> = db
        .query(&["curl"], "--")
        .map(|m| m.label.into_owned())
        .collect();
    assert_eq!(labels, vec!["--anyauth", "--user-agent", "--verbose"]);

    let shorts: Vec<String> = db
        .query(&["curl"], "-v")
        .map(|m| m.label.into_owned())
        .collect();
    assert!(shorts.contains(&"-v".to_owned()));

    // Description survives the round-trip.
    let any: Vec<_> = db.query(&["curl"], "--an").collect();
    assert_eq!(any.len(), 1);
    assert_eq!(any[0].label, "--anyauth");
    assert_eq!(any[0].description, Some("use most secure auth"));
}
