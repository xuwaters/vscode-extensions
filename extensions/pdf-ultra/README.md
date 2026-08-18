# PDF Ultra

A PDF viewer that is an editor, not an attachment. Open a `.pdf` and it renders
in the tab — continuous pages, real selectable text, find, an outline, and a
reload that keeps your place when the file is rebuilt beside you.

Built on [pdf.js](https://mozilla.github.io/pdf.js/), rendered by a
[FAST](https://fast.design) element, and entirely offline: no CDN, no web
fonts, no telemetry, no remote origin of any kind.

## What it does

**Reading**

- Continuous scrolling column, virtualized — a 500-page document costs about
  what a 5-page one costs, because only the pages within a screen of the
  viewport hold a raster
- A real text layer, so text selects, copies, and reads out to a screen reader
- Find in document, with every match on the page highlighted and `Enter` /
  `Shift+Enter` stepping through them
- Outline sidebar, collapsible and resizable, that follows where you are
- Link annotations work: internal ones jump within the document, external ones
  are handed to the host — the page itself never navigates
- Zoom by step, by typed percentage (`150`, `150%`, `1.5x` all read the same),
  fit-width, fit-page, or 100%
- Rotation, and colour inversion for reading white pages in a dark editor
- Page numbers, `PageUp`/`PageDown`/`Home`/`End`, and the page you are on in the
  status bar

**Living beside a build**

A PDF is usually an output. Change the file on disk — recompile the LaTeX, run
the Typst export, regenerate the report — and the tab reloads on the page you
were reading, at the zoom you were reading it at. Writes are debounced, so a
producer that rewrites the file in several passes does not flash an error at
you halfway through.

**Reopening**

Close a document and open it later and it comes back on the page you left it,
per workspace. Turn that off with `pdfUltra.rememberPosition`.

**Exporting** the current page as a PNG, at twice actual size, through a save
dialog. That is the extension's only write path, and it writes a new file — the
viewer is read-only by construction, so nothing it does can touch the document
it is showing.

## Getting started

Open a PDF. That is the whole setup — the extension registers itself as the
default editor for `*.pdf`, so nothing needs configuring and nothing else needs
installing. Reopen With gets you back to any other viewer you have.

## Commands and keys

| Command | Key |
| --- | --- |
| Find in Document | `Cmd/Ctrl+F` |
| Go to Page… | `Cmd/Ctrl+G` |
| Zoom In / Out / 100% | `Cmd/Ctrl+=` / `Cmd/Ctrl+-` / `Cmd/Ctrl+0` |
| Toggle Outline | `Cmd/Ctrl+K Cmd/Ctrl+O` |
| Toggle Colour Inversion | `Cmd/Ctrl+K Cmd/Ctrl+I` |
| Reload Document | `Cmd/Ctrl+K Cmd/Ctrl+R` |

Also in the palette, under **PDF Ultra**: next/previous page, fit width, fit
page, rotate either way, export the current page as a PNG, and open the file in
whatever the operating system uses for PDFs.

## Settings

| Setting | Default | What it does |
| --- | --- | --- |
| `pdfUltra.defaultZoom` | `fit-width` | Zoom a document opens at |
| `pdfUltra.background` | `editor` | Colour behind the pages |
| `pdfUltra.invertColors` | `never` | `always`, or `auto` to follow the theme |
| `pdfUltra.textLayer` | `true` | Selection, find, and screen-reader text |
| `pdfUltra.links` | `true` | Make link annotations clickable |
| `pdfUltra.outline.visible` | `false` | Show the outline by default |
| `pdfUltra.outline.width` | `240` | Sidebar width; drag its edge to change |
| `pdfUltra.reloadOnChange` | `true` | Reload when the file is rewritten |
| `pdfUltra.rememberPosition` | `true` | Reopen on the page last read |
| `pdfUltra.maxCanvasPixels` | `16777216` | Ceiling on one page's bitmap |
| `pdfUltra.renderAhead` | `1` | Screens rendered either side of the viewport |

## How it is put together

`src/` is the extension host: the custom editor, the commands, the status bar,
the file watch, and the host half of the message protocol.

`webview/` is the page, in three layers that only ever depend downwards:

| Layer | Rule | Holds |
| --- | --- | --- |
| `viewer/` | Reactive. Everything the reader can see the state of. | `element.ts` and its `template.ts` / `styles.css`, plus the two controllers it delegates to — `search.ts` and `outlineState.ts` |
| `render/` | pdf.js and the DOM, but no state the reader sees. | `pageColumn.ts` (the virtualized column), `pdfjs.ts` (loader, worker boot, document parameters), `destinations.ts`, `highlight.ts` |
| `model/` | No DOM, no pdf.js. Pure functions — and where most of the tests are. | `layout.ts`, `find.ts`, `outline.ts`, `zoom.ts`, `chunks.ts` |

The two bundle entry points sit at the top of `webview/`, so what gets built is
answerable without opening anything: `index.ts` is the page, `pdfWorker.ts` is
the pdf.js worker.

The split between the element and the column is the load-bearing one. The
*chrome* is reactive and lives in a FAST template; the *pages* are not. A page
column is hundreds of boxes of which a handful hold a canvas at any moment,
rasterized and released as you scroll, and expressing that as bindings would
mean a binding per page and a canvas per binding. So `PageColumn` owns the
column imperatively, models its own scroll geometry rather than measuring the
DOM back, and the element owns everything you can see the state of.

### Security

A PDF is an untrusted document that can come from anywhere, and pdf.js is a
large parser sitting between it and the page. So:

- `default-src 'none'`, and no remote origin is permitted at all
- No inline or injected script can run — `script-src` is a nonce and
  `'wasm-unsafe-eval'`, which lets `WebAssembly.instantiate` compile pdf.js's
  image decoders without re-enabling `eval`
- XFA is off, no scripting layer exists, and pdf.js's annotation layer is not
  mounted — link annotations are read out and rebuilt as our own overlay, so
  every link out goes through the host's scheme allow-list
- `quickjs-eval.wasm`, pdf.js's interpreter for document scripting, is left out
  of the package: shipping a JavaScript engine nothing can reach is attack
  surface for no feature
- Every message from the webview is shape-validated host-side, one hand-written
  guard per variant

### Where pdf.js's data lives

`dist/pdfjs/` carries the cMaps (CID-keyed fonts, which is most CJK), the 14
standard fonts (for documents that embed none), and the JBIG2 / JPEG 2000 /
ICC decoders. They are copied into the bundle at build time and loaded from the
extension's own URL, never from a CDN.

## Building

```sh
pnpm install
pnpm --filter wx-vsce-pdf-ultra build
pnpm --filter wx-vsce-pdf-ultra test
pnpm --filter wx-vsce-pdf-ultra package   # → wx-vsce-pdf-ultra-<version>.vsix
```
