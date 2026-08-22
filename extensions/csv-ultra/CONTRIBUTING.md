# Contributing to CSV Ultra

Developer notes: how to build it, how it is put together, and why it is put
together that way. For what the extension does, see [README.md](README.md).

## Building

From the repo root:

```sh
pnpm install
pnpm --filter wx-vsce-csv-ultra build       # tsdown → dist/
pnpm --filter wx-vsce-csv-ultra typecheck   # host and webview tsconfigs
pnpm --filter wx-vsce-csv-ultra test        # vitest
pnpm --filter wx-vsce-csv-ultra package     # → wx-vsce-csv-ultra-<version>.vsix
```

`pnpm --filter wx-vsce-csv-ultra watch` rebuilds on change while the Extension
Development Host is running.

Packaging is driven by [.vscodeignore](.vscodeignore), which is a copy of a
shared template — edit `scripts/templates/.vscodeignore` at the repo root and
run `pnpm sync-vscodeignore`, never the copy.

TypeScript throughout, bundled by [tsdown.config.mts](tsdown.config.mts) into
two artifacts: the extension host (CommonJS, Node) and the webview (ESM,
browser). One runtime dependency,
[`@microsoft/fast-element`](https://github.com/microsoft/fast), bundled in —
`vsce package --no-dependencies` ships no `node_modules`.

## Layout

`src/` is the extension host, and also holds the parts shared across the webview
boundary:

| Folder | Holds |
| --- | --- |
| `src/csv/` | The format. `parse.ts` (RFC 4180 and its tolerances), `serialize.ts` (quoting), `dialect.ts` (the sniffer), `values.ts` (column labels, number and date typing, sort order, header detection, block summaries) |
| `src/document/` | `table.ts` (the parse cache), `edits.ts` (pure offset arithmetic — which bytes an edit overwrites), `dialectFor.ts` |
| `src/editor/` | `provider.ts` (custom editor, commands, keybinding contexts), `session.ts` (one open tab), `html.ts` (the page and its CSP), `layoutMemory.ts` |
| `src/text/` | The rainbow. `rainbow.ts` (decorations and the status bar), `paint.ts` (pure — which colour each field gets, as offsets) |

`src/messages.ts` is the protocol, imported by both sides, so a change to one is
a type error in the other.

`webview/` is the page, in three layers that only ever depend downwards:

| Layer | Rule | Holds |
| --- | --- | --- |
| `viewer/` | Reactive. Everything the reader can see the state of, and every decision about what a gesture means. | `element.ts` and its `template.ts` / `styles.css` |
| `render/` | The DOM, but no state the reader sees. | `sheet.ts` — the scrolling body: a few hundred recycled boxes over a table of any size. It reports gestures; it decides nothing. |
| `model/` | No DOM. Pure functions — and where most of the tests are. | `selection.ts`, `metrics.ts` (virtualization arithmetic), `view.ts` (find), `clipboard.ts` |

`webview/index.ts` is the bundle's only entry point and the wire between the
element and the host. Keeping `acquireVsCodeApi` out of the element is what lets
it be mounted in a test.

## The parts that carry weight

**One parser, both sides.** `src/csv/` is imported by the host *and* by the
webview, so the table on screen and the bytes on disk cannot disagree about what
the file says. It lives under `src/` rather than a third folder because that is
the path the packaging step already knows to leave out of the VSIX.

**The page never writes text.** It says "row 4, column 2 now holds this";
`edits.ts` decides which bytes that is, and the session turns those offsets into
a `WorkspaceEdit`. So undo, redo, dirty state, save, hot exit, revert and the
diff view are all VS Code's own, and none of them is reimplemented here. It also
means a one-cell edit rewrites one record and leaves every other byte — the
other rows' quoting, the line endings, the trailing newline, the raggedness —
exactly where it was.

**Every message from the webview is shape-validated host-side**, one
hand-written guard per variant, ranges as well as types. A webview is a hostile
input boundary even when we wrote the far side, and this one has a *write* on
it: a row index off by a thousand is not a rendering bug, it is data loss.

**Nothing is laid out by the browser.** `metrics.ts` is the reason a million-row
file scrolls: the body is one box the size of the whole table with a few hundred
absolutely-positioned cells inside it. Columns keep a dense prefix sum — there
are never enough of them for it to matter — and rows are `index × height`
corrected by the handful dragged to a size of their own, because a prefix sum
over two million rows would be 16 MB of numbers rebuilt on every drag.

**Chrome is reactive, cells are not.** The toolbar, menus, find bar and footer
live in a FAST template; the cells are built and positioned imperatively by
`Sheet` every frame. Expressing recycled boxes as bindings would mean a binding
per cell.

**`TableCache` is keyed by the document's version.** Two things want the parse
and neither can afford to redo it — the session, turning a cell edit into an
offset, and the rainbow decorator, running on every scroll of every visible text
editor. A changed document *is* a different key, so there is no invalidation to
get wrong. The text itself is deliberately not cached.

**`csvUltra.editing` is a keybinding context, not a nicety.** VS Code forwards
every keystroke a webview sees to its own keybinding resolver whatever the page
does with the event, so `⌘Z` while typing in a cell would undo the last
committed edit instead of the last character. A page cannot decline on its own
behalf; it can only say so, and the contributed bindings stand down on it.

## The webview tsconfig

`@microsoft/fast-element` ships *legacy* decorators, so `webview/tsconfig.json`
turns `experimentalDecorators` on and the webview bundle is built against it.
Under the root tsconfig the transform passes `@customElement` through as a
standard decorator — syntax no engine parses. Every source test still passes,
because they import the TypeScript; the bundle just fails to load as a whole and
the tab is an empty `<body>` with no `<csv-grid>` in it. `src/bundle.test.ts` is
the guard: it parses the built `dist/webview.js` the way the page does, and
skips on a checkout that has not run a build.

`html.ts` writes `<csv-grid>` into the markup rather than constructing it from
the bootstrap for a related reason — fast-element 3 defines a custom element
asynchronously, so constructing the class the moment its module evaluates gets
`Illegal constructor`. The HTML parser has no such problem.

## Security

`default-src 'none'`, `script-src` a per-load nonce, and no remote origin is
permitted at all. Nothing in the page loads anything: the file's text arrives
over `postMessage` and the grid is drawn from it — no image, no font, no fetch,
no worker. The one concession is `'unsafe-inline'` for styles, which is what a
shadow root's adopted stylesheet needs.

A cell holding `<script>` or `javascript:` is text in a `textContent`, never
markup; the policy is the second line of defence rather than the first.

## Tests

`pnpm test` runs vitest over both halves:

- The RFC 4180 parser and its tolerances, the writer's quoting, the delimiter
  sniffer, and value typing and sort order (`src/csv/`)
- The edit engine, asserting on the *file* each edit produces rather than on
  offsets (`src/document/edits.test.ts`)
- The protocol guards (`src/messages.test.ts`) and layout eviction
  (`src/editor/layoutMemory.test.ts`)
- The rainbow's paint plan (`src/text/paint.test.ts`)
- The virtualization arithmetic, the selection model, find and the clipboard
  (`webview/model/`)
- `<csv-grid>` itself, mounted under happy-dom
  (`webview/viewer/element.test.ts`)
- The built bundles, when there are any (`src/bundle.test.ts`)
