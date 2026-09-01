# Contributing to WGSL / GLSL Shader

Developer notes: how to build it, how it is put together, and why it is put
together that way. For what the extension does, see [README.md](README.md).

## Building

From the repo root:

```sh
pnpm install
pnpm --filter wx-vsce-wgsl-shader build:wasm   # wasm-pack: crates/wgsl/wgsl-lsp-wasm → wasm/
pnpm --filter wx-vsce-wgsl-shader build        # tsdown → dist/extension.js, dist/server.js
pnpm --filter wx-vsce-wgsl-shader typecheck    # src/ and server/
pnpm --filter wx-vsce-wgsl-shader test         # vitest
pnpm --filter wx-vsce-wgsl-shader package      # → wx-vsce-wgsl-shader-<version>.vsix
```

`build:wasm` needs [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) and
runs `--target nodejs`, so the server `require`s the module like any other
CommonJS file. Run it once before `build`, and again after any change under
`crates/wgsl/`. `package` runs both for you.

**The Rust side has its own loop, and it is much the faster one.** Almost every
change can be finished and tested without building the WASM at all:

```sh
cargo test -p wgsl-syntax -p wgsl-lsp-core
```

Do **not** run `cargo fmt` here. The crates are hand-formatted and there is no
`rustfmt.toml`; running it rewrites every file.

Packaging is driven by [.vscodeignore](.vscodeignore), a copy of a shared
template — edit `scripts/templates/.vscodeignore` at the repo root and run
`pnpm sync-vscodeignore`, never the copy. `wasm/` and `dist/` ship; `src/`,
`server/`, `examples/` and this file do not.

## Layout

```
crates/wgsl/
  wgsl-syntax/     lexer, outline parser, builtin tables — no LSP, no naga
  wgsl-lsp-core/   the server: naga bridge, name resolution, every feature
  wgsl-lsp-wasm/   the wasm-bindgen surface, and the only crate that knows WASM exists

extensions/wgsl-shader/
  server/main.ts   the child process: JSON-RPC in, three WASM calls, notifications out
  src/lsp/         the client, and the embedded-shader virtual documents
  src/             activation, status bar, workspace scan, rust-analyzer hint
  syntaxes/        TextMate grammars, injections included
```

The split mirrors `crates/typst/`, which is the repo's other real language
server, and it exists for the same reason: the interesting code should be
testable without a WASM toolchain. `wgsl-lsp-wasm` is ~120 lines and wraps
exactly four methods; `tests/support/mod.rs` in `wgsl-lsp-core` wraps three of
the same four over a native harness, which is how the whole feature set is
exercised by `cargo test`.

## The two analyses

This is the central design decision, and most questions about the code come
back to it.

| | `wgsl-syntax` | naga |
| --- | --- | --- |
| Runs on | any input, always | only sources it can parse in full |
| Knows about | tokens, declarations, scopes, occurrences | types, validity |
| Rebuilt | every edit, eagerly | lazily, on first ask after an edit |
| Feeds | completion context, hover text, definition, symbols, folding, tokens, rename | diagnostics, inferred types, member lists |

naga is authoritative where it speaks. But it only speaks for a *complete*
parse, and there are two large holes in that:

- **Dialects.** naga's GLSL front end covers the vertex, fragment and compute
  stages at `#version 440`/`450`/`460 core`. A `#version 300 es` fragment
  shader is valid GLSL that it cannot read at all.
- **Time.** The file is invalid for most of the keystrokes an editor asks
  about. `camera.` — the exact state a member completion is requested in — does
  not parse in either language.

So every feature prefers naga and has a syntax-only fallback. That is what
`wgsl-syntax` is for, and why it is a separate crate with no naga dependency:
if it ever needs a valid parse to answer, the design has been broken.

### The last-good module

