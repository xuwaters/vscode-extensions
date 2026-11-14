# Contributing to RON Ultra

These are developer notes: how to build the extension, how it is put together,
and why. For what the extension does, see [README.md](README.md).

## Building

From the repo root:

```sh
pnpm install
pnpm --filter wx-vsce-ron-ultra build:wasm   # wasm-pack: crates/ron-ultra/ron-analyzer → wasm/
pnpm --filter wx-vsce-ron-ultra build        # tsdown → dist/extension.js
pnpm --filter wx-vsce-ron-ultra typecheck
pnpm --filter wx-vsce-ron-ultra test         # vitest
pnpm --filter wx-vsce-ron-ultra package      # → wx-vsce-ron-ultra-<version>.vsix
```

`build:wasm` needs [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) and runs
with `--target nodejs`, so the extension host `require`s the module like any
other CommonJS file, with no `fetch` and no async instantiation. Run it once
before `build`, and again after any change under `crates/ron-ultra/`. `package`
runs both for you.

The Rust side has its own loop, and it is the faster one. Every feature can be
written and tested without building the WASM:

```sh
cargo test -p ron-analyzer
```

To try the extension by hand, open [examples/game-config.ron](examples/game-config.ron)
in the Extension Development Host.

Packaging is driven by [.vscodeignore](.vscodeignore), which is a copy of a
shared template. Edit `scripts/templates/.vscodeignore` at the repo root and run
`pnpm sync-vscodeignore`; never edit the copy. `wasm/` ships, because it is the
parser. `src/`, `examples/` and this file do not.

`pnpm run licenses` regenerates [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)
with cargo-about, using `about.toml` and `about.hbs` next to the crate. Rerun it
whenever the crate's dependencies change.

## Layout

The engine is `crates/ron-ultra/ron-analyzer/`, built on `crates/analyzer-core`
like the repo's other analyzers:

| File | Holds |
| --- | --- |
| `lexer.rs` | The token stream, comments included, with each token's preceding newline count |
| `parser.rs` | Recursive descent with local recovery, producing the lossless AST; duplicate detection |
| `ast.rs` | `Value` / `Container` / `Entry` / `Comment`, and who owns which comment |
| `extensions.rs` | The four `#![enable(...)]` extensions and their hover text |
| `diagnostics.rs` | The `RON001`…`RON011` codes, and which of them block formatting |
| `workspace.rs` | Per-file store: source, AST, span table, diagnostics |
| `features/` | `formatting.rs`, `document_symbols.rs`, `folding.rs`, `hover.rs`, `selection.rs`, and `locate.rs` (the root-to-cursor walk that hover and smart select share) |
| `wasm_api.rs` | The only file that knows WASM exists |

Everything inside the crate works in byte offsets. Conversion to UTF-16
line and column happens in `wasm_api.rs` on the way out.

`src/` is the extension host. `analyzer.ts` loads the WASM and wraps it in
typed methods that never throw. `config.ts` reads the `ronUltra.*` settings.
`diagnostics.ts` and `providers/` are each a thin adapter over one bridge call,
and `types.ts` mirrors what `wasm_api.rs` serializes.

## The parts that carry weight

**The AST is lossless.** Scalars keep only their source span, not a decoded
value; strings additionally keep their decoded text so that duplicate keys can
be compared. The formatter therefore reprints `0xFF_u8`, `1.e5`, `r#"..."#`
and every escape exactly as written. It never needs a serializer that agrees
with the parser about round-tripping, because it never decodes anything.

**Comments are owned, not floating.** The lexer emits comments as tokens. The
parser collects them into a pending list as it looks past them, then hands them
out at the point where their owner is known:

- Comments on lines before an entry are its `leading` comments.
- Comments on the same line after its value or comma are `trailing`.
- Comments before a closing bracket are the container's `dangling` comments.
- Comments inside an entry, after the key, are the value's `prefix`.

The formatter just prints each comment where its owner goes, so no position
heuristics are needed at print time.

**Newline counts, not line numbers.** Each token records how many newlines came
before it. That single number is what the parser needs to tell a trailing
comment from a leading one, to keep a blank line between entries, and to see
whether the author broke a container after its opening bracket (`broken`). The
formatter's collapse/expand rule comes from that last fact, which is what makes
the layout controllable and idempotent.

**The parser always produces a tree.** A missing value becomes
`ValueKind::Missing`. A missing comma is reported and the next entry is parsed
anyway. A stray closing bracket that matches nothing open is skipped, and one
that closes an outer container ends the inner one. Every loop has a progress
guard, so even `(: : :)` terminates. A half-typed file still gets an outline,
folding and hovers.

**The formatter refuses broken files.** `DiagnosticCode::blocks_formatting`
lists the codes that leave holes or untrustworthy slices in the AST, and
`format_file` returns `None` when any of them is present. Duplicates and
unknown extensions are real errors for a deserializer, but the tree is intact,
so they do not block formatting.

**Hover hit-tests strictly; smart select does not.** `locate::steps_to` takes a
`strict` flag. A hover is about the character under the mouse, so spans are
half-open and hovering a comma does not land on its neighbour. A selection
starts from a cursor between two characters, so a span also claims the offset
at its end, and a cursor just after `600` still selects `600`.

## Tests

The Rust tests are where the behaviour is pinned down. They are plain `cargo
test -p ron-analyzer` and need no WASM and no editor:

- The lexer: every number form, strings, raw and byte strings, chars, nested
  comments, newline counts (`lexer.rs`)
- The parser: comment attachment, recovery, duplicates, attributes (`parser.rs`)
- The formatter: collapse and expand, width, indentation, comment placement,
  blank lines, CRLF and refusals, with every case also checked for idempotence
  (`features/formatting.rs`, the largest suite)
- The outline, folding, hover, smart select and value paths (`features/`)
- The serialized WASM surface (`wasm_api.rs`)

`pnpm test` runs vitest:

- `src/analyzer.test.ts` drives the **real WASM artifact** end to end, checks
  that the bundled example is clean and already formatted, and skips itself when
  `wasm/` has not been built.
- `src/grammar.test.ts` tokenizes with `vscode-textmate` and asserts the scopes
  of the TextMate grammar.
