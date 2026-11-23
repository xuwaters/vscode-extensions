# Contributing to RON Ultra

These are developer notes: how to build the extension, how it is put together,
and why. For what the extension does, see [README.md](README.md).

## Building

From the repo root:

```sh
pnpm install
pnpm --filter wx-vsce-ron-ultra build:wasm   # wasm-pack: crates/ron-ultra/ron-lsp-wasm → wasm/
pnpm --filter wx-vsce-ron-ultra build        # tsdown → dist/extension.js, dist/server.js
pnpm --filter wx-vsce-ron-ultra typecheck
pnpm --filter wx-vsce-ron-ultra test         # vitest
pnpm --filter wx-vsce-ron-ultra package      # → wx-vsce-ron-ultra-<version>.vsix
```

`build:wasm` needs [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) and runs
with `--target nodejs`, so the server process `require`s the module like any
other CommonJS file, with no `fetch` and no async instantiation. Run it once
before `build`, and again after any change under `crates/ron-ultra/`. `package`
runs both for you.

The Rust side has its own loop, and it is the faster one. Every feature can be
written and tested without building the WASM:

```sh
cargo test -p ron-analyzer -p ron-lsp-core
```

To try the extension by hand, open
[examples/game/assets/game-config.ron](examples/game/assets/game-config.ron) in
the Extension Development Host. It is annotated with
`@[crate::config::GameConfig]`, and the small crate around it
([examples/game/](examples/game/)) declares that struct. The crate has its own
`[workspace]` table so the repository's Cargo workspace ignores it.

Packaging is driven by [.vscodeignore](.vscodeignore), which is a copy of a
shared template. Edit `scripts/templates/.vscodeignore` at the repo root and run
`pnpm sync-vscodeignore`; never edit the copy. `wasm/` and `dist/` ship. `src/`,
`server/`, `examples/` and this file do not.

`pnpm run licenses` regenerates [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)
with cargo-about, using `about.toml` and `about.hbs` in
`crates/ron-ultra/ron-lsp-wasm/`. Rerun it whenever the dependencies of that
crate (or of the two below it) change.

## Layout

Three crates in `crates/ron-ultra/`, on the same split as wgsl-shader's server:

| Crate | Holds |
| --- | --- |
| `ron-analyzer` | All the analysis, in byte offsets: the RON parser and its features, the Rust declaration reader, and the schema checker. No LSP, no WASM |
| `ron-lsp-core` | The language server minus a transport: method dispatch, open documents, settings, diagnostics publishing, and conversion to `lsp-types` shapes and UTF-16 positions |
| `ron-lsp-wasm` | The `#[wasm_bindgen]` surface and the `readFile` host callback. The only crate that knows WASM exists |

Inside `ron-analyzer`:

| Path | Holds |
| --- | --- |
| `lexer.rs`, `parser.rs`, `ast.rs` | The RON token stream, recursive descent with local recovery, and the lossless AST |
| `extensions.rs` | The four `#![enable(...)]` extensions and their hover text |
| `diagnostics.rs` | The `RON001`…`RON019` codes, and which of them block formatting |
| `workspace.rs` | Per-file store: source, AST, span table, syntax diagnostics |
| `rust/` | Reading Rust: `lexer.rs` and `parser.rs` skim item declarations, `syntax.rs` is what they produce, `cargo.rs` reads manifests, `world.rs` resolves modules and names over files loaded on demand |
| `schema/` | `annotation.rs` finds `@[...]`, `ty.rs` lowers Rust types to what RON can express, `check.rs` walks the RON value against one, `case.rs` holds serde's `rename_all` rules |
| `features/` | `formatting.rs`, `document_symbols.rs`, `folding.rs`, `hover.rs`, `selection.rs`, `completion.rs`, `definition.rs`, and `locate.rs` (the root-to-cursor walk that hover and smart select share) |

