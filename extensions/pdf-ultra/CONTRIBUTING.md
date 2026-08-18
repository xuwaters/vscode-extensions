# Contributing to PDF Ultra

Developer notes: how to build it, how it is put together, and why it is put
together that way. For what the extension does, see [README.md](README.md).

## Building

From the repo root:

```sh
pnpm install
pnpm --filter wx-vsce-pdf-ultra build       # tsdown → dist/
pnpm --filter wx-vsce-pdf-ultra typecheck   # host and webview tsconfigs
pnpm --filter wx-vsce-pdf-ultra test        # vitest
pnpm --filter wx-vsce-pdf-ultra package     # → wx-vsce-pdf-ultra-<version>.vsix
```

`pnpm --filter wx-vsce-pdf-ultra watch` rebuilds on change while the Extension
Development Host is running.

Packaging is driven by [.vscodeignore](.vscodeignore), which is a copy of a
shared template — edit `scripts/templates/.vscodeignore` at the repo root and
run `pnpm sync-vscodeignore`, never the copy.

## Layout

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

## Security

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

Exporting a page as a PNG is the extension's only write path, and it writes a
new file through a save dialog. The viewer is read-only by construction.

## Where pdf.js's data lives

`dist/pdfjs/` carries the cMaps (CID-keyed fonts, which is most CJK), the 14
standard fonts (for documents that embed none), and the JBIG2 / JPEG 2000 /
ICC decoders. They are copied into the bundle at build time by
[tsdown.config.mts](tsdown.config.mts) and loaded from the extension's own URL,
never from a CDN.
