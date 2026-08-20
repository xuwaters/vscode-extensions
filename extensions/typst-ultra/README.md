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
  math, parameters, imports, labels, and packages — including the arguments a
  package takes through a `..sink` and documents in a doc comment, which no
  signature records (cetz's `circle(.., radius: 2, fill: red)`)
- Hover, with the page number a label resolves to, and a package's own doc
  comment in full — parameters, examples, and style keys, not just its first
  sentence
- Goto-definition for local bindings, imports, package items, and labels — from
  `@intro` and from `<intro>` used as a value in code alike
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
- Zoom, colour inversion, find-in-page
- A page box you can type into, and arrow keys,
  `PageUp`/`PageDown`/`Home`/`End` to turn the page from the keyboard
- Fit width and fit page are modes, not one-shots: they stay switched on and
  follow the panel as it is resized, until you set the zoom by hand
- A compile error dims the last good pages rather than blanking the preview

**Snippets** — 84 of them, for people who have not written Typst
before: whole-document scaffolds (`article`, `report`, `book`, `letter`,
`slides`), set and show rules, figures, tables, math, and the control flow.
Every one is compiled by the real typst in the test suite, so a snippet cannot
ship broken.

**Code in your document, in its own colours** — a raw block that names a
language is coloured by that language's grammar, the same one the rest of your
editor uses:

````typst
```rust
fn main() { println!("hi"); }
```
````

73 languages and 149 tags, aliases included, so `` ```rs ``, `` ```c++ `` and
`` ```Python `` all land. The block is a real embedded region, not just paint:
`Cmd/Ctrl+/` inside it comments the way that language comments. Tags nothing
recognises stay raw-coloured as before, and a handful of languages VS Code does
not bundle a grammar for — `toml`, `kotlin`, `zig`, `wgsl`, `proto` — light up
as soon as an extension for them is installed.

**Export** to PDF, SVG, PNG, and HTML, all in-process — from the title-bar
button, the preview's own toolbar, or the palette. The save dialog opens on the
document's own folder, so exporting next to the source is one keystroke.

**Packages** from [Typst Universe](https://typst.app/universe) are downloaded
and cached in typst's standard cache directory, shared with `typst-cli` so
nothing is fetched twice.

**Fonts**: typst's default set ships in a companion extension,
`weixu.wx-vsce-typst-ultra-fonts`, so output matches `typst compile` out of the
box while a code release stays 6.4 MB smaller. It is a declared dependency, so a
gallery install pulls it in automatically — and installing this VSIX by hand
means installing that one too, because VS Code will not activate an extension
whose dependency is missing. System fonts are indexed in the background either
way.

## Getting started

Open a `.typ` file. That is the whole setup — the server starts on the first
typst document, and the compile root follows whichever file you are looking at.

A `.bib` file in the same workspace is picked up as well, and editing one
recompiles the document that cites it rather than trying to compile the
bibliography. Opening a `.bib` in a workspace with no typst files in it does
nothing at all — a LaTeX project's bibliography is none of this extension's
business.

For a project of several files — an entry point plus its data, template, and
chapters — say which file is the document: click the status bar item, or run
**Typst Ultra: Pin This File as Compile Root**. Editing any of the other files
then still checks and previews the whole thing, instead of compiling a page of
`#let` bindings on its own and showing you a blank preview. `typstUltra.mainFile`
is the same answer written down, so a project can check it into
`.vscode/settings.json` and everyone who clones it gets it. The extension offers
to settle this for you once per workspace: when it first sees a project with a
`main.typ`, or when a preview compiles cleanly to no pages at all.

`Cmd/Ctrl+Shift+V` puts the preview beside the editor; press it again from
either side and you are still in Split. The editor title bar carries the same
three modes as buttons, plus Export.

### If you have never written Typst

Type `article` in an empty `.typ` file and press `Tab`. You get a complete
document — page size, fonts, a title block, a first section — with the parts
you have to fill in already selected; `Tab` walks you through them. `report`,
`book`, `letter`, and `slides` are the other scaffolds.

From there the snippets follow the shape of the language, and the description
beside each one says what it is for:

| You want | Type |
| --- | --- |
| A figure with a caption you can reference | `figure` |
| A table | `table`, or `figuretable` for a numbered one |
| Maths | `mathblock`, then `frac`, `sum`, `integral`, `mat`, `cases` |
| A bibliography and a citation | `bibliography`, `cite` |
| Page size, fonts, justification | `setpage`, `settext`, `setpar` |
| To restyle every heading or code block at once | `showheading`, `showraw` |
| Chapters in separate files | `book`, `include` |
| A note box, a callout | `callout` |
| A package from Typst Universe | `import` |

The suggestion list stays out of the way while you write prose: it does not
open on its own as you type words, only when you ask for it with `Ctrl+Space`
(`⌃Space`) — or when you type one of the characters that starts real Typst
code, `#`, `@`, `.`, `/`, `:`, `$`, `"`. `Tab` accepts a completion; `Enter`
always means a new line, so the list never eats a paragraph break.

Both are per-language defaults — `editor.quickSuggestions` and
`editor.acceptSuggestionOnEnter`. Override them in your settings under
`"[typst]"` if you prefer the VSCode behaviour.

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

### Reading with the keyboard

In the pages themselves: `←` / `→` and `PgUp` / `PgDn` turn the page, `Home` and
`End` go to the first and last. `←` / `→` give way to the scroller when the page
is zoomed wider than the tab, because otherwise there would be no way to reach
its right-hand edge without a mouse — the same rule `pdf-ultra` follows. Zoom is
`Cmd/Ctrl+=` / `Cmd/Ctrl+-`, and `Cmd/Ctrl+0` is 100%; those work wherever the
focus is in the preview, including the toolbar's boxes.

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
| `mainFile` | `""` | Compile and preview entry point, workspace-relative. Empty follows the focused editor |
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

The TextMate grammar is deliberately minimal — it only has to make a file look
like Typst in the moments before the first semantic-token response (decision
0007). Embedded raw blocks are the one part of it that does real work, and the
one part that is generated: `scripts/embedded/languages.json` is the table of
languages, `pnpm run build:grammar` expands it into
`syntaxes/typst-embedded.tmLanguage.json` and into the `embeddedLanguages` map
in `package.json`, and the test suite fails if either is stale (decision 0013).

It ships as two packages. This one is 10.4 MB, almost all of it the WASM
artifact, which moves whenever the Rust does. The fonts are another 6.4 MB that
move only when upstream typst-assets does, so they are a separate VSIX that this
one names in `extensionDependencies` — a typo fix no longer reships them
(decision 0012).


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