The `camera.` problem does not have a fallback good enough on its own — you
cannot know what `camera` is without types. So `state::Document` keeps the last
`naga::Module` that *did* parse, behind an `Rc`, and every type question goes
through `Document::module()` rather than `analysis().module`. Diagnostics are
the one exception: they must reflect the file as it is now, so they read
`analysis()` directly.

A module from one keystroke ago names the same structs and fields as the
current text in all but the rarest case. See
`state::tests::a_module_survives_an_edit_that_breaks_the_parse`.

## Embedded shaders

`src/lsp/embedded.ts` extracts `/* wgsl */ "…"` blocks from Rust and TS/JS. The
coordinate model is the whole trick, and it is the one `fast-element-ultra`
uses for `html` templates: **nothing is ever re-mapped.**

`virtualDocument` returns a buffer *the same length as the host file*, with
everything outside the shader replaced by spaces and newlines left in place. A
`Position` therefore means the same thing in the virtual document as in the
host, and forwarding a request is `executeCommand(…, virtualUri, position)`
with no translation on the way there or back.

Length preservation is what makes that true, so it is tested as a property over
300 generated templates rather than as examples — see
`src/lsp/embedded.test.ts`. Interpolations become identifiers of exactly their
own width for the same reason: `${uniforms.scale}` must not move the text after
it.

The virtual document's path ends in `.wgsl` or `.glsl`, which is how VS Code
assigns it a language id, which is how the client's document selector picks it
up, which is how the server ever sees it. Changing that path breaks the whole
chain silently.

## Adding a feature

1. Add the handler as an inherent method on `Server` in a new
   `wgsl-lsp-core/src/features/*.rs`.
2. Add a line to `dispatch.rs`.
3. Advertise it in `capabilities.rs` — conditionally, if a setting can switch it
   off. Capabilities are per-server and settings are per-language, so a
   capability is advertised when *either* language wants it and the handler
   declines for a document whose own language does not.
4. Add it to `REQUESTS` in `server/main.ts`. A method missing from that list is
   the single most likely reason a feature works in `cargo test` and not in the
   editor.
5. Test it in `tests/features.rs`, through the harness.

If the setting is new, it needs to exist in three places that must agree:
`settings.rs`, `src/config.ts`, and `contributes.configuration` in
`package.json`.

## Testing

```sh
cargo test -p wgsl-syntax          # lexer, parser, scopes, builtin tables
cargo test -p wgsl-lsp-core        # naga bridge, and every feature end to end
pnpm --filter wx-vsce-wgsl-shader test
```

The Rust tests are the bulk of it. `tests/features.rs` drives the same three
entry points the WASM binding does, so a green run there means the feature
works — what it cannot catch is the crossing itself, which is what
`server/engine.test.ts` is for. That one skips itself when `wasm/` has not been
built.

Fixtures mark the cursor with `|`; `support::Harness::open_at` strips it. Two
things are worth testing for every new feature, because they are where this
kind of code actually breaks:

- **a half-typed document** — see
  `every_request_survives_a_half_typed_document`;
- **a dialect naga skips** — a `#version 300 es` file must still answer.

## Debugging

`F5` opens the Extension Development Host. The server is a child process:

- Its log is in the *WGSL / GLSL Shader* output channel, along with the LSP
  trace when `wgslShader.trace.server` is set.
- To attach a debugger, launch the client and connect to `localhost:6019` — the
  `debug` server options in `src/lsp/client.ts` pass `--inspect`.
- `WGSL / GLSL: Restart Language Server` picks up a rebuilt `dist/server.js`
  without reloading the window. It does *not* pick up a rebuilt `wasm/`, which
  the process only loads once.

`examples/` holds a file per shape the extension handles — plain WGSL and GLSL,
and both languages embedded in Rust and TypeScript. The four standalone shaders
double as `include_str!` fixtures in
`analysis::tests::the_shipped_examples_validate`, so a change that stops them
validating fails `cargo test` before it reaches an editor.
