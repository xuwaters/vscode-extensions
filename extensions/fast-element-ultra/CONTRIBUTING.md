# Contributing to FAST Element Ultra

Developer notes: how to build it, how it is put together, and why it is put
together that way. For what the extension does, see [README.md](README.md).

The design record is [docs/rfc/011-fast-element-ultra](../../docs/rfc/011-fast-element-ultra) —
`proposal.md`, the eleven ADRs under `decisions/`, and `design/architecture.md`,
`design/crates.md`, `design/component-model.md`, `design/rules.md`,
`design/features.md`. Comments in this extension cite them by name.

## Building

The engine is Rust compiled to WebAssembly, so a build needs a Rust toolchain
and [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) on top of the usual
`pnpm install`. From the repo root:

```sh
pnpm install
pnpm --filter wx-vsce-fast-element-ultra build:wasm   # wasm-pack → wasm/
pnpm --filter wx-vsce-fast-element-ultra build        # tsdown → dist/, then assemble
pnpm --filter wx-vsce-fast-element-ultra typecheck    # extension + test tsconfigs
pnpm --filter wx-vsce-fast-element-ultra test         # vitest
pnpm --filter wx-vsce-fast-element-ultra package      # → wx-vsce-fast-element-ultra-<version>.vsix
```

`build:wasm` first, always: `build` fails loudly if `wasm/` is missing, and the
tests skip themselves (`wasmBuilt`) rather than pretend to pass. The Rust half
has its own tests, which need no toolchain but cargo:

```sh
cargo test -p fast-template-syntax -p fast-html-data -p fast-analyzer-core
```

`pnpm --filter wx-vsce-fast-element-ultra watch` rebuilds the TypeScript on
change while the Extension Development Host is running; a change under
`crates/fast/` needs `build:wasm` again.

Two generators are run by hand, and their output is committed:

- `pnpm --filter wx-vsce-fast-element-ultra generate:htmldata` regenerates
  `crates/fast/fast-html-data/src/generated.rs` from `@vscode/web-custom-data`
  and the curated `generator/svg-data.json` (VS Code ships no SVG data, and the
  corpus is full of inline SVG).
- `pnpm --filter wx-vsce-fast-element-ultra licenses` regenerates
  [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) with `cargo about`.

## Packaging, and why it is unusual

tsserver resolves a plugin only from a probe location's
`node_modules/<pluginName>`, and VS Code probes the extension's *install*
directory. So the plugin has to be a real package directory inside the
extension, both in the working tree (for F5 sessions) and in the VSIX:

Three commands do it, all under [scripts/](scripts/README.md) and all reached
through `node scripts/main.mjs <command>`:

- `assemble-tsplugin` builds `node_modules/wx-fast-element-tsplugin/` from the
  bundle and the WASM artifact, plus a two-line `main.js` — tsserver expects
  `require(plugin)` to *be* the factory, and the bundler emits it as a default
  export. `pnpm build` runs it every time, because pnpm may prune that
  directory on install.
- `inject-tsplugin` puts that directory back into the packaged VSIX. The
  `.vscodeignore` negation cannot do it: vsce collects files with
  `ignore: 'node_modules/**'`, so nothing under `node_modules` is ever offered
  to the ignore rules. A VSIX is a zip; the command rewrites it after packaging.
- `verify-vsix` (`pnpm verify:vsix`) is the check on all of that: extract the
  VSIX to a clean directory, resolve the plugin the way tsserver's probe does,
  call the factory with the real TypeScript, run the engine. The other half —
  installing into a real VS Code and reading
  `Enabling plugin wx-fast-element-tsplugin` in the TS Server log — needs a
  desktop and stays a manual step.

[.vscodeignore](.vscodeignore) is a copy of a shared template — edit
`scripts/templates/.vscodeignore` at the repo root and run
`pnpm sync-vscodeignore`, never the copy. This extension's own additions live
in [.vscodeignore-extra](.vscodeignore-extra), which the sync appends.

## Layout

Four Rust crates under [crates/fast/](../../crates/fast), compiling to one
~575 KB WASM artifact:

| Crate | Holds |
| --- | --- |
| `fast-template-syntax` | The tolerant tokenizer and tree for interpolated HTML. Placeholders are consulted by offset, never pattern-matched, so `${…}` is recognised in content, attribute-value and attribute-name position alike |
| `fast-html-data` | HTML/SVG/MathML element, attribute and event tables generated into the binary, plus the runtime loader for VS Code custom-data JSON |
| `fast-analyzer-core` | The engine: `registry.rs` (what we know about a tag), `documents.rs` (parsed virtual documents and offset conversion), `rules.rs` (the rule pass), `ide.rs` (every position query), `config.rs` (the severity table), `protocol.rs` (the boundary types) |
| `fast-analyzer-wasm` | The wasm-bindgen surface, and the only crate that knows WASM exists |

The TypeScript side is in two halves that never mix:

