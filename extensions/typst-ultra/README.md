# Typst Ultra

A [Typst](https://typst.app) language server and live preview, built on
**unmodified upstream typst** and shipped as **one WASM artifact** that works
identically on every platform — no per-platform binary, no install-time
download, no code signing.

## What it does

**Language features**

- Diagnostics as you type, including errors inside imported files, filed under
  the right URI with the `#import` chain attached
- Completion — 181 items at a bare cursor, context-aware across code, markup,
  math, parameters, imports, labels, and packages
- Hover, with the page number a label resolves to
- Goto-definition for local bindings, imports, and package items
- Find references and rename, for labels and local bindings
- Document and workspace symbols, nested by heading level
- Semantic tokens from the real parser — 22 tags, `full` and `full/delta`
- Folding ranges, selection ranges, and document links
- Formatting and range formatting via `typstyle`
- Signature help, inlay hints, code actions, and code lenses

**Preview**

- Repaints as you type, rendering pages as SVG
- Two-way sync: cursor → page position, click on a page → source position
- Virtualized — a 200-page document keeps the DOM bounded
- Zoom, fit-width/page, colour inversion, page numbers, find-in-page
- A compile error dims the last good pages rather than blanking the preview

**Export** to PDF, SVG, and PNG, all in-process.

**Packages** from [Typst Universe](https://typst.app/universe) are downloaded
and cached in typst's standard cache directory, shared with `typst-cli` so
nothing is fetched twice.

**Fonts**: typst's default set ships with the extension, so output matches
`typst compile` out of the box. System fonts are indexed in the background.

## Getting started

Open a `.typ` file. That is the whole setup — the server starts on the first
typst document, and the compile root follows whichever file you are looking at.

For a project with chapters, pin the entry file so editing a chapter still
checks the whole document: click the status bar item, or run
**Typst: Pin This File as Compile Root**. The extension offers this once, the
first time it sees a project with a `main.typ`.

`Cmd/Ctrl+K V` opens the preview beside the editor.

## Commands

| Command | Default keybinding |
| --- | --- |
| Typst: Show Preview | `Cmd/Ctrl+Shift+V` |
| Typst: Show Preview to the Side | `Cmd/Ctrl+K V` |
| Typst: Sync Preview to Cursor | `Cmd/Ctrl+K Cmd/Ctrl+J` |
| Typst: Pin This File as Compile Root | |
| Typst: Unpin Compile Root | |
| Typst: Export… | |
| Typst: Export PDF | |
| Typst: Toggle Preview Color Inversion | |
| Typst: Restart Language Server | |
| Typst: Show Log | |
| Typst: Clear Package Cache | |

## Settings

All under `typstUltra.`. The ones worth knowing about:

| Setting | Default | |
| --- | --- | --- |
| `mainFile` | `""` | Compile entry point. Empty follows the focused editor |
| `compile.when` | `"onType"` | `onType` · `onSave` · `never` |
| `compile.debounce` | `150` | ms of quiet before recompiling |
| `fonts.system` | `true` | Index installed fonts in the background |
| `packages.enabled` | `true` | Turn off for air-gapped setups |
| `preview.scrollSync` | `"both"` | Which way the editor and preview follow each other |
| `preview.invertColors` | `"never"` | `never` · `always` · `auto` |
| `export.outputPath` | `"$dir/$name"` | Supports `$dir`, `$name`, `$root` |
| `memory.evictAge` | `1` | comemo cache age — see below |

`memory.evictAge` deserves a note, because `1` looks wrong next to typst-cli's
`10`. Measurement found it is not a latency-versus-memory trade-off at all: at
75 pages, age 1 gives a p95 of 18 ms and a 106 MB heap, against age 10's 278 ms
and 394 MB. Age 1 still retains everything the previous compile touched, which
is exactly the working set incremental recompilation needs.

## How it is built

The engine is four Rust crates under `crates/typst/`, compiled to one
`wasm32-unknown-unknown` artifact and run in a Node child process as a language
server:

| Crate | |
| --- | --- |
| `typst-session` | `World` implementation, VFS, fonts, packages, compile lifecycle |
| `typst-lsp-core` | LSP dispatch and every IDE feature |
| `typst-preview-core` | Per-page SVG, hashing and diffing, jump mapping |
| `typst-lsp-wasm` | The `wasm-bindgen` surface — the only crate that knows WASM exists |

It depends on **published typst 0.15.1 crates with no patches**, which is the
constraint that makes following upstream releases a version bump rather than a
rebase of a fork.

The server runs as a child process rather than inside the extension host,
which is a departure from the other Rust-backed extensions in this repo: a cold
compile blocks for up to 262 ms, the WASM heap is never returned to the OS, and
a compiler panic would otherwise take down every extension in the window.

### Building

```sh
pnpm run build:wasm    # cargo + wasm-pack → wasm/  (~19 MB, one artifact)
pnpm run build:fonts   # typst's default font set → assets/fonts/  (9.2 MB)
pnpm run build         # tsdown → dist/ (host, server, webview)
pnpm test              # vitest
cargo test             # the engine, natively, with no WASM toolchain needed
```

`pnpm run package` does all of it and produces the VSIX.

## What it deliberately does not do

No DAP debugging, coverage, profiling, `typst test`, symbol picker, font
browser, `tinymist.lock` project resolution, LaTeX import, or editing from the
preview. [tinymist](https://github.com/Myriad-Dreamin/tinymist) does all of
those and does them well; it also maintains a patched fork of the typst compiler
and ships a per-platform binary. This is the ~20% of the surface that covers
~95% of daily editing, on a maintenance base that stays cheap.

The full reasoning, including every measurement behind these choices, lives in
the repository under `docs/rfc/010-typst-ultra/`.

## License

`NO LICENSE`. The third-party notices for the typst compiler, typstyle, the
bundled fonts, and the Rust dependency graph are in `LICENSE.md` and
`THIRD-PARTY-NOTICES.md`, both shipped in the package.
