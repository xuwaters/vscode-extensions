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

**Bibliographies** — `.bib` files get the same treatment as `.typ` files

- Diagnostics as you type: duplicate keys, unclosed entries, missing required
  fields, entry types typst will not recognise
- Entries as the document outline and as workspace symbols, so `knuth1984` is
  one `Cmd/Ctrl+T` away
- Completion for entry types (as fill-in skeletons), field names, `crossref`
  keys, and `@string` abbreviations
- Hover on an entry, a field, or a citation; `url` and `doi` fields are links
- A formatter that puts a bibliography in canonical shape and refuses to touch
  one that does not parse
- In your document: `@knuth1984` hovers, jumps to the entry, and renames across
  the document and the bibliography at once — before the first compile, and
  before `bibliography()` is even written

**Preview**

- Repaints as you type, rendering pages as SVG
- Three view modes — Edit, Split, Preview — from the editor title bar, the
  status bar, or `Cmd/Ctrl+Shift+V`
- Follows the active editor: click a second `.typ` and the preview switches to it
- Two-way sync: cursor → page position, click on a page → source position
- Virtualized — a 200-page document keeps the DOM bounded
- Zoom, colour inversion, page numbers, find-in-page
- Fit width and fit page are modes, not one-shots: they stay switched on and
  follow the panel as it is resized, until you set the zoom by hand
- A compile error dims the last good pages rather than blanking the preview

**Export** to PDF, SVG, PNG, and HTML, all in-process — from the title-bar
button, the preview's own toolbar, or the palette. The save dialog opens on the
document's own folder, so exporting next to the source is one keystroke.

**Packages** from [Typst Universe](https://typst.app/universe) are downloaded
and cached in typst's standard cache directory, shared with `typst-cli` so
nothing is fetched twice.

**Fonts**: typst's default set ships with the extension, so output matches
`typst compile` out of the box. System fonts are indexed in the background.

## Getting started

Open a `.typ` file. That is the whole setup — the server starts on the first
typst document, and the compile root follows whichever file you are looking at.

A `.bib` file in the same workspace is picked up as well, and editing one
recompiles the document that cites it rather than trying to compile the
bibliography. Opening a `.bib` in a workspace with no typst files in it does
nothing at all — a LaTeX project's bibliography is none of this extension's
business.

For a project with chapters, pin the entry file so editing a chapter still
checks the whole document: click the status bar item, or run
**Typst Ultra: Pin This File as Compile Root**. The extension offers this once, the
first time it sees a project with a `main.typ`.

`Cmd/Ctrl+Shift+V` puts the preview beside the editor; press it again from
either side and you are still in Split. The editor title bar carries the same
three modes as buttons, plus Export.

## View modes

| Mode | Layout | Entered by |
| --- | --- | --- |
| **Edit** | the text editor only | closing the preview, or the `$(edit)` button |
| **Split** | editor and preview side by side | `Cmd/Ctrl+Shift+V`, `Cmd/Ctrl+K V` |
| **Preview** | the document alone, in the tab the source was in | the `$(preview)` button |

The preview follows whichever `.typ` you are looking at. Pin it with
`Cmd/Ctrl+K Cmd/Ctrl+L` (from inside the preview) to keep it on one file while
you edit another. The group the side preview opens in is locked, so files you
open next land in the main group rather than on top of the preview
(`preview.lockPreviewGroup`).

## Commands

All under the **Typst Ultra** category.

| Command | Default keybinding |
| --- | --- |
| Typst Ultra: Switch to Split View | `Cmd/Ctrl+Shift+V` |
| Typst Ultra: Open Preview to the Side | `Cmd/Ctrl+K V` |
| Typst Ultra: Toggle Focus Between Editor and Preview | `Cmd/Ctrl+K Cmd/Ctrl+V` |
| Typst Ultra: Cycle View Mode (Edit / Split / Preview) | `Cmd/Ctrl+K Cmd/Ctrl+M` |
| Typst Ultra: Toggle Edit / Preview View | `Cmd/Ctrl+K Cmd/Ctrl+P` |
| Typst Ultra: Toggle Preview Pin | `Cmd/Ctrl+K Cmd/Ctrl+L` (in the preview) |
| Typst Ultra: Sync Preview to Cursor | `Cmd/Ctrl+K Cmd/Ctrl+J` |
| Typst Ultra: Open Preview | |
| Typst Ultra: Switch to Edit View / Preview View / Switch View Mode… | |
| Typst Ultra: Export… | |
| Typst Ultra: Export PDF | |
| Typst Ultra: Pin This File as Compile Root | |
| Typst Ultra: Unpin Compile Root | |
| Typst Ultra: Select Compile Root… | |
| Typst Ultra: Toggle Preview Color Inversion | |
| Typst Ultra: Restart Language Server | |
| Typst Ultra: Show Log | |
| Typst Ultra: Clear Package Cache | |

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
| `preview.defaultMode` | `"split"` | `split` · `preview` |
| `preview.lockPreviewGroup` | `true` | Keep newly-opened files out of the preview's group |
| `export.outputPath` | `"$dir/$name"` | Supports `$dir`, `$name`, `$root` — where the export dialog opens |
| `export.askForLocation` | `true` | Ask where to save; off writes straight to `export.outputPath` |
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


## What it deliberately does not do

No DAP debugging, coverage, profiling, `typst test`, symbol picker, font
browser, `tinymist.lock` project resolution, LaTeX import, or editing from the
preview. [tinymist](https://github.com/Myriad-Dreamin/tinymist) does all of
those and does them well; it also maintains a patched fork of the typst compiler
and ships a per-platform binary. This is the ~20% of the surface that covers
~95% of daily editing, on a maintenance base that stays cheap.

## License

`NO LICENSE`. The third-party notices for the typst compiler, typstyle, the
bundled fonts, and the Rust dependency graph are in `LICENSE.md` and
`THIRD-PARTY-NOTICES.md`, both shipped in the package.
