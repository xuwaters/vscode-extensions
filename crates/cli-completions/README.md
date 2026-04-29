# `cli-completions`

Runtime decoder and packed blob format for static command-line completion
data. Designed to be paired with one or more *data crates* that ship a
generated blob — see RFC 005 for the motivation and the sister crate
`cli-completions-data-fish` (GPL-2-or-later) for the fish-shell-derived
data set.

This crate has **no runtime dependencies** beyond the Rust standard
library, performs **no I/O**, and runs no subprocesses. It is suitable
for use inside `wasm32-unknown-unknown` analyzers loaded into VSCode.

## Usage

```rust
use cli_completions::{Builder, CompletionsDb, DirectiveInput, EntryFlags};

// Producer (typically a build script):
let mut b = Builder::new();
b.add(DirectiveInput {
    command: "curl",
    short: None,
    long: Some("anyauth"),
    description: Some("(HTTP) Use most secure authentication method automatically"),
    flags: EntryFlags::default(),
    subcommand_path: &[],
    arg_values: &[],
});
let blob: Vec<u8> = b.build();

// Consumer (typically a query at editor cursor time):
let db = CompletionsDb::from_bytes(&blob).unwrap();
for m in db.query(&["curl"], "--an") {
    println!("{}\t{}", m.label, m.description.unwrap_or(""));
}
```

## License

MIT. See `LICENSE`. The runtime crate carries no third-party data and is
freely embeddable. Data crates that produce blobs may carry their own
license terms — check theirs separately.