`server/` is the language server process: `main.ts` loads the WASM and routes
every method it lists to it, and `host.ts` is the `readFile` callback. `src/` is
the extension host: `client.ts` starts the server over IPC and watches
`**/{*.rs,Cargo.toml}`, and `config.ts` gathers the `ronUltra.*` settings
into the object the server deserializes.

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

**Rust is read lazily, through the host, and synchronously.** The server is
never told about Rust files. Resolving `@[crate::config::GameConfig]` reads the
`Cargo.toml` files above the RON file until one has a `[package]`, then the
crate root, then one file per `mod` on the path. Each read is a synchronous
`readFile` call into the Node process, which is what lets resolution be plain
recursive code. Every read is cached in `rust::world::World`, misses included,
because a resolver probes `foo.rs` before `foo/mod.rs`. A watched-file event
for a URI the world has read (or looked for and missed) invalidates it, and
every open document is checked again. Events for files it never touched are
ignored, so a large workspace costs nothing until an annotation reaches into it.

**The Rust reader skims, it does not parse.** `rust::parser` recognizes `mod`,
`use`, `struct`, `enum` and `type` at item level, and skips every other item by
matching brackets. It never fails: a field or item it cannot read is dropped
and the next one read, so a Rust file you are halfway through editing still
contributes every declaration that is intact. It keeps doc comments and
`#[serde(...)]` attributes, including those inside `cfg_attr`, and ignores the rest.

**Unknown means anything.** A type the schema cannot see into lowers to
`Ty::Any`, which accepts every value. That covers foreign crates' structs,
`serde_json::Value`, trait objects, `#[serde(with)]` fields, macro output and
unresolved names. A missed error costs less than an invented one, so each
check is only as strict as what it can actually see.

**One walk, three consumers.** `schema::check::Checker` produces diagnostics
and `Facts` in the same pass. The facts record the type at each value, the
Rust field behind each key, the struct or variant behind each name, and the
`Body` each container should hold. Hover, go to definition and completion read
the facts instead of re-deriving types from the cursor. That keeps them in
agreement with the diagnostics on renames, flattening, untagged enums and
extensions, and keeps completion working inside a half-typed value. Untagged
enums are checked by trial: each variant is run through a quiet checker that
only counts errors, and the first one that fits is checked for real.

## Tests

The Rust tests are where the behaviour is pinned down. They are plain `cargo
test -p ron-analyzer -p ron-lsp-core` and need no WASM and no editor:

- The RON lexer and parser: every number form, strings, comments, comment
  attachment, recovery, duplicates, attributes (`lexer.rs`, `parser.rs`)
- The formatter, with every case also checked for idempotence
  (`features/formatting.rs`, the largest suite)
- The outline, folding, hover, smart select and value paths (`features/`)
- The Rust reader: items, serde attributes, `use` trees, half-written files
  (`rust/parser.rs`); manifests (`rust/cargo.rs`); module files, imports,
  globs, workspace dependencies and cycle termination (`rust/world.rs`)
- The schema: type lowering, every diagnostic, serde attributes, tagged and
  untagged enums, generics, extensions (`schema/`), plus completion and
  go to definition against the same fixture crate (`features/`)
- The server through `on_request` / `on_notification` / `drain`: publishing,
  watched-file invalidation, settings, every request, and the shipped example,
  which must be clean and already formatted (`ron-lsp-core/tests/lsp.rs`)

`pnpm test` runs vitest:

- `server/engine.test.ts` drives the **real WASM artifact** across the JS
  boundary, with the real `readFile` reading a crate in a temporary directory.
- `server/lsp.test.ts` spawns the **bundled** `dist/server.js` over stdio and
  goes from `initialize` through diagnostics and completion to `shutdown` and
  `exit`.
- `src/grammar.test.ts` tokenizes with `vscode-textmate` and asserts the scopes
  of the TextMate grammar, including the `@[...]` annotation inside comments.

The first two skip themselves until `wasm/` (and `dist/`) are built.
