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
| Preview panel | `WebviewPanel` beside or in the editor's column | `typstUltra.showPreview` / `showPreviewToSide` |
| Custom editor | `typstUltra.preview`, `priority: "option"` for `.typ` | User opts in via `workbench.editorAssociations` |
| Serializer | `WebviewPanelSerializer` | Panel survives window reload |

The custom editor is a `CustomTextEditorProvider` in read-only mode: it renders the same webview against
the document, so a `.typ` can be opened as a finished document with no flash of source. We register it at
`priority: "option"` and **never write `workbench.editorAssociations` ourselves** — the global-settings
mutation that MPE does is the anti-pattern RFC 009 called out, and it applies here too.

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
  | { type: 'init'; settings: PreviewSettings; baseUri: string }
  | { type: 'metrics'; seq: number; uri: string;
      pages: { index: number; widthPt: number; heightPt: number; hash: string }[] }
  | { type: 'pages'; seq: number;
      patches: ({ op: 'replace'; index: number; hash: string; svg: string }
              | { op: 'drop'; index: number })[] }
  | { type: 'cursor'; page: number; xPt: number; yPt: number }   // editor cursor moved
  | { type: 'status'; state: 'compiling' | 'ok' | 'error'; message?: string }
  | { type: 'settings'; settings: PreviewSettings };

type WebviewToHost =
  | { type: 'ready' }
  | { type: 'viewport'; first: number; last: number; known: Record<number, string> }
  | { type: 'click'; page: number; xPt: number; yPt: number }
  | { type: 'scrolled'; page: number; yPt: number }              // preview→editor sync
  | { type: 'openLink'; href: string }
  | { type: 'state'; zoom: number; fit: FitMode; inverted: boolean }
  | { type: 'error'; message: string; context: string };
```

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
| **Find** | `enableFindWidget: true` on the panel — free, and it works because the SVG contains real `<text>` runs |
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
them using `typstUltra.export.outputPath` (`$dir`, `$name`, `$root` placeholders) and offers "Open" /
"Reveal in Finder". Export never triggers a compile of its own — if the document currently has errors, the
command reports that instead of silently exporting a stale file.

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
| JSON-RPC serialize + Node IPC (386 KB) | ~15 ms | estimate, **unmeasured** |
| Extension host → webview `postMessage` | ~15 ms | estimate, **unmeasured** |
| `DOMParser` + adopt + paint | ~20 ms | estimate, **unmeasured** |
| **Total** | **~65 ms** | ~55 ms of it estimated |

The honest reading: the engine half is measured and comfortable; the transport half is estimated and is
where the risk lives. Two escape hatches if the estimate is wrong, in order of preference:

1. **Coordinate precision.** The 386 KB is dominated by 3,144 `<use>` elements with full-precision floats.
   Rounding to 2 decimal places in a post-pass would cut it substantially at sub-pixel visual cost.
2. **PNG mode.** `typst_render` at 144 PPI produces a much smaller payload for text-heavy pages, at the
   cost of zoom fidelity and find-in-preview. This is already an **accepted Phase-4 optimization**
   (`typstUltra.preview.renderMode`, [0006](../decisions/0006-preview-rendering.md)); if the transport
   measurements come in badly it gets promoted into Phase 3 instead.

The first thing Phase 3 should do is replace the estimated rows in this table with measurements —
[P3-05](../tasks/phase-3-preview.md), which is sequenced before the webview work for exactly this reason.