| Folder | Holds |
| --- | --- |
| `src/` | The extension host, and no analysis at all: forwards `fastElementUltra.*` settings into the plugin, draws colour swatches, runs the workspace-analysis command over tsserver's protocol, shows the language status item |
| `tsplugin/` | The TypeScript server plugin. `index.ts` (the factory and the version gate), `extract.ts` (discovery — components, virtual documents, imports), `oracle.ts` (the type answers), `service.ts` (the decorated language service), `engine.ts` (the WASM wrapper), `css.ts`, `config.ts`, `protocol.ts`, `logger.ts` |

`tsplugin/protocol.ts` mirrors `fast-analyzer-core/src/protocol.rs` by hand.
The integration tests drive the real WASM through those types, so drift fails
loudly rather than silently.

## The parts that carry weight

**The engine decides nothing it cannot know.** It does no I/O and never calls
back into the host: the plugin feeds it component facts, dependency lists and
virtual documents, and it answers with diagnostics plus *binding facts* — the
questions that belong to the type checker. `oracle.ts` answers those in
TypeScript, with `checker.isTypeAssignableTo`, not with a re-implementation of
assignability. Being differently right about assignability than the compiler
the user builds with is worse than leaning on an internal API (decision 0010).

**Containment is the plugin's try/catch, not `catch_unwind`.** On
`wasm32-unknown-unknown` a panic is a trap: `panic = "abort"` compiles the
panic path to `unreachable`, so `catch_unwind` catches nothing and the call
surfaces in JS as a `RuntimeError`. `SafeEngine` counts throws and poisons the
instance on the second — a trapped instance's memory is not trustworthy — and
every decorated language-service method falls back to the undecorated one. The
end state of any engine failure is TypeScript, unmodified (decision 0011).
`debugPanic()` exists so that path is tested against the real artifact rather
than a mock.

**The virtual document is length-preserving.** Every `${…}` becomes an
underscore run of exactly the same length, so `sourceOffset = templateStart +
documentOffset` with no mapping table anywhere. That is a property, and
`test/virtualdoc.test.ts` tests it as one, over generated templates.

**UTF-16 in, UTF-16 out.** Every offset crossing the boundary is in JavaScript
string units; the engine converts to and from byte offsets exactly once, at its
own edge (`documents.rs`). The plugin never converts.

**Discovery reads the checker, never the AST alone.** The tag name of
`@customElement({ name: CSV_GRID_TAG })` comes from the *type* of the name
expression — which is what makes a `const`, an imported `const` and an
`as const` member access all work. That bug is what motivated the RFC.

**The registry merges by confidence, five levels deep** (design/component-model.md
§4): declared components, then their JSDoc facts, then VS Code custom data,
then `globalTags`/`globalAttributes`/`globalEvents`, then built-in HTML data. A
lower level fills gaps and never overrides a higher one.

**`css` never enters the Rust engine** (decision 0005). It goes to
`vscode-css-languageservice` in the substituted text, so offsets map back with
`+ templateStart`. Note the deep ESM import path in `css.ts`: the package's
`main` is a UMD build whose internal relative requires survive bundling and
then fail at runtime.

**The plugin bundle must never carry `typescript`.** tsserver passes its own
module to the factory. [tsdown.config.mts](tsdown.config.mts) keeps it and
`vscode` external, and bundles everything else — `vsce package
--no-dependencies` ships no `node_modules` of its own.

## Tests

`pnpm test` runs vitest over a real `ts.LanguageService` on in-memory fixtures,
the real WASM engine and the plugin's own code paths — no tsserver, no mocks.
Fixtures resolve `@microsoft/fast-element` to the genuine installed package, so
decorators and directive types are the real ones (`test/harness.ts`).

- **The corpus gate** (`corpus.test.ts`) — this repo's own FAST extensions are
  the permanent regression corpus: every element discovered, and the full rule
  set with `strict` on producing *silence* over every template.
- **Discovery** (`discovery.test.ts`) — every registration and member
  declaration form, inheritance, `$emit`, JSDoc facts.
- **Diagnostics** (`diagnostics.test.ts`) — a seeded mistake of each rule's
  kind, reported at the right place with the right message.
- **Features** (`features.test.ts`) — the position features through the
  decorated language service; `ref('…')` completion and member rename reaching
  template strings are tested against csv-ultra's real files.
- **The parser differential** (`parser-differential.test.ts`) — our tree against
  parse5's, over the corpus and an adversarial set. Decision 0003 rests on it,
  and the deliberate divergences are enumerated as expectations of *our*
  behaviour, not silently allowed.
- **Containment and the assembled plugin** (`smoke.test.ts`) — panic the real
  artifact on purpose, then check that TypeScript's own features still work.
- **The virtual document** (`virtualdoc.test.ts`), and the Rust-side tests under
  `crates/fast/`.
- **Measurements** (`measure.test.ts`) are opt-in —
  `FAST_MEASURE=1 pnpm vitest run test/measure.test.ts` — because numbers belong
  in `research/`, produced on purpose rather than silently in every run.
