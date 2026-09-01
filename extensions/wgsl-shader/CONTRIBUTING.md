# Contributing to WGSL / GLSL Shader

Developer notes: how to build it, how it is put together, and why it is put
together that way. For what the extension does, see [README.md](README.md).

## Building

From the repo root:

```sh
pnpm install
pnpm --filter wx-vsce-wgsl-shader build:wasm   # wasm-pack: crates/wgsl-shader/wgsl-lsp-wasm → wasm/
pnpm --filter wx-vsce-wgsl-shader build        # tsdown → dist/extension.js, dist/server.js
pnpm --filter wx-vsce-wgsl-shader typecheck    # src/ and server/
pnpm --filter wx-vsce-wgsl-shader test         # vitest
pnpm --filter wx-vsce-wgsl-shader licenses     # cargo-about → THIRD-PARTY-NOTICES.md
pnpm --filter wx-vsce-wgsl-shader package      # → wx-vsce-wgsl-shader-<version>.vsix
```

`build:wasm` needs [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) and
runs `--target nodejs`, so the server `require`s the module like any other
CommonJS file. Run it once before `build`, and again after any change under
`crates/wgsl-shader/`. `package` runs both for you.

**The Rust side has its own loop, and it is much the faster one.** Almost every
change can be finished and tested without building the WASM at all:

```sh
cargo test -p glsl-spec -p glsl-syntax -p glsl-analysis -p wgsl-syntax -p wgsl-lsp-core
```

Do **not** run `cargo fmt` here. The crates are hand-formatted and there is no
`rustfmt.toml`; running it rewrites every file.

Packaging is driven by [.vscodeignore](.vscodeignore), a copy of a shared
template — edit `scripts/templates/.vscodeignore` at the repo root and run
`pnpm sync-vscodeignore`, never the copy. `wasm/` and `dist/` ship; `src/`,
`server/`, `examples/` and this file do not.

## Layout

```
crates/wgsl-shader/
  wgsl-syntax/     WGSL lexer, outline parser, builtin tables — no LSP, no naga
  wgsl-lsp-core/   the server: dispatch, document state, every feature, the naga bridge
  wgsl-lsp-wasm/   the wasm-bindgen surface, and the only crate that knows WASM exists
  glsl-spec/       GLSL's predeclared surface as data — builtins, keywords, types
  glsl-spec-gen/   native-only generator: temp/docs.gl → glsl-spec/src/generated/
  glsl-syntax/     GLSL lexer, preprocessor, recovering parser, lossless CST
  glsl-analysis/   GLSL scopes, type model, overload resolution, diagnostics

extensions/wgsl-shader/
  server/main.ts   the child process: JSON-RPC in, three WASM calls, notifications out
  src/lsp/         the client, and the embedded-shader virtual documents
  src/             activation, status bar, workspace scan, rust-analyzer hint
  syntaxes/        TextMate grammars, injections included
```

Seven crates, and the two axes that matter about them:

| Crate | Ships in the wasm | Depends on |
| --- | --- | --- |
| `glsl-spec` | yes | nothing |
| `glsl-spec-gen` | **no** — a native dev tool | `roxmltree` |
| `glsl-syntax` | yes | `analyzer-core` |
| `glsl-analysis` | yes | `analyzer-core`, `glsl-syntax`, `glsl-spec` |
| `wgsl-syntax` | yes | `analyzer-core` |
| `wgsl-lsp-core` | yes | all of the above, plus `naga` (`wgsl-in` only) and `lsp-types` |
| `wgsl-lsp-wasm` | it *is* the wasm | `wgsl-lsp-core`, `wasm-bindgen` |

`crates/analyzer-core` is the repo-wide shared floor — spans, text, the
diagnostic shapes — and is not part of this extension.

The four `glsl-*` and `wgsl-syntax` crates are deliberately LSP-free: nothing in
them mentions `lsp-types`, so a rule can be tested as a rule. `wgsl-lsp-core` is
where the two languages meet a protocol, and it is the only crate that knows
about both.

