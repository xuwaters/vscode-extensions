# RFC 010 — Preview and Custom Editor

The live preview: how pages get from the compiler to the screen, how the two sides stay in sync, and why
the numbers in [spike.md §7](../research/spike.md#7-page-svg-anatomy) force a virtualized design.

---

## 1. The model

One preview panel that follows the active `.typ` editor, with a lock command — the same model as
[markdown-preview-ultra](../../009-markdown-preview-ultra/proposal.md#61-view-modes), which is already proven
in this repo. Multiple simultaneous previews are explicitly not supported.

| Surface | Contribution | When |
| --- | --- | --- |
| Preview panel | `WebviewPanel` `typstUltra.preview`, beside or in the editor's column | `typstUltra.showPreview` / `showPreviewToSide`, Split mode |
| Preview editor | `typstUltra.editor`, `priority: "option"` for `.typ` | Preview mode asks for it by name; or the user opts in via `workbench.editorAssociations` |
| Serializer | `WebviewPanelSerializer` | Panel survives window reload |

The preview editor is a `CustomTextEditorProvider` in read-only mode: it renders the same webview against
the document, so a `.typ` can be opened as a finished document with no flash of source. We register it at
`priority: "option"` and **never write `workbench.editorAssociations` ourselves** — the global-settings
mutation that MPE does is the anti-pattern RFC 009 called out, and it applies here too. Preview mode
reaches it by naming the view type in `vscode.openWith`, which needs no setting at all.

### View modes ([Amendment 001](../proposal-amendment-001-preview-ux.md))

Three modes, derived from the layout rather than stored — a stored mode goes stale the moment a tab is
dragged. The rule is [`modeState.ts`](../../../../extensions/typst-ultra/src/preview/modeState.ts), which
is `vscode`-free and tested; [`modes.ts`](../../../../extensions/typst-ultra/src/preview/modes.ts) drives
it, the title bar, and the status bar.

| Mode | Layout | Entered by |
| --- | --- | --- |
| **Edit** | text editor only | `setModeEdit`, closing the preview |
| **Split** | editor + preview beside, the preview's group locked | **`cmd+shift+v`**, `cmd+k v`, `setModeSplit` |
| **Preview** | the preview *in the tab the source was in* | `setModePreview` |

Transitions reuse what is on screen: with a panel open, `revealPanel` moves it between columns without
reloading the webview; Edit ⇄ Preview swaps the editor *inside* the tab through Reopen With
([`editors.ts`](../../../../extensions/typst-ultra/src/preview/editors.ts)), so the tab bar is the same
width either side of a switch. The page the reader was on is carried across by
[`pageMemory.ts`](../../../../extensions/typst-ultra/src/preview/pageMemory.ts), shared by both surfaces
and applied once per document — not on every compile, which would snap the view to a page boundary on
every keystroke.

**Following the editor takes three parts, not one.** The panel retargets, *and* the host sends
`typst/compile { uri }` so the server's subject follows too — it otherwise follows the last edit, and
clicking between two open files produces none — *and* the webview drops its pages and scroll position when
`metrics` arrives carrying a different URI. The preview editor claims the compile the same way when its tab
becomes active, since it is not a text editor and nothing else reports it. A pinned compile root
([0008](../decisions/0008-compile-root.md)) outranks all of this and suppresses the notification.

---

## 2. Page rendering and virtualization

A text-heavy A4 page is **386 KB of SVG**, and a 30-page document is **11.4 MB**
([spike.md §7](../research/spike.md#7-page-svg-anatomy)). Materializing a whole document is not viable, so the webview
renders a window, not a document.

```
webview page list
┌─────────────────────────────────────────────────────────┐
│  page 1   placeholder  (sized from PageMetrics, no SVG) │  ← recycled
│  page 2   placeholder                                   │
│ ─────────────────────── prefetch margin ─────────────── │
│  page 3   <svg …>   rendered                            │
│ ═══════════ viewport ═══════════════════════════════════ │
│  page 4   <svg …>   rendered                            │
│  page 5   <svg …>   rendered                            │
│ ─────────────────────── prefetch margin ─────────────── │
│  page 6   placeholder                                   │  ← recycled
│  …                                                      │
└─────────────────────────────────────────────────────────┘
```

- **Placeholders** are empty boxes at the exact page dimensions, so the scrollbar and scroll position are
  correct for the whole document from the first frame. Dimensions come from `PageMetrics`, which the server
  computes without rendering any SVG.
- **The render window** is the viewport plus one page of margin in each direction. Scrolling requests the
  newly-entered pages; pages leaving the window plus their margin are dropped back to placeholders,
  bounding webview DOM size regardless of document length.
- **Page identity is a content hash**, not an index. Insert a paragraph on page 2 of a 60-page document
  and pages 3–60 shift by one but keep their hashes — so the webview re-anchors its rendered pages instead
  of re-fetching 58 of them.

Cost per keystroke in the common case: one page render (~5 ms) and one ~386 KB string across two IPC hops.

---

## 3. Update protocol

Typed discriminated unions in `src/preview/messages.ts`, imported by both sides — the repo's existing
convention ([markdown-preview-ultra/src/messages.ts](../../../../extensions/markdown-preview-ultra/src/messages.ts)).

```typescript
type HostToWebview =
  | { type: 'init'; settings: PreviewSettings }
  | { type: 'metrics'; seq: number; uri: string;
      pages: { index: number; widthPt: number; heightPt: number; hash: string }[] }
  | { type: 'pages'; seq: number; patches: PagePatch[] }
  | { type: 'cursor'; page: number; xPt: number; yPt: number }   // editor cursor moved
  | { type: 'status'; state: 'compiling' | 'ok' | 'error'; message?: string }
  | { type: 'settings'; settings: PreviewSettings }
  | { type: 'goToPage'; page: number };

type PagePatch =
  | { op: 'replace'; index: number; hash: string;
      format: 'svg' | 'png'; content: string }
  | { op: 'unchanged'; index: number }
  | { op: 'removed'; index: number };

type WebviewToHost =
  | { type: 'ready' }
  | { type: 'viewport'; first: number; last: number;
      known: Record<number, string>; zoom: number }
  | { type: 'click'; page: number; xPt: number; yPt: number }
  | { type: 'scrolled'; page: number; yPt: number }              // preview→editor sync
  | { type: 'openLink'; href: string }
  | { type: 'state'; zoom: number; fit: FitMode; inverted: boolean }
  | { type: 'export' }                                           // toolbar button
  | { type: 'openSource' }                                       // toolbar button
  | { type: 'error'; message: string; context: string };
```

The last two are payload-free on purpose: a button that told the host *which file* to export would be a
webview naming a path, and the host already knows which document it is showing. They go through the same
hand-written guard as every other variant, which drops anything they try to carry.

Three differences from the RFC's sketch, each with a reason:

- **`unchanged` is a real variant**, not an omission. The server reports every requested page so the
  webview can tell "your copy is current" from "that request was dropped".
- **`format` and `content`** replace `svg`, because [P4-05](../tasks/phase-4-polish.md)'s PNG mode ships
  raster pages over the same protocol.
- **`viewport` carries the zoom.** A raster page is baked at one resolution, so the server has to know the
  size the page will be shown at. Vector pages ignore it.

Rules that keep this honest:

- **`seq` is monotonic per document.** The webview ignores any `metrics`/`pages` message whose `seq` is not
  greater than the last applied one, so an out-of-order or superseded compile can never corrupt the view.
- **Every `WebviewToHost` message is shape-validated host-side** by a hand-written guard per variant — no
  `any` dispatch. This is the lesson from MPE's CVE history that RFC 009 §10 recorded, and it applies
  identically here.
- **`viewport` drives everything.** The webview reports what it can see and what it already holds
  (`known`); the host asks the server for exactly the difference. There is no push-everything path.
- **`openLink`** is handled host-side with a scheme allowlist (`https`, `http`, `mailto`) plus
  workspace-relative resolution for `.typ` targets. The webview never navigates.

---

## 4. SVG injection, and why it is safe

The webview inserts compiler-produced SVG markup into the DOM. That deserves an explicit argument rather
than an assumption.

1. **The SVG is engine-generated, not user-authored.** `typst_svg::svg` emits a fixed vocabulary —
   `<path>`, `<use>`, `<symbol>`, `<g>`, `<defs>`, `<image>`, `<text>` — from the layout `Frame`. A typst
   document cannot inject raw markup into it; there is no `html` escape hatch in the paged export target.
2. **Content is still attacker-influenced**, because a document may come from anywhere. So we do not rely
   solely on point 1:
   - **CSP**: `default-src 'none'; img-src ${webview.cspSource} data:; style-src ${cspSource} 'unsafe-inline'; script-src 'nonce-…'`.
     No `script-src` value permits inline SVG `<script>`, so even a hypothetical injected script cannot run.
   - **Parse, don't concatenate**: page SVG is parsed with `DOMParser.parseFromString(svg, 'image/svg+xml')`
     and the resulting root is adopted. A parse error yields an error card, not a partial DOM.
   - **Strip on adopt**: `<script>`, `<foreignObject>`, and any `on*` attribute are removed from the parsed
     tree before insertion. This should always be a no-op; if it ever is not, that is a compiler bug we
     want to survive rather than execute.
   - **Embedded raster images** arrive as `data:` URIs from the compiler, which the CSP permits and which
     cannot execute.
3. **No remote loads of any kind.** No CDN, no web fonts, no telemetry.

---

## 5. Two-way sync

Both directions are upstream's, unmodified — which is exactly why we need no compiler patch here
([crates.md §4](crates.md#4-typst-preview-core)).

### Editor → preview

```
cursor moves ──▶ debounce 50ms ──▶ typst/jumpFromCursor { uri, offset }
                                     └─ typst_ide::jump_from_cursor(doc, source, cursor)
                                          └─▶ Vec<PagedPosition { page, point }>
                                   ──▶ postMessage { type: 'cursor', page, xPt, yPt }
                                   ──▶ scroll page into view + flash the indicator
```

`jump_from_cursor` returns positions only for `Text` / `MathText` leaves, so putting the cursor in a
comment or on a `#let` keyword correctly yields nothing — we leave the preview where it is rather than
jumping somewhere arbitrary. Multiple positions can come back (content used more than once); we pick the
first and let the indicator show all of them.

`typstUltra.preview.cursorIndicator` draws a small marker at the position, fading after ~1.5 s.

### Preview → editor

```
click on page ──▶ { type: 'click', page, xPt, yPt }
              ──▶ typst/jumpFromClick
                    └─ typst_ide::jump_from_click(world, doc, position)
                         └─▶ Jump::File(FileId, offset) | Jump::Url(url) | Jump::Position(pos)
              ──▶ File → reveal + focus that offset in the right editor
                  Url  → env.openExternal after the scheme allowlist
                  Position → scroll the preview (an internal document link)
```

Coordinate conversion happens webview-side: a click's client coordinates are divided by the current zoom
and converted to typographic points using the page's known `widthPt`/`heightPt`, so the server receives
document-space points and never needs to know about zoom or device pixel ratio.

### Scroll sync and loop protection

Scrolling the preview reports its viewport-center page and offset; the host reveals the corresponding
source line via `jumpFromClick` at that point. Each side stamps the origin of its last programmatic scroll
and ignores reciprocal sync for **100 ms** — the same guard RFC 009 §6.2 uses, and for the same reason.
`typstUltra.preview.scrollSync` can restrict this to one direction or disable it.

---

## 6. Webview chrome

All dependency-free; the webview bundle should stay well under 50 KB.

| Feature | Behaviour |
| --- | --- |
| **Zoom** | `ctrl/cmd +` / `-` / `0`; also a fit-width / fit-page / actual-size toggle. Applied as a CSS transform on the page container so no re-render is needed. Persisted via `setState` |
| **Color inversion** | A CSS filter for reading white pages in a dark editor. `never` / `always` / `auto`. Images are exempted from the filter so photos are not inverted |
| **Background** | `editor` (VSCode variables) / `white` / `gray` |
| **Page numbers** | A small overlay per page, plus a "go to page" input |
| **Placement** | A toolbar across the **top**, in the flow — continuing the editor's title bar rather than sitting at the far end of the panel. The status banner sits below it, so a compile error cannot swallow the controls |
| **Find** | `enableFindWidget: true` on the panel — free, and it works because the SVG contains real `<text>` runs |
| **Leaving the page** | An Edit button (`✎`) and an Export button (`⭳`), separated from the reading controls. What each means is the host's business: the panel hands focus to the editor beside it, the full-tab preview hands the tab itself back |
| **Status** | A thin bar showing compile state and time, driven by `typst/compileStatus` |
| **Error card** | On compile failure the last good pages stay visible, dimmed, with an error banner. The preview never goes blank mid-edit |

That last row matters more than it looks: because `Session` keeps the last good document
([crates.md §2.3](crates.md#23-the-compile-lifecycle)), a transient syntax error while typing does not
destroy the preview.

---

## 7. Export

| Format | Implementation | Options |
| --- | --- | --- |
| PDF | `typst_pdf::pdf(doc, &PdfOptions)` | Page range, PDF standard where upstream exposes it |
| SVG | `typst_svg::svg_merged(doc, opts, gap)` | Whole document, shared glyph defs |
| PNG | `typst_render::render(page, &RenderOptions)` per page | `pixel_per_pt` from a PPI setting (default 144) |

Export runs in the server on the last good document and returns bytes over `typst/export`; the host writes
them and offers "Open" / "Reveal in Finder". Export never triggers a compile of its own — if the document
currently has errors, the command reports that instead of silently exporting a stale file.

**Where it goes** ([Amendment 001 §3](../proposal-amendment-001-preview-ux.md#3-export-with-a-destination)):
a save dialog opens on `typstUltra.export.outputPath` (`$dir`, `$name`, `$root`), which starts at the
document's own folder under the document's own name — so accepting the default is one keystroke and moving
it is an ordinary file dialog. The dialog comes **before** the export request, so cancelling costs nothing,
and `export.askForLocation: false` skips it entirely. The answer is reduced to a base path by `baseFor`,
because PNG writes one file per page and has to keep suffixing.

The command also names its subject — `typst/compile { uri }` — before asking. The server exports whatever
it last compiled, and in a two-document workspace that is not necessarily the file whose Export button was
clicked.

---

## 8. Performance budget

Where the ~120 ms keystroke-to-repaint target from [proposal.md §8](../proposal.md#8-performance-targets)
goes, for a 30-page document at steady state:

| Step | Budget | Basis |
| --- | --- | --- |
| Debounce | 150 ms | Setting, not counted in the repaint budget |
| `Source::edit` incremental reparse | < 1 ms | upstream |
| `typst::compile` + `comemo::evict` | ~7 ms | [measured](../research/spike.md#42-eviction-age-sweep) |
| `measure()` — hash all pages, no render | ~2 ms | frame hashing only |
| `typst_svg::svg` for one changed page | ~5 ms | [measured](../research/spike.md#7-page-svg-anatomy) |
| JSON-RPC serialize + parse | **0.8 ms** | [measured](../research/transport.md) |
| Node IPC round trip (394 KB) | **2.9 ms** | [measured](../research/transport.md) |
| Extension host → webview `postMessage` | **0.1 ms** | [measured](../research/transport.md), via `structuredClone` |
| `DOMParser` + adopt + paint | ~20 ms | **still an estimate** — see below |
| **Total** | **~38 ms** | one row of it estimated, against a 120 ms target |

**The transport half came in 8× cheaper than estimated**: 3.7 ms against ~30 ms
([P3-05](../tasks/phase-3-preview.md), [transport.md](../research/transport.md)). The engine half was
already comfortable. So the repaint budget is not close to binding, and neither escape hatch was needed
in Phase 3.

Two things that did not survive contact:

1. **Coordinate precision was not the lever it looked like.** The premise — 3,144 `<use>` elements at
   *full-precision floats* — is only half right. `typst-svg` already rounds to 9 decimal places and
   formats through `ryu`, so rounding to 2 saves **2.9%**, not "substantially".
   [P4-11](../tasks/phase-4-polish.md) is implemented and kept, because it is free, but it is not a lever.
2. **A real page is 470 KB, not 386 KB** — a two-column paper packs more glyphs per page than the spike's
   fixture ([corpus.md](../research/corpus.md)). The transport numbers above were taken at 394 KB, so
   scale them ~16% for the worst case; the conclusion is unaffected.

**PNG mode** remains the real lever if page size ever binds, and is implemented as planned in Phase 4
(`typstUltra.preview.renderMode`, [0006](../decisions/0006-preview-rendering.md)) — not because it was
needed, but because a document full of raster images is a case the corpus still does not cover.

The one row still carrying an estimate is `DOMParser` + adopt + paint, and closing it needs a real
browser: `happy-dom` reports 46 ms for a 394 KB page, but it is a pure-JS parser that does no layout at
all, so that figure bounds nothing in either direction.
