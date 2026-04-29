# `cli-completions-data-fish`

Generated database of command-line completions, derived from a vendored
snapshot of the [fish-shell](https://github.com/fish-shell/fish-shell)
`share/completions/*.fish` files. Pairs with the runtime crate
[`cli-completions`](../cli-completions/) which owns the binary blob format
and the zero-copy decoder.

This crate's `build.rs` parses the static subset of every `complete`
directive in `data/fish-snapshot/`, encodes it into
`data/completions.bin`, and exposes the embedded blob via
[`embedded()`](src/lib.rs).

```rust
use cli_completions_data_fish as fish;

let db = fish::embedded();
for m in db.query(&["curl"], "--an") {
    println!("{}\t{}", m.label, m.description.unwrap_or(""));
}
```

## Snapshot

`data/fish-snapshot.toml` records the upstream commit, fetch date, and
SHA-256 of the generated blob. The sync workflow lives outside this crate
(see RFC 005); this crate only consumes whatever .fish files are in
`data/fish-snapshot/`.

## License

GPL-2.0-or-later — fish-shell is GPL-2-or-later, and the redistributed
.fish files inherit those terms. See `LICENSE` for the full text and
RFC 005 §11 for the design rationale behind the two-crate split.