The split mirrors `crates/typst/`, which is the repo's other real language
server, and it exists for the same reason: the interesting code should be
testable without a WASM toolchain. `wgsl-lsp-wasm` is ~120 lines and wraps
exactly four methods; `tests/support/mod.rs` in `wgsl-lsp-core` wraps three of
the same four over a native harness, which is how the whole feature set is
exercised by `cargo test`.

## One server, two languages, two pipelines

This is the central design decision, and most questions about the code come
back to it. `wgsl-lsp-core` dispatches on `Language`, and the two sides look
nothing alike underneath:

| | WGSL | GLSL |
| --- | --- | --- |
| Syntax layer | `wgsl-syntax` — resilient lexer + outline parser | `glsl-syntax` — lexer, preprocessor, recovering parser, lossless CST |
| Types and validity | **naga**, an external compiler front end | **`glsl-analysis`**, ours |
| Built-in surface | `wgsl-syntax`'s hand-written tables | `glsl-spec`, generated from the reference pages |
| Dialects covered | WGSL | 1.10–4.60 core/compatibility, ES 1.00–3.20, six stages |
| Adapter | `analysis/mod.rs` | `glsl/adapter.rs` — one projection every feature reads |

RFC 012 built the GLSL column. Before it, GLSL went through a 464-line
heuristic token walk in `wgsl-syntax` for its outline and through *naga's* GLSL
front end for validation — and naga implements Vulkan GLSL at
`#version 440`/`450`/`460` only, so `analysis/dialect.rs` existed to detect the
sources naga would mangle and switch validation off for them. The walk, the
tables, `dialect.rs` and naga's `glsl-in` feature are all gone
([decision 0008](../../docs/rfc/012-glsl-analyzer/decisions/0008-naga-glsl-in-dropped.md)).
naga is a WGSL dependency now and nothing else.

What survives from that design, and matters on both sides, is **time**: the file
is invalid for most of the keystrokes an editor asks about. `camera.` — the
exact state a member completion is requested in — parses in neither language. So
every feature has an answer that does not need a valid parse. That is why
`wgsl-syntax` has no naga dependency and why `glsl-syntax` produces a tree for
input it could not parse: if either ever needs a clean parse to answer, the
design has been broken.

### The last-good module (WGSL)

The `camera.` problem does not have a fallback good enough on its own — you
cannot know what `camera` is without types. So `state::Document` keeps the last
`naga::Module` that *did* parse, behind an `Rc`, and every type question goes
through `Document::module()` rather than `analysis().module`. Diagnostics are
the one exception: they must reflect the file as it is now, so they read
`analysis()` directly.

A module from one keystroke ago names the same structs and fields as the
current text in all but the rarest case. See
`state::tests::a_module_survives_an_edit_that_breaks_the_parse`.

### Conservatism instead, on the GLSL side

GLSL needs no last-good module, because `glsl-analysis` types the current tree
however broken it is. What it has instead is a rule about staying quiet: an
analysis whose *preprocessing or parse* failed publishes that diagnostic and
**no** semantic ones, because a semantic rule applied to a tree missing a brace
invents errors about code nobody wrote. The same instinct runs through the
diagnostics catalogue — a file with an `#extension` line gets no unknown-name
errors, and a `gl_`-prefixed or vendor-suffixed name is never "unknown". See
`glsl_dialects::a_file_that_did_not_parse_reports_no_semantic_errors`, and the
false-positive corpus gate below, which is the enforcement.

## The GLSL builtin spec, and how to regenerate it

`glsl-spec` is the language's predeclared surface as embedded Rust data: 161
builtin functions over 717 overloads, 31 `gl_*` variables, and the generic
families (`genType`, `gvec4`, `gsampler2D`) the overloads are written over, each
entry carrying a desktop version mask, an ES version mask and a stage mask. It
is **generated** — `glsl-spec/src/generated/` is written by `glsl-spec-gen` from
the docs.gl reference pages and committed, so a change to it is a reviewable
diff rather than a build step nobody can see.

