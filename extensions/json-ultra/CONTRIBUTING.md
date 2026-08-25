# Contributing to JSON Ultra

Developer notes: how to build it, how it is put together, and why it is put
together that way. For what the extension does, see [README.md](README.md).

## Building

From the repo root:

```sh
pnpm install
pnpm --filter wx-vsce-json-ultra build:wasm   # wasm-pack: crates/json-analyzer → wasm/
pnpm --filter wx-vsce-json-ultra build        # tsdown → dist/extension.js, dist/webview.js
pnpm --filter wx-vsce-json-ultra typecheck    # host and webview tsconfigs
pnpm --filter wx-vsce-json-ultra test         # vitest
pnpm --filter wx-vsce-json-ultra package      # → wx-vsce-json-ultra-<version>.vsix
```

`build:wasm` needs [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) and
runs `--target nodejs`, so the extension host `require`s the module like any
other CommonJS file — no `fetch`, no async instantiation. `wasm-opt` is off in
`Cargo.toml`; the bundle is small enough that the extra build dependency is not
worth it. Run it once before `build`, and again after any change under
`crates/json-analyzer/`. `package` runs both for you.

`pnpm --filter wx-vsce-json-ultra watch` rebuilds the TypeScript on change
while the Extension Development Host is running; the WASM half is not watched.

The Rust side has its own loop, and it is the faster one — most features can be
finished and tested without ever building the WASM:

```sh
cargo test -p json-analyzer
```

Packaging is driven by [.vscodeignore](.vscodeignore), which is a copy of a
shared template — edit `scripts/templates/.vscodeignore` at the repo root and
run `pnpm sync-vscodeignore`, never the copy. `wasm/` ships (it is the parser);
`src/`, `webview/` and this file do not.

## Layout

The parser is Rust, in `crates/json-analyzer/`, shared with nothing else but
built on `crates/analyzer-core` like the repo's other analyzers:

| File | Holds |
| --- | --- |
| `lexer.rs` | The JSON5 superset token stream, spans and all |
| `parser.rs` | Recursive descent with local recovery, producing the lossless AST |
| `ast.rs` | `Value`/`Member`/`Comment` — scalars keep their source span, comments attach to what they annotate |
| `flavor.rs` | The four dialects, and what each one *allows* |
| `diagnostics.rs` | The `JSON001`…`JSON013` codes |
| `workspace.rs` | Per-file store: source, flavor, AST, span table, diagnostics |
| `features/` | `formatting.rs` (pretty-print, compact JSONL, key sort), `document_symbols.rs`, `folding.rs`, `hover.rs`, `table.rs` |
| `wasm_api.rs` | The only file that knows WASM exists |

`spans.rs` just re-exports `analyzer-core`'s span primitives. Everything inside
the crate is byte offsets; the conversion to line/column happens in
`wasm_api.rs` on the way out, so no feature carries an editor's coordinate
system around.

`src/` is the extension host, and also holds the parts shared across the webview
boundary:

| File | Holds |
| --- | --- |
| `analyzer.ts` | The bridge: loads the WASM, wraps its string-typed surface in typed, infallible methods |
| `config.ts` | Typed, per-resource readers over `jsonUltra.*`, plus the marshalling into Rust's `FormatOptions` |
| `diagnostics.ts` | The collection, and the two languages it covers |
| `providers/` | Formatting, document symbols, folding, hover — each a thin adapter over one bridge call |
| `oxc.ts` | oxfmt discovery and the child process |
| `preview/` | `provider.ts` (the custom text editor), `html.ts` (the page and its CSP) |
| `messages.ts` | The host ⇄ webview protocol and its validator, imported by both sides |
| `types.ts` | The shape of everything `wasm_api.rs` serializes |

`webview/` is the table page: `index.ts` (bootstrap — the only place
`acquireVsCodeApi` exists), `table.ts` (the virtualized view), `styles.css`
(inlined into the bundle by the `raw-assets` plugin in
[tsdown.config.mts](tsdown.config.mts), so the page loads one file).

## The parts that carry weight

**One parser, four dialects.** The parser accepts the JSON5 superset
unconditionally; `Flavor` only decides which constructs are *diagnosed*. That
is what keeps recovery uniform — a stray comment in strict JSON still parses,
still folds, still formats, and just carries a `JSON008`. The alternative, a
parser per dialect, would mean four recovery strategies to keep in step and a
`.json` file with a comment in it that has no AST at all.