```sh
git clone https://github.com/BSVino/docs.gl temp/docs.gl   # once; never committed
cargo run -p glsl-spec-gen                                 # rewrite the tables
cargo run -p glsl-spec-gen -- --check                      # fail if they are stale
```

`temp/` is read-only reference material for the whole repo: the checkout is read
in place, page by page, and nothing from it is ever copied into the tree. The
generator takes `--docs-gl <dir>` and `--out <dir>` if you need them, and prints
what it read; a page it cannot parse **stops the run**, because a silent gap in
the builtin table is a wrong answer in an editor months later with no way back
to the cause.

Two things are not generated and are edited by hand, in `glsl-spec/src/`:

- `keywords.rs` — keywords, basic types and precision defaults, which are
  grammar rather than library.
- `legacy.rs` — 39 functions and 58 variables of the compatibility profile and
  GLSL ES 1.00 (`gl_ModelViewMatrix`, `texture2D`, `ftransform`). docs.gl
  documents none of that surface, and the corpus proves it is still in use, so
  it is a hand-written table with a `compatibility` flag beside the version
  masks
  ([decision 0007](../../docs/rfc/012-glsl-analyzer/decisions/0007-legacy-builtin-table.md)).

The Khronos prose the tables embed is Open Publication License v1.0 material;
the attribution is generated into every file's header and restated in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md). Changing `emit::header` is the
way to change it — the determinism test fails if the committed files disagree,
so the notice cannot rot while the tables move on.

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
cargo test -p glsl-spec            # the generated tables, versions, families
cargo test -p glsl-syntax          # lexer, preprocessor, parser, CST, outline
cargo test -p glsl-analysis        # scopes, types, overloads, every diagnostic
cargo test -p glsl-spec-gen        # the generator (skips without temp/docs.gl)
cargo test -p wgsl-syntax          # WGSL lexer, parser, scopes, builtin tables
cargo test -p wgsl-lsp-core        # naga bridge, and every feature end to end
pnpm --filter wx-vsce-wgsl-shader test
```

The Rust tests are the bulk of it. `tests/features.rs` drives the same three
entry points the WASM binding does, so a green run there means the feature
works — what it cannot catch is the crossing itself, which is what
`server/engine.test.ts` is for. That one skips itself when `wasm/` has not been
built.

### The corpus gates, and how they skip

The GLSL parser and analyzer are gauged against Khronos's own test tree — about
1,700 shaders spanning every version, profile and stage, most of them
deliberately broken. It is **read in place** from `temp/glslang/Test` and never
copied ([decision 0004](../../docs/rfc/012-glsl-analyzer/decisions/0004-corpus-in-place.md)):

```sh
git clone https://github.com/KhronosGroup/glslang temp/glslang
GLSL_CORPUS=/some/other/Test cargo test -p glsl-analysis   # or point it elsewhere
```

**Without that checkout the gates print `SKIP …` and pass.** A contributor who
has not cloned glslang is not blocked, and nothing silently reports success it
did not earn — the skip line names the directory it looked in. Four gates read
it:

| Gate | Crate | Requires |
| --- | --- | --- |
| `corpus_preprocess` | `glsl-syntax` | zero panics; every byte covered |
| `corpus_parse` | `glsl-syntax` | zero panics; byte-for-byte round-trip of the source |
| `corpus_analyze` | `glsl-analysis` | zero panics. Diagnostic counts are printed, not asserted — a corpus of deliberate errors has no right answer |
| `corpus_no_false_errors` | `glsl-analysis` | **zero error-severity diagnostics** on a curated list of 208 valid shaders |

That last one is the bar, and the list only grows: a file that goes in never
comes out, because a rule that needs a file removed is a rule that is wrong. The
extension's own `examples/` are gated the same way by
`the_extensions_examples_analyse_cleanly`, which walks the directory rather than
naming files — a new example is under the gate the moment it exists.

### Budgets

RFC 012 §8 set four numbers and all four are asserted by tests, not by prose:

| Budget | Asserted in |
| --- | --- |
| Reparse + reanalyse a 1,000-line shader, ≤ 5 ms native | `wgsl-lsp-core/tests/budgets.rs` |
| The same through the wasm binding, ≤ 25 ms | `src/wasmBudget.test.ts` |
| Generated spec source ≤ 1.5 MB | `glsl-spec`'s `generated_source_is_within_budget` |
| Wasm growth ≤ +900 KB | `glsl-spec`'s `static_footprint_leaves_room_in_the_wasm_budget`, and the measured build in [research/measurements.md](../../docs/rfc/012-glsl-analyzer/research/measurements.md) |

`budgets.rs` **measures in any build and asserts only in an optimised one** — a
debug build is five to ten times slower and would be measuring the optimiser, so
`cargo test --release -p wgsl-lsp-core` is what enforces it. `wasmBudget.test.ts`
skips when `wasm/` has not been built. Both print their number, so a change that
costs 20 % shows up in the log before it ever reaches the assertion.

Fixtures mark the cursor with `|`; `support::Harness::open_at` strips it. Three
things are worth testing for every new feature, because they are where this
kind of code actually breaks:

- **a half-typed document** — see
  `every_request_survives_a_half_typed_document` and its GLSL twin;
- **all three GLSL dialects** — `tests/glsl_dialects.rs` has `ES`, `OPENGL` and
  `VULKAN` fixtures at the top and asks each question three times, because the
  whole point of RFC 012 is that the answer no longer depends on which GLSL the
  file is written in;
- **both languages** — a capability is per-server and a setting is per-language,
  so a handler must decline for a document whose own language did not ask.

## Debugging

`F5` opens the Extension Development Host. The server is a child process:

- Its log is in the *WGSL / GLSL Shader* output channel, along with the LSP
  trace when `wgslShader.trace.server` is set.
- To attach a debugger, launch the client and connect to `localhost:6019` — the
  `debug` server options in `src/lsp/client.ts` pass `--inspect`.
- `WGSL / GLSL: Restart Language Server` picks up a rebuilt `dist/server.js`
  without reloading the window. It does *not* pick up a rebuilt `wasm/`, which
  the process only loads once.

`examples/` holds a file per shape the extension handles, and every one of them
is also a test fixture — so a change that stops an example validating fails
`cargo test` long before it reaches an editor:

| File | Shape | Gated by |
| --- | --- | --- |
| `test.wgsl` | WGSL | `analysis::tests::the_shipped_wgsl_example_validates` |
| `test.vert` `test.frag` `test.comp` | Vulkan GLSL 4.50, one per stage | `glsl_dialects::the_shipped_examples_produce_no_errors`, and the outline-parity fixtures in `glsl-syntax` |
| `test-es300.frag` | GLSL ES 3.00 — precision statements, combined sampler, a macro and an `#ifdef` | the same, plus `each_shipped_example_is_analysed_as_the_dialect_it_declares` |
| `test-opengl.frag` | Desktop OpenGL 3.30 core — combined samplers, driver-assigned bindings | the same |
| `test-embedded{,-glsl}.{rs,ts}` | shaders in Rust and TypeScript string literals | `src/grammar.test.ts` |

The three `test.{vert,frag,comp}` files carry a further constraint the parity
fixtures impose: **their byte offsets are hard-coded** in
`glsl-syntax/src/tests/outline.rs`, which transcribes what the deleted heuristic
walk found. Editing them, comments included, means re-deriving spans from the
thing under test. Add a file instead; the directory-walking gates pick it up on
their own.