**The AST is lossless.** Scalars keep their source span rather than a decoded
value, and comments attach to the member or element they annotate. Both
properties exist for the formatter: it can reprint a file — optionally with
every object's keys sorted — without normalizing an escape, re-spelling a
number, or orphaning a comment. It also means the formatter never needs a
serializer that agrees with the parser about round-tripping, because it never
decodes anything in the first place.

**The formatter refuses broken files.** Anything in `BLOCKING` — a syntax
error, an unterminated string or comment, an invalid number, content past the
top level — means the AST has holes or untrustworthy raw slices, and
`format_file` returns `None`. Format-on-save fires while you are still typing;
rewriting a half-typed document is worse than doing nothing.

**Key order is the reader's, not the byte comparator's.** `compare_keys` folds
case and compares digit runs as numbers, falling back to exact code points only
to break a tie — so `item2` precedes `item10`, `Editor` sits beside `editor`
instead of in a separate uppercase block, and `$schema` and `[astro]` group
ahead of the words because their punctuation sorts below every letter. The
fallback is what makes it a total order, and therefore a stable sort that does
not shuffle a file that is already sorted.

**Sort composes with oxfmt by running first.** When the project is configured
for oxc, the buffer is sorted by the WASM analyzer and the *sorted text* is what
goes down oxfmt's stdin. So delegating the layout does not cost you the one
feature oxfmt does not have.

**Every path out of oxfmt that is not a clean exit falls back.** No config, no
binary, non-zero exit, a timeout, an EPIPE on stdin — each resolves to `null`,
logs a line, and the built-in formatter runs. An external binary is the one
part of this extension that can be missing, broken or slow on a machine that is
not yours; a format request must still produce a formatted file.

**The bridge is infallible by construction.** Every method catches, and a
missing or broken `wasm/` degrades to a null bridge: the extension still
activates, providers return nothing, and the output channel says to run
`build:wasm`. A `catch` around a WASM call is not paranoia — a panic crosses
that boundary as an exception, and the alternative is a dead extension host.

**The preview is a `CustomTextEditorProvider`, at `priority: "option"`.** A
`.jsonl` file *is* text, so VS Code keeps owning the document — encoding,
watchers, dirty state, the text editor you can reopen at any time — and this
provider simply never produces an edit. `option` keeps the text editor the
default, which is right for a format people mostly read as lines.

**Every message from the webview is shape-validated host-side**, one
hand-written guard per variant, ranges as well as types. The preview is
read-only, so the blast radius is small — but a webview is a hostile input
boundary even when we wrote the far side, and `copy` carries a string to the
clipboard and `openLine` an index into a document.

**Nothing in the table is laid out by the browser.** A few hundred recycled
boxes are positioned from arithmetic each animation frame; headers and row
numbers live *outside* the scroller and are translated by the scroll offsets,
because a sticky header inside a two-axis scroller is a fight with the browser
nobody wins. Cell text is always `textContent`, never markup.

## Tests

The Rust tests are where the behaviour is pinned, and they are plain
`cargo test -p json-analyzer` — the features are span-based and need no WASM,
no editor and no DOM:

- The lexer, over the JSON5 superset (`lexer.rs`)
- The parser and its recovery, per flavor (`parser.rs`)
- The formatter — comment placement, byte-for-byte scalars, the JSONL compact
  layout, the refusals, and the key comparator (`features/formatting.rs`, the
  largest suite here)
- The outline, folding, hover paths and JSONL table extraction (`features/`)
- The serialized WASM surface itself (`wasm_api.rs`)

`pnpm test` runs vitest over the TypeScript:

- `src/analyzer.test.ts` drives the **real WASM artifact** end to end, and
  skips itself when `wasm/` has not been built
- `src/messages.test.ts` covers the protocol guards
- `src/bundle.test.ts` asserts on the built artifacts — that the page's module
  parses, that the host bundle has no `acquireVsCodeApi` and the page bundle no
  `require("vscode")` — and skips on a checkout with no `dist/`

Both skip-guards are deliberate: a fresh clone runs `pnpm test` green without a
Rust toolchain, and the suites that need artifacts say what is missing rather
than failing on it.
